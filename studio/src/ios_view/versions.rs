use super::*;
use crate::apple::api::{self, Transport};
use reqwest::Method;
use serde_json::{Value, json};

pub(super) struct Creation {
    app_id: String,
    result: Receiver<Result<Created, String>>,
}

struct Created {
    version: Value,
    details: Result<AppDetails, String>,
}

pub(super) fn next_version(versions: &[Value]) -> Result<String, String> {
    let latest = versions
        .iter()
        .filter(|version| attr(version, "platform") == "IOS")
        .max_by_key(|version| {
            let numbers: Vec<_> = attr(version, "versionString")
                .split('.')
                .map(|part| part.parse::<u64>().unwrap_or_default())
                .collect();
            (attr(version, "createdDate"), numbers)
        })
        .ok_or("Load an existing iOS version before creating a new one.")?;
    let current = attr(latest, "versionString");
    if current
        .split('.')
        .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("The latest version does not have a numeric version number.".into());
    }
    let (prefix, last) = current.rsplit_once('.').unwrap_or(("", current));
    let next = last
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or("The latest version number cannot be incremented.")?;
    Ok(if prefix.is_empty() {
        next.to_string()
    } else {
        format!("{prefix}.{next}")
    })
}

fn create<T: Transport>(transport: &mut T, app_id: &str) -> Result<Value, String> {
    api::validate_id(app_id)?;
    // Fetch again at click time: the displayed selection may be an older release.
    let versions = api::list(
        transport,
        &format!("/apps/{app_id}/versions?platform=IOS&limit=200"),
        &AtomicBool::new(false),
    )?;
    let number = next_version(&versions)?;
    let (status, version) = transport.send(
        Method::POST,
        &format!("/apps/{app_id}/versions"),
        Some(&json!({"platform": "IOS", "versionString": number})),
    )?;
    if !(200..300).contains(&status) {
        return Err(match version.pointer("/error/code").and_then(Value::as_str) {
            Some("write_outcome_unknown") => "Version creation could not be confirmed. Refresh versions before trying again.".into(),
            _ if status == 409 => "Apple could not create this version. Check for an existing draft, then refresh versions.".into(),
            _ if status == 422 || status == 400 => "Apple rejected the new version. Check the current version and release status, then refresh versions.".into(),
            _ => api::api_error(status, &version),
        });
    }
    api::id(&version)?;
    if attr(&version, "platform") != "IOS"
        || attr(&version, "versionString") != number
        || details::state(&version) != "PREPARE_FOR_SUBMISSION"
        || version.get("appId").is_some_and(|id| id != app_id)
    {
        return Err(
            "Version creation could not be confirmed. Refresh versions before trying again.".into(),
        );
    }
    Ok(version)
}

impl IosView {
    pub(super) fn creating_version(&self, app_id: &str) -> bool {
        self.creation
            .as_ref()
            .is_some_and(|job| job.app_id == app_id)
    }

    pub(super) fn start_version(
        &mut self,
        app_id: &str,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
        if self.creation.is_some()
            || self.job.is_some()
            || self.age_copy.pasting(app_id)
            || self.metadata.saving(app_id)
        {
            return;
        }
        if let Some(entry) = self.cache.get_mut(app_id) {
            entry.error = None;
        }
        self.review_job = None;
        let (sender, result) = mpsc::channel();
        self.creation = Some(Creation {
            app_id: app_id.into(),
            result,
        });
        let app_id = app_id.to_owned();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ParsequeradeApi::new(credentials).and_then(|mut api| {
                let version = create(&mut api, &app_id)?;
                let details = details::load_for_version(
                    api,
                    &app_id,
                    Some(attr(&version, "id")),
                    &AtomicBool::new(false),
                );
                Ok(Created { version, details })
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub(super) fn poll_version(&mut self) {
        let Some(job) = &self.creation else { return };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(
                "Version creation could not be confirmed. Refresh versions before trying again."
                    .into(),
            ),
        };
        let app_id = self.creation.take().unwrap().app_id;
        let entry = self.cache.entry(app_id.clone()).or_default();
        match result {
            Ok(created) => {
                let details = match created.details {
                    Ok(details) => details,
                    Err(error) => {
                        // Creation succeeded even if a subsequent read failed. Select
                        // the confirmed draft and never retain the previous version's fields.
                        let mut details = entry.data.take().unwrap_or_default();
                        details.versions.insert(0, created.version.clone());
                        details.version = Some(created.version);
                        details.version_localizations.clear();
                        details.review_contact = None;
                        details.warnings.retain(|warning| {
                            !warning.starts_with("iOS version:")
                                && !warning.starts_with("Version localizations:")
                        });
                        details.warnings.push(format!("The new version was created, but its details could not be refreshed: {error}"));
                        details
                    }
                };
                self.workspaces
                    .entry(app_id.clone())
                    .or_default()
                    .reconcile(&details);
                entry.data = Some(details);
                entry.error = None;
                if self.selected.as_deref() == Some(&app_id) {
                    self.review_job = None;
                    self.media.clear();
                }
            }
            Err(error) => entry.error = Some(error),
        }
    }
}

#[cfg(test)]
mod tests;
