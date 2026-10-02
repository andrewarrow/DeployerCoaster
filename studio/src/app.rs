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

pub struct App {
    preferences: Preferences,
    preferences_path: Option<PathBuf>,
    workspace: Option<Workspace>,
    pending_action: Option<PendingAction>,
    quit: bool,
    show_settings: bool,
    show_about: bool,
    status: Option<String>,
    status_is_error: bool,
    play_store: PlayStore,
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
            show_settings: false,
            show_about: false,
            status_is_error: preference_error.is_some(),
            status: preference_error,
            play_store: PlayStore::default(),
        };
        if let Some(path) = initial_path {
            app.open_path(path);
        }
        app
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ctx.set_theme(self.preferences.appearance.theme());
        #[cfg(not(target_os = "macos"))]
        if self.pending_action.is_none() {
            self.keyboard_shortcuts(&ctx);
        }
        let narrow_layout = ui.available_width() < 800.0;

        let mut selected_command = None;
        #[cfg(not(target_os = "macos"))]
        egui::Panel::top("app_menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
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

        if self.preferences.show_activity && (self.workspace.is_some() || self.status.is_some()) {
            egui::Panel::bottom("activity_panel")
                .show_separator_line(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(self.status.as_deref().unwrap_or("Ready"));
                    });
                });
        }

        if !narrow_layout
            && self.preferences.show_sidebar
            && self.workspace.is_some()
            && !self.preferences.recent_workspaces.is_empty()
        {
            egui::Panel::left("recent_workspaces")
                .default_size(190.0)
                .show(ui, |ui| {
                    ui.heading("Recent workspaces");
                    ui.add_space(4.0);
                    for path in self.preferences.recent_workspaces.clone() {
                        let label = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or_else(|| path.to_str().unwrap_or("Workspace"));
                        if ui
                            .add(egui::Button::new(label).truncate())
                            .on_hover_text(path.display().to_string())
                            .clicked()
                        {
                            selected_command = Some(Command::OpenPath(path));
                        }
                    }
                });
        }

        if !narrow_layout && self.preferences.show_inspector && self.workspace.is_some() {
            egui::Panel::right("workspace_details")
                .default_size(220.0)
                .show(ui, |ui| {
                    ui.heading("Workspace details");
                    if let Some(workspace) = &self.workspace {
                        ui.label("Path");
                        ui.label(
                            workspace
                                .path
                                .as_deref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| "Not saved".to_owned()),
                        );
                        ui.add_space(8.0);
                        ui.label("Format");
                        ui.label(format!(".{}", WORKSPACE_EXTENSION));
                        ui.add_space(8.0);
                        ui.label("State");
                        ui.label(if workspace.is_dirty() {
                            "Unsaved changes"
                        } else {
                            "Saved"
                        });
                    }
                });
        }

        egui::CentralPanel::default_margins().show(ui, |ui| {
            if let Some(workspace) = &mut self.workspace {
                if ui.available_width() < 440.0 {
                    ui.label("Workspace name");
                    ui.add(
                        egui::TextEdit::singleline(&mut workspace.document.name)
                            .desired_width(ui.available_width().min(260.0))
                            .hint_text("Workspace name"),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Workspace name");
                        ui.add(
                            egui::TextEdit::singleline(&mut workspace.document.name)
                                .desired_width(ui.available_width().min(260.0))
                                .hint_text("Workspace name"),
                        );
                    });
                }
                if narrow_layout {
                    ui.add_space(8.0);
                    ui.label("Path");
                    ui.label(
                        workspace
                            .path
                            .as_deref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "Not saved".to_owned()),
                    );
                    ui.add_space(8.0);
                    ui.label("State");
                    ui.label(if workspace.is_dirty() {
                        "Unsaved changes"
                    } else {
                        "Saved"
                    });
                } else if !self.preferences.show_inspector && workspace.is_dirty() {
                    ui.add_space(8.0);
                    ui.label("Unsaved changes");
                }
            } else {
                self.play_store.ui(ui);
            }

            if let Some(status) = &self.status
                && (self.status_is_error || !self.preferences.show_activity)
            {
                ui.add_space(12.0);
                if self.status_is_error {
                    ui.colored_label(ui.visuals().error_fg_color, status);
                } else {
                    ui.weak(status);
                }
            }
        });

        if let Some(command) = selected_command {
            self.command(command);
        }
        self.settings_window(&ctx);
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
            Command::Settings => self.show_settings = true,
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

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
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
                    ctx.set_theme(appearance.theme());
                    self.save_preferences();
                }
            });
        self.show_settings = open;
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
                ui.label(format!("Version {}", metadata::version_label()));
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
            show_settings: false,
            show_about: false,
            status: None,
            status_is_error: false,
            play_store: PlayStore::default(),
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
