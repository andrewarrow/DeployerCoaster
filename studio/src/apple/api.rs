use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

use parsequerade::{Client, Query};
use reqwest::{Method, Url};
use serde_json::{Value, json};

use super::AppleCredentials;

const MAX_PAGES: usize = 100;

pub(crate) trait Transport {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String>;
}

// Existing UI jobs run on worker threads. Drive the async SDK there, keeping one
// client/runtime for the whole job so JWTs, HTTP pooling and page cursors are reused.
pub(crate) struct ParsequeradeApi {
    client: Client,
    runtime: tokio::runtime::Runtime,
}

fn sdk_client(credentials: &AppleCredentials) -> Result<Client, String> {
    Client::new(
        &credentials.issuer_id,
        &credentials.key_id,
        &credentials.private_key,
    )
    .map_err(|error| api_error(error.status, &json!({"error": error})))
}

pub(super) fn validate_connection(credentials: &AppleCredentials) -> Result<(), String> {
    sdk_client(credentials).map(|_| ())
}

// Validate locally; no Apple key is uploaded to the hosted facade.
pub(super) fn create_connection(
    issuer: &str,
    key_id: &str,
    private_key: &str,
) -> Result<AppleCredentials, String> {
    let credentials = AppleCredentials {
        issuer_id: issuer.trim().into(),
        key_id: key_id.trim().into(),
        private_key: private_key.into(),
    };
    validate_connection(&credentials)?;
    Ok(credentials)
}

impl ParsequeradeApi {
    pub fn new(credentials: AppleCredentials) -> Result<Self, String> {
        Ok(Self {
            client: sdk_client(&credentials)?,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "Could not initialize the Apple connection.".to_owned())?,
        })
    }
}

// This URL is only a parser for relative facade paths; it is never requested.
fn resource_url(path: &str) -> Result<Url, String> {
    let raw_path = path.split('?').next().unwrap_or_default();
    if !raw_path.starts_with('/') || raw_path.starts_with("//") || path.contains('#') {
        return Err("Invalid Parsequerade resource path.".into());
    }
    for segment in raw_path[1..].split('/') {
        validate_id(segment)?;
    }
    Url::parse(&format!("https://local.invalid{path}"))
        .map_err(|_| "Invalid Parsequerade resource path.".to_owned())
}

impl Transport for ParsequeradeApi {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let url = resource_url(path)?;
        let result = if method == Method::GET && body.is_none() {
            let query = url.query_pairs().fold(Query::new(), |query, (key, value)| {
                query.with(key.into_owned(), value.into_owned())
            });
            self.runtime
                .block_on(self.client.get::<Value>(url.path(), &query))
        } else if method == Method::POST && body.is_some() && is_age_rating_path(path) {
            let parts: Vec<_> = path.split('/').collect();
            let body = body.expect("checked above");
            self.runtime
                .block_on(self.client.update_age_rating(parts[2], parts[4], body))
                .map(|rating| age_rating_response(rating, body))
        } else if method == Method::POST && body.is_some() && is_version_collection_path(path) {
            let body = body.expect("checked above");
            let attributes = body.as_object().ok_or("Invalid version attributes.")?;
            if attributes.len() != 2 {
                return Err(
                    "Only platform and versionString are accepted for new versions.".into(),
                );
            }
            let app_id = path.split('/').nth(2).expect("validated path");
            self.runtime.block_on(self.client.create_version(
                app_id,
                body["platform"].as_str().unwrap_or_default(),
                body["versionString"].as_str().unwrap_or_default(),
            ))
        } else if method == Method::PATCH && body.is_some() && url.query().is_none() {
            self.runtime.block_on(
                self.client
                    .update_metadata(url.path(), body.expect("checked above")),
            )
        } else {
            return Err("This write is not supported through Parsequerade.".into());
        };
        // Keep status-aware consumers (especially optional 404s) and their test
        // transports intact. SDK errors already exclude private upstream details.
        match result {
            Ok(value) => Ok((200, value)),
            Err(error) => {
                log::debug!(target: "parsequerade", "{method} {}: {}", url.path(), error.code);
                Ok((error.status, json!({"error": error})))
            }
        }
    }
}

fn is_version_collection_path(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 4
        && parts[0].is_empty()
        && parts[1] == "apps"
        && validate_id(parts[2]).is_ok()
        && parts[3] == "versions"
}

fn age_rating_response(rating: parsequerade::AgeRating, attributes: &Value) -> Value {
    // Typed SDK responses omit None. Retain submitted nulls so clearing an
    // override also clears the displayed value when the UI merges this response.
    let mut value = attributes.clone();
    value
        .as_object_mut()
        .expect("SDK validated attributes")
        .extend(
            serde_json::to_value(rating)
                .expect("serializable age rating")
                .as_object()
                .expect("age rating object")
                .clone(),
        );
    value
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

pub(crate) fn api_error(status: u16, value: &Value) -> String {
    let code = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match (status, code) {
        (_, "write_outcome_unknown") => "The paste result could not be confirmed. Refresh Age Rating before trying again.",
        (_, "invalid_credentials") | (_, "upstream_authentication_failed") => "Apple rejected the API key credentials. Check the Issuer ID, Key ID, and .p8 key, then reconnect in Apple settings.",
        (_, "invalid_cursor") => {
            "This page cursor expired. Refresh to load the latest information."
        }
        (400, _) => {
            "Parsequerade rejected the request. Check your Apple key and settings, then retry."
        }
        (401, _) => "Apple could not verify this API key. Reconnect in Apple settings.",
        (403, _) => "Access was denied. Check your Apple key's App Store Connect permissions.",
        (404, _) => "This information is unavailable through Parsequerade.",
        (429, _) => "The request limit was reached. Wait a moment and try again.",
        (500..=599, _) => {
            "App Store Connect is temporarily unavailable. Please retry."
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
    let original = resource_url(path)?;
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
