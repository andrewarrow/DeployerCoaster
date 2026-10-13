use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

use reqwest::Method;
use serde_json::Value;

use super::api::{Transport, checked_url, id, validate_id};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct AppDetails {
    pub app: Value,
    pub info: Option<Value>,
    pub version: Option<Value>,
    pub info_localizations: Vec<Value>,
    pub version_localizations: Vec<Value>,
    pub warnings: Vec<String>,
}

pub(crate) fn attr<'a>(resource: &'a Value, field: &str) -> &'a str {
    resource["attributes"][field].as_str().unwrap_or_default()
}

pub(crate) fn state(resource: &Value) -> &str {
    ["state", "appVersionState", "appStoreState"]
        .into_iter()
        .map(|field| attr(resource, field))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn preferred(resources: &[Value]) -> Option<&Value> {
    // Match Copy App: prefer editable metadata, then the live listing, then other states.
    resources.iter().min_by_key(|resource| {
        let priority = match state(resource) {
            "PREPARE_FOR_SUBMISSION" => 0,
            "READY_FOR_DISTRIBUTION" | "READY_FOR_SALE" => 1,
            "REPLACED_WITH_NEW_VERSION" | "REPLACED_WITH_NEW_INFO" => 3,
            _ => 2,
        };
        (priority, std::cmp::Reverse(attr(resource, "createdDate")))
    })
}

fn english(mut resources: Vec<Value>) -> Vec<Value> {
    resources.retain(|resource| {
        let locale = attr(resource, "locale");
        locale == "en" || locale.starts_with("en-")
    });
    resources.sort_by(|a, b| {
        let a = attr(a, "locale");
        let b = attr(b, "locale");
        (a != "en-US", a).cmp(&(b != "en-US", b))
    });
    resources
}

struct Reader<'a, T> {
    transport: T,
    cancelled: &'a AtomicBool,
}

impl<T: Transport> Reader<'_, T> {
    fn get(&mut self, path: &str) -> Result<Value, String> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err("App detail loading cancelled.".into());
        }
        checked_url(path)?;
        let (status, value) = self.transport.send(Method::GET, path, None)?;
        if !(200..300).contains(&status) {
            return Err(match status {
                401 => "Apple could not verify your API key. Check your Apple settings and system clock.".into(),
                403 => "Apple denied access. Check your API key's permissions in App Store Connect.".into(),
                404 => "This information is unavailable in App Store Connect.".into(),
                429 => "Apple is receiving too many requests. Wait a moment and refresh.".into(),
                _ => format!("App Store Connect could not load this information (HTTP {status}). Try again."),
            });
        }
        Ok(value)
    }

    fn list(&mut self, path: &str) -> Result<Vec<Value>, String> {
        let original = checked_url(path)?;
        let mut next = Some(path.to_owned());
        let mut pages = HashSet::new();
        let mut resources = Vec::new();
        while let Some(path) = next {
            let url = checked_url(&path)?;
            if url.path() != original.path() || !pages.insert(url.to_string()) {
                return Err("Apple returned an unexpected or repeated page.".into());
            }
            let value = self.get(&path)?;
            let data = value["data"]
                .as_array()
                .ok_or_else(|| "Apple returned an invalid list.".to_owned())?;
            for resource in data {
                id(resource)?;
            }
            resources.extend(data.iter().cloned());
            next = value
                .pointer("/links/next")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .map(str::to_owned);
        }
        Ok(resources)
    }
}

pub(crate) fn load<T: Transport>(
    transport: T,
    app_id: &str,
    cancelled: &AtomicBool,
) -> Result<AppDetails, String> {
    validate_id(app_id)?;
    let mut reader = Reader {
        transport,
        cancelled,
    };
    let response = reader.get(&format!("/v1/apps/{app_id}"))?;
    let app = response["data"].clone();
    if !app.is_object() || id(&app)? != app_id {
        return Err("Apple returned invalid app details.".into());
    }
    let mut details = AppDetails {
        app,
        ..Default::default()
    };
    match reader.list(&format!("/v1/apps/{app_id}/appInfos?limit=200")) {
        Ok(infos) => {
            details.info = preferred(&infos).cloned();
            if let Some(info) = &details.info {
                let info_id = id(info)?;
                match reader.list(&format!(
                    "/v1/appInfos/{info_id}/appInfoLocalizations?limit=200"
                )) {
                    Ok(localizations) => details.info_localizations = english(localizations),
                    Err(error) => details.warnings.push(format!("App localizations: {error}")),
                }
            }
        }
        Err(error) => details.warnings.push(format!("App information: {error}")),
    }
    match reader.list(&format!(
        "/v1/apps/{app_id}/appStoreVersions?filter[platform]=IOS&limit=200"
    )) {
        Ok(versions) => {
            let versions: Vec<_> = versions
                .into_iter()
                .filter(|version| attr(version, "platform") == "IOS")
                .collect();
            details.version = preferred(&versions).cloned();
            if let Some(version) = &details.version {
                let version_id = id(version)?;
                match reader.list(&format!(
                    "/v1/appStoreVersions/{version_id}/appStoreVersionLocalizations?limit=200"
                )) {
                    Ok(localizations) => details.version_localizations = english(localizations),
                    Err(error) => details
                        .warnings
                        .push(format!("Version localizations: {error}")),
                }
            }
        }
        Err(error) => details.warnings.push(format!("iOS version: {error}")),
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("App detail loading cancelled.".into());
    }
    Ok(details)
}
