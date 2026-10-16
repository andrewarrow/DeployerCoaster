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

mod age_copy;
mod chrome;
mod completeness;
mod documents;
mod editor;
mod media;
mod metadata;
mod versions;

use documents::{Document, Workspace};

#[cfg(test)]
mod preview;
#[cfg(test)]
mod tests;

// App document windows have independent egui contexts and views. Keep one typed
// clipboard in App so a section copied in one window is available in another.
pub(crate) type AgeRatingClipboard = std::rc::Rc<std::cell::RefCell<Option<serde_json::Value>>>;

struct Job<T = AppDetails> {
    result: Receiver<Result<T, String>>,
    cancelled: Arc<AtomicBool>,
}

impl<T> Drop for Job<T> {
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
    review_job: Option<Job<Option<serde_json::Value>>>,
    creation: Option<versions::Creation>,
    age_copy: age_copy::AgeCopy,
    media: media::Gallery,
    metadata: metadata::Metadata,
}

enum Action {
    Refresh,
    Version(String),
    NewVersion,
    Save,
}

impl IosView {
    pub(crate) fn with_age_rating_clipboard(clipboard: AgeRatingClipboard) -> Self {
        Self {
            age_copy: age_copy::AgeCopy::with_clipboard(clipboard),
            ..Default::default()
        }
    }

    pub(crate) fn clear_selection(&mut self) {
        self.job = None;
        self.review_job = None;
        self.selected = None;
        self.media.cancel();
    }

    fn select(&mut self, app_id: &str) {
        if self.selected.as_deref() != Some(app_id) {
            self.job = None;
            self.review_job = None;
            self.selected = Some(app_id.to_owned());
            self.media.clear();
        }
    }

    fn poll(&mut self) {
        self.poll_details();
        self.poll_version();
        let Some(job) = &self.review_job else { return };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("Review contact loading stopped. Please refresh again.".into())
            }
        };
        if let Some(details) = self
            .selected
            .as_ref()
            .and_then(|id| self.cache.get_mut(id))
            .and_then(|entry| entry.data.as_mut())
        {
            details.review_contact = Some(result);
        }
        self.review_job = None;
    }

    fn poll_details(&mut self) {
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
                    self.media.clear();
                    self.workspaces
                        .entry(selected.clone())
                        .or_default()
                        .reconcile(&details);
                    entry.data = Some(details);
                    entry.error = None;
                }
                Err(error) => {
                    entry.error = Some(error);
                    self.review_job = None;
                }
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
        if self.job.is_some()
            || self.age_copy.pasting(app_id)
            || self.creating_version(app_id)
            || self.metadata.saving(app_id)
        {
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
        self.review_job = None;
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

    fn start_review(
        &mut self,
        app_id: &str,
        version_id: String,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.review_job = Some(Job {
            result,
            cancelled: cancelled.clone(),
        });
        let app_id = app_id.to_owned();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ParsequeradeApi::new(credentials).and_then(|api| {
                details::load_review_contact(api, &app_id, &version_id, &cancelled)
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
        self.age_copy.poll(&mut self.cache);
        self.metadata.poll(&mut self.cache, &mut self.workspaces);
        let Some((app_id, name)) = selection else {
            self.clear_selection();
            return;
        };
        self.select(app_id);
        self.poll();
        self.media.poll_generation(ui.ctx());
        let Some(credentials) = credentials else {
            Frame::new().inner_margin(16).show(ui, |ui| {
                ui.label("Connect an Apple account in Settings to load app details.");
            });
            return;
        };
        if !self.cache.contains_key(app_id) && self.job.is_none() {
            self.start(app_id, None, credentials.clone(), ui.ctx());
        }

        // Resume an interrupted check when returning to cached details, without reloading them.
        if self.job.is_none()
            && !self.creating_version(app_id)
            && self.review_job.is_none()
            && let Some(details) = self
                .cache
                .get_mut(app_id)
                .and_then(|entry| entry.data.as_mut())
            && details.review_contact.is_none()
        {
            if let Some(version) = &details.version {
                let version_id = attr(version, "id").to_owned();
                self.start_review(app_id, version_id, credentials.clone(), ui.ctx());
            } else {
                details.review_contact = Some(Ok(None));
            }
        }

        let palette = Palette::new(ui.visuals().dark_mode);
        let saving = self.metadata.saving(app_id);
        let loading = self.job.is_some()
            || self.age_copy.pasting(app_id)
            || self.creating_version(app_id)
            || saving;
        let entry = self.cache.get(app_id);
        let details = entry.and_then(|entry| entry.data.as_ref());
        let workspace = self.workspaces.entry(app_id.into()).or_default();
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::W)) {
            workspace.close(workspace.active);
        }
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
                    .default_size(236.0)
                    .size_range(220.0..=300.0)
                    .frame(palette.frame(palette.sidebar, Margin::ZERO))
                    .show_collapsible(ui, &mut expanded, |ui| {
                        chrome::navigator(ui, workspace, details, name, palette)
                    });
                workspace.navigator = expanded;
            }
            if width >= 1120.0 {
                let mut expanded = workspace.inspector;
                egui::Panel::right("resource_inspector")
                    .default_size(260.0)
                    .size_range(240.0..=320.0)
                    .frame(palette.frame(palette.inspector, Margin::ZERO))
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
                    if self
                        .metadata
                        .controls(ui, workspace, app_id, loading, palette)
                    {
                        action = Some(Action::Save);
                    }
                    // Register the background before child controls so their text menus
                    // keep priority, while empty margins and the full viewport accept a menu.
                    let background = (workspace.active == Document::AgeRating).then(|| {
                        ui.interact(
                            ui.available_rect_before_wrap(),
                            ui.id().with("age_rating_background"),
                            egui::Sense::click(),
                        )
                    });
                    egui::ScrollArea::vertical()
                        .id_salt(("document", workspace.active, &workspace.locale))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let margin = if ui.available_width() < 500.0 { 16 } else { 28 };
                            Frame::new()
                                .inner_margin(Margin::symmetric(margin, 18))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width().min(760.0));
                                    if workspace.active == Document::AgeRating {
                                        self.age_copy.feedback(ui, app_id, palette);
                                    }
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
                                        if workspace.active == Document::Media {
                                            if ui.ctx().content_rect().width() < 700.0 {
                                                chrome::version_picker(
                                                    ui,
                                                    Some(details),
                                                    loading,
                                                    &mut action,
                                                );
                                                ui.add_space(12.0);
                                            }
                                            self.media.ui(
                                                ui,
                                                details,
                                                &workspace.locale,
                                                credentials,
                                                palette,
                                                loading,
                                            );
                                        } else {
                                            self.media.cancel();
                                            ui.add_enabled_ui(!saving, |ui| {
                                                editor::document(
                                                    ui,
                                                    workspace,
                                                    details,
                                                    palette,
                                                    loading,
                                                    &mut action,
                                                );
                                            });
                                        }
                                    } else if loading {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label("Loading app details…");
                                        });
                                    }
                                });
                        });
                    if let Some(background) = background {
                        self.age_copy.context_menu(
                            &background,
                            app_id,
                            details,
                            loading,
                            credentials,
                        );
                    }
                });
        });
        if let Some(action) = action {
            if matches!(action, Action::Save) {
                self.metadata.start(
                    app_id,
                    &self.workspaces[app_id],
                    credentials.clone(),
                    ui.ctx(),
                );
                return;
            }
            if matches!(action, Action::NewVersion) {
                self.media.cancel();
                self.start_version(app_id, credentials.clone(), ui.ctx());
                return;
            }
            let version = match action {
                Action::Version(id) => Some(id),
                Action::Refresh => details
                    .and_then(|d| d.version.as_ref())
                    .map(|v| attr(v, "id").to_owned()),
                Action::NewVersion | Action::Save => unreachable!(),
            };
            self.media.cancel();
            self.start(app_id, version, credentials.clone(), ui.ctx());
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    toolbar: Color32,
    sidebar: Color32,
    inspector: Color32,
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
                toolbar: Color32::from_rgb(45, 44, 45),
                sidebar: Color32::from_rgb(41, 40, 41),
                inspector: Color32::from_rgb(43, 42, 43),
                editor: Color32::from_rgb(36, 35, 36),
                field: Color32::from_rgb(29, 28, 29),
                control: Color32::from_gray(67),
                hover: Color32::from_gray(78),
                tab: Color32::from_gray(55),
                border: Color32::from_gray(56),
                text: Color32::from_gray(222),
                muted: Color32::from_gray(166),
                accent: Color32::from_rgb(10, 132, 255),
                selection: Color32::from_rgb(0, 88, 208),
                error: Color32::from_rgb(255, 137, 137),
            }
        } else {
            Self {
                toolbar: Color32::from_gray(236),
                sidebar: Color32::from_gray(239),
                inspector: Color32::from_gray(246),
                editor: Color32::WHITE,
                field: Color32::WHITE,
                control: Color32::from_gray(250),
                hover: Color32::from_gray(226),
                tab: Color32::WHITE,
                border: Color32::from_gray(211),
                text: Color32::from_gray(38),
                muted: Color32::from_gray(103),
                accent: Color32::from_rgb(0, 112, 220),
                selection: Color32::from_rgb(0, 88, 208),
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
        .insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(12.0));
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(7.0, 3.0);
    style.spacing.interact_size.y = 22.0;
    style.spacing.indent = 14.0;
    style.animation_time = 0.15;
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
