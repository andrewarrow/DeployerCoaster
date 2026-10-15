use std::{
    collections::HashSet,
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use reqwest::{Method, Url, blocking::Client};
use serde_json::{Value, json};

use super::AppleCredentials;

const SERVICE: &str = "https://parsequerade.mainbrella.com";
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
const MAX_PAGES: usize = 100;

pub(crate) trait Transport {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String>;
}

pub(crate) struct ParsequeradeApi {
    http: Client,
    credentials: AppleCredentials,
}

fn client() -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|_| "Could not initialize the Parsequerade connection.".to_owned())
}

pub(super) fn validate_connection(credentials: &AppleCredentials) -> Result<(), String> {
    validate_id(&credentials.connection_id)?;
    let expected = format!("{SERVICE}/v1/connections/{}", credentials.connection_id);
    if credentials.base_url != expected
        || credentials.api_key.is_empty()
        || !credentials
            .api_key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(
            "The saved Parsequerade connection is invalid. Reconnect in Apple settings.".into(),
        );
    }
    Ok(())
}

pub(super) fn create_connection(
    issuer: &str,
    key_id: &str,
    private_key: &str,
) -> Result<AppleCredentials, String> {
    create_connection_with(
        &client()?,
        &format!("{SERVICE}/v1/connections"),
        issuer,
        key_id,
        private_key,
    )
}

fn create_connection_with(
    http: &Client,
    endpoint: &str,
    issuer: &str,
    key_id: &str,
    private_key: &str,
) -> Result<AppleCredentials, String> {
    let response = http.post(endpoint).json(&json!({
        "issuerId": issuer, "keyId": key_id, "privateKey": private_key,
        "capabilities": ["read", "write"]
    })).send().map_err(|_| "Could not reach Parsequerade to connect your Apple key. Check your connection and retry.".to_owned())?;
    let status = response.status().as_u16();
    let value = read_response(response)?;
    if status != 201 {
        return Err(api_error(status, &value));
    }
    let mut credentials: AppleCredentials = serde_json::from_value(value)
        .map_err(|_| "Parsequerade returned an invalid connection. Please retry.".to_owned())?;
    validate_connection(&credentials)?;
    use sha2::{Digest, Sha256};
    credentials.account_id = format!("{:x}", Sha256::digest(issuer.as_bytes()));
    Ok(credentials)
}

impl ParsequeradeApi {
    pub fn new(credentials: AppleCredentials) -> Result<Self, String> {
        validate_connection(&credentials)?;
        Ok(Self {
            credentials,
            http: client()?,
        })
    }
}

// Only resource paths relative to this connection are accepted; cursors are always query data.
fn resource_url(base: &str, path: &str) -> Result<Url, String> {
    let raw_path = path.split('?').next().unwrap_or_default();
    if !raw_path.starts_with('/') || raw_path.starts_with("//") || path.contains('#') {
        return Err("Invalid Parsequerade resource path.".into());
    }
    for segment in raw_path[1..].split('/') {
        validate_id(segment)?;
    }
    Url::parse(&format!("{base}{path}"))
        .map_err(|_| "Invalid Parsequerade resource path.".to_owned())
}

impl Transport for ParsequeradeApi {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let age_write = method == Method::POST && is_age_rating_path(path) && body.is_some();
        if !age_write && (method != Method::GET || body.is_some()) {
            return Err(
                "Only age-rating paste is supported for writes through Parsequerade.".into(),
            );
        }
        let url = resource_url(&self.credentials.base_url, path)?;
        log::debug!(target: "parsequerade", "{method} {path}");
        let mut request = self
            .http
            .request(method.clone(), url)
            .bearer_auth(&self.credentials.api_key)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().map_err(|_| {
            log::debug!(target: "parsequerade", "{method} {path}: request failed");
            if age_write {
                "The paste result could not be confirmed. Refresh Age Rating before trying again."
                    .to_owned()
            } else {
                "Could not reach Parsequerade. Check your connection and try again.".to_owned()
            }
        })?;
        let status = response.status().as_u16();
        let value = read_response(response).map_err(|error| {
            log::debug!(target: "parsequerade", "{method} {path} -> HTTP {status}: {error}");
            if age_write {
                "The paste result could not be confirmed. Refresh Age Rating before trying again."
                    .to_owned()
            } else {
                error
            }
        })?;
        log::debug!(target: "parsequerade", "{method} {path} -> HTTP {status}");
        Ok((status, value))
    }
}

fn is_age_rating_path(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 6
        && parts[0].is_empty()
        && parts[1] == "apps"
        && validate_id(parts[2]).is_ok()
        && parts[3] == "app-infos"
        && validate_id(parts[4]).is_ok()
        && parts[5] == "age-rating"
}

fn read_response(response: reqwest::blocking::Response) -> Result<Value, String> {
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Parsequerade's response. Try again.".to_owned())?;
    if bytes.len() as u64 > MAX_RESPONSE {
        return Err("Parsequerade's response exceeded the supported size limit.".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| "Parsequerade returned an unreadable response. Try again.".into())
}

pub(crate) fn api_error(status: u16, value: &Value) -> String {
    let code = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match (status, code) {
        (_, "write_forbidden") => "This connection is read-only. Replace the connection in Apple settings to enable age-rating paste.",
        (_, "write_outcome_unknown") => "The paste result could not be confirmed. Refresh Age Rating before trying again.",
        (_, "invalid_credentials") | (_, "upstream_authentication_failed") => "Apple rejected the connection credentials. Check the Issuer ID, Key ID, and .p8 key, then reconnect in Apple settings.",
        (_, "invalid_cursor") => {
            "This page cursor expired. Refresh to load the latest information."
        }
        (400, _) => {
            "Parsequerade rejected the request. Check your Apple key and settings, then retry."
        }
        (401, _) => "Parsequerade could not verify this connection. Reconnect in Apple settings.",
        (403, _) => "Access was denied. Check your Apple key's App Store Connect permissions.",
        (404, _) => "This information is unavailable through Parsequerade.",
        (429, _) => "The request limit was reached. Wait a moment and try again.",
        (500..=599, _) => {
            "Parsequerade or App Store Connect is temporarily unavailable. Please retry."
        }
        _ => "Parsequerade could not complete the request. Please retry.",
    }
    .into()
}

pub(crate) fn list<T: Transport>(
    transport: &mut T,
    path: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<Value>, String> {
    let original = resource_url(SERVICE, path)?;
    let mut next = path.to_owned();
    let mut cursors = HashSet::new();
    let mut resources = Vec::new();
    for _ in 0..MAX_PAGES {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Loading cancelled.".into());
        }
        let (status, value) = transport.send(Method::GET, &next, None)?;
        if !(200..300).contains(&status) {
            return Err(api_error(status, &value));
        }
        let items = value["items"]
            .as_array()
            .ok_or("Parsequerade returned an invalid list.")?;
        for item in items {
            id(item)?;
        }
        resources.extend(items.iter().cloned());
        match value.get("nextCursor") {
            Some(Value::Null) => return Ok(resources),
            Some(Value::String(cursor)) if !cursor.is_empty() && cursor.len() <= 8192 => {
                if !cursors.insert(cursor.clone()) {
                    return Err("Parsequerade returned a repeated page. Please refresh.".into());
                }
                let mut url = original.clone();
                url.query_pairs_mut().append_pair("cursor", cursor);
                next = format!("{}?{}", url.path(), url.query().unwrap_or_default());
            }
            _ => return Err("Parsequerade returned an invalid page cursor.".into()),
        }
    }
    Err("Parsequerade returned too many pages. Please refresh.".into())
}

pub(crate) fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Err("Parsequerade returned an invalid resource ID.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn id(resource: &Value) -> Result<&str, String> {
    let value = resource["id"].as_str().unwrap_or_default();
    validate_id(value)?;
    Ok(value)
}

#[cfg(test)]
mod tests;
