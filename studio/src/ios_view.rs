use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

use egui::{Color32, Frame, Margin, RichText, Ui};

use crate::apple::{
    AppleCredentials,
    api::ParsequeradeApi,
    details::{self, AppDetails, attr},
};

mod chrome;
mod documents;
mod editor;

use documents::{Document, Workspace};

#[cfg(test)]
mod preview;
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
    workspaces: HashMap<String, Workspace>,
    job: Option<Job>,
}

enum Action {
    Refresh,
    Version(String),
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
                    self.workspaces
                        .entry(selected.clone())
                        .or_default()
                        .reconcile(&details);
                    entry.data = Some(details);
                    entry.error = None;
                }
                Err(error) => entry.error = Some(error),
            }
        }
        self.job = None;
    }

    fn start(
        &mut self,
        app_id: &str,
        version_id: Option<String>,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
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
            let result = ParsequeradeApi::new(credentials).and_then(|api| {
                if version_id.is_some() {
                    details::load_for_version(api, &app_id, version_id.as_deref(), &cancelled)
                } else {
                    details::load(api, &app_id, &cancelled)
                }
            });
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
            Frame::new().inner_margin(16).show(ui, |ui| {
                ui.label("Connect an Apple account in Settings to load app details.");
            });
            return;
        };
        if !self.cache.contains_key(app_id) && self.job.is_none() {
            self.start(app_id, None, credentials.clone(), ui.ctx());
        }

        let palette = Palette::new(ui.visuals().dark_mode);
        let loading = self.job.is_some();
        let entry = self.cache.get(app_id);
        let details = entry.and_then(|entry| entry.data.as_ref());
        let workspace = self.workspaces.entry(app_id.into()).or_default();
        if let Some(details) = details {
            workspace.reconcile(details);
        }
        let mut action = None;
        ui.push_id(("app_workspace", app_id), |ui| {
            configure_workspace(ui, palette);
            let width = ui.available_width();
            egui::Panel::top("workspace_toolbar")
                .exact_size(44.0)
                .frame(palette.frame(palette.toolbar, Margin::symmetric(10, 5)))
                .show(ui, |ui| {
                    action = chrome::toolbar(ui, workspace, details, name, loading, palette);
                });
            egui::Panel::bottom("workspace_status")
                .exact_size(22.0)
                .frame(palette.frame(palette.toolbar, Margin::symmetric(10, 2)))
                .show(ui, |ui| {
                    chrome::status(ui, workspace, details, loading, palette)
                });
            if width >= 700.0 {
                let mut expanded = workspace.navigator;
                egui::Panel::left("resource_navigator")
                    .default_size(224.0)
                    .size_range(188.0..=280.0)
                    .frame(palette.frame(palette.sidebar, Margin::ZERO))
                    .show_collapsible(ui, &mut expanded, |ui| {
                        chrome::navigator(ui, workspace, name, palette)
                    });
                workspace.navigator = expanded;
            }
            if width >= 1120.0 {
                let mut expanded = workspace.inspector;
                egui::Panel::right("resource_inspector")
                    .default_size(252.0)
                    .size_range(224.0..=300.0)
                    .frame(palette.frame(palette.sidebar, Margin::ZERO))
                    .show_collapsible(ui, &mut expanded, |ui| {
                        chrome::inspector(ui, workspace, details, palette)
                    });
                workspace.inspector = expanded;
            }
            egui::CentralPanel::default()
                .frame(palette.frame(palette.editor, Margin::ZERO))
                .show(ui, |ui| {
                    chrome::tabs(ui, workspace, palette);
                    chrome::jump_bar(ui, workspace, details, palette);
                    egui::ScrollArea::vertical()
                        .id_salt(("document", workspace.active, &workspace.locale))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let margin = if ui.available_width() < 500.0 { 16 } else { 28 };
                            Frame::new()
                                .inner_margin(Margin::symmetric(margin, 24))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width().min(760.0));
                                    if let Some(error) =
                                        entry.and_then(|entry| entry.error.as_deref())
                                    {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(error).color(palette.error),
                                            )
                                            .wrap(),
                                        );
                                        if details.is_some() {
                                            ui.label(
                                                RichText::new(
                                                    "Previously loaded details are shown.",
                                                )
                                                .color(palette.muted),
                                            );
                                        }
                                        ui.add_space(12.0);
                                    }
                                    if let Some(details) = details {
                                        for warning in &details.warnings {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(warning).color(palette.error),
                                                )
                                                .wrap(),
                                            );
                                        }
                                        editor::document(
                                            ui,
                                            workspace,
                                            details,
                                            palette,
                                            loading,
                                            &mut action,
                                        );
                                    } else if loading {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label("Loading app details…");
                                        });
                                    }
                                });
                        });
                });
        });
        if let Some(action) = action {
            let version = match action {
                Action::Version(id) => Some(id),
                Action::Refresh => details
                    .and_then(|d| d.version.as_ref())
                    .map(|v| attr(v, "id").to_owned()),
            };
            self.start(app_id, version, credentials.clone(), ui.ctx());
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    toolbar: Color32,
    sidebar: Color32,
    editor: Color32,
    field: Color32,
    control: Color32,
    hover: Color32,
    tab: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    selection: Color32,
    error: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                toolbar: Color32::from_gray(45),
                sidebar: Color32::from_gray(39),
                editor: Color32::from_gray(37),
                field: Color32::from_gray(26),
                control: Color32::from_gray(60),
                hover: Color32::from_gray(69),
                tab: Color32::from_gray(57),
                border: Color32::from_gray(63),
                text: Color32::from_gray(222),
                muted: Color32::from_gray(166),
                accent: Color32::from_rgb(10, 155, 255),
                selection: Color32::from_rgb(0, 99, 225),
                error: Color32::from_rgb(255, 137, 137),
            }
        } else {
            Self {
                toolbar: Color32::from_gray(236),
                sidebar: Color32::from_gray(242),
                editor: Color32::WHITE,
                field: Color32::WHITE,
                control: Color32::from_gray(250),
                hover: Color32::from_gray(226),
                tab: Color32::WHITE,
                border: Color32::from_gray(211),
                text: Color32::from_gray(38),
                muted: Color32::from_gray(103),
                accent: Color32::from_rgb(0, 112, 220),
                selection: Color32::from_rgb(0, 99, 225),
                error: Color32::from_rgb(180, 42, 45),
            }
        }
    }
    fn frame(self, fill: Color32, margin: Margin) -> Frame {
        Frame::new().fill(fill).inner_margin(margin)
    }
}

fn configure_workspace(ui: &mut Ui, palette: Palette) {
    let style = ui.style_mut();
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(12.0));
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(7.0, 3.0);
    style.spacing.interact_size.y = 22.0;
    style.spacing.indent = 14.0;
    style.visuals.override_text_color = Some(palette.text);
    style.visuals.weak_text_color = Some(palette.muted);
    style.visuals.indent_has_left_vline = false;
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, palette.border);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, palette.border);
    style.visuals.widgets.inactive.bg_fill = palette.control;
    style.visuals.widgets.inactive.weak_bg_fill = palette.control;
    style.visuals.widgets.hovered.bg_fill = palette.hover;
    style.visuals.widgets.hovered.weak_bg_fill = palette.hover;
    style.visuals.text_edit_bg_color = Some(palette.field);
    style.visuals.selection.bg_fill = palette.selection;
    style.visuals.hyperlink_color = palette.accent;
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widget.corner_radius = egui::CornerRadius::same(4);
    }
}
