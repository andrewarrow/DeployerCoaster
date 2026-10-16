use super::*;
use crate::apple::api::{self, Transport};
use reqwest::{Method, Url};
use serde_json::{Value, json};

const UNCONFIRMED: &str =
    "Build attachment could not be confirmed. Refresh Builds before trying again.";

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    app_id: String,
    version_id: String,
}

struct Available {
    items: Vec<Value>,
    attached: Option<String>,
}

struct Attachment {
    scope: Scope,
    result: Receiver<Result<String, String>>,
}

#[derive(Default)]
pub(super) struct Builds {
    scope: Option<Scope>,
    data: Option<Available>,
    error: Option<String>,
    load: Option<Job<Available>>,
    attachment: Option<Attachment>,
    feedback: Option<(Scope, Result<String, String>)>,
}

impl Builds {
    pub(super) fn clear(&mut self) {
        self.load = None;
        self.scope = None;
        self.data = None;
        self.error = None;
    }

    pub(super) fn attaching(&self, app_id: &str) -> bool {
        self.attachment
            .as_ref()
            .is_some_and(|job| job.scope.app_id == app_id)
    }

    pub(super) fn poll(&mut self, cache: &mut HashMap<String, Entry>) {
        if let Some(load) = &self.load {
            let result = match load.result.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Build loading stopped. Please refresh again.".into()))
                }
            };
            if let Some(result) = result {
                self.load = None;
                match result {
                    Ok(data) => {
                        self.data = Some(data);
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        let Some(attachment) = &self.attachment else {
            return;
        };
        let result = match attachment.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(UNCONFIRMED.into()),
        };
        let scope = self.attachment.take().unwrap().scope;
        if let Ok(build_id) = &result {
            if let Some(details) = cache
                .get_mut(&scope.app_id)
                .and_then(|entry| entry.data.as_mut())
            {
                for version in details
                    .version
                    .iter_mut()
                    .chain(details.versions.iter_mut())
                {
                    if attr(version, "id") == scope.version_id {
                        version["buildId"] = build_id.clone().into();
                    }
                }
            }
            if self.scope.as_ref() == Some(&scope)
                && let Some(data) = &mut self.data
            {
                data.attached = Some(build_id.clone());
            }
        } else if self.scope.as_ref() == Some(&scope) {
            // Require a fresh read after any failed write, including uncertain outcomes.
            self.data = None;
            self.error = result.as_ref().err().cloned();
        }
        self.feedback = Some((scope, result.map(|_| "Build attached.".into())));
    }

    pub(super) fn ui(
        &mut self,
        ui: &mut Ui,
        details: &AppDetails,
        credentials: &AppleCredentials,
        palette: Palette,
        loading: bool,
    ) {
        let version = details.version.as_ref();
        let scope = Scope {
            app_id: attr(&details.app, "id").into(),
            version_id: version.map(|v| attr(v, "id")).unwrap_or_default().into(),
        };
        if self.scope.as_ref() != Some(&scope) {
            self.clear();
            self.scope = Some(scope.clone());
        }
        if self.data.is_none() && self.error.is_none() && self.load.is_none() && !loading {
            self.start_load(details, credentials.clone(), ui.ctx());
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::style::semibold("Builds"));
            if ui
                .add_enabled(
                    !loading && self.load.is_none() && self.attachment.is_none(),
                    egui::Button::new("Refresh"),
                )
                .clicked()
            {
                self.start_load(details, credentials.clone(), ui.ctx());
            }
        });
        if let Some(version) = version {
            ui.add(
                egui::Label::new(format!(
                    "iOS {} · {}",
                    attr(version, "versionString"),
                    documents::readable(details::state(version))
                ))
                .wrap(),
            );
        }
        let editable = version.is_some_and(|v| details::state(v) == "PREPARE_FOR_SUBMISSION");
        if !editable {
            ui.add(
                egui::Label::new(
                    RichText::new("Select a Prepare for Submission version to attach a build.")
                        .color(palette.muted),
                )
                .wrap(),
            );
        }
        ui.add_space(12.0);
        if self.load.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading builds…");
            });
        }
        if self.attaching(&scope.app_id) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Attaching build…");
            });
        } else if let Some(error) = &self.error {
            ui.add(egui::Label::new(RichText::new(error).color(palette.error)).wrap());
        } else if let Some((feedback_scope, result)) = &self.feedback
            && feedback_scope == &scope
        {
            let (message, color) = match result {
                Ok(message) => (message, palette.muted),
                Err(message) => (message, palette.error),
            };
            ui.add(egui::Label::new(RichText::new(message).color(color)).wrap());
        }
        let Some(data) = &self.data else { return };
        if data.items.is_empty() && self.load.is_none() {
            ui.label("No builds are available for this iOS version.");
        }
        let mut attach_id = None;
        for build in &data.items {
            let build_id = attr(build, "id");
            let attached = data.attached.as_deref() == Some(build_id);
            let unavailable = unavailable_reason(build);
            ui.push_id(build_id, |ui| {
                ui.horizontal_top(|ui| {
                    let width = (ui.available_width() - 90.0).max(20.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let number = attr(build, "version");
                            ui.add(
                                egui::Label::new(crate::style::semibold(format!(
                                    "Build {}",
                                    if number.is_empty() { build_id } else { number }
                                )))
                                .wrap(),
                            );
                            let status = unavailable.unwrap_or("Ready to attach");
                            ui.add(
                                egui::Label::new(RichText::new(status).color(palette.muted)).wrap(),
                            );
                            for (label, field) in
                                [("Uploaded", "uploadedDate"), ("Expires", "expirationDate")]
                            {
                                let date = attr(build, field);
                                if !date.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!("{label} {date}"))
                                                .size(12.0)
                                                .color(palette.muted),
                                        )
                                        .wrap(),
                                    );
                                }
                            }
                        },
                    );
                    if ui
                        .add_enabled(
                            editable
                                && !loading
                                && self.load.is_none()
                                && self.attachment.is_none()
                                && self.error.is_none()
                                && unavailable.is_none()
                                && !attached,
                            egui::Button::new(if attached { "Attached" } else { "Attach" })
                                .min_size(egui::vec2(76.0, 44.0)),
                        )
                        .clicked()
                    {
                        attach_id = Some(build_id.to_owned());
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
            });
        }
        if let Some(build_id) = attach_id {
            self.start_attach(scope, build_id, credentials.clone(), ui.ctx());
        }
    }

    fn start_load(
        &mut self,
        details: &AppDetails,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
        let app_id = attr(&details.app, "id").to_owned();
        let version = details.version.clone();
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.load = Some(Job {
            result,
            cancelled: cancelled.clone(),
        });
        self.error = None;
        self.feedback = None;
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ParsequeradeApi::new(credentials)
                .and_then(|mut api| load(&mut api, &app_id, version.as_ref(), &cancelled));
            if !cancelled.load(Ordering::Relaxed) {
                let _ = sender.send(result);
                ctx.request_repaint();
            }
        });
    }

    fn start_attach(
        &mut self,
        scope: Scope,
        build_id: String,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
        let (sender, result) = mpsc::channel();
        self.attachment = Some(Attachment {
            scope: scope.clone(),
            result,
        });
        self.feedback = None;
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ParsequeradeApi::new(credentials)
                .and_then(|mut api| attach(&mut api, &scope.app_id, &scope.version_id, &build_id));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
}

fn unavailable_reason(build: &Value) -> Option<&'static str> {
    if build["expired"] == true {
        return Some("Expired");
    }
    if build["buildAudienceType"] == "INTERNAL_ONLY" {
        return Some("Internal testing only");
    }
    match attr(build, "processingState") {
        "VALID" if build["expired"] == false => None,
        "PROCESSING" => Some("Processing"),
        "FAILED" => Some("Processing failed"),
        "INVALID" => Some("Invalid build"),
        _ => Some("Build availability unknown"),
    }
}

fn load<T: Transport>(
    api: &mut T,
    app_id: &str,
    version: Option<&Value>,
    cancelled: &AtomicBool,
) -> Result<Available, String> {
    api::validate_id(app_id)?;
    let mut url = Url::parse(&format!("https://local.invalid/apps/{app_id}/builds")).unwrap();
    url.query_pairs_mut().extend_pairs([
        ("platform", "IOS"),
        ("sort", "-uploadedDate"),
        ("limit", "200"),
    ]);
    if let Some(version) = version {
        api::id(version)?;
        let number = attr(version, "versionString");
        if number.is_empty() {
            return Err("The selected version has no version number. Refresh versions.".into());
        }
        url.query_pairs_mut()
            .append_pair("prereleaseVersion", number);
    }
    let items = api::list(
        api,
        &format!("{}?{}", url.path(), url.query().unwrap()),
        cancelled,
    )?;
    if items
        .iter()
        .any(|build| build.get("appId").is_some_and(|owner| owner != app_id))
    {
        return Err("Parsequerade returned builds for another app.".into());
    }
    let attached = if let Some(version) = version {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Build loading cancelled.".into());
        }
        let (status, value) = api.send(
            Method::GET,
            &format!("/apps/{app_id}/versions/{}/build", api::id(version)?),
            None,
        )?;
        match status {
            200..=299 => {
                if value.get("appId").is_some_and(|owner| owner != app_id) {
                    return Err("Parsequerade returned a build for another app.".into());
                }
                Some(api::id(&value)?.to_owned())
            }
            404 => None,
            _ => return Err(api::api_error(status, &value)),
        }
    } else {
        None
    };
    Ok(Available { items, attached })
}

fn attach<T: Transport>(
    api: &mut T,
    app_id: &str,
    version_id: &str,
    build_id: &str,
) -> Result<String, String> {
    for id in [app_id, version_id, build_id] {
        api::validate_id(id)?;
    }
    let (status, version) = api.send(
        Method::PATCH,
        &format!("/apps/{app_id}/versions/{version_id}/build"),
        Some(&json!({"buildId": build_id})),
    )?;
    if !(200..300).contains(&status) {
        return Err(match version.pointer("/error/code").and_then(Value::as_str) {
            Some("write_outcome_unknown") => UNCONFIRMED.into(),
            Some("version_not_editable") => "The selected version is no longer in Prepare for Submission. Refresh versions.".into(),
            Some("build_version_mismatch") => "This build does not match the selected version and platform.".into(),
            Some("build_not_available") => "This build is not available for App Store submission. Refresh Builds.".into(),
            _ if status == 409 || status == 422 => "Apple rejected this build attachment. Check the build’s eligibility and export compliance, then refresh Builds.".into(),
            _ => api::api_error(status, &version),
        });
    }
    if attr(&version, "id") != version_id
        || attr(&version, "buildId") != build_id
        || attr(&version, "appId") != app_id
    {
        return Err(UNCONFIRMED.into());
    }
    Ok(build_id.into())
}

#[cfg(test)]
mod tests;
