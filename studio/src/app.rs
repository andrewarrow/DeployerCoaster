use std::path::PathBuf;

#[cfg(not(target_os = "macos"))]
use egui::{Key, Modifiers};

use crate::{
    commands::{Command, PendingAction},
    metadata,
    play_store::PlayStore,
    preferences::{Appearance, Preferences},
    workspace::{WORKSPACE_EXTENSION, Workspace},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SettingsSection {
    #[default]
    General,
    Android,
    Apple,
    Dynadot,
}

impl SettingsSection {
    const ALL: [Self; 4] = [Self::General, Self::Android, Self::Apple, Self::Dynadot];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Android => "Android",
            Self::Apple => "Apple",
            Self::Dynadot => "Dynadot",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppWindow {
    Settings,
    Android,
    Apple,
}

impl AppWindow {
    pub fn title(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Android => "Android",
            Self::Apple => "Apple",
        }
    }
}

pub struct App {
    preferences: Preferences,
    preferences_path: Option<PathBuf>,
    workspace: Option<Workspace>,
    pending_action: Option<PendingAction>,
    quit: bool,
    dashboard: crate::dashboard::Dashboard,
    settings_section: SettingsSection,
    show_about: bool,
    status: Option<String>,
    status_is_error: bool,
    play_store: PlayStore,
    apple: crate::apple::AppleSettings,
    apple_store: crate::apple::AppleStore,
    dynadot: crate::dynadot::Dynadot,
    requested_app_windows: Vec<AppWindow>,
}

impl App {
    pub fn new(initial_path: Option<PathBuf>) -> Self {
        let preferences_path = Preferences::path();
        let (preferences, preference_error) = match preferences_path.as_deref() {
            Some(path) => match Preferences::load(path) {
                Ok(preferences) => (preferences, None),
                Err(error) => (Preferences::default(), Some(error)),
            },
            None => (Preferences::default(), None),
        };

        let mut app = Self {
            preferences,
            preferences_path,
            workspace: None,
            pending_action: None,
            quit: false,
            dashboard: crate::dashboard::Dashboard::default(),
            settings_section: SettingsSection::default(),
            show_about: false,
            status_is_error: preference_error.is_some(),
            status: preference_error,
            play_store: PlayStore::load(),
            apple: crate::apple::AppleSettings::load(),
            apple_store: crate::apple::AppleStore::default(),
            dynadot: crate::dynadot::Dynadot::load(),
            requested_app_windows: Vec::new(),
        };
        if let Some(path) = initial_path {
            app.open_path(path);
        }
        app
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.dynadot.poll();
        self.play_store.poll();
        self.apple_store.set_credentials(self.apple.credentials());
        self.apple_store
            .set_reporting_vendor(self.apple.saved_vendor_number());
        self.apple_store.poll();
        let ctx = ui.ctx().clone();
        ctx.set_theme(self.preferences.appearance.theme());
        #[cfg(not(target_os = "macos"))]
        if self.pending_action.is_none() {
            self.keyboard_shortcuts(&ctx);
        }

        let mut selected_command = None;
        #[cfg(not(target_os = "macos"))]
        egui::Panel::top("app_menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    for (label, command) in
                        [("Android", Command::Android), ("Apple", Command::Apple)]
                    {
                        if ui.button(label).clicked() {
                            selected_command = Some(command);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Open…").clicked() {
                        selected_command = Some(Command::OpenWorkspace);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(self.workspace.is_some(), egui::Button::new("Save"))
                        .clicked()
                    {
                        selected_command = Some(Command::Save);
                        ui.close();
                    }
                    if ui
                        .add_enabled(self.workspace.is_some(), egui::Button::new("Save As…"))
                        .clicked()
                    {
                        selected_command = Some(Command::SaveAs);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.workspace.is_some(),
                            egui::Button::new("Close Workspace"),
                        )
                        .clicked()
                    {
                        selected_command = Some(Command::CloseWorkspace);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        selected_command = Some(Command::Quit);
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui
                        .selectable_label(self.preferences.show_sidebar, "Recent workspaces")
                        .clicked()
                    {
                        selected_command = Some(Command::ToggleSidebar);
                    }
                    if ui
                        .selectable_label(self.preferences.show_inspector, "Workspace details")
                        .clicked()
                    {
                        selected_command = Some(Command::ToggleInspector);
                    }
                    if ui
                        .selectable_label(self.preferences.show_activity, "Activity")
                        .clicked()
                    {
                        selected_command = Some(Command::ToggleActivity);
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Settings…").clicked() {
                        selected_command = Some(Command::Settings);
                        ui.close();
                    }
                    if ui.button("About DeployerCoaster").clicked() {
                        selected_command = Some(Command::About);
                        ui.close();
                    }
                });
            });
        });

        if self.preferences.show_activity && self.status.is_some() {
            egui::Panel::bottom("activity_panel").show(ui, |ui| {
                ui.label(self.status.as_deref().unwrap_or("Ready"));
            });
        }
        if let Some(workspace) = &mut self.workspace {
            egui::Panel::top("open_workspace")
                .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(16, 8)))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Workspace");
                        ui.add(
                            egui::TextEdit::singleline(&mut workspace.document.name)
                                .desired_width(200.0)
                                .hint_text("Workspace name"),
                        );
                        if workspace.is_dirty() {
                            ui.weak("Unsaved changes");
                        }
                        if self.preferences.show_inspector {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(
                                        workspace
                                            .path
                                            .as_deref()
                                            .map(|path| path.display().to_string())
                                            .unwrap_or_else(|| "Not saved".into()),
                                    )
                                    .small()
                                    .weak(),
                                )
                                .truncate(),
                            );
                        }
                    });
                });
        }
        self.play_store.prepare_dashboard(&ctx);
        self.apple_store.prepare_dashboard(&ctx);
        if let Some(command) = self.dashboard.ui(
            ui,
            &mut self.play_store,
            &mut self.apple_store,
            &mut self.dynadot,
            self.preferences.show_sidebar,
        ) {
            selected_command = Some(command);
        }
        if let Some(status) = &self.status
            && (self.status_is_error || !self.preferences.show_activity)
        {
            egui::Area::new(egui::Id::new("workspace_status"))
                .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -16.0])
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(320.0);
                        if self.status_is_error {
                            ui.colored_label(ui.visuals().error_fg_color, status);
                        } else {
                            ui.label(status);
                        }
                    });
                });
        }

        if let Some(command) = selected_command {
            self.command(command);
        }
        self.about_window(&ctx);
        self.discard_modal(&ctx);
    }

    pub fn command(&mut self, command: Command) {
        match command {
            Command::OpenWorkspace => self.request_action(PendingAction::Open),
            Command::OpenPath(path) => self.request_action(PendingAction::OpenPath(path)),
            Command::Save => self.save(),
            Command::SaveAs => self.save_as(),
            Command::CloseWorkspace => self.request_action(PendingAction::CloseWorkspace),
            Command::Quit => self.request_action(PendingAction::Quit),
            Command::Settings => {
                self.settings_section = SettingsSection::General;
                self.requested_app_windows.push(AppWindow::Settings);
            }
            Command::DynadotSettings => {
                self.settings_section = SettingsSection::Dynadot;
                self.requested_app_windows.push(AppWindow::Settings);
            }
            Command::AppleSettings => {
                self.settings_section = SettingsSection::Apple;
                self.requested_app_windows.push(AppWindow::Settings);
            }
            Command::Android => self.requested_app_windows.push(AppWindow::Android),
            Command::Apple => self.requested_app_windows.push(AppWindow::Apple),
            Command::About => {
                #[cfg(target_os = "macos")]
                crate::macos::show_about_panel();
                #[cfg(not(target_os = "macos"))]
                {
                    self.show_about = true;
                }
            }
            Command::ToggleSidebar => {
                self.preferences.show_sidebar = !self.preferences.show_sidebar;
                self.save_preferences();
            }
            Command::ToggleInspector => {
                self.preferences.show_inspector = !self.preferences.show_inspector;
                self.save_preferences();
            }
            Command::ToggleActivity => {
                self.preferences.show_activity = !self.preferences.show_activity;
                self.save_preferences();
            }
            Command::ConfirmDiscard => {
                if let Some(action) = self.pending_action.take() {
                    self.apply_action(action);
                }
            }
            Command::CancelPending => self.pending_action = None,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn take_app_window_request(&mut self) -> Option<AppWindow> {
        if self.requested_app_windows.is_empty() {
            None
        } else {
            Some(self.requested_app_windows.remove(0))
        }
    }

    /// Returns true when the main window should be focused before opening Settings.
    pub fn app_window_ui(&mut self, window: AppWindow, ui: &mut egui::Ui) -> bool {
        ui.ctx().set_theme(self.preferences.appearance.theme());
        if window == AppWindow::Settings {
            self.store_settings(ui);
            return false;
        }
        let mut settings = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(16)))
            .show(ui, |ui| match window {
                AppWindow::Android => self.play_store.apps_ui(ui),
                AppWindow::Apple => {
                    self.apple_store.set_credentials(self.apple.credentials());
                    self.apple_store
                        .set_reporting_vendor(self.apple.saved_vendor_number());
                    settings = self.apple_store.apps_ui(ui);
                }
                AppWindow::Settings => unreachable!(),
            });
        if settings {
            self.settings_section = SettingsSection::Apple;
            self.requested_app_windows.push(AppWindow::Settings);
        }
        settings
    }

    pub fn window_error(&mut self, window: AppWindow) {
        self.set_error(format!(
            "Could not open the {} window. Please try again.",
            window.title()
        ));
    }

    #[cfg(target_os = "macos")]
    pub fn native_menu_state(&self) -> crate::macos::MenuState {
        crate::macos::MenuState {
            has_workspace: self.workspace.is_some(),
            has_pending_action: self.pending_action.is_some(),
            show_sidebar: self.preferences.show_sidebar,
            show_inspector: self.preferences.show_inspector,
            show_activity: self.preferences.show_activity,
        }
    }

    pub fn title(&self) -> String {
        let workspace = self.workspace.as_ref().map(|workspace| {
            let dirty = if workspace.is_dirty() { " *" } else { "" };
            format!("{}{}", workspace.document.name, dirty)
        });
        match workspace {
            Some(name) => format!("{name} — {}", metadata::APP_NAME),
            None => metadata::APP_NAME.to_owned(),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        let shortcut = ctx.input_mut(|input| {
            let command = input.modifiers.command;
            if !command {
                return None;
            }
            if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
                Some(Command::SaveAs)
            } else if input.consume_key(Modifiers::COMMAND, Key::O) {
                Some(Command::OpenWorkspace)
            } else if input.consume_key(Modifiers::COMMAND, Key::S) {
                Some(Command::Save)
            } else if input.consume_key(Modifiers::COMMAND, Key::Q) {
                Some(Command::Quit)
            } else {
                None
            }
        });
        if let Some(command) = shortcut {
            self.command(command);
        }
    }

    fn request_action(&mut self, action: PendingAction) {
        if self.workspace.as_ref().is_some_and(Workspace::is_dirty) {
            self.pending_action = Some(action);
        } else {
            self.apply_action(action);
        }
    }

    fn apply_action(&mut self, action: PendingAction) {
        self.status = None;
        self.status_is_error = false;
        match action {
            PendingAction::Open => self.open_dialog(),
            PendingAction::OpenPath(path) => self.open_path(path),
            PendingAction::CloseWorkspace => {
                self.workspace = None;
                self.set_status("Workspace closed");
            }
            PendingAction::Quit => self.quit = true,
        }
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DeployerCoaster workspace", &[WORKSPACE_EXTENSION])
            .pick_file()
        {
            self.open_path(path);
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match Workspace::open(path.clone()) {
            Ok(workspace) => {
                self.workspace = Some(workspace);
                self.preferences.remember(path);
                if self.save_preferences() {
                    self.set_status("Workspace opened");
                }
            }
            Err(error) => self.set_error(error),
        }
    }

    fn save(&mut self) {
        if self.workspace.is_none() {
            return;
        }
        let Some(path) = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.path.clone())
        else {
            self.save_as();
            return;
        };
        self.save_to(path);
    }

    fn save_as(&mut self) {
        if self.workspace.is_none() {
            return;
        }
        let suggested_name = self
            .workspace
            .as_ref()
            .map(|workspace| format!("{}.{}", workspace.document.name, WORKSPACE_EXTENSION))
            .unwrap_or_else(|| format!("Workspace.{WORKSPACE_EXTENSION}"));
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DeployerCoaster workspace", &[WORKSPACE_EXTENSION])
            .set_file_name(suggested_name)
            .save_file()
        {
            self.save_to(path);
        }
    }

    fn save_to(&mut self, path: PathBuf) {
        let Some(workspace) = &mut self.workspace else {
            return;
        };
        match workspace.document.save(&path) {
            Ok(()) => {
                let saved_document = workspace.document.clone();
                workspace.mark_saved(path.clone(), saved_document);
                self.preferences.remember(path);
                if self.save_preferences() {
                    self.set_status("Workspace saved");
                }
            }
            Err(error) => self.set_error(format!("Could not save workspace: {error}")),
        }
    }

    fn save_preferences(&mut self) -> bool {
        if let Some(path) = &self.preferences_path
            && let Err(error) = self.preferences.save(path)
        {
            self.set_error(format!("Could not save preferences: {error}"));
            return false;
        }
        true
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
        self.status_is_error = false;
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
        self.status_is_error = true;
    }

    fn store_settings(&mut self, ui: &mut egui::Ui) {
        let narrow = ui.available_width() < 620.0;
        if narrow {
            ui.horizontal(|ui| {
                ui.label("Settings");
                egui::ComboBox::from_id_salt("settings_category")
                    .selected_text(self.settings_section.label())
                    .show_ui(ui, |ui| {
                        for section in SettingsSection::ALL {
                            ui.selectable_value(
                                &mut self.settings_section,
                                section,
                                section.label(),
                            );
                        }
                    });
            });
            ui.add_space(8.0);
        } else {
            let fill = if ui.visuals().dark_mode {
                egui::Color32::from_gray(34)
            } else {
                egui::Color32::from_gray(238)
            };
            egui::Panel::left("settings_navigation")
                .exact_size(184.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(
                    egui::Frame::NONE
                        .fill(fill)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Settings").strong());
                    ui.add_space(8.0);
                    for section in SettingsSection::ALL {
                        if ui
                            .add_sized(
                                [ui.available_width(), 44.0],
                                egui::Button::selectable(self.settings_section == section, "")
                                    .left_text(section.label()),
                            )
                            .clicked()
                        {
                            self.settings_section = section;
                        }
                    }
                });
        }
        let margin = if narrow { 8 } else { 24 };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(margin)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("settings_content", self.settings_section.label()))
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(600.0));
                        ui.heading(self.settings_section.label());
                        ui.add_space(16.0);
                        match self.settings_section {
                            SettingsSection::Dynadot => self.dynadot.settings_ui(ui),
                            SettingsSection::General => self.appearance_settings(ui),
                            SettingsSection::Android => {
                                ui.label(egui::RichText::new("Google Play Store").strong());
                                ui.add_space(8.0);
                                self.play_store.settings_ui(ui);
                                ui.add_space(8.0);
                                if ui
                                    .add(
                                        egui::Button::new("Open Android apps")
                                            .min_size(egui::vec2(160.0, 44.0)),
                                    )
                                    .clicked()
                                {
                                    self.command(Command::Android);
                                }
                            }
                            SettingsSection::Apple => {
                                self.apple.ui(ui);
                                ui.add_space(8.0);
                                if ui
                                    .add(
                                        egui::Button::new("Open Apple apps")
                                            .min_size(egui::vec2(160.0, 44.0)),
                                    )
                                    .clicked()
                                {
                                    self.command(Command::Apple);
                                }
                            }
                        }
                    });
            });
    }

    fn appearance_settings(&mut self, ui: &mut egui::Ui) {
        let mut appearance = self.preferences.appearance;
        egui::ComboBox::from_label("Appearance")
            .selected_text(match appearance {
                Appearance::System => "System",
                Appearance::Light => "Light",
                Appearance::Dark => "Dark",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut appearance, Appearance::System, "System");
                ui.selectable_value(&mut appearance, Appearance::Light, "Light");
                ui.selectable_value(&mut appearance, Appearance::Dark, "Dark");
            });
        if appearance != self.preferences.appearance {
            self.preferences.appearance = appearance;
            ui.ctx().set_theme(appearance.theme());
            self.save_preferences();
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = self.show_about;
        egui::Window::new(format!("About {}", metadata::APP_NAME))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(metadata::APP_NAME);
                ui.label(format!("Version {}", metadata::GIT_SHA));
                ui.hyperlink_to("Website", metadata::WEBSITE_URL);
                ui.hyperlink_to("GitHub", metadata::GITHUB_URL);
            });
        self.show_about = open;
    }

    fn discard_modal(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_action.as_ref() else {
            return;
        };
        let action_label = match action {
            PendingAction::Open | PendingAction::OpenPath(_) => "open another workspace",
            PendingAction::CloseWorkspace => "close this workspace",
            PendingAction::Quit => "quit",
        };
        let response = egui::Modal::new(egui::Id::new("discard_changes_modal")).show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 32.0).clamp(0.0, 420.0));
            ui.heading("Discard unsaved changes?");
            ui.label(format!("Your changes will be lost if you {action_label}."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    self.command(Command::CancelPending);
                }
                if ui.button("Discard Changes").clicked() {
                    self.command(Command::ConfirmDiscard);
                }
            });
        });
        if response.should_close() {
            self.pending_action = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_workspace(workspace: Workspace) -> App {
        App {
            preferences: Preferences::default(),
            preferences_path: None,
            workspace: Some(workspace),
            pending_action: None,
            quit: false,
            dashboard: crate::dashboard::Dashboard::default(),
            settings_section: SettingsSection::default(),
            show_about: false,
            status: None,
            status_is_error: false,
            play_store: PlayStore::default(),
            apple: crate::apple::AppleSettings::default(),
            apple_store: crate::apple::AppleStore::default(),
            dynadot: crate::dynadot::Dynadot::default(),
            requested_app_windows: Vec::new(),
        }
    }

    #[test]
    fn sales_setup_opens_the_apple_settings_section() {
        let mut app = app_with_workspace(Workspace::new());
        app.command(Command::AppleSettings);
        assert!(app.settings_section == SettingsSection::Apple);
        assert_eq!(app.take_app_window_request(), Some(AppWindow::Settings));
    }

    #[test]
    fn store_settings_fit_supported_window_sizes() {
        for section in SettingsSection::ALL {
            for (width, height) in [
                (390.0, 844.0),
                (768.0, 1024.0),
                (1280.0, 800.0),
                (1440.0, 900.0),
            ] {
                let mut app = app_with_workspace(Workspace::new());
                app.workspace = None;
                app.settings_section = section;
                let ctx = egui::Context::default();
                crate::style::configure(&ctx);
                for _ in 0..2 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        ..Default::default()
                    };
                    let _ = ctx.run_ui(input, |ui| {
                        app.app_window_ui(AppWindow::Settings, ui);
                        assert!(
                            ui.min_rect().right() <= width,
                            "Overflow in {} at {width}px",
                            section.label()
                        );
                    });
                }
            }
        }
    }

    #[test]
    fn cancelling_dirty_replacement_keeps_workspace() {
        let mut workspace = Workspace::new();
        workspace.document.name = "Keep this workspace".to_owned();
        let mut app = app_with_workspace(workspace);

        app.command(Command::OpenWorkspace);
        assert_eq!(app.pending_action, Some(PendingAction::Open));
        app.command(Command::CancelPending);

        assert!(app.pending_action.is_none());
        assert_eq!(
            app.workspace.as_ref().unwrap().document.name,
            "Keep this workspace"
        );
    }

    #[test]
    fn failed_open_after_discard_keeps_current_workspace() {
        let mut workspace = Workspace::new();
        workspace.document.name = "Keep this workspace".to_owned();
        let mut app = app_with_workspace(workspace);
        let directory = tempfile::tempdir().unwrap();
        let missing_path = directory
            .path()
            .join(format!("missing.{WORKSPACE_EXTENSION}"));

        app.command(Command::OpenPath(missing_path));
        assert!(matches!(
            app.pending_action,
            Some(PendingAction::OpenPath(_))
        ));
        app.command(Command::ConfirmDiscard);

        assert!(app.pending_action.is_none());
        assert_eq!(
            app.workspace.as_ref().unwrap().document.name,
            "Keep this workspace"
        );
        assert!(app.status_is_error);
    }
}
