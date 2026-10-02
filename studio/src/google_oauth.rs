mod api;
#[cfg(test)]
mod tests;

use api::{
    CONSOLE_FILE, ConsoleSession, OAuthClient, access_token, fetch_branding_icon_url,
    fetch_clients, fetch_projects,
};
#[cfg(test)]
use api::{CONSOLE_PATH, SESSION_ERROR};

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

use serde::Deserialize;

const PROJECTS_API: &str = "https://cloudresourcemanager.googleapis.com/v3/projects:search";

type ProjectsResult = Result<Vec<Project>, String>;

#[derive(Default)]
pub(crate) struct GoogleOAuth {
    projects: Vec<Project>,
    job: Option<Receiver<ProjectsResult>>,
    attempted: bool,
    loaded: bool,
    error: Option<String>,
    search: String,
    selected: Option<String>,
    clients: Vec<OAuthClient>,
    client_project: Option<String>,
    client_job: Option<Receiver<Result<Vec<OAuthClient>, String>>>,
    client_error: Option<String>,
    console_curl: String,
    session_feedback: Option<String>,
    icons: crate::app_icons::AppIcons,
    branding_job: Option<BrandingJob>,
    branding_attempted: bool,
}

struct BrandingJob {
    receiver: Receiver<(String, egui::ColorImage)>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for BrandingJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    project_id: String,
    display_name: String,
    name: String,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ProjectPage {
    projects: Vec<Project>,
    next_page_token: String,
}

impl GoogleOAuth {
    #[cfg(test)]
    pub(crate) fn test_connection() -> Self {
        Self {
            attempted: true,
            loaded: true,
            branding_attempted: true,
            ..Default::default()
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        self.attempted = true;
        self.error = None;
        let (sender, receiver) = mpsc::channel();
        self.job = Some(receiver);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = access_token().and_then(|token| {
                let http = reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(20))
                    .build()
                    .map_err(|_| "Could not start the Google connection.".to_owned())?;
                fetch_projects(&http, &token, PROJECTS_API)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.try_recv() {
            Ok(result) => {
                self.job = None;
                match result {
                    Ok(projects) => {
                        self.projects = projects;
                        self.branding_job = None;
                        self.branding_attempted = false;
                        self.icons = Default::default();
                        self.loaded = true;
                        self.error = None;
                        if self
                            .selected
                            .as_ref()
                            .is_some_and(|id| !self.projects.iter().any(|p| &p.project_id == id))
                        {
                            self.selected = None;
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.error = Some("Project sync stopped. Try refreshing again.".into());
            }
        }
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        self.poll_branding();
        if self.loaded && self.job.is_none() && !self.branding_attempted {
            self.refresh_branding(ui.ctx());
        }
        if !self.attempted {
            self.refresh(ui.ctx());
        }
        ui.horizontal(|ui| {
            ui.heading("Google OAuth");
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::new("Refresh projects").min_size(egui::vec2(120.0, 44.0)),
                )
                .clicked()
            {
                self.refresh(ui.ctx());
            }
        });
        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading projects…");
            });
        }
        if let Some(error) = &self.error {
            ui.add(egui::Label::new(error).wrap());
            egui::CollapsingHeader::new("Google connection").show(ui, |ui| {
                ui.label("Uses the active Google Cloud CLI account.");
                ui.label("To sign in, run this in Terminal, then refresh:");
                ui.code("gcloud auth login");
                ui.hyperlink_to(
                    "Google Cloud CLI setup",
                    "https://docs.cloud.google.com/sdk/docs/install",
                );
            });
        }
        if self.loaded && self.projects.is_empty() {
            ui.label("No active Google Cloud projects are visible to this account.");
            return;
        }
        ui.add_space(8.0);
        if ui.available_width() >= 800.0 {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(280.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.project_list(ui),
                );
                ui.separator();
                ui.vertical(|ui| self.clients_ui(ui));
            });
        } else {
            self.project_picker(ui);
            ui.add_space(12.0);
            self.clients_ui(ui);
        }
    }

    fn project_list(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(format!("Projects ({})", self.projects.len())).strong());
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search projects…")
                .desired_width(ui.available_width()),
        );
        let query = self.search.trim().to_lowercase();
        let mut matches = 0;
        egui::ScrollArea::vertical()
            .id_salt("google_projects")
            .show(ui, |ui| {
                for project in &self.projects {
                    if !project.matches(&query) {
                        continue;
                    }
                    matches += 1;
                    if project_row(
                        ui,
                        project,
                        self.selected.as_ref() == Some(&project.project_id),
                        &mut self.icons,
                    )
                    .clicked()
                    {
                        self.selected = Some(project.project_id.clone());
                    }
                }
                if matches == 0 && !query.is_empty() {
                    ui.label("No projects match your search.");
                }
            });
    }

    fn project_picker(&mut self, ui: &mut egui::Ui) {
        ui.label("Project");
        let selected = self
            .projects
            .iter()
            .find(|p| Some(&p.project_id) == self.selected.as_ref());
        egui::ComboBox::from_id_salt("google_project_picker")
            .selected_text(
                selected
                    .map(|p| p.display_name.as_str())
                    .unwrap_or("Select a project"),
            )
            .width(ui.available_width().min(400.0))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search projects…"));
                let query = self.search.trim().to_lowercase();
                for project in &self.projects {
                    if project.matches(&query)
                        && project_row(
                            ui,
                            project,
                            self.selected.as_ref() == Some(&project.project_id),
                            &mut self.icons,
                        )
                        .clicked()
                    {
                        self.selected = Some(project.project_id.clone());
                    }
                }
            });
    }

    fn clients_ui(&mut self, ui: &mut egui::Ui) {
        let selected = self
            .projects
            .iter()
            .find(|p| Some(&p.project_id) == self.selected.as_ref())
            .cloned();
        let Some(project) = selected else {
            ui.label("Select a project to view its OAuth clients.");
            return;
        };
        if self.client_project.as_ref() != Some(&project.project_id) {
            // Drop the receiver so results from the previous project cannot be applied.
            self.client_job = None;
            self.clients.clear();
            self.client_project = Some(project.project_id.clone());
            self.refresh_clients(&project, ui.ctx());
        }
        self.poll_clients();
        ui.heading(&project.display_name);
        ui.add(egui::Label::new(format!("{} · {}", project.project_id, project.name)).wrap());
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.client_job.is_none(),
                    egui::Button::new("Refresh clients").min_size(egui::vec2(120.0, 44.0)),
                )
                .clicked()
            {
                self.refresh_clients(&project, ui.ctx());
            }
            let mut url =
                reqwest::Url::parse("https://console.cloud.google.com/auth/clients").unwrap();
            url.query_pairs_mut()
                .append_pair("project", &project.project_id);
            ui.hyperlink_to("Open in Google Cloud", url.as_str());
        });
        if self.client_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading OAuth clients…");
            });
        }
        if let Some(error) = &self.client_error {
            ui.add(egui::Label::new(error).wrap());
        }
        egui::CollapsingHeader::new("Console connection")
            .default_open(self.client_error.is_some())
            .show(ui, |ui| {
                ui.add(egui::Label::new("In Google Cloud → Google Auth Platform → Clients, reload with Developer Tools → Network open. Copy the SERVICE_USAGE_GRAPHQL:batchGraphql client-list request as cURL and paste it here.").wrap());
                let label = ui.label("Client-list request (cURL)");
                ui.add(egui::TextEdit::singleline(&mut self.console_curl).password(true).desired_width(ui.available_width())).labelled_by(label.id);
                if ui.add_enabled(!self.console_curl.trim().is_empty(), egui::Button::new("Save and connect").min_size(egui::vec2(140.0, 44.0))).clicked() {
                    let result = ConsoleSession::parse_curl(&self.console_curl).and_then(|session| {
                        let path = crate::storage::credential_path(CONSOLE_FILE)?;
                        let bytes = serde_json::to_vec(&session).map_err(|_| "Could not encode the Console session.".to_owned())?;
                        crate::storage::save_credentials(&path, &bytes)
                    });
                    match result {
                        Ok(()) => {
                            self.console_curl.clear();
                            self.clients.clear();
                            self.client_job = None;
                            self.session_feedback = Some("Console session saved on this device.".into());
                            self.branding_job = None;
                            self.branding_attempted = false;
                            self.icons = Default::default();
                            self.refresh_clients(&project, ui.ctx());
                        }
                        Err(error) => self.client_error = Some(error),
                    }
                }
                if let Some(message) = &self.session_feedback { ui.label(message); }
            });
        ui.add_space(8.0);
        if self.client_job.is_none() && self.client_error.is_none() && self.clients.is_empty() {
            ui.label("This project has no OAuth clients.");
        }
        client_list_ui(ui, &self.clients);
    }

    fn refresh_clients(&mut self, project: &Project, ctx: &egui::Context) {
        if self.client_job.is_some() {
            return;
        }
        self.client_error = None;
        let (sender, receiver) = mpsc::channel();
        self.client_job = Some(receiver);
        let project = project.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ConsoleSession::load().and_then(|session| {
                let http = reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(20))
                    .build()
                    .map_err(|_| "Could not start the Console connection.".to_owned())?;
                fetch_clients(&http, &session, &project)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn poll_clients(&mut self) {
        let Some(job) = &self.client_job else {
            return;
        };
        match job.try_recv() {
            Ok(result) => {
                self.client_job = None;
                match result {
                    Ok(clients) => {
                        self.clients = clients;
                        self.client_error = None;
                    }
                    Err(error) => self.client_error = Some(error),
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.client_job = None;
                self.client_error = Some("Client sync stopped. Refresh to try again.".into());
            }
        }
    }

    fn refresh_branding(&mut self, ctx: &egui::Context) {
        self.branding_attempted = true;
        let Ok(session) = ConsoleSession::load() else {
            return;
        };
        let projects = self.projects.clone();
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        self.branding_job = Some(BrandingJob {
            receiver,
            cancelled,
        });
        let ctx = ctx.clone();
        thread::spawn(move || {
            let Ok(http) = reqwest::blocking::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
            else {
                return;
            };
            for project in projects {
                if cancellation.load(Ordering::Relaxed) {
                    break;
                }
                // Branding is optional: a missing logo or inaccessible brand leaves the initial.
                let Some(image) = session
                    .branding_url(&project)
                    .ok()
                    .and_then(|url| {
                        fetch_branding_icon_url(&http, &session, &url)
                            .ok()
                            .flatten()
                    })
                    .and_then(|url| crate::app_icons::google_artwork(&http, &url, &cancellation))
                else {
                    continue;
                };
                if sender.send((project.project_id, image)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
    }

    fn poll_branding(&mut self) {
        while let Some(job) = &self.branding_job {
            match job.receiver.try_recv() {
                Ok((key, image)) => self.icons.insert_image(key, image),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.branding_job = None;
                    break;
                }
            }
        }
    }
}

fn project_row(
    ui: &mut egui::Ui,
    project: &Project,
    selected: bool,
    icons: &mut crate::app_icons::AppIcons,
) -> egui::Response {
    let icon_id = ui.make_persistent_id(("google_branding_icon", &project.project_id));
    let label = format!("{}\n{}", project.display_name, project.project_id);
    let response = egui::Button::new((
        egui::Atom::custom(icon_id, egui::vec2(36.0, 36.0)),
        label,
        egui::Atom::grow(),
    ))
    .selected(selected)
    .min_size(egui::vec2(ui.available_width(), 52.0))
    .wrap_mode(egui::TextWrapMode::Truncate)
    .atom_ui(ui);
    if let Some(rect) = response.rect(icon_id) {
        icons.paint_icon(ui, rect, &project.project_id, &project.display_name);
    }
    response.response
}

impl Project {
    fn matches(&self, query: &str) -> bool {
        self.display_name.to_lowercase().contains(query)
            || self.project_id.to_lowercase().contains(query)
    }
}

fn client_list_ui(ui: &mut egui::Ui, clients: &[OAuthClient]) {
    egui::ScrollArea::both()
        .id_salt("google_oauth_clients")
        .show(ui, |ui| {
            if ui.available_width() < 640.0 {
                for client in clients {
                    ui.label(egui::RichText::new(&client.display_name).strong());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(client.type_label());
                        ui.label(client.creation_time.get(..10).unwrap_or("—"));
                        if ui.button("Copy ID").clicked() {
                            ui.ctx().copy_text(client.client_id.clone());
                        }
                    });
                    ui.add(
                        egui::Label::new(egui::RichText::new(&client.client_id).monospace()).wrap(),
                    );
                    ui.add_space(12.0);
                }
            } else {
                egui::Grid::new("oauth_client_table")
                    .num_columns(5)
                    .spacing([16.0, 12.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for label in ["Name", "Created", "Type", "Client ID", ""] {
                            ui.label(egui::RichText::new(label).strong());
                        }
                        ui.end_row();
                        for client in clients {
                            ui.label(&client.display_name);
                            ui.label(client.creation_time.get(..10).unwrap_or("—"));
                            ui.label(client.type_label());
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&client.client_id).monospace(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&client.client_id);
                            if ui.button("Copy").on_hover_text("Copy client ID").clicked() {
                                ui.ctx().copy_text(client.client_id.clone());
                            }
                            ui.end_row();
                        }
                    });
            }
        });
}
