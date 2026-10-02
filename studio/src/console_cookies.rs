use std::{
    collections::HashSet,
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{
    StatusCode, Url,
    blocking::{Client, Response},
    header::{COOKIE, HeaderValue},
};
use serde_json::Value;
use sha1::{Digest, Sha1};

const ORIGIN: &str = "https://play.google.com";
const MAX_COOKIES: usize = 64 * 1024;
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
const SESSION_ERROR: &str = "Play Console could not use this session. Sign in again and copy a fresh document.cookie value.";

// Session credentials stay in memory for this sync and deliberately have no Debug implementation.
pub(crate) struct Connection {
    console_url: Url,
    developer_id: String,
    auth_user: String,
    cookies: HeaderValue,
    signing_cookie: String,
}

impl Connection {
    pub(crate) fn parse(console_url: &str, cookies: &str) -> Result<Self, String> {
        let url_error = "Paste the Play Console app-list URL for your developer account.";
        let url = Url::parse(console_url.trim()).map_err(|_| url_error.to_owned())?;
        if url.scheme() != "https"
            || url.host_str() != Some("play.google.com")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(url_error.into());
        }
        let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
        let (auth_user, developer_id) = match parts.as_slice() {
            [
                "",
                "console",
                "u",
                user,
                "developers",
                developer,
                "app-list",
            ] => (*user, *developer),
            ["", "console", "developers", developer, "app-list"] => ("0", *developer),
            _ => return Err(url_error.into()),
        };
        if !numeric_id(developer_id, 32) || !numeric_id(auth_user, 10) {
            return Err(url_error.into());
        }
        let canonical_url = Url::parse(&format!(
            "{ORIGIN}/console/u/{auth_user}/developers/{developer_id}/app-list"
        ))
        .map_err(|_| url_error.to_owned())?;

        let cookies = cookies.trim();
        let cookies = cookies
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                cookies
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(cookies);
        if cookies.is_empty() || cookies.len() > MAX_COOKIES {
            return Err("Paste the full value returned by document.cookie.".into());
        }
        let mut cookie_header = HeaderValue::from_str(cookies).map_err(|_| {
            "The cookie value contains invalid characters. Copy it again.".to_owned()
        })?;
        cookie_header.set_sensitive(true);
        let mut sapisid = None;
        let mut fallback = None;
        let mut names = HashSet::new();
        for part in cookies.split(';') {
            let (name, value) = part
                .trim()
                .split_once('=')
                .filter(|(name, _)| !name.is_empty())
                .ok_or("Paste the full value returned by document.cookie.")?;
            if !names.insert(name) {
                return Err("The cookie value contains duplicate names. Copy it again.".into());
            }
            match name {
                "SAPISID" => sapisid = Some(value),
                "__Secure-3PAPISID" => fallback = Some(value),
                _ => {}
            }
        }
        let signing_cookie = sapisid
            .or(fallback)
            .filter(|value| !value.is_empty())
            .ok_or("This cookie value is missing SAPISID. Sign in to Play Console and copy document.cookie again.")?;
        Ok(Self {
            console_url: canonical_url,
            developer_id: developer_id.to_owned(),
            auth_user: auth_user.to_owned(),
            cookies: cookie_header,
            signing_cookie: signing_cookie.to_owned(),
        })
    }

    pub(crate) fn fetch(
        self,
        cancelled: &AtomicBool,
    ) -> Result<crate::console_sync::Snapshot, String> {
        let http = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| "Could not initialize the Play Console connection.".to_owned())?;
        let summaries_url = format!(
            "https://playconsoleapps-pa.clients6.google.com/v1/developers/{}/appSummaries",
            self.developer_id
        );
        self.fetch_with_client(&http, self.console_url.as_str(), &summaries_url, cancelled)
    }

    fn fetch_with_client(
        &self,
        http: &Client,
        console_url: &str,
        summaries_url: &str,
        cancelled: &AtomicBool,
    ) -> Result<crate::console_sync::Snapshot, String> {
        check_cancelled(cancelled)?;
        let response = http
            .get(console_url)
            .header(COOKIE, self.cookies.clone())
            .send()
            .map_err(|_| {
                "Could not reach Play Console. Check your connection and try again.".to_owned()
            })?;
        let bytes = read_response(response)?;
        check_cancelled(cancelled)?;
        let html = std::str::from_utf8(&bytes).map_err(|_| SESSION_ERROR.to_owned())?;
        let setup = console_setup(html)?;
        let api_key = setup
            .get("8")
            .and_then(Value::as_str)
            .filter(|key| key.starts_with("AIza") && key.len() == 39)
            .ok_or("Could not read Play Console's API configuration. Try syncing with the browser extension.")?;
        let mut apps = Vec::new();
        let mut seen_packages = HashSet::new();
        let mut seen_tokens = HashSet::new();
        let mut page_token = String::new();
        loop {
            check_cancelled(cancelled)?;
            let mut request = http
                .get(summaries_url)
                .query(&[("fetchGamingPlatform", "true"), ("pageSize", "500")])
                .header(COOKIE, self.cookies.clone())
                .header("Origin", ORIGIN)
                .header("Referer", format!("{ORIGIN}/"))
                .header("Content-Type", "application/json+protobuf")
                .header("X-Goog-AuthUser", &self.auth_user)
                .header("X-Goog-Api-Key", api_key)
                .header(
                    "Authorization",
                    authorization(&self.signing_cookie, unix_seconds()?)?,
                );
            if let Some(session_id) = setup.get("27").and_then(Value::as_str) {
                request = request.header("X-Play-Console-Session-Id", session_id);
            }
            if !page_token.is_empty() {
                request = request.query(&[("pageToken", &page_token)]);
            }
            let response = request.send().map_err(|_| {
                "Could not load Play Console icons. Check your connection and try again.".to_owned()
            })?;
            let bytes = read_response(response)?;
            check_cancelled(cancelled)?;
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "Play Console returned an unreadable app list.".to_owned())?;
            for app in crate::play_console::parse_console_apps(&value)? {
                if seen_packages.insert(app.package_name.clone()) {
                    apps.push(app);
                }
            }
            page_token = value
                .get("2")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into();
            if page_token.is_empty() {
                break;
            }
            if !seen_tokens.insert(page_token.clone()) || seen_tokens.len() > 100 {
                return Err(
                    "Play Console returned an invalid page sequence. Try syncing again.".into(),
                );
            }
        }
        Ok(crate::console_sync::Snapshot {
            developer_id: self.developer_id.clone(),
            apps,
        })
    }
}

fn numeric_id(value: &str, max_length: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_length
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "Check your system clock before syncing Play Console.".to_owned())
}

fn authorization(cookie: &str, timestamp: u64) -> Result<HeaderValue, String> {
    // Google's SAPISIDHASH signs "timestamp cookie origin", as verified against the HAR.
    let digest = Sha1::digest(format!("{timestamp} {cookie} {ORIGIN}").as_bytes());
    let mut header = HeaderValue::from_str(&format!("SAPISIDHASH {timestamp}_{digest:x}"))
        .map_err(|_| "Could not authenticate the Play Console request.".to_owned())?;
    header.set_sensitive(true);
    Ok(header)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Play Console sync was cancelled.".into())
    } else {
        Ok(())
    }
}

fn read_response(response: Response) -> Result<Vec<u8>, String> {
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) || response.status().is_redirection()
    {
        return Err(SESSION_ERROR.into());
    }
    if !response.status().is_success() {
        return Err("Play Console is unavailable. Try syncing again later.".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the Play Console response.".to_owned())?;
    if bytes.len() as u64 > MAX_RESPONSE {
        return Err(
            "The Play Console response is too large. Try syncing with the browser extension."
                .into(),
        );
    }
    Ok(bytes)
}

fn console_setup(html: &str) -> Result<Value, String> {
    let marker = "window.serializedInitialChunks['startupData']";
    let rest = html.split_once(marker).ok_or(SESSION_ERROR)?.1.trim_start();
    let rest = rest.strip_prefix('=').ok_or(SESSION_ERROR)?.trim_start();
    let literal = rest.strip_prefix('"').ok_or(SESSION_ERROR)?;
    // Decode the JavaScript string without evaluating any page scripts. JSON accepts
    // the ordinary escapes; convert JavaScript's \xNN escapes to \u00NN first.
    let mut json_string = String::from("\"");
    let mut chars = literal.chars();
    let mut closed = false;
    while let Some(character) = chars.next() {
        if character == '"' {
            json_string.push('"');
            closed = true;
            break;
        }
        json_string.push(character);
        if character == '\\' {
            let escaped = chars.next().ok_or(SESSION_ERROR)?;
            if escaped == 'x' {
                json_string.push_str("u00");
                for _ in 0..2 {
                    let hex = chars
                        .next()
                        .filter(|c| c.is_ascii_hexdigit())
                        .ok_or(SESSION_ERROR)?;
                    json_string.push(hex);
                }
            } else {
                json_string.push(escaped);
            }
        }
    }
    if !closed {
        return Err(SESSION_ERROR.into());
    }
    let decoded: String =
        serde_json::from_str(&json_string).map_err(|_| SESSION_ERROR.to_owned())?;
    let startup: Value = serde_json::from_str(&decoded).map_err(|_| SESSION_ERROR.to_owned())?;
    startup
        .get("1")
        .cloned()
        .ok_or_else(|| SESSION_ERROR.to_owned())
}
