use std::time::Duration;

use reqwest::{Method, Url, blocking::Client};
use serde_json::Value;

use super::{AppleCredentials, generate_token};

const BASE: &str = "https://api.appstoreconnect.apple.com/";

pub(crate) trait Transport {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String>;
}

pub(crate) struct AppleApi {
    http: Client,
    credentials: AppleCredentials,
}

impl AppleApi {
    pub fn new(credentials: AppleCredentials) -> Result<Self, String> {
        Ok(Self {
            credentials,
            http: Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|_| "Could not initialize the Apple connection.".to_owned())?,
        })
    }
}

impl Transport for AppleApi {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let read_only = method == Method::GET;
        let url = checked_url(path)?;
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(generate_token(&self.credentials)?)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().map_err(|_| if read_only { "Could not reach App Store Connect. Check your connection and try again.".to_owned() } else { "Could not reach App Store Connect. Check your connection. A timed-out write may still have been applied; inspect the destination before pasting again.".to_owned() })?;
        let status = response.status().as_u16();
        let bytes = response.bytes().map_err(|_| {
            if read_only {
                "Could not read Apple's response. Try again.".to_owned()
            } else {
                "Could not read Apple's response. Check the destination before pasting again."
                    .to_owned()
            }
        })?;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| if read_only { "Apple returned an unreadable response. Try again.".to_owned() } else { "Apple returned an unreadable response. Check the destination before pasting again.".to_owned() })?
        };
        Ok((status, value))
    }
}

pub(crate) fn checked_url(path: &str) -> Result<Url, String> {
    let base = Url::parse(BASE).unwrap();
    let url = base
        .join(path)
        .map_err(|_| "Apple returned an invalid URL.".to_owned())?;
    if url.origin() != base.origin()
        || !url.path().starts_with("/v1/")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Apple returned an unexpected URL.".into());
    }
    Ok(url)
}

pub(crate) fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Err("Apple returned an invalid resource ID.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn id(resource: &Value) -> Result<&str, String> {
    let value = resource["id"].as_str().unwrap_or_default();
    validate_id(value)?;
    Ok(value)
}
