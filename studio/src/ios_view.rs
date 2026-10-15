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
    api::ParsequeradeApi,
    details::{self, AppDetails, attr, state},
};
use crate::dashboard::Icon;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../target/ios_visual_preview.rs"]
mod visual_preview;

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
            let result = ParsequeradeApi::new(credentials)
                .and_then(|api| details::load(api, &app_id, &cancelled));
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
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width().min(1040.0));
                        ui.add_space(8.0);
                        ui.horizontal_top(|ui| {
                            let title_width =
                                (ui.available_width() - 80.0 - ui.spacing().item_spacing.x)
                                    .max(0.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(title_width, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(title_width);
                                    ui.add(
                                        Label::new(
                                            RichText::new(if name.is_empty() {
                                                app_id
                                            } else {
                                                name
                                            })
                                            .heading(),
                                        )
                                        .wrap(),
                                    );
                                },
                            );
                            if ui
                                .add_enabled(
                                    self.job.is_none(),
                                    egui::Button::new("Refresh").min_size(egui::vec2(80.0, 36.0)),
                                )
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
                                    Label::new(
                                        RichText::new(error).color(ui.visuals().error_fg_color),
                                    )
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
            });
    }
}

fn details_ui(ui: &mut Ui, details: &AppDetails) {
    for warning in &details.warnings {
        ui.add(Label::new(RichText::new(warning).color(ui.visuals().error_fg_color)).wrap());
    }
    ui.add_space(16.0);
    if ui.available_width() >= 740.0 {
        let gap = 24.0;
        let width = (ui.available_width() - gap) / 2.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.x = 8.0;
                    app_information(ui, details);
                },
            );
            ui.add_space(gap);
            ui.allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ios_version(ui, details);
                },
            );
        });
    } else {
        app_information(ui, details);
        ui.add_space(20.0);
        ios_version(ui, details);
    }

    ui.add_space(24.0);
    section_heading(ui, "English localizations", Icon::Globe, Accent::Violet);
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
    for (index, locale) in locales.into_iter().enumerate() {
        if index > 0 {
            ui.add_space(8.0);
        }
        ui.label(RichText::new(locale).size(15.0).strong());
        let info = details
            .info_localizations
            .iter()
            .find(|resource| attr(resource, "locale") == locale);
        let version = details
            .version_localizations
            .iter()
            .find(|resource| attr(resource, "locale") == locale);
        if ui.available_width() >= 740.0 {
            let gap = 24.0;
            let width = ui.available_width() - gap;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(width * 2.0 / 3.0, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let column_width = width * 2.0 / 3.0;
                        ui.set_width(column_width);
                        ui.spacing_mut().item_spacing.x = 8.0;
                        localization_listing(ui, info, version);
                    },
                );
                ui.add_space(gap);
                ui.allocate_ui_with_layout(
                    egui::vec2(width / 3.0, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let column_width = width / 3.0;
                        ui.set_width(column_width);
                        ui.spacing_mut().item_spacing.x = 8.0;
                        localization_links(ui, info, version);
                    },
                );
            });
        } else {
            localization_listing(ui, info, version);
            ui.add_space(16.0);
            localization_links(ui, info, version);
        }
    }
}

fn app_information(ui: &mut Ui, details: &AppDetails) {
    section_heading(ui, "App information", Icon::Apps, Accent::Blue);
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
        row(ui, "Status", &readable(state(info)));
        for (label, field) in [
            ("Primary category", "primaryCategoryId"),
            ("Secondary category", "secondaryCategoryId"),
        ] {
            let category = info[field].as_str().unwrap_or_default();
            row(ui, label, &readable(category));
        }
    }
}

fn ios_version(ui: &mut Ui, details: &AppDetails) {
    section_heading(ui, "iOS version", Icon::Phone, Accent::Green);
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
}

fn localization_listing(ui: &mut Ui, info: Option<&Value>, version: Option<&Value>) {
    if let Some(info) = info {
        fields(ui, info, &[("Name", "name"), ("Subtitle", "subtitle")]);
    }
    if let Some(version) = version {
        prose_fields(
            ui,
            version,
            &[
                ("Description", "description"),
                ("Promotional text", "promotionalText"),
                ("What's new", "whatsNew"),
                ("Keywords", "keywords"),
            ],
        );
    }
}

fn localization_links(ui: &mut Ui, info: Option<&Value>, version: Option<&Value>) {
    section_heading(ui, "Links & privacy", Icon::Shield, Accent::Amber);
    if let Some(info) = info {
        link_fields(
            ui,
            info,
            &[
                ("Privacy policy", "privacyPolicyUrl"),
                ("Privacy choices", "privacyChoicesUrl"),
            ],
        );
    }
    if let Some(version) = version {
        link_fields(
            ui,
            version,
            &[("Support", "supportUrl"), ("Marketing", "marketingUrl")],
        );
    }
}

fn link_fields(ui: &mut Ui, resource: &Value, fields: &[(&str, &str)]) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        for (label, field) in fields {
            let value = attr(resource, field);
            let value = if value.trim().is_empty() {
                "—"
            } else {
                value
            };
            ui.add(
                Label::new(
                    RichText::new(*label)
                        .size(12.0)
                        .strong()
                        .color(muted_text_color(ui)),
                )
                .wrap(),
            );
            ui.add(Label::new(value).wrap().selectable(true));
            ui.add_space(4.0);
        }
    });
}

#[derive(Clone, Copy)]
enum Accent {
    Blue,
    Green,
    Violet,
    Amber,
}

fn section_heading(ui: &mut Ui, title: &str, icon: Icon, accent: Accent) {
    let (color, tint) = if ui.visuals().dark_mode {
        let color = match accent {
            Accent::Blue => egui::Color32::from_rgb(111, 178, 255),
            Accent::Green => egui::Color32::from_rgb(100, 216, 157),
            Accent::Violet => egui::Color32::from_rgb(188, 157, 255),
            Accent::Amber => egui::Color32::from_rgb(255, 190, 105),
        };
        (color, color.gamma_multiply(0.16))
    } else {
        let color = match accent {
            Accent::Blue => egui::Color32::from_rgb(36, 105, 193),
            Accent::Green => egui::Color32::from_rgb(28, 126, 77),
            Accent::Violet => egui::Color32::from_rgb(111, 77, 183),
            Accent::Amber => egui::Color32::from_rgb(157, 97, 21),
        };
        (color, color.gamma_multiply(0.12))
    };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 8.0, tint);
        icon.paint(ui, rect.min + egui::vec2(7.0, 7.0), 18.0, color);
        ui.add_space(8.0);
        ui.add(Label::new(RichText::new(title).size(16.0).strong()).wrap());
    });
    ui.add_space(8.0);
}

fn fields(ui: &mut Ui, resource: &Value, fields: &[(&str, &str)]) {
    for (label, field) in fields {
        row(ui, label, attr(resource, field));
    }
}

fn prose_fields(ui: &mut Ui, resource: &Value, fields: &[(&str, &str)]) {
    for (label, field) in fields {
        let value = attr(resource, field);
        let value = if value.trim().is_empty() {
            "—"
        } else {
            value
        };
        ui.add_space(8.0);
        ui.label(
            RichText::new(*label)
                .size(12.0)
                .strong()
                .color(muted_text_color(ui)),
        );
        ui.add(Label::new(value).wrap().selectable(true));
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

fn muted_text_color(ui: &Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgb(160, 168, 181)
    } else {
        egui::Color32::from_rgb(89, 100, 119)
    }
}

fn row(ui: &mut Ui, label: &str, value: &str) {
    let value = if value.trim().is_empty() {
        "—"
    } else {
        value
    };
    if ui.available_width() < 320.0 {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.add(Label::new(RichText::new(label).color(muted_text_color(ui))).wrap());
            ui.add(Label::new(value).wrap().selectable(true));
        });
    } else {
        let label_width = 144.0;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(label_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(label_width);
                    ui.add(Label::new(RichText::new(label).color(muted_text_color(ui))).wrap());
                },
            );
            ui.add(Label::new(value).wrap().selectable(true));
        });
    }
}
