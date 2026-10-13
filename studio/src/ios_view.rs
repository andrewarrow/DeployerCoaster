use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

use egui::{Label, RichText, Ui};
use serde_json::Value;

use crate::apple::{
    AppleCredentials,
    api::AppleApi,
    details::{self, AppDetails, attr, state},
};

#[cfg(test)]
mod tests;

struct Job {
    result: Receiver<Result<AppDetails, String>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Entry {
    data: Option<AppDetails>,
    error: Option<String>,
}

#[derive(Default)]
pub(crate) struct IosView {
    selected: Option<String>,
    cache: HashMap<String, Entry>,
    job: Option<Job>,
}

impl IosView {
    pub(crate) fn clear_selection(&mut self) {
        self.job = None;
        self.selected = None;
    }

    fn select(&mut self, app_id: &str) {
        if self.selected.as_deref() != Some(app_id) {
            self.job = None;
            self.selected = Some(app_id.to_owned());
        }
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("App detail loading stopped. Please refresh again.".into())
            }
        };
        if let Some(selected) = &self.selected {
            let entry = self.cache.entry(selected.clone()).or_default();
            match result {
                Ok(details) => {
                    entry.data = Some(details);
                    entry.error = None;
                }
                Err(error) => entry.error = Some(error),
            }
        }
        self.job = None;
    }

    fn start(&mut self, app_id: &str, credentials: AppleCredentials, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if let Some(entry) = self.cache.get_mut(app_id) {
            entry.error = None;
        }
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.job = Some(Job {
            result,
            cancelled: cancelled.clone(),
        });
        let app_id = app_id.to_owned();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result =
                AppleApi::new(credentials).and_then(|api| details::load(api, &app_id, &cancelled));
            if !cancelled.load(Ordering::Relaxed) {
                let _ = sender.send(result);
                ctx.request_repaint();
            }
        });
    }

    pub(crate) fn ui(
        &mut self,
        ui: &mut Ui,
        selection: Option<(&str, &str)>,
        credentials: Option<&AppleCredentials>,
    ) {
        let Some((app_id, name)) = selection else {
            self.clear_selection();
            return;
        };
        self.select(app_id);
        self.poll();
        let Some(credentials) = credentials else {
            ui.label("Connect an Apple account in Settings to load app details.");
            return;
        };
        if !self.cache.contains_key(app_id) && self.job.is_none() {
            self.start(app_id, credentials.clone(), ui.ctx());
        }

        egui::ScrollArea::vertical()
            .id_salt(("ios_app_details", app_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(800.0));
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        Label::new(
                            RichText::new(if name.is_empty() { app_id } else { name }).heading(),
                        )
                        .wrap(),
                    );
                    if ui
                        .add_enabled(self.job.is_none(), egui::Button::new("Refresh"))
                        .clicked()
                    {
                        self.start(app_id, credentials.clone(), ui.ctx());
                    }
                });
                if self.job.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            if self
                                .cache
                                .get(app_id)
                                .is_some_and(|entry| entry.data.is_some())
                            {
                                "Refreshing app details…"
                            } else {
                                "Loading app details…"
                            },
                        );
                    });
                }
                if let Some(entry) = self.cache.get(app_id) {
                    if let Some(error) = &entry.error {
                        ui.add(
                            Label::new(RichText::new(error).color(ui.visuals().error_fg_color))
                                .wrap(),
                        );
                        if entry.data.is_some() {
                            ui.label("Previously loaded details are shown.");
                        }
                    }
                    if let Some(details) = &entry.data {
                        details_ui(ui, details);
                    }
                }
            });
    }
}

fn details_ui(ui: &mut Ui, details: &AppDetails) {
    for warning in &details.warnings {
        ui.add(Label::new(RichText::new(warning).color(ui.visuals().error_fg_color)).wrap());
    }
    ui.add_space(8.0);
    row(ui, "App ID", details.app["id"].as_str().unwrap_or_default());
    fields(
        ui,
        &details.app,
        &[
            ("Bundle ID", "bundleId"),
            ("SKU", "sku"),
            ("Primary locale", "primaryLocale"),
        ],
    );
    if let Some(info) = &details.info {
        row(ui, "App information", &readable(state(info)));
        for (label, field) in [
            ("Primary category", "primaryCategory"),
            ("Secondary category", "secondaryCategory"),
        ] {
            let category = info["relationships"][field]["data"]["id"]
                .as_str()
                .unwrap_or_default();
            row(ui, label, &readable(category));
        }
    }
    ui.add_space(8.0);
    ui.label(RichText::new("iOS version").strong());
    if let Some(version) = &details.version {
        row(ui, "Version", attr(version, "versionString"));
        row(ui, "Status", &readable(state(version)));
        fields(
            ui,
            version,
            &[("Copyright", "copyright"), ("Created", "createdDate")],
        );
    } else if !details
        .warnings
        .iter()
        .any(|warning| warning.starts_with("iOS version:"))
    {
        ui.label("No iOS App Store version is available.");
    }

    ui.add_space(8.0);
    ui.label(RichText::new("English localizations").strong());
    let mut locales: Vec<_> = details
        .info_localizations
        .iter()
        .chain(&details.version_localizations)
        .map(|resource| attr(resource, "locale"))
        .collect();
    locales.sort_by_key(|locale| (*locale != "en-US", *locale));
    locales.dedup();
    if locales.is_empty() {
        if !details
            .warnings
            .iter()
            .any(|warning| warning.contains("localizations:"))
        {
            ui.label("No English localization is available.");
        }
        return;
    }
    for locale in locales {
        ui.add_space(8.0);
        ui.label(RichText::new(locale).strong());
        if let Some(info) = details
            .info_localizations
            .iter()
            .find(|resource| attr(resource, "locale") == locale)
        {
            fields(
                ui,
                info,
                &[
                    ("Name", "name"),
                    ("Subtitle", "subtitle"),
                    ("Privacy policy URL", "privacyPolicyUrl"),
                    ("Privacy choices URL", "privacyChoicesUrl"),
                ],
            );
        }
        if let Some(version) = details
            .version_localizations
            .iter()
            .find(|resource| attr(resource, "locale") == locale)
        {
            fields(
                ui,
                version,
                &[
                    ("Description", "description"),
                    ("Keywords", "keywords"),
                    ("Promotional text", "promotionalText"),
                    ("What's new", "whatsNew"),
                    ("Support URL", "supportUrl"),
                    ("Marketing URL", "marketingUrl"),
                ],
            );
        }
    }
}

fn fields(ui: &mut Ui, resource: &Value, fields: &[(&str, &str)]) {
    for (label, field) in fields {
        row(ui, label, attr(resource, field));
    }
}

fn readable(value: &str) -> String {
    value
        .split('_')
        .map(|word| {
            let word = word.to_lowercase();
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn row(ui: &mut Ui, label: &str, value: &str) {
    let value = if value.trim().is_empty() {
        "—"
    } else {
        value
    };
    if ui.available_width() < 480.0 {
        ui.label(label);
        ui.add(Label::new(value).wrap().selectable(true));
    } else {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(144.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(label);
                },
            );
            ui.add(Label::new(value).wrap().selectable(true));
        });
    }
}
