use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::Method;
use serde_json::Value;

use super::api::{self, Transport, id, validate_id};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct AppDetails {
    pub app: Value,
    pub info: Option<Value>,
    pub version: Option<Value>,
    pub versions: Vec<Value>,
    pub info_localizations: Vec<Value>,
    pub version_localizations: Vec<Value>,
    pub warnings: Vec<String>,
}

pub(crate) fn attr<'a>(resource: &'a Value, field: &str) -> &'a str {
    resource[field].as_str().unwrap_or_default()
}

pub(crate) fn state(resource: &Value) -> &str {
    ["state", "appVersionState", "appStoreState"]
        .into_iter()
        .map(|field| attr(resource, field))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn preference_key(resource: &Value) -> (u8, std::cmp::Reverse<&str>) {
    // Prefer editable metadata, then the live listing, then other states.
    let priority = match state(resource) {
        "PREPARE_FOR_SUBMISSION" => 0,
        "READY_FOR_DISTRIBUTION" | "READY_FOR_SALE" => 1,
        "REPLACED_WITH_NEW_VERSION" | "REPLACED_WITH_NEW_INFO" => 3,
        _ => 2,
    };
    (priority, std::cmp::Reverse(attr(resource, "createdDate")))
}

fn preferred(resources: &[Value]) -> Option<&Value> {
    resources
        .iter()
        .min_by_key(|resource| preference_key(resource))
}

fn sort_localizations(mut resources: Vec<Value>) -> Vec<Value> {
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
        let (status, value) = self.transport.send(Method::GET, path, None)?;
        if !(200..300).contains(&status) {
            return Err(api::api_error(status, &value));
        }
        Ok(value)
    }

    fn list(&mut self, path: &str) -> Result<Vec<Value>, String> {
        api::list(&mut self.transport, path, self.cancelled)
    }
}

pub(crate) fn load<T: Transport>(
    transport: T,
    app_id: &str,
    cancelled: &AtomicBool,
) -> Result<AppDetails, String> {
    load_for_version(transport, app_id, None, cancelled)
}

pub(crate) fn load_for_version<T: Transport>(
    transport: T,
    app_id: &str,
    version_id: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<AppDetails, String> {
    validate_id(app_id)?;
    if let Some(version_id) = version_id {
        validate_id(version_id)?;
    }
    let mut reader = Reader {
        transport,
        cancelled,
    };
    let response = reader.get(&format!("/apps/{app_id}"))?;
    let app = response;
    if !app.is_object() || id(&app)? != app_id {
        return Err("Parsequerade returned invalid app details.".into());
    }
    let mut details = AppDetails {
        app,
        ..Default::default()
    };
    match reader.list(&format!("/apps/{app_id}/app-infos?limit=200")) {
        Ok(infos) => {
            details.info = preferred(&infos).cloned();
            if let Some(info) = &details.info {
                let info_id = id(info)?;
                match reader.list(&format!(
                    "/apps/{app_id}/app-infos/{info_id}/localizations?limit=200"
                )) {
                    Ok(localizations) => {
                        details.info_localizations = sort_localizations(localizations)
                    }
                    Err(error) => details.warnings.push(format!("App localizations: {error}")),
                }
            }
        }
        Err(error) => details.warnings.push(format!("App information: {error}")),
    }
    match reader.list(&format!("/apps/{app_id}/versions?platform=IOS&limit=200")) {
        Ok(versions) => {
            let mut versions: Vec<_> = versions
                .into_iter()
                .filter(|version| attr(version, "platform") == "IOS")
                .collect();
            versions.sort_by(|a, b| preference_key(a).cmp(&preference_key(b)));
            details.versions = versions;
            details.version = match version_id {
                Some(requested_id) => details
                    .versions
                    .iter()
                    .find(|version| attr(version, "id") == requested_id)
                    .cloned()
                    .ok_or_else(|| {
                        format!("Requested iOS version {requested_id} is unavailable for this app.")
                    })
                    .map(Some)?,
                None => preferred(&details.versions).cloned(),
            };
            if let Some(version) = &details.version {
                let version_id = id(version)?;
                match reader.list(&format!(
                    "/apps/{app_id}/versions/{version_id}/localizations?limit=200"
                )) {
                    Ok(localizations) => {
                        details.version_localizations = sort_localizations(localizations)
                    }
                    Err(error) => details
                        .warnings
                        .push(format!("Version localizations: {error}")),
                }
            }
        }
        Err(error) => {
            if version_id.is_some() {
                return Err(format!("Could not load iOS versions: {error}"));
            }
            details.warnings.push(format!("iOS version: {error}"));
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("App detail loading cancelled.".into());
    }
    Ok(details)
}
