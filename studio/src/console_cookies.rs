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
const COOKIE_INPUT_ERROR: &str = "Paste the full Cookie request header from Developer Tools → Network for the Play Console app-list page.";
const SESSION_ERROR: &str = "Play Console could not use this session. Sign in, reload the app-list page, and copy its full Cookie request header from Developer Tools → Network. document.cookie omits required session cookies.";

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
        log::debug!(
            "Validating Play Console input: URL present={}, cookie bytes={}",
            !console_url.trim().is_empty(),
            cookies.len()
        );
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
        // Accept either the request header's value or a copied "Cookie: …" line.
        let cookies = cookies
            .get(..7)
            .filter(|prefix| prefix.eq_ignore_ascii_case("cookie:"))
            .map(|_| cookies[7..].trim())
            .unwrap_or(cookies);
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
            return Err(COOKIE_INPUT_ERROR.into());
        }
        let mut cookie_header = HeaderValue::from_str(cookies).map_err(|_| {
            "The cookie value contains invalid characters. Copy it again.".to_owned()
        })?;
        cookie_header.set_sensitive(true);
        let mut sapisid = None;
        let mut fallback = None;
        let mut has_session_cookie = false;
        let mut names = HashSet::new();
        for part in cookies.split(';') {
            let (name, value) = part
                .trim()
                .split_once('=')
                .filter(|(name, _)| !name.is_empty())
                .ok_or(COOKIE_INPUT_ERROR)?;
            if !names.insert(name) {
                return Err("The cookie value contains duplicate names. Copy it again.".into());
            }
            match name {
                "SAPISID" => sapisid = Some(value),
                "__Secure-3PAPISID" => fallback = Some(value),
                "SID" | "__Secure-1PSID" | "__Secure-3PSID" if !value.is_empty() => {
                    has_session_cookie = true;
                }
                _ => {}
            }
        }
        log::debug!(
            "Parsed {} cookies: SAPISID present={}, fallback signing cookie present={}",
            names.len(),
            sapisid.is_some_and(|value| !value.is_empty()),
            fallback.is_some_and(|value| !value.is_empty())
        );
        if !has_session_cookie {
            return Err("These cookies are missing the browser session. Copy the full Cookie request header from Developer Tools → Network, rather than document.cookie.".into());
        }
        let signing_cookie = sapisid
            .or(fallback)
            .filter(|value| !value.is_empty())
            .ok_or("This Cookie header is missing SAPISID. Sign in to Play Console, reload the app-list page, and copy its Cookie request header again.")?;
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
        log::debug!("Requesting Play Console startup page");
        let response = http
            .get(console_url)
            .header(COOKIE, self.cookies.clone())
            .send()
            .map_err(|error| {
                log::debug!(
                    "Play Console startup request failed: {}",
                    error.without_url()
                );
                "Could not reach Play Console. Check your connection and try again.".to_owned()
            })?;
        let bytes = read_response(response, "startup")?;
        check_cancelled(cancelled)?;
        let html = std::str::from_utf8(&bytes).map_err(|_| {
            log::debug!("Play Console startup response is not UTF-8");
            SESSION_ERROR.to_owned()
        })?;
        let setup = console_setup(html).inspect_err(|_| {
            log::debug!("Could not decode Play Console startup configuration");
        })?;
        log::debug!(
            "Decoded Play Console startup configuration: API key field present={}, session ID present={}",
            setup.get("8").is_some(),
            setup.get("27").and_then(Value::as_str).is_some()
        );
        let api_key = setup
            .get("8")
            .and_then(Value::as_str)
            .filter(|key| key.starts_with("AIza") && key.len() == 39)
            .ok_or("Could not read Play Console's API configuration. Reload Play Console and try syncing again.")?;
        let mut apps = Vec::new();
        let mut seen_packages = HashSet::new();
        let mut seen_tokens = HashSet::new();
        let mut page_token = String::new();
        let mut page_number = 0;
        loop {
            check_cancelled(cancelled)?;
            page_number += 1;
            log::debug!("Requesting Play Console app summaries page {page_number}");
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
                    authorization(&self.signing_cookie, unix_milliseconds()?)?,
                );
            if let Some(session_id) = setup.get("27").and_then(Value::as_str) {
                request = request.header("X-Play-Console-Session-Id", session_id);
            }
            if !page_token.is_empty() {
                request = request.query(&[("pageToken", &page_token)]);
            }
            let response = request.send().map_err(|error| {
                log::debug!(
                    "Play Console app summaries request failed: {}",
                    error.without_url()
                );
                "Could not load Play Console icons. Check your connection and try again.".to_owned()
            })?;
            let bytes = read_response(response, "app summaries")?;
            check_cancelled(cancelled)?;
            let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
                log::debug!(
                    "Play Console app summaries JSON failed: category={:?}, line={}, column={}",
                    error.classify(),
                    error.line(),
                    error.column()
                );
                "Play Console returned an unreadable app list.".to_owned()
            })?;
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
            log::debug!(
                "Parsed Play Console app summaries page {page_number}: total apps={}, next page present={}",
                apps.len(),
                !page_token.is_empty()
            );
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

fn unix_milliseconds() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| "Check your system clock before syncing Play Console.".to_owned())
}

fn authorization(cookie: &str, timestamp: u128) -> Result<HeaderValue, String> {
    // Play Console signs "timestamp cookie origin" using milliseconds, as verified
    // against both the appSummaries and appList:startupData requests in the HAR.
    let digest = Sha1::digest(format!("{timestamp} {cookie} {ORIGIN}").as_bytes());
    let mut header = HeaderValue::from_str(&format!("SAPISIDHASH {timestamp}_{digest:x}"))
        .map_err(|_| "Could not authenticate the Play Console request.".to_owned())?;
    header.set_sensitive(true);
    Ok(header)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        log::debug!("Play Console sync cancellation observed");
        Err("Play Console sync was cancelled.".into())
    } else {
        Ok(())
    }
}

fn read_response(response: Response, stage: &str) -> Result<Vec<u8>, String> {
    log::debug!(
        "Play Console {stage} response: status={}, content length={:?}, content type={:?}, redirect location present={}",
        response.status(),
        response.content_length(),
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        response.headers().contains_key(reqwest::header::LOCATION)
    );
    if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
        match location
            .to_str()
            .ok()
            .and_then(|location| response.url().join(location).ok())
        {
            Some(url) => {
                log::debug!(
                    "Play Console {stage} redirect destination: {}",
                    diagnostic_url(&url)
                );
                for (name, value) in url.query_pairs() {
                    if matches!(name.as_ref(), "continue" | "redirect_uri" | "returnUrl") {
                        if let Ok(return_url) = Url::parse(&value) {
                            log::debug!(
                                "Play Console {stage} redirect return destination: {}",
                                diagnostic_url(&return_url)
                            );
                        }
                    }
                }
                if url.host_str() == Some("accounts.google.com") {
                    log::debug!(
                        "Play Console redirected to Google sign-in. document.cookie excludes HttpOnly cookies, so a valid signing cookie alone may not provide the full authenticated browser session."
                    );
                }
            }
            None => log::debug!("Play Console {stage} redirect location is not a valid URL"),
        }
    }
    log::debug!(
        "Play Console {stage} response authentication metadata: challenge present={}, Set-Cookie headers={}",
        response
            .headers()
            .contains_key(reqwest::header::WWW_AUTHENTICATE),
        response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .count()
    );
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
        .map_err(|error| {
            log::debug!(
                "Could not read Play Console {stage} response: kind={:?}",
                error.kind()
            );
            "Could not read the Play Console response.".to_owned()
        })?;
    log::debug!("Read Play Console {stage} response: {} bytes", bytes.len());
    if bytes.len() as u64 > MAX_RESPONSE {
        return Err("The Play Console response is too large to sync.".into());
    }
    Ok(bytes)
}

// Only expose known route segments. Unknown segments can contain account IDs or tokens.
// Query values, fragments, and userinfo never appear in diagnostics.
fn diagnostic_url(url: &Url) -> String {
    let path = url
        .path()
        .split('/')
        .map(|segment| match segment {
            "" | "console" | "u" | "developers" | "app-list" | "ServiceLogin"
            | "InteractiveLogin" | "signin" | "v2" | "v3" | "identifier" | "challenge" | "pwd"
            | "oauth" | "oauth2" | "auth" | "AccountChooser" => segment,
            _ => "[redacted]",
        })
        .collect::<Vec<_>>()
        .join("/");
    let parameters = url
        .query_pairs()
        .map(|(name, _)| {
            // Keep arbitrary server input on one log line and bound its size.
            name.chars()
                .filter(|character| character.is_ascii_alphanumeric() || *character == '_')
                .take(64)
                .collect::<String>()
        })
        .take(32)
        .collect::<Vec<_>>();
    format!(
        "{}{path} (query parameter names: {parameters:?})",
        url.origin().ascii_serialization()
    )
}

fn console_setup(html: &str) -> Result<Value, String> {
    let marker = "window.serializedInitialChunks['startupData']";
    log::debug!(
        "Play Console startup data marker present={}",
        html.contains(marker)
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_URL: &str = "https://play.google.com/console/u/1/developers/123456/app-list";

    #[test]
    fn signs_the_millisecond_timestamp_in_the_header() {
        // Independently computed SHA-1 of the captured format with a synthetic cookie.
        let header = authorization("test-signing-cookie", 1790935325102).unwrap();
        assert_eq!(
            header.to_str().unwrap(),
            "SAPISIDHASH 1790935325102_f668cea14c43f77854a9074999b72f11194567c0"
        );
        assert!(header.is_sensitive());
    }

    #[test]
    fn accepts_full_cookie_headers_and_preserves_values() {
        for input in [
            "SID=test-session; SAPISID=test-signing-cookie; S=sso=one:billing=two",
            "Cookie: SID=test-session; SAPISID=test-signing-cookie; S=sso=one:billing=two",
            "cookie: SID=test-session; SAPISID=test-signing-cookie; S=sso=one:billing=two",
        ] {
            let connection = Connection::parse(TEST_URL, input).unwrap();
            assert_eq!(connection.auth_user, "1");
            assert_eq!(connection.signing_cookie, "test-signing-cookie");
            assert_eq!(
                connection.cookies.to_str().unwrap(),
                "SID=test-session; SAPISID=test-signing-cookie; S=sso=one:billing=two"
            );
            assert!(connection.cookies.is_sensitive());
        }
    }

    #[test]
    fn incomplete_session_errors_point_to_network_headers_without_leaking_values() {
        for input in ["SAPISID=do-not-leak", "SID=; SAPISID=do-not-leak"] {
            let error = Connection::parse(TEST_URL, input).err().unwrap();
            assert!(error.contains("Network"));
            assert!(!error.contains("do-not-leak"));
        }
        assert!(
            Connection::parse(
                TEST_URL,
                "__Secure-3PSID=test-session; __Secure-3PAPISID=test-signing-cookie"
            )
            .is_ok()
        );
    }

    #[test]
    fn fetch_sends_complete_session_and_millisecond_signatures() {
        use std::{io::Write, net::TcpListener, thread};

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let api_key = format!("AIza{}", "x".repeat(35));
        let setup = serde_json::json!({"1": {"8": api_key, "27": "test-session-id"}});
        let html = format!(
            "window.serializedInitialChunks['startupData'] = {};",
            serde_json::to_string(&setup.to_string()).unwrap()
        );
        let server = thread::spawn(move || {
            for (index, body) in [
                html,
                r#"{"1":[{"2":"First","5":"example.first"}],"2":"next-page"}"#.into(),
                r#"{"1":[{"2":"Second","5":"example.second"}]}"#.into(),
            ]
            .into_iter()
            .enumerate()
            {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                let headers = request.to_ascii_lowercase();
                assert!(
                    headers.contains("cookie: sid=test-session; sapisid=test-signing-cookie\r\n")
                );
                if index > 0 {
                    assert!(headers.contains("x-goog-authuser: 1\r\n"));
                    assert!(headers.contains("x-play-console-session-id: test-session-id\r\n"));
                    assert!(headers.contains(&format!(
                        "x-goog-api-key: {}\r\n",
                        api_key.to_ascii_lowercase()
                    )));
                    let auth = request
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("authorization")
                                .then_some(value.trim())
                        })
                        .unwrap();
                    let timestamp = auth
                        .strip_prefix("SAPISIDHASH ")
                        .unwrap()
                        .split_once('_')
                        .unwrap()
                        .0
                        .parse::<u128>()
                        .unwrap();
                    assert!(timestamp.abs_diff(unix_milliseconds().unwrap()) < 5_000);
                    assert_eq!(
                        auth,
                        authorization("test-signing-cookie", timestamp)
                            .unwrap()
                            .to_str()
                            .unwrap()
                    );
                    assert!(request.contains("fetchGamingPlatform=true&pageSize=500"));
                    assert_eq!(request.contains("pageToken=next-page"), index == 2);
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let connection =
            Connection::parse(TEST_URL, "SID=test-session; SAPISID=test-signing-cookie").unwrap();
        let http = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let snapshot = connection
            .fetch_with_client(
                &http,
                &format!("{base}/startup"),
                &format!("{base}/appSummaries"),
                &AtomicBool::new(false),
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(snapshot.developer_id, "123456");
        assert_eq!(snapshot.apps.len(), 2);
        assert_eq!(snapshot.apps[1].package_name, "example.second");
    }

    #[test]
    fn redirect_diagnostics_exclude_credentials_tokens_and_account_ids() {
        let url = Url::parse(
            "https://user:password@accounts.google.com/v3/signin/challenge/private-token?continue=secret-return-url&token=secret-query#secret-fragment",
        )
        .unwrap();
        assert_eq!(
            diagnostic_url(&url),
            "https://accounts.google.com/v3/signin/challenge/[redacted] (query parameter names: [\"continue\", \"token\"])"
        );
        let url =
            Url::parse("https://play.google.com/console/u/0/developers/123456/app-list").unwrap();
        assert_eq!(
            diagnostic_url(&url),
            "https://play.google.com/console/u/[redacted]/developers/[redacted]/app-list (query parameter names: [])"
        );
    }
}
