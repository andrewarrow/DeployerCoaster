use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{collections::HashSet, path::PathBuf, process::Command};

use super::{Project, ProjectPage, ProjectsResult};

pub(super) fn access_token() -> Result<String, String> {
    if let Ok(token) = std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN")
        && !token.trim().is_empty()
    {
        return Ok(token.trim().to_owned());
    }
    let mut executables = vec![PathBuf::from("gcloud")];
    if let Some(home) = dirs::home_dir() {
        executables.push(home.join("dev/google-cloud-sdk/bin/gcloud"));
        executables.push(home.join("google-cloud-sdk/bin/gcloud"));
    }
    executables.extend([
        PathBuf::from("/opt/homebrew/bin/gcloud"),
        PathBuf::from("/opt/homebrew/share/google-cloud-sdk/bin/gcloud"),
        PathBuf::from("/usr/local/bin/gcloud"),
    ]);
    for executable in executables {
        if let Ok(output) = Command::new(executable)
            .args(["auth", "print-access-token", "--quiet"])
            .output()
            && output.status.success()
            && let Ok(token) = String::from_utf8(output.stdout)
            && !token.trim().is_empty()
        {
            return Ok(token.trim().to_owned());
        }
    }
    Err("Sign in to Google Cloud with gcloud auth login, then refresh projects.".into())
}

pub(super) fn fetch_projects(
    http: &reqwest::blocking::Client,
    token: &str,
    endpoint: &str,
) -> ProjectsResult {
    let mut projects = Vec::new();
    let mut page_token = String::new();
    let mut seen = HashSet::new();
    loop {
        let response = http
            .get(endpoint)
            .bearer_auth(token)
            .query(&[
                ("query", "state:ACTIVE"),
                ("pageSize", "100"),
                ("pageToken", &page_token),
            ])
            .send()
            .map_err(|_| {
                "Could not reach Google Cloud. Check your connection and refresh.".to_owned()
            })?;
        match response.status().as_u16() {
            200 => {}
            401 => return Err("Google Cloud sign-in expired. Run gcloud auth login and refresh.".into()),
            403 => return Err("Google Cloud denied project access. Check the active account, project permissions, and Resource Manager API access.".into()),
            429 => return Err("Google Cloud's rate limit was reached. Try refreshing later.".into()),
            _ => return Err("Google Cloud could not list projects. Try refreshing again.".into()),
        }
        let page: ProjectPage = response
            .json()
            .map_err(|_| "Google Cloud returned an invalid project list.".to_owned())?;
        projects.extend(page.projects);
        if page.next_page_token.is_empty() {
            projects.sort_by_cached_key(|p| (p.display_name.to_lowercase(), p.project_id.clone()));
            let mut ids = HashSet::new();
            projects.retain(|p| ids.insert(p.project_id.clone()));
            return Ok(projects);
        }
        if !seen.insert(page.next_page_token.clone()) || seen.len() > 1000 {
            return Err(
                "Google Cloud returned invalid project pagination. Refresh to try again.".into(),
            );
        }
        page_token = page.next_page_token;
    }
}

pub(super) const CONSOLE_FILE: &str = "google-console-session.json";
pub(super) const CONSOLE_PATH: &str =
    "/v3/entityServices/ServiceUsageEntityService/schemas/SERVICE_USAGE_GRAPHQL:batchGraphql";
pub(super) const SESSION_ERROR: &str = "The Console session expired or lacks access to this project. Copy a fresh client-list request from Google Cloud and reconnect.";

// Browser session credentials are never logged or included in errors.
#[derive(Deserialize, Serialize)]
pub(super) struct ConsoleSession {
    pub(super) url: String,
    pub(super) cookies: String,
    pub(super) auth_user: String,
    pub(super) body: serde_json::Value,
}

impl ConsoleSession {
    pub(super) fn branding_url(&self, project: &Project) -> Result<reqwest::Url, String> {
        let number = project
            .name
            .strip_prefix("projects/")
            .and_then(|number| number.parse::<u64>().ok())
            .ok_or("Invalid Google Cloud project number.")?;
        let console_url =
            reqwest::Url::parse(&self.url).map_err(|_| "Invalid Console request URL.")?;
        let key = console_url
            .query_pairs()
            .find_map(|(name, value)| (name == "key").then_some(value.into_owned()))
            .ok_or("The Console request is missing its API key. Reconnect.")?;
        let mut url = reqwest::Url::parse(&format!(
            "https://clientauthconfig.clients6.google.com/v1/brands/lookupkey/brand/{number}"
        ))
        .unwrap();
        url.query_pairs_mut()
            .append_pair("key", &key)
            .append_pair("readMask", "iconUrl");
        Ok(url)
    }

    pub(super) fn load() -> Result<Self, String> {
        let path = crate::storage::credential_path(CONSOLE_FILE)?;
        let bytes = std::fs::read(path).map_err(|_| {
            "Connect a Google Cloud Console session to list OAuth clients.".to_owned()
        })?;
        let session: Self = serde_json::from_slice(&bytes).map_err(|_| {
            "Could not load the Console session. Reconnect with a fresh request.".to_owned()
        })?;
        session.validate()?;
        Ok(session)
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| "Invalid Console request URL.")?;
        if url.scheme() != "https"
            || url.host_str() != Some("cloudconsole-pa.clients6.google.com")
            || url.path() != CONSOLE_PATH
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some_and(|p| p != 443)
            || self.body["querySignature"].as_str().is_none()
            || !self.body["variables"].is_object()
            || self.auth_user.is_empty()
            || !self.auth_user.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(
                "Copy the OAuth client-list batchGraphql request from Google Cloud.".into(),
            );
        }
        self.authorization()?;
        Ok(())
    }

    pub(super) fn parse_curl(input: &str) -> Result<Self, String> {
        let invalid = "Paste the entire OAuth client-list cURL request from Google Cloud.";
        let args = crate::console_cookies::curl_arguments(input).map_err(|_| invalid)?;
        if args.first().map(String::as_str) != Some("curl") {
            return Err(invalid.into());
        }
        let mut url = None;
        let mut cookies = None;
        let mut auth_user = "0".to_owned();
        let mut body = None;
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-H" | "--header" => {
                    let header = args.next().ok_or(invalid)?;
                    if let Some((name, value)) = header.split_once(':') {
                        match name.trim().to_ascii_lowercase().as_str() {
                            "cookie" => cookies = Some(value.trim().to_owned()),
                            "x-goog-authuser" => auth_user = value.trim().to_owned(),
                            _ => {}
                        }
                    }
                }
                "-b" | "--cookie" => cookies = Some(args.next().ok_or(invalid)?.clone()),
                "--data-raw" | "--data" | "--data-binary" | "-d" => {
                    body = Some(
                        serde_json::from_str::<serde_json::Value>(args.next().ok_or(invalid)?)
                            .map_err(|_| invalid)?,
                    );
                }
                "--url" => url = Some(args.next().ok_or(invalid)?.clone()),
                _ if arg.starts_with("https://") => url = Some(arg.clone()),
                _ => {}
            }
        }
        let session = Self {
            url: url.ok_or(invalid)?,
            cookies: cookies.ok_or(invalid)?,
            auth_user,
            body: body.ok_or(invalid)?,
        };
        session.validate()?;
        Ok(session)
    }

    pub(super) fn authorization(&self) -> Result<String, String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "Could not read the system clock.")?
            .as_secs();
        let mut hashes = Vec::new();
        for (cookie_name, prefix) in [
            ("SAPISID", "SAPISIDHASH"),
            ("__Secure-1PAPISID", "SAPISID1PHASH"),
            ("__Secure-3PAPISID", "SAPISID3PHASH"),
        ] {
            if let Some(value) = self
                .cookies
                .split(';')
                .filter_map(|part| part.trim().split_once('='))
                .find_map(|(name, value)| {
                    (name == cookie_name && !value.is_empty()).then_some(value)
                })
            {
                let digest = Sha1::digest(
                    format!("{timestamp} {value} https://console.cloud.google.com").as_bytes(),
                );
                hashes.push(format!("{prefix} {timestamp}_{digest:x}"));
            }
        }
        if hashes.is_empty() {
            return Err(
                "The request is missing Google session cookies. Copy the full cURL request again."
                    .into(),
            );
        }
        Ok(hashes.join(" "))
    }
}

pub(super) fn fetch_branding_icon_url(
    http: &reqwest::blocking::Client,
    session: &ConsoleSession,
    url: &reqwest::Url,
) -> Result<Option<String>, String> {
    let mut cookie_header = reqwest::header::HeaderValue::from_str(&session.cookies)
        .map_err(|_| "Invalid Console cookies. Reconnect.")?;
    cookie_header.set_sensitive(true);
    let response = http
        .get(url.clone())
        .header("Origin", "https://console.cloud.google.com")
        .header("Referer", "https://console.cloud.google.com/")
        .header("X-Goog-AuthUser", &session.auth_user)
        .header("Authorization", session.authorization()?)
        .header(reqwest::header::COOKIE, cookie_header)
        .send()
        .map_err(|_| "Could not load Google Cloud branding.")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(SESSION_ERROR.into());
    }
    let brand: serde_json::Value = response
        .json()
        .map_err(|_| "Google Cloud returned invalid branding.")?;
    Ok(brand["iconUrl"]
        .as_str()
        .filter(|url| crate::app_icons::validate_google_artwork_url(url))
        .map(str::to_owned))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OAuthClient {
    pub(super) client_id: String,
    pub(super) display_name: String,
    pub(super) display_type: String,
    #[serde(default)]
    pub(super) creation_time: String,
}

impl OAuthClient {
    pub(super) fn type_label(&self) -> &str {
        match self.display_type.as_str() {
            "CLIENT_TYPE_WEB_APPLICATION" => "Web application",
            "CLIENT_TYPE_IOS" => "iOS",
            "CLIENT_TYPE_ANDROID" => "Android",
            "CLIENT_TYPE_DESKTOP" => "Desktop app",
            "CLIENT_TYPE_CHROME_APP" => "Chrome app",
            "CLIENT_TYPE_DEVICE" => "TV / limited input",
            "CLIENT_TYPE_UWP" => "Universal Windows",
            "CLIENT_TYPE_SERVICE_ACCOUNT" => "Service account",
            _ => "Unknown",
        }
    }
}

pub(super) fn fetch_clients(
    http: &reqwest::blocking::Client,
    session: &ConsoleSession,
    project: &Project,
) -> Result<Vec<OAuthClient>, String> {
    let number = project
        .name
        .strip_prefix("projects/")
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or("Invalid Google Cloud project number.")?;
    let mut clients = Vec::new();
    let mut body = session.body.clone();
    body["requestContext"]["projectId"] = project.project_id.clone().into();
    body["requestContext"]["selectedPurview"] =
        serde_json::json!({"projectId": project.project_id});
    body["variables"]["projectId"] = project.project_id.clone().into();
    body["variables"]["projectNumber"] = number.into();
    body["variables"]["pageSize"] = 50.into();
    body["variables"]
        .as_object_mut()
        .ok_or("Invalid Console request.")?
        .remove("pageToken");
    let mut seen = HashSet::new();
    loop {
        let mut cookie_header = reqwest::header::HeaderValue::from_str(&session.cookies)
            .map_err(|_| "Invalid Console cookies. Reconnect.")?;
        cookie_header.set_sensitive(true);
        let response = http
            .post(&session.url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://console.cloud.google.com")
            .header("Referer", "https://console.cloud.google.com/")
            .header("X-Goog-AuthUser", &session.auth_user)
            .header("Authorization", session.authorization()?)
            .header(reqwest::header::COOKIE, cookie_header)
            .json(&body)
            .send()
            .map_err(|_| {
                "Could not reach Google Cloud Console. Check your connection and refresh."
                    .to_owned()
            })?;
        if !response.status().is_success() {
            return Err(SESSION_ERROR.into());
        }
        let value: serde_json::Value = response
            .json()
            .map_err(|_| "Google Cloud returned an invalid OAuth client response.".to_owned())?;
        let result = value
            .pointer("/0/results/0")
            .ok_or("Google Cloud returned an unsupported OAuth client response.")?;
        if result
            .get("errors")
            .and_then(|e| e.as_array())
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(SESSION_ERROR.into());
        }
        let list = result
            .pointer("/data/oAuthClientsList")
            .ok_or("Google Cloud returned an unsupported OAuth client list.")?;
        let batch: Vec<OAuthClient> = serde_json::from_value(list["data"].clone())
            .map_err(|_| "Google Cloud returned invalid OAuth clients.".to_owned())?;
        clients.extend(batch);
        let next = list["nextPageToken"].as_str().unwrap_or_default();
        if next.is_empty() {
            let mut ids = HashSet::new();
            clients.retain(|client| ids.insert(client.client_id.clone()));
            return Ok(clients);
        }
        if !seen.insert(next.to_owned()) || seen.len() > 1000 {
            return Err(
                "Google Cloud returned invalid client pagination. Refresh to try again.".into(),
            );
        }
        body["variables"]["pageToken"] = next.into();
    }
}
