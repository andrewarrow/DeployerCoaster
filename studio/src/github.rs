use std::{
    collections::BTreeMap,
    fs,
    process::Command,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::app_icons::{AppIcons, IconRequest};

const API: &str = "https://api.github.com";
const CREDENTIAL_FILE: &str = "github-token.json";
const LINK_FILE: &str = "github-app-links.json";

#[derive(Default, Serialize, Deserialize)]
struct AppLinks(BTreeMap<String, Option<String>>);

impl AppLinks {
    fn load(path: &std::path::Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Could not load app organization links.".to_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err("Could not read app organization links.".into()),
        }
    }

    fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| "Could not create the settings directory.")?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|_| "Could not encode app organization links.".to_owned())?;
        crate::storage::atomic_write(path, &bytes)
            .map_err(|_| "Could not save app organization links.".into())
    }
}

type OrganizationResult = Result<Vec<Organization>, String>;

#[derive(Default)]
pub(crate) struct GitHub {
    organizations: Vec<Organization>,
    icons: AppIcons,
    job: Option<Receiver<OrganizationResult>>,
    attempted: bool,
    loaded: bool,
    error: Option<String>,
    token: String,
    links: AppLinks,
    links_loaded: bool,
    link_error: Option<String>,
}

#[derive(Deserialize)]
struct Organization {
    login: String,
    avatar_url: String,
    description: Option<String>,
}

impl GitHub {
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
            let result = resolve_token().and_then(|token| {
                let http = reqwest::blocking::Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(20))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|_| "Could not start the GitHub connection.".to_owned())?;
                fetch_organizations(&http, &token, API)
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
                    Ok(organizations) => {
                        self.organizations = organizations;
                        self.icons = AppIcons::default();
                        self.loaded = true;
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.error = Some("GitHub sync stopped. Try refreshing again.".into());
            }
        }
    }

    pub(crate) fn prepare(&mut self, ctx: &egui::Context) {
        self.poll();
        if !self.links_loaded {
            self.links_loaded = true;
            match crate::storage::credential_path(LINK_FILE).and_then(|path| AppLinks::load(&path))
            {
                Ok(links) => self.links = links,
                Err(error) => self.link_error = Some(error),
            }
        }
        if !self.attempted {
            self.refresh(ctx);
        }
        if self.loaded {
            self.icons.ensure_github_started(
                self.organizations
                    .iter()
                    .map(|org| IconRequest {
                        key: org.login.clone(),
                        artwork_url: Some(org.avatar_url.clone()),
                    })
                    .collect(),
                ctx,
            );
        }
    }

    #[cfg(test)]
    pub(crate) fn test_connection(login: &str) -> Self {
        let mut icons = AppIcons::default();
        icons.ensure_github_started(Vec::new(), &egui::Context::default());
        Self {
            organizations: vec![Organization {
                login: login.into(),
                avatar_url: String::new(),
                description: None,
            }],
            icons,
            attempted: true,
            loaded: true,
            links_loaded: true,
            ..Default::default()
        }
    }

    pub(crate) fn linked_login(&self, identifier: &str, name: &str) -> Option<&str> {
        let login = match self.links.0.get(identifier) {
            Some(Some(login)) => login.as_str(),
            Some(None) => return None,
            None => name,
        };
        self.organizations
            .iter()
            .find(|org| org.login.eq_ignore_ascii_case(login))
            .map(|org| org.login.as_str())
    }

    pub(crate) fn paint_org_icon(&mut self, ui: &egui::Ui, rect: egui::Rect, login: &str) {
        self.icons.paint_icon(ui, rect, login, login);
    }

    // Returns true when the connection page is requested.
    pub(crate) fn organization_selector(
        &mut self,
        ui: &mut egui::Ui,
        identifier: &str,
        name: &str,
    ) -> bool {
        let previous = self.links.0.get(identifier).cloned();
        let mut selection = previous.clone();
        let selected_text = match &selection {
            Some(Some(login)) => login.clone(),
            Some(None) => "Not linked".into(),
            None => self
                .linked_login(identifier, name)
                .map(|login| format!("{login} (automatic)"))
                .unwrap_or_else(|| "Automatic (app name)".into()),
        };
        let mut open_connection = false;
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::new(("app_github_org", identifier), "GitHub org")
                .selected_text(selected_text)
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selection, None, "Automatic (app name)");
                    ui.selectable_value(&mut selection, Some(None), "Not linked");
                    for org in &self.organizations {
                        ui.selectable_value(
                            &mut selection,
                            Some(Some(org.login.clone())),
                            &org.login,
                        );
                    }
                });
            if self.job.is_some() {
                ui.spinner();
            } else if self.error.is_some() || (self.loaded && self.organizations.is_empty()) {
                open_connection = ui.button("GitHub connection").clicked();
            }
        });
        if selection != previous {
            match selection {
                Some(value) => {
                    self.links.0.insert(identifier.to_owned(), value);
                }
                None => {
                    self.links.0.remove(identifier);
                }
            }
            let result =
                crate::storage::credential_path(LINK_FILE).and_then(|path| self.links.save(&path));
            match result {
                Ok(()) => self.link_error = None,
                Err(error) => {
                    // Preserve the last saved choice if persistence fails.
                    match previous {
                        Some(value) => {
                            self.links.0.insert(identifier.to_owned(), value);
                        }
                        None => {
                            self.links.0.remove(identifier);
                        }
                    }
                    self.link_error = Some(error);
                }
            }
        }
        if let Some(error) = &self.link_error {
            ui.add(egui::Label::new(error).wrap());
        }
        if let Some(Some(login)) = self.links.0.get(identifier)
            && self.loaded
            && self.linked_login(identifier, name).is_none()
        {
            ui.add(
                egui::Label::new(format!("{login} isn't visible to this GitHub connection."))
                    .wrap(),
            );
        }
        open_connection
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) {
        self.prepare(ui.ctx());
        ui.horizontal(|ui| {
            ui.heading("GitHub orgs");
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::new("Refresh").min_size(egui::vec2(72.0, 44.0)),
                )
                .clicked()
            {
                self.refresh(ui.ctx());
            }
        });
        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading organizations…");
            });
        }
        if let Some(error) = &self.error {
            ui.add(egui::Label::new(error).wrap());
        }
        egui::CollapsingHeader::new("GitHub connection")
            .default_open(!self.loaded && self.error.is_some())
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(520.0));
                ui.add(egui::Label::new(
                    "Uses your GitHub CLI login, GH_TOKEN, or GITHUB_TOKEN. You can also save a personal access token here."
                ).wrap());
                ui.hyperlink_to(
                    "Create a classic token with read:org access",
                    "https://github.com/settings/tokens/new?scopes=read%3Aorg&description=DeployerCoaster",
                );
                ui.label("Personal access token");
                let label = ui.add(
                    egui::TextEdit::singleline(&mut self.token)
                        .password(true)
                        .hint_text("Paste token")
                        .desired_width(ui.available_width()),
                );
                label.widget_info(|| egui::WidgetInfo::labeled(
                    egui::WidgetType::TextEdit, true, "Personal access token"
                ));
                if ui.add_enabled(
                    !self.token.trim().is_empty() && self.job.is_none(),
                    egui::Button::new("Save and connect").min_size(egui::vec2(140.0, 44.0)),
                ).clicked() {
                    let result = crate::storage::credential_path(CREDENTIAL_FILE).and_then(|path| {
                        let bytes = serde_json::to_vec(self.token.trim())
                            .map_err(|_| "Could not encode the GitHub token.".to_owned())?;
                        crate::storage::save_credentials(&path, &bytes)
                    });
                    match result {
                        Ok(()) => {
                            self.token.clear();
                            self.organizations.clear();
                            self.icons = AppIcons::default();
                            self.loaded = false;
                            self.refresh(ui.ctx());
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            });
        if self.loaded && self.organizations.is_empty() {
            ui.label("No organizations are visible to this GitHub connection.");
        }
        egui::ScrollArea::vertical()
            .id_salt("github_organizations")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(720.0));
                for org in &self.organizations {
                    ui.horizontal(|ui| {
                        self.icons.ui_icon(ui, &org.login, &org.login);
                        ui.vertical(|ui| {
                            ui.hyperlink_to(
                                &org.login,
                                format!("https://github.com/{}", org.login),
                            );
                            if let Some(description) =
                                org.description.as_deref().filter(|s| !s.is_empty())
                            {
                                ui.add(egui::Label::new(description).wrap());
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
            });
    }
}

// Keep credential lookup off the UI thread and never log token values.
fn resolve_token() -> Result<String, String> {
    let path = crate::storage::credential_path(CREDENTIAL_FILE)?;
    match fs::read(path) {
        Ok(bytes) => {
            let token: String = serde_json::from_slice(&bytes)
                .map_err(|_| "Could not load the saved GitHub token. Save it again.".to_owned())?;
            if !token.trim().is_empty() {
                return Ok(token.trim().to_owned());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not read the saved GitHub token.".into()),
    }
    for name in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(token) = std::env::var(name)
            && !token.trim().is_empty()
        {
            return Ok(token.trim().to_owned());
        }
    }
    // Finder-launched macOS apps may not inherit Homebrew's PATH.
    let executables: &[&str] = if cfg!(target_os = "macos") {
        &["gh", "/opt/homebrew/bin/gh", "/usr/local/bin/gh"]
    } else {
        &["gh"]
    };
    for executable in executables {
        if let Ok(output) = Command::new(executable)
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
            && output.status.success()
            && let Ok(token) = String::from_utf8(output.stdout)
            && !token.trim().is_empty()
        {
            return Ok(token.trim().to_owned());
        }
    }
    Err("Connect GitHub with gh auth login or a personal access token.".into())
}

fn fetch_organizations(
    http: &reqwest::blocking::Client,
    token: &str,
    api: &str,
) -> OrganizationResult {
    let mut organizations = Vec::new();
    for page in 1..=1000 {
        let response = http
            .get(format!("{api}/user/orgs"))
            .query(&[("per_page", 100), ("page", page)])
            .bearer_auth(token)
            .header("User-Agent", "DeployerCoaster-Studio")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .send()
            .map_err(|_| "Could not reach GitHub. Check your connection and refresh.".to_owned())?;
        match response.status().as_u16() {
            200 => {}
            401 => return Err("GitHub rejected this token. Sign in again or save a new token.".into()),
            403 => return Err("GitHub denied access. Check read:org access, organization SSO authorization, or the API rate limit.".into()),
            429 => return Err("GitHub's API rate limit was reached. Try refreshing later.".into()),
            _ => return Err("GitHub could not list organizations. Try refreshing again.".into()),
        }
        let batch: Vec<Organization> = response
            .json()
            .map_err(|_| "GitHub returned an invalid organization list.".to_owned())?;
        let last_page = batch.len() < 100;
        organizations.extend(batch);
        if last_page {
            organizations.sort_by_cached_key(|org| org.login.to_lowercase());
            organizations.dedup_by(|a, b| a.login.eq_ignore_ascii_case(&b.login));
            return Ok(organizations);
        }
    }
    Err("GitHub returned too many organization pages. Try refreshing again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[test]
    fn app_links_persist_overrides_and_explicit_unlinks_by_identifier() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("links.json");
        let mut github = GitHub::test_connection("cubacadabra");
        assert_eq!(
            github.linked_login("com.cubacadabra.app", "CUBACADABRA"),
            Some("cubacadabra")
        );
        assert_eq!(github.linked_login("other.app", "Unrelated app"), None);
        github
            .links
            .0
            .insert("other.app".into(), Some("cubacadabra".into()));
        github.links.0.insert("com.cubacadabra.app".into(), None);
        github.links.save(&path).unwrap();
        github.links = AppLinks::load(&path).unwrap();
        assert_eq!(
            github.linked_login("other.app", "Unrelated app"),
            Some("cubacadabra")
        );
        assert_eq!(
            github.linked_login("com.cubacadabra.app", "cubacadabra"),
            None
        );
        github.links.0.remove("com.cubacadabra.app");
        assert_eq!(
            github.linked_login("com.cubacadabra.app", "cubacadabra"),
            Some("cubacadabra")
        );
        github
            .links
            .0
            .insert("other.app".into(), Some("unavailable-org".into()));
        assert_eq!(github.linked_login("other.app", "cubacadabra"), None);
    }

    fn mock_api(responses: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.windows(4).any(|s| s == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (api, server)
    }

    #[test]
    fn fetches_all_pages_with_authentication_and_avatar_urls() {
        let first: Vec<_> = (0..100)
            .map(|i| {
                serde_json::json!({
                    "login": format!("org-{i:03}"),
                    "avatar_url": format!("https://avatars.githubusercontent.com/u/{i}"),
                    "description": null,
                })
            })
            .collect();
        let (api, server) = mock_api(vec![
            (200, serde_json::to_string(&first).unwrap()),
            (200, r#"[{"login":"another-org","avatar_url":"https://avatars.githubusercontent.com/u/101","description":"Example organization"}]"#.into()),
        ]);
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let orgs = fetch_organizations(&http, "test-token", &api).unwrap();
        assert_eq!(orgs.len(), 101);
        assert_eq!(orgs[0].login, "another-org");
        assert_eq!(
            orgs[0].avatar_url,
            "https://avatars.githubusercontent.com/u/101"
        );
        assert_eq!(orgs[0].description.as_deref(), Some("Example organization"));
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /user/orgs?per_page=100&page=1 "));
        assert!(requests[1].starts_with("GET /user/orgs?per_page=100&page=2 "));
        assert!(requests.iter().all(|r| {
            r.to_lowercase()
                .contains("authorization: bearer test-token")
        }));
        assert!(requests[0].contains("application/vnd.github+json"));
    }

    #[test]
    fn reports_authentication_errors_without_provider_response_text() {
        for (status, message) in [(401, "rejected"), (403, "denied"), (429, "rate limit")] {
            let (api, server) = mock_api(vec![(status, "sensitive provider response".into())]);
            let error = fetch_organizations(&reqwest::blocking::Client::new(), "test-token", &api)
                .err()
                .unwrap();
            assert!(error.contains(message));
            assert!(!error.contains("sensitive"));
            server.join().unwrap();
        }
    }

    #[test]
    fn organization_page_fits_supported_sizes_and_states() {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            for state in 0..3 {
                let mut github = GitHub {
                    attempted: true,
                    loaded: state != 0,
                    error: (state == 0).then(|| {
                        "Connect GitHub with gh auth login or a personal access token.".into()
                    }),
                    ..Default::default()
                };
                // Mark artwork as started to keep this layout check offline.
                github
                    .icons
                    .ensure_github_started(Vec::new(), &egui::Context::default());
                if state == 2 {
                    github.organizations.push(Organization {
                        login: "a-long-github-organization-name-12345678".into(),
                        avatar_url: String::new(),
                        description: Some("An organization with a long description that should wrap neatly within the available width on small and large screens.".into()),
                    });
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
                                github.ui(ui);
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
