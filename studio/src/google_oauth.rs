use std::{
    collections::HashSet,
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

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

fn access_token() -> Result<String, String> {
    if let Ok(token) = std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN")
        && !token.trim().is_empty()
    {
        return Ok(token.trim().to_owned());
    }
    let mut executables = vec![PathBuf::from("gcloud")];
    if let Some(home) = dirs::home_dir() {
        executables.push(home.join("dev/google-cloud-sdk/bin/gcloud"));
        executables.push(home.join("google-cloud-sdk/bin/gcloud"));
    }
    executables.extend([
        PathBuf::from("/opt/homebrew/bin/gcloud"),
        PathBuf::from("/opt/homebrew/share/google-cloud-sdk/bin/gcloud"),
        PathBuf::from("/usr/local/bin/gcloud"),
    ]);
    for executable in executables {
        if let Ok(output) = Command::new(executable)
            .args(["auth", "print-access-token", "--quiet"])
            .output()
            && output.status.success()
            && let Ok(token) = String::from_utf8(output.stdout)
            && !token.trim().is_empty()
        {
            return Ok(token.trim().to_owned());
        }
    }
    Err("Sign in to Google Cloud with gcloud auth login, then refresh projects.".into())
}

fn fetch_projects(http: &reqwest::blocking::Client, token: &str, endpoint: &str) -> ProjectsResult {
    let mut projects = Vec::new();
    let mut page_token = String::new();
    let mut seen = HashSet::new();
    loop {
        let response = http
            .get(endpoint)
            .bearer_auth(token)
            .query(&[
                ("query", "state:ACTIVE"),
                ("pageSize", "100"),
                ("pageToken", &page_token),
            ])
            .send()
            .map_err(|_| {
                "Could not reach Google Cloud. Check your connection and refresh.".to_owned()
            })?;
        match response.status().as_u16() {
            200 => {}
            401 => return Err("Google Cloud sign-in expired. Run gcloud auth login and refresh.".into()),
            403 => return Err("Google Cloud denied project access. Check the active account, project permissions, and Resource Manager API access.".into()),
            429 => return Err("Google Cloud's rate limit was reached. Try refreshing later.".into()),
            _ => return Err("Google Cloud could not list projects. Try refreshing again.".into()),
        }
        let page: ProjectPage = response
            .json()
            .map_err(|_| "Google Cloud returned an invalid project list.".to_owned())?;
        projects.extend(page.projects);
        if page.next_page_token.is_empty() {
            projects.sort_by_cached_key(|p| (p.display_name.to_lowercase(), p.project_id.clone()));
            let mut ids = HashSet::new();
            projects.retain(|p| ids.insert(p.project_id.clone()));
            return Ok(projects);
        }
        if !seen.insert(page.next_page_token.clone()) || seen.len() > 1000 {
            return Err(
                "Google Cloud returned invalid project pagination. Refresh to try again.".into(),
            );
        }
        page_token = page.next_page_token;
    }
}

const CONSOLE_FILE: &str = "google-console-session.json";
const CONSOLE_PATH: &str =
    "/v3/entityServices/ServiceUsageEntityService/schemas/SERVICE_USAGE_GRAPHQL:batchGraphql";
const SESSION_ERROR: &str = "The Console session expired or lacks access to this project. Copy a fresh client-list request from Google Cloud and reconnect.";

// Browser session credentials are never logged or included in errors.
#[derive(Deserialize, Serialize)]
struct ConsoleSession {
    url: String,
    cookies: String,
    auth_user: String,
    body: serde_json::Value,
}

impl ConsoleSession {
    fn branding_url(&self, project: &Project) -> Result<reqwest::Url, String> {
        let number = project
            .name
            .strip_prefix("projects/")
            .and_then(|number| number.parse::<u64>().ok())
            .ok_or("Invalid Google Cloud project number.")?;
        let console_url =
            reqwest::Url::parse(&self.url).map_err(|_| "Invalid Console request URL.")?;
        let key = console_url
            .query_pairs()
            .find_map(|(name, value)| (name == "key").then_some(value.into_owned()))
            .ok_or("The Console request is missing its API key. Reconnect.")?;
        let mut url = reqwest::Url::parse(&format!(
            "https://clientauthconfig.clients6.google.com/v1/brands/lookupkey/brand/{number}"
        ))
        .unwrap();
        url.query_pairs_mut()
            .append_pair("key", &key)
            .append_pair("readMask", "iconUrl");
        Ok(url)
    }

    fn load() -> Result<Self, String> {
        let path = crate::storage::credential_path(CONSOLE_FILE)?;
        let bytes = std::fs::read(path).map_err(|_| {
            "Connect a Google Cloud Console session to list OAuth clients.".to_owned()
        })?;
        let session: Self = serde_json::from_slice(&bytes).map_err(|_| {
            "Could not load the Console session. Reconnect with a fresh request.".to_owned()
        })?;
        session.validate()?;
        Ok(session)
    }

    fn validate(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| "Invalid Console request URL.")?;
        if url.scheme() != "https"
            || url.host_str() != Some("cloudconsole-pa.clients6.google.com")
            || url.path() != CONSOLE_PATH
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some_and(|p| p != 443)
            || self.body["querySignature"].as_str().is_none()
            || !self.body["variables"].is_object()
            || self.auth_user.is_empty()
            || !self.auth_user.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(
                "Copy the OAuth client-list batchGraphql request from Google Cloud.".into(),
            );
        }
        self.authorization()?;
        Ok(())
    }

    fn parse_curl(input: &str) -> Result<Self, String> {
        let invalid = "Paste the entire OAuth client-list cURL request from Google Cloud.";
        let args = crate::console_cookies::curl_arguments(input).map_err(|_| invalid)?;
        if args.first().map(String::as_str) != Some("curl") {
            return Err(invalid.into());
        }
        let mut url = None;
        let mut cookies = None;
        let mut auth_user = "0".to_owned();
        let mut body = None;
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-H" | "--header" => {
                    let header = args.next().ok_or(invalid)?;
                    if let Some((name, value)) = header.split_once(':') {
                        match name.trim().to_ascii_lowercase().as_str() {
                            "cookie" => cookies = Some(value.trim().to_owned()),
                            "x-goog-authuser" => auth_user = value.trim().to_owned(),
                            _ => {}
                        }
                    }
                }
                "-b" | "--cookie" => cookies = Some(args.next().ok_or(invalid)?.clone()),
                "--data-raw" | "--data" | "--data-binary" | "-d" => {
                    body = Some(
                        serde_json::from_str::<serde_json::Value>(args.next().ok_or(invalid)?)
                            .map_err(|_| invalid)?,
                    );
                }
                "--url" => url = Some(args.next().ok_or(invalid)?.clone()),
                _ if arg.starts_with("https://") => url = Some(arg.clone()),
                _ => {}
            }
        }
        let session = Self {
            url: url.ok_or(invalid)?,
            cookies: cookies.ok_or(invalid)?,
            auth_user,
            body: body.ok_or(invalid)?,
        };
        session.validate()?;
        Ok(session)
    }

    fn authorization(&self) -> Result<String, String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "Could not read the system clock.")?
            .as_secs();
        let mut hashes = Vec::new();
        for (cookie_name, prefix) in [
            ("SAPISID", "SAPISIDHASH"),
            ("__Secure-1PAPISID", "SAPISID1PHASH"),
            ("__Secure-3PAPISID", "SAPISID3PHASH"),
        ] {
            if let Some(value) = self
                .cookies
                .split(';')
                .filter_map(|part| part.trim().split_once('='))
                .find_map(|(name, value)| {
                    (name == cookie_name && !value.is_empty()).then_some(value)
                })
            {
                let digest = Sha1::digest(
                    format!("{timestamp} {value} https://console.cloud.google.com").as_bytes(),
                );
                hashes.push(format!("{prefix} {timestamp}_{digest:x}"));
            }
        }
        if hashes.is_empty() {
            return Err(
                "The request is missing Google session cookies. Copy the full cURL request again."
                    .into(),
            );
        }
        Ok(hashes.join(" "))
    }
}

fn fetch_branding_icon_url(
    http: &reqwest::blocking::Client,
    session: &ConsoleSession,
    url: &reqwest::Url,
) -> Result<Option<String>, String> {
    let mut cookie_header = reqwest::header::HeaderValue::from_str(&session.cookies)
        .map_err(|_| "Invalid Console cookies. Reconnect.")?;
    cookie_header.set_sensitive(true);
    let response = http
        .get(url.clone())
        .header("Origin", "https://console.cloud.google.com")
        .header("Referer", "https://console.cloud.google.com/")
        .header("X-Goog-AuthUser", &session.auth_user)
        .header("Authorization", session.authorization()?)
        .header(reqwest::header::COOKIE, cookie_header)
        .send()
        .map_err(|_| "Could not load Google Cloud branding.")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(SESSION_ERROR.into());
    }
    let brand: serde_json::Value = response
        .json()
        .map_err(|_| "Google Cloud returned invalid branding.")?;
    Ok(brand["iconUrl"]
        .as_str()
        .filter(|url| crate::app_icons::validate_google_artwork_url(url))
        .map(str::to_owned))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthClient {
    client_id: String,
    display_name: String,
    display_type: String,
    #[serde(default)]
    creation_time: String,
}

impl OAuthClient {
    fn type_label(&self) -> &str {
        match self.display_type.as_str() {
            "CLIENT_TYPE_WEB_APPLICATION" => "Web application",
            "CLIENT_TYPE_IOS" => "iOS",
            "CLIENT_TYPE_ANDROID" => "Android",
            "CLIENT_TYPE_DESKTOP" => "Desktop app",
            "CLIENT_TYPE_CHROME_APP" => "Chrome app",
            "CLIENT_TYPE_DEVICE" => "TV / limited input",
            "CLIENT_TYPE_UWP" => "Universal Windows",
            "CLIENT_TYPE_SERVICE_ACCOUNT" => "Service account",
            _ => "Unknown",
        }
    }
}

fn fetch_clients(
    http: &reqwest::blocking::Client,
    session: &ConsoleSession,
    project: &Project,
) -> Result<Vec<OAuthClient>, String> {
    let number = project
        .name
        .strip_prefix("projects/")
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or("Invalid Google Cloud project number.")?;
    let mut clients = Vec::new();
    let mut body = session.body.clone();
    body["requestContext"]["projectId"] = project.project_id.clone().into();
    body["requestContext"]["selectedPurview"] =
        serde_json::json!({"projectId": project.project_id});
    body["variables"]["projectId"] = project.project_id.clone().into();
    body["variables"]["projectNumber"] = number.into();
    body["variables"]["pageSize"] = 50.into();
    body["variables"]
        .as_object_mut()
        .ok_or("Invalid Console request.")?
        .remove("pageToken");
    let mut seen = HashSet::new();
    loop {
        let mut cookie_header = reqwest::header::HeaderValue::from_str(&session.cookies)
            .map_err(|_| "Invalid Console cookies. Reconnect.")?;
        cookie_header.set_sensitive(true);
        let response = http
            .post(&session.url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://console.cloud.google.com")
            .header("Referer", "https://console.cloud.google.com/")
            .header("X-Goog-AuthUser", &session.auth_user)
            .header("Authorization", session.authorization()?)
            .header(reqwest::header::COOKIE, cookie_header)
            .json(&body)
            .send()
            .map_err(|_| {
                "Could not reach Google Cloud Console. Check your connection and refresh."
                    .to_owned()
            })?;
        if !response.status().is_success() {
            return Err(SESSION_ERROR.into());
        }
        let value: serde_json::Value = response
            .json()
            .map_err(|_| "Google Cloud returned an invalid OAuth client response.".to_owned())?;
        let result = value
            .pointer("/0/results/0")
            .ok_or("Google Cloud returned an unsupported OAuth client response.")?;
        if result
            .get("errors")
            .and_then(|e| e.as_array())
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(SESSION_ERROR.into());
        }
        let list = result
            .pointer("/data/oAuthClientsList")
            .ok_or("Google Cloud returned an unsupported OAuth client list.")?;
        let batch: Vec<OAuthClient> = serde_json::from_value(list["data"].clone())
            .map_err(|_| "Google Cloud returned invalid OAuth clients.".to_owned())?;
        clients.extend(batch);
        let next = list["nextPageToken"].as_str().unwrap_or_default();
        if next.is_empty() {
            let mut ids = HashSet::new();
            clients.retain(|client| ids.insert(client.client_id.clone()));
            return Ok(clients);
        }
        if !seen.insert(next.to_owned()) || seen.len() > 1000 {
            return Err(
                "Google Cloud returned invalid client pagination. Refresh to try again.".into(),
            );
        }
        body["variables"]["pageToken"] = next.into();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    fn mock_api(pages: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/v3/projects:search",
            listener.local_addr().unwrap()
        );
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in pages {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(request).unwrap());
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (url, server)
    }

    fn test_session(url: String) -> ConsoleSession {
        ConsoleSession {
            url,
            cookies: "SID=test-session; SAPISID=test-signing-cookie".into(),
            auth_user: "0".into(),
            body: serde_json::json!({
                "requestContext": {"projectId": "old-project"},
                "querySignature": "test-signature",
                "variables": {"projectId": "old-project", "projectNumber": 1}
            }),
        }
    }

    #[test]
    fn console_request_parser_restricts_endpoint_and_never_executes_curl() {
        let command = format!(
            "curl 'https://cloudconsole-pa.clients6.google.com{CONSOLE_PATH}' -H 'Cookie: SID=test-session; SAPISID=test-signing-cookie' -H 'X-Goog-AuthUser: 1' --data-raw '{{\"querySignature\":\"test\",\"variables\":{{}}}}'"
        );
        let session = ConsoleSession::parse_curl(&command).unwrap();
        assert_eq!(session.auth_user, "1");
        assert!(session.authorization().unwrap().starts_with("SAPISIDHASH "));
        assert!(
            ConsoleSession::parse_curl(
                &command.replace("cloudconsole-pa.clients6.google.com", "example.com")
            )
            .is_err()
        );
        assert!(
            ConsoleSession::parse_curl(
                &command.replace("SERVICE_USAGE_GRAPHQL:batchGraphql", "other:method")
            )
            .is_err()
        );
        assert!(
            ConsoleSession::parse_curl(
                &command.replace("--data-raw", "--data-binary @credentials.json")
            )
            .is_err()
        );
    }

    #[test]
    fn branding_uses_each_project_number_and_only_accepts_google_artwork() {
        let session = test_session(format!(
            "https://cloudconsole-pa.clients6.google.com{CONSOLE_PATH}?key=test-key"
        ));
        let mut project = Project {
            project_id: "groupicorn".into(),
            display_name: "Groupicorn".into(),
            name: "projects/123".into(),
        };
        let url = session.branding_url(&project).unwrap();
        assert_eq!(url.host_str(), Some("clientauthconfig.clients6.google.com"));
        assert_eq!(url.path(), "/v1/brands/lookupkey/brand/123");
        assert!(
            url.query_pairs()
                .any(|(name, value)| name == "key" && value == "test-key")
        );
        assert!(
            url.query_pairs()
                .any(|(name, value)| name == "readMask" && value == "iconUrl")
        );
        project.name = "projects/456".into();
        assert!(
            session
                .branding_url(&project)
                .unwrap()
                .path()
                .ends_with("/456")
        );
        project.name = "projects/invalid".into();
        assert!(session.branding_url(&project).is_err());
        for (status, body, expected) in [
            (
                200,
                r#"{"iconUrl":"https://lh3.googleusercontent.com/branding-icon"}"#,
                Some("https://lh3.googleusercontent.com/branding-icon"),
            ),
            (200, r#"{}"#, None),
            (200, r#"{"iconUrl":""}"#, None),
            (
                200,
                r#"{"iconUrl":"https://googleusercontent.com.evil.test/icon"}"#,
                None,
            ),
            (
                200,
                r#"{"iconUrl":"http://lh3.googleusercontent.com/icon"}"#,
                None,
            ),
            (404, "private-provider-message", None),
        ] {
            let (url, server) = mock_api(vec![(status, body.into())]);
            assert_eq!(
                fetch_branding_icon_url(
                    &reqwest::blocking::Client::new(),
                    &session,
                    &url.parse().unwrap()
                )
                .unwrap()
                .as_deref(),
                expected
            );
            let requests = server.join().unwrap();
            assert!(requests[0].to_lowercase().contains("x-goog-authuser: 0"));
        }
        let (url, server) = mock_api(vec![(403, "private-provider-message".into())]);
        assert_eq!(
            fetch_branding_icon_url(
                &reqwest::blocking::Client::new(),
                &session,
                &url.parse().unwrap()
            )
            .unwrap_err(),
            SESSION_ERROR
        );
        server.join().unwrap();
    }

    #[test]
    fn client_list_follows_pages_and_surfaces_graphql_permission_errors() {
        let client = serde_json::json!({"clientId": "test.apps.googleusercontent.com", "displayName": "Web client", "displayType": "CLIENT_TYPE_WEB_APPLICATION", "creationTime": "2026-10-02T12:00:00Z"});
        let response = |clients: serde_json::Value, next: &str| {
            serde_json::json!([{
            "results": [{"data": {"oAuthClientsList": {"data": clients, "nextPageToken": next}}}]
        }]).to_string()
        };
        let (url, server) = mock_api(vec![
            (200, response(serde_json::json!([client]), "next")),
            (200, response(serde_json::json!([client]), "")),
        ]);
        let project = Project {
            project_id: "groupicorn".into(),
            display_name: "Groupicorn".into(),
            name: "projects/123".into(),
        };
        let clients = fetch_clients(
            &reqwest::blocking::Client::new(),
            &test_session(url),
            &project,
        )
        .unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].type_label(), "Web application");
        let requests = server.join().unwrap();
        assert!(requests[0].contains("\"projectId\":\"groupicorn\""));
        assert!(requests[0].contains("\"projectNumber\":123"));
        assert!(requests[1].contains("\"pageToken\":\"next\""));
        let (url, server) = mock_api(vec![(200, r#"[{"results":[{"data":{"oAuthClientsList":{"data":[]}},"errors":[{"message":"private-provider-message"}]}]}]"#.into())]);
        let error = fetch_clients(
            &reqwest::blocking::Client::new(),
            &test_session(url),
            &project,
        )
        .err()
        .unwrap();
        assert_eq!(error, SESSION_ERROR);
        server.join().unwrap();
    }

    #[test]
    #[ignore = "Uses the local gcloud login and saved Google Cloud Console session for read-only API calls"]
    fn live_google_projects_and_groupicorn_clients() {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap();
        let projects = fetch_projects(&http, &access_token().unwrap(), PROJECTS_API).unwrap();
        assert!(!projects.is_empty());
        let project = projects
            .iter()
            .find(|p| p.project_id == "groupicorn")
            .unwrap();
        let clients = fetch_clients(&http, &ConsoleSession::load().unwrap(), project).unwrap();
        assert!(
            clients
                .iter()
                .any(|c| c.display_name == "iOS client 1" && c.type_label() == "iOS")
        );
        assert!(clients.iter().any(|c| c.display_name == "cloudflare"));
        assert!(clients.iter().any(|c| c.display_name == "Web client 1"));
        println!(
            "Verified {} projects and {} Groupicorn OAuth clients.",
            projects.len(),
            clients.len()
        );
    }

    #[test]
    #[ignore = "Uses the local gcloud login and saved Console session for read-only branding and image requests"]
    fn live_google_project_branding() {
        let http = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let session = ConsoleSession::load().unwrap();
        let projects = fetch_projects(&http, &access_token().unwrap(), PROJECTS_API).unwrap();
        let cancelled = AtomicBool::new(false);
        let mut count = 0;
        for project in &projects {
            let url =
                fetch_branding_icon_url(&http, &session, &session.branding_url(project).unwrap())
                    .unwrap();
            if project.project_id == "deployercoaster" {
                assert!(url.is_some());
            }
            if let Some(url) = url {
                let image = crate::app_icons::google_artwork(&http, &url, &cancelled).unwrap();
                assert!(image.size[0] > 0 && image.size[1] > 0);
                count += 1;
            }
        }
        assert!(count > 0);
        println!(
            "Verified branding and decoded {count} logos across {} Google projects.",
            projects.len()
        );
    }

    #[test]
    fn projects_follow_pagination_sort_and_deduplicate() {
        let (url, server) = mock_api(vec![
            (200, r#"{"projects":[{"projectId":"zeta","displayName":"Zeta","name":"projects/1"}],"nextPageToken":"next"}"#.into()),
            (200, r#"{"projects":[{"projectId":"alpha","displayName":"Alpha","name":"projects/2"},{"projectId":"zeta","displayName":"Zeta","name":"projects/1"}]}"#.into()),
        ]);
        let projects =
            fetch_projects(&reqwest::blocking::Client::new(), "test-token", &url).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].project_id, "alpha");
        let requests = server.join().unwrap();
        assert!(requests[1].contains("pageToken=next"));
        assert!(requests[0].contains("query=state%3AACTIVE"));
        assert!(
            requests[0]
                .to_lowercase()
                .contains("authorization: bearer test-token")
        );
    }

    #[test]
    fn rejects_repeated_pagination_and_reports_errors_without_response_text() {
        let (url, server) = mock_api(vec![
            (200, r#"{"nextPageToken":"repeat"}"#.into()),
            (200, r#"{"nextPageToken":"repeat"}"#.into()),
        ]);
        assert!(
            fetch_projects(&reqwest::blocking::Client::new(), "test-token", &url)
                .err()
                .unwrap()
                .contains("pagination")
        );
        server.join().unwrap();
        for status in [401, 403, 429, 500] {
            let (url, server) = mock_api(vec![(status, "private-provider-message".into())]);
            let error = fetch_projects(&reqwest::blocking::Client::new(), "test-token", &url)
                .err()
                .unwrap();
            assert!(!error.contains("private-provider-message"));
            server.join().unwrap();
        }
    }

    #[test]
    fn project_page_fits_supported_sizes_and_states() {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            for state in 0..5 {
                let mut page = GoogleOAuth {
                    attempted: true,
                    branding_attempted: true,
                    loaded: state > 0,
                    error: (state == 0).then(|| {
                        "Sign in to Google Cloud with gcloud auth login, then refresh projects."
                            .into()
                    }),
                    ..Default::default()
                };
                if state > 1 {
                    page.projects.push(Project {
                        project_id: "a-project-with-a-long-id-12345".into(),
                        display_name: "A project with a long display name for layout checks".into(),
                        name: "projects/123456789012".into(),
                    });
                }
                if state >= 3 {
                    page.selected = Some(page.projects[0].project_id.clone());
                    page.client_project = page.selected.clone();
                    page.clients = vec![OAuthClient {
                        client_id:
                            "123456789012-examplelongclientidentifier.apps.googleusercontent.com"
                                .into(),
                        display_name: "An OAuth client with a long name".into(),
                        display_type: "CLIENT_TYPE_WEB_APPLICATION".into(),
                        creation_time: "2026-10-02T12:00:00Z".into(),
                    }];
                }
                if state == 4 {
                    page.icons.insert_image(
                        page.projects[0].project_id.clone(),
                        egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
                    );
                }
                let ctx = egui::Context::default();
                crate::style::configure(&ctx);
                for _ in 0..2 {
                    let _ = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            egui::CentralPanel::default().show(ui, |ui| {
                                page.ui(ui);
                                assert!(
                                    ui.min_rect().right() <= width,
                                    "Overflow at {width}px in state {state}"
                                );
                            });
                        },
                    );
                }
            }
        }
    }
}
