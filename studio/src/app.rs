use std::path::{Path, PathBuf};

#[cfg(not(target_os = "macos"))]
use egui::{Key, Modifiers};

use crate::{
    commands::{Command, PendingAction},
    metadata,
    play_store::PlayStore,
    preferences::{Appearance, Preferences},
    wizard::{Wizard, WizardAction},
    workspace::{WORKSPACE_EXTENSION, Workspace},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SettingsSection {
    #[default]
    General,
    Android,
    Apple,
    ChatGPT,
    Dynadot,
}

impl SettingsSection {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Android,
        Self::Apple,
        Self::ChatGPT,
        Self::Dynadot,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Android => "Android",
            Self::Apple => "Apple",
            Self::ChatGPT => "ChatGPT",
            Self::Dynadot => "Dynadot",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppWindow {
    Settings,
    Wizard,
    Android,
    Apple,
    Sales,
    Document(u64),
}

impl AppWindow {
    pub fn title(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Wizard => "Wizard",
            Self::Android => "Android",
            Self::Apple => "Apple",
            Self::Sales => "Sales",
            Self::Document(_) => "App",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppWindowAction {
    None,
    FocusMain,
    Close,
}

struct AppDocument {
    id: u64,
    app: crate::apple::AppleApp,
    account: Option<String>,
    view: crate::ios_view::IosView,
}

pub struct App {
    preferences: Preferences,
    preferences_path: Option<PathBuf>,
    workspace: Option<Workspace>,
    pending_action: Option<PendingAction>,
    quit: bool,
    studio_view: crate::studio_view::StudioView,
    documents: Vec<AppDocument>,
    next_document_id: u64,
    age_rating_clipboard: crate::ios_view::AgeRatingClipboard,
    dashboard: crate::dashboard::Dashboard,
    codex: crate::codex_ui::Codex,
    codex_default_root: PathBuf,
    settings_section: SettingsSection,
    show_about: bool,
    status: Option<String>,
    status_is_error: bool,
    play_store: PlayStore,
    apple: crate::apple::AppleSettings,
    apple_store: crate::apple::AppleStore,
    dynadot: crate::dynadot::Dynadot,
    requested_app_windows: Vec<AppWindow>,
    wizard: Wizard,
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

        let codex_default_root = std::env::current_dir()
            .ok()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let codex_root = codex_workspace_root(initial_path.as_deref(), &codex_default_root);
        let mut app = Self {
            preferences,
            preferences_path,
            workspace: None,
            pending_action: None,
            quit: false,
            studio_view: crate::studio_view::StudioView::default(),
            documents: Vec::new(),
            next_document_id: 0,
            age_rating_clipboard: Default::default(),
            dashboard: crate::dashboard::Dashboard::load(),
            codex: crate::codex_ui::Codex::new(codex_root),
            codex_default_root,
            settings_section: SettingsSection::default(),
            show_about: false,
            status_is_error: preference_error.is_some(),
            status: preference_error,
            play_store: PlayStore::load(),
            apple: crate::apple::AppleSettings::load(),
            apple_store: crate::apple::AppleStore::default(),
            dynadot: crate::dynadot::Dynadot::load(),
            requested_app_windows: Vec::new(),
            wizard: Wizard::load(),
        };
        if let Some(path) = initial_path {
            app.open_path(path);
        }
        app
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ctx.set_theme(self.preferences.appearance.theme());
        self.apple.poll(&ctx);
        #[cfg(not(target_os = "macos"))]
        if self.pending_action.is_none()
            && let Some(command) = self.keyboard_shortcuts(&ctx)
        {
            self.command(command);
        }
        #[cfg(not(target_os = "macos"))]
        if let Some(command) = self.desktop_menu(ui, None) {
            self.command(command);
        }
        if crate::native_titlebar_active(&ctx) {
            egui::Panel::top("native_window_title")
                .exact_size(32.0)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    let response = ui.interact(
                        rect,
                        ui.id().with("window_drag"),
                        egui::Sense::click_and_drag(),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        metadata::APP_NAME,
                        egui::FontId::new(13.0, egui::FontFamily::Name("semibold".into())),
                        ui.visuals().text_color(),
                    );
                    if response.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    if response.double_clicked() {
                        let maximized =
                            ui.input(|input| input.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                });
        }
        self.apple_store.set_credentials(self.apple.credentials());
        if let Some(command) = self.studio_view.ui(ui, &mut self.apple_store) {
            self.command(command);
        }
        if let Some(app) = self.studio_view.take_app_request() {
            self.open_document(app);
        }
        self.about_window(&ctx);
        self.discard_modal(&ctx);
    }

    fn open_document(&mut self, app: crate::apple::AppleApp) {
        let account = self.apple_store.selection_account();
        // Reopening an app raises its existing window and keeps its edits intact.
        let id = if let Some(document) = self
            .documents
            .iter()
            .find(|document| document.account == account && document.app.id == app.id)
        {
            document.id
        } else {
            let id = self.next_document_id;
            self.next_document_id += 1;
            self.documents.push(AppDocument {
                id,
                app,
                account,
                view: crate::ios_view::IosView::with_age_rating_clipboard(
                    self.age_rating_clipboard.clone(),
                ),
            });
            id
        };
        self.requested_app_windows.push(AppWindow::Document(id));
    }

    pub fn window_title(&self, window: AppWindow) -> String {
        match window {
            AppWindow::Document(id) => self
                .documents
                .iter()
                .find(|document| document.id == id)
                .map(|document| format!("{} — {}", document.app.title(), metadata::APP_NAME))
                .unwrap_or_else(|| metadata::APP_NAME.to_owned()),
            _ => window.title().to_owned(),
        }
    }

    // Keep the previous main view intact while the new platform views are built.
    #[allow(dead_code)]
    fn legacy_ui(&mut self, ui: &mut egui::Ui) {
        self.poll_codex(ui.ctx());
        self.dynadot.poll();
        self.play_store.poll();
        self.apple.poll(ui.ctx());
        self.apple_store.set_credentials(self.apple.credentials());
        self.apple_store
            .set_reporting_vendor(self.apple.saved_vendor_number());
        self.apple_store.poll();
        let ctx = ui.ctx().clone();
        ctx.set_theme(self.preferences.appearance.theme());
        #[cfg(not(target_os = "macos"))]
        if self.pending_action.is_none()
            && let Some(command) = self.keyboard_shortcuts(&ctx)
        {
            self.command(command);
        }

        let mut selected_command = None;
        #[cfg(not(target_os = "macos"))]
        {
            selected_command = self.desktop_menu(ui, None);
        }

        egui::Panel::top("chatgpt_connection")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ui, |ui| self.codex.toolbar(ui));

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
        let workspace_path = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.path.clone());
        let chat_full_width = self.codex.open && ui.available_width() < 1000.0;
        if chat_full_width {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(16)))
                .show(ui, |ui| self.codex.chat_ui(ui, workspace_path.as_deref()));
        } else {
            if self.codex.open {
                egui::Panel::right("codex_chat")
                    .resizable(true)
                    .default_size(360.0)
                    .size_range(300.0..=480.0)
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(12)))
                    .show(ui, |ui| self.codex.chat_ui(ui, workspace_path.as_deref()));
            }
            if let Some(command) = self.dashboard.ui(
                ui,
                &mut self.play_store,
                &mut self.apple_store,
                &mut self.dynadot,
                self.preferences.show_sidebar,
            ) {
                selected_command = Some(command);
            }
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

    fn poll_codex(&mut self, ctx: &egui::Context) {
        let root = codex_workspace_root(
            self.workspace
                .as_ref()
                .and_then(|workspace| workspace.path.as_deref()),
            &self.codex_default_root,
        );
        self.codex.set_root(&root);
        if self.codex.poll(ctx) {
            self.refresh_workspace_after_codex();
        }
        if let Some(result) = self.codex.take_rewritten_description() {
            self.wizard.receive_rewritten_description(result);
        }
        if let Some(root) = self.codex.take_created_website() {
            self.wizard.receive_created_website(root);
        }
    }

    fn refresh_workspace_after_codex(&mut self) {
        if let Some(workspace) = &self.workspace {
            if workspace.is_dirty() {
                self.set_status("Codex finished. Unsaved workspace edits were kept; reopen the file to review changes on disk.");
            } else if let Some(path) = workspace.path.clone() {
                match Workspace::open(path) {
                    Ok(updated) => self.workspace = Some(updated),
                    Err(error) => self.set_error(format!(
                        "Could not reload the workspace after Codex: {error}"
                    )),
                }
            }
        }
    }

    pub fn command(&mut self, command: Command) {
        match command {
            Command::OpenWorkspace => self.studio_view.open(),
            Command::OpenPath(path) => self.request_action(PendingAction::OpenPath(path)),
            Command::Save => self.save(),
            Command::SaveAs => self.save_as(),
            // Document close commands are handled by the focused native window.
            Command::CloseWorkspace => {}
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
            Command::Wizard => self.requested_app_windows.push(AppWindow::Wizard),
            Command::Android => self.requested_app_windows.push(AppWindow::Android),
            Command::Apple => self.requested_app_windows.push(AppWindow::Apple),
            Command::Sales => self.requested_app_windows.push(AppWindow::Sales),
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

    pub fn app_window_ui(&mut self, window: AppWindow, ui: &mut egui::Ui) -> AppWindowAction {
        self.poll_codex(ui.ctx());
        self.apple.poll(ui.ctx());
        ui.ctx().set_theme(self.preferences.appearance.theme());
        if let AppWindow::Document(id) = window {
            #[cfg(not(target_os = "macos"))]
            {
                let shortcut = if self.pending_action.is_none() {
                    self.keyboard_shortcuts(ui.ctx())
                        .filter(|command| !matches!(command, Command::Save | Command::SaveAs))
                } else {
                    None
                };
                let command = self.desktop_menu(ui, Some(window)).or(shortcut);
                if matches!(command, Some(Command::CloseWorkspace)) {
                    return AppWindowAction::Close;
                }
                if let Some(command) = command {
                    let open = matches!(command, Command::OpenWorkspace);
                    self.command(command);
                    if open {
                        return AppWindowAction::FocusMain;
                    }
                }
            }
            let credentials = self.apple.credentials();
            self.apple_store.set_credentials(credentials.clone());
            let account = self.apple_store.selection_account();
            let Some(document) = self.documents.iter_mut().find(|document| document.id == id)
            else {
                return AppWindowAction::Close;
            };
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
                .show(ui, |ui| {
                    if document.account != account {
                        document.view = crate::ios_view::IosView::with_age_rating_clipboard(self.age_rating_clipboard.clone());
                        if crate::native_titlebar_active(ui.ctx()) {
                            ui.add_space(44.0);
                        }
                        ui.label("This app belongs to a different Apple account. Reopen it from File → Open App.");
                    } else {
                        document.view.ui(ui, Some((&document.app.id, document.app.title())), credentials.as_ref());
                    }
                });
            self.about_window(ui.ctx());
            self.discard_modal(ui.ctx());
            return AppWindowAction::None;
        }
        if window == AppWindow::Settings {
            self.store_settings(ui);
            return AppWindowAction::None;
        }
        if window == AppWindow::Wizard {
            return match self
                .wizard
                .ui(ui, &mut self.codex, &self.codex_default_root)
            {
                WizardAction::Finished => AppWindowAction::Close,
                WizardAction::None => AppWindowAction::None,
            };
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
                AppWindow::Sales => {
                    self.apple_store.set_credentials(self.apple.credentials());
                    self.apple_store
                        .set_reporting_vendor(self.apple.saved_vendor_number());
                    self.apple_store.prepare_dashboard(ui.ctx());
                    egui::ScrollArea::vertical()
                        .id_salt("sales_reports")
                        .auto_shrink([false, false])
                        .show(ui, |ui| settings = self.apple_store.sales_ui(ui, None));
                }
                AppWindow::Wizard => unreachable!(),
                AppWindow::Settings | AppWindow::Document(_) => unreachable!(),
            });
        if settings {
            self.settings_section = SettingsSection::Apple;
            self.requested_app_windows.push(AppWindow::Settings);
        }
        if settings {
            AppWindowAction::FocusMain
        } else {
            AppWindowAction::None
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn desktop_menu(&mut self, ui: &mut egui::Ui, window: Option<AppWindow>) -> Option<Command> {
        let mut selected_command = None;
        egui::Panel::top("app_menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    for (label, command) in [
                        ("Wizard", Command::Wizard),
                        ("Android", Command::Android),
                        ("Apple", Command::Apple),
                        ("Sales", Command::Sales),
                    ] {
                        if ui.button(label).clicked() {
                            selected_command = Some(command);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Open App…").clicked() {
                        selected_command = Some(Command::OpenWorkspace);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            window.is_none() && self.workspace.is_some(),
                            egui::Button::new("Save"),
                        )
                        .clicked()
                    {
                        selected_command = Some(Command::Save);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            window.is_none() && self.workspace.is_some(),
                            egui::Button::new("Save As…"),
                        )
                        .clicked()
                    {
                        selected_command = Some(Command::SaveAs);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            matches!(window, Some(AppWindow::Document(_))),
                            egui::Button::new("Close App"),
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
        selected_command
    }

    pub fn app_window_closed(&mut self, window: AppWindow) {
        if window == AppWindow::Wizard {
            self.codex.abandon_wizard_generation();
        }
        if let AppWindow::Document(id) = window {
            self.documents.retain(|document| document.id != id);
        }
    }

    pub fn window_error(&mut self, window: AppWindow) {
        let error = format!(
            "Could not open the {} window. Please try again.",
            window.title()
        );
        self.studio_view.set_error(error.clone());
        self.set_error(error);
    }

    #[cfg(target_os = "macos")]
    pub fn native_menu_state(&self, window: Option<AppWindow>) -> crate::macos::MenuState {
        crate::macos::MenuState {
            has_workspace: window.is_none() && self.workspace.is_some(),
            has_selected_app: matches!(window, Some(AppWindow::Document(_))),
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
    fn keyboard_shortcuts(&self, ctx: &egui::Context) -> Option<Command> {
        ctx.input_mut(|input| {
            let command = input.modifiers.command;
            if !command {
                return None;
            }
            if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
                Some(Command::SaveAs)
            } else if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::W) {
                Some(Command::CloseWorkspace)
            } else if input.consume_key(Modifiers::COMMAND, Key::O) {
                Some(Command::OpenWorkspace)
            } else if input.consume_key(Modifiers::COMMAND, Key::S) {
                Some(Command::Save)
            } else if input.consume_key(Modifiers::COMMAND, Key::Q) {
                Some(Command::Quit)
            } else {
                None
            }
        })
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
            PendingAction::OpenPath(path) => self.open_path(path),
            PendingAction::Quit => self.quit = true,
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match Workspace::open(path.clone()) {
            Ok(workspace) => {
                self.codex.return_to_workspace();
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
                            SettingsSection::ChatGPT => self.codex.settings_ui(ui),
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
            PendingAction::OpenPath(_) => "open another workspace",
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

fn codex_workspace_root(workspace_path: Option<&Path>, default_root: &Path) -> PathBuf {
    workspace_path
        .and_then(Path::parent)
        .and_then(|parent| parent.canonicalize().ok())
        .unwrap_or_else(|| default_root.to_owned())
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
            studio_view: crate::studio_view::StudioView::default(),
            documents: Vec::new(),
            next_document_id: 0,
            age_rating_clipboard: Default::default(),
            dashboard: crate::dashboard::Dashboard::default(),
            codex: crate::codex_ui::Codex::test_disconnected(std::env::current_dir().unwrap()),
            codex_default_root: std::env::current_dir().unwrap(),
            settings_section: SettingsSection::default(),
            show_about: false,
            status: None,
            status_is_error: false,
            play_store: PlayStore::default(),
            apple: crate::apple::AppleSettings::default(),
            apple_store: crate::apple::AppleStore::default(),
            dynadot: crate::dynadot::Dynadot::default(),
            requested_app_windows: Vec::new(),
            wizard: Wizard::default(),
        }
    }

    fn ios_app(id: &str, name: &str) -> crate::apple::AppleApp {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": name, "bundleId": format!("com.example.app{id}")
        }))
        .unwrap()
    }

    #[test]
    fn apps_open_independent_windows_and_reopening_focuses_the_existing_one() {
        let mut app = app_with_workspace(Workspace::new());
        app.open_document(ios_app("123", "First App"));
        let first = app.take_app_window_request().unwrap();
        app.command(Command::OpenWorkspace);
        app.open_document(ios_app("456", "Second App"));
        let second = app.take_app_window_request().unwrap();
        assert_ne!(first, second);
        assert_eq!(app.documents.len(), 2);
        assert_eq!(
            app.window_title(first),
            format!("First App — {}", metadata::APP_NAME)
        );
        assert_eq!(
            app.window_title(second),
            format!("Second App — {}", metadata::APP_NAME)
        );

        app.open_document(ios_app("123", "First App"));
        assert_eq!(app.take_app_window_request(), Some(first));
        assert_eq!(app.documents.len(), 2);

        // Closing either app leaves the other open, and never quits the application.
        app.app_window_closed(first);
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.documents[0].app.id, "456");
        assert!(!app.should_quit());
        app.open_document(ios_app("123", "First App"));
        assert_ne!(app.take_app_window_request(), Some(first));
        assert_eq!(app.documents.len(), 2);
        app.app_window_closed(second);
        assert_eq!(app.documents[0].app.id, "123");
    }

    #[test]
    fn document_windows_are_scoped_to_the_account_that_opened_them() {
        let mut app = app_with_workspace(Workspace::new());
        app.apple_store
            .set_credentials(Some(crate::apple::AppleCredentials::test_credentials(
                "one",
            )));
        app.open_document(ios_app("123", "First Account App"));
        let first = app.take_app_window_request().unwrap();
        app.apple_store
            .set_credentials(Some(crate::apple::AppleCredentials::test_credentials(
                "two",
            )));
        app.open_document(ios_app("123", "Second Account App"));
        let second = app.take_app_window_request().unwrap();
        assert_ne!(first, second);
        assert_ne!(app.documents[0].account, app.documents[1].account);

        // With the account disconnected, old windows show no cached app content.
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(app.app_window_ui(first, ui), AppWindowAction::None);
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect();
        assert!(
            labels
                .iter()
                .any(|text| text.contains("different Apple account"))
        );
        assert!(!labels.iter().any(|text| text.contains("First Account App")));
        assert_eq!(app.documents.len(), 2);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_close_app_menu_tracks_the_focused_document_window() {
        let mut app = app_with_workspace(Workspace::new());
        app.open_document(ios_app("123", "First App"));
        let document = app.take_app_window_request().unwrap();
        assert!(app.native_menu_state(Some(document)).has_selected_app);
        assert!(!app.native_menu_state(None).has_selected_app);
        assert!(
            !app.native_menu_state(Some(AppWindow::Settings))
                .has_selected_app
        );
        assert!(!app.native_menu_state(Some(document)).has_workspace);
        assert!(app.native_menu_state(None).has_workspace);
    }

    #[test]
    fn codex_refresh_reloads_saved_workspaces_and_preserves_unsaved_edits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.dcstudio");
        let workspace = Workspace::new();
        workspace.document.save(&path).unwrap();
        let mut app = app_with_workspace(Workspace::open(path.clone()).unwrap());
        let mut updated = workspace.document.clone();
        updated.name = "Codex edit".into();
        updated.save(&path).unwrap();
        app.refresh_workspace_after_codex();
        assert_eq!(app.workspace.as_ref().unwrap().document.name, "Codex edit");
        assert!(!app.workspace.as_ref().unwrap().is_dirty());
        app.workspace.as_mut().unwrap().document.name = "My unsaved edit".into();
        updated.name = "Second Codex edit".into();
        updated.save(&path).unwrap();
        app.refresh_workspace_after_codex();
        assert_eq!(
            app.workspace.as_ref().unwrap().document.name,
            "My unsaved edit"
        );
        assert!(app.workspace.as_ref().unwrap().is_dirty());
    }

    #[test]
    fn codex_invalid_workspace_edits_keep_the_last_valid_document_and_directory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.dcstudio");
        Workspace::new().document.save(&path).unwrap();
        let mut app = app_with_workspace(Workspace::open(path.clone()).unwrap());
        std::fs::write(&path, "invalid JSON").unwrap();
        app.refresh_workspace_after_codex();
        assert_eq!(app.workspace.as_ref().unwrap().document.name, "Untitled");
        assert!(app.status_is_error);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            codex_workspace_root(Some(&path), Path::new("/elsewhere")),
            directory.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn sales_setup_opens_the_apple_settings_section() {
        let mut app = app_with_workspace(Workspace::new());
        app.command(Command::AppleSettings);
        assert!(app.settings_section == SettingsSection::Apple);
        assert_eq!(app.take_app_window_request(), Some(AppWindow::Settings));
    }

    #[test]
    fn closing_and_reopening_wizard_preserves_review_progress() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wizard.json");
        let saved = serde_json::json!({
            "step": 3,
            "app_name": "Example",
            "original_description": "Original description",
            "rewritten_description": {
                "name": "Example",
                "original": "Original description",
                "description": "Example rewritten description"
            },
            "website_started": false
        });
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut app = app_with_workspace(Workspace::new());
        app.wizard = Wizard::from_path(path.clone());
        app.app_window_closed(AppWindow::Wizard);
        app.command(Command::Wizard);
        assert_eq!(app.take_app_window_request(), Some(AppWindow::Wizard));
        assert_eq!(serde_json::to_value(&app.wizard).unwrap(), saved);
        assert_eq!(
            serde_json::to_value(Wizard::from_path(path)).unwrap(),
            saved
        );
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
    fn opening_app_picker_preserves_unsaved_workspace_without_a_file_dialog() {
        let mut workspace = Workspace::new();
        workspace.document.name = "Keep this workspace".to_owned();
        let mut app = app_with_workspace(workspace);

        app.command(Command::OpenWorkspace);
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
