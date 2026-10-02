use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, RedirectUrl, Scope, TokenResponse, TokenUrl,
    basic::{BasicClient, BasicTokenResponse},
    url::Url,
};
use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize};

const PUBLISHER_SCOPE: &str = "https://www.googleapis.com/auth/androidpublisher";
const REPORTING_SCOPE: &str = "https://www.googleapis.com/auth/playdeveloperreporting";
const APPS_URL: &str = "https://playdeveloperreporting.googleapis.com/v1beta1/apps:search";
const CLIENT_FILENAME: &str =
    "client_secret_40330924720-un43h7i7gmclerblihu0qjh815tfchr6.apps.googleusercontent.com.json";
const CALLBACK_PATH: &str = "/oauth/callback";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

type GoogleClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

// Credentials and tokens deliberately have no Debug implementation.
#[derive(Clone, Deserialize, Serialize)]
struct Credentials {
    client_id: String,
    client_secret: String,
}

impl Credentials {
    fn load() -> Result<Self, String> {
        let path = credentials_path().ok_or(
            "Could not find the Google OAuth desktop client. Set DEPLOYERCOASTER_GOOGLE_CLIENT_SECRET to its JSON file.",
        )?;
        let bytes = fs::read(&path).map_err(|_| {
            format!(
                "Could not read the Google OAuth client at {}.",
                path.display()
            )
        })?;
        Self::parse(&bytes)
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct ClientFile {
            installed: Option<Credentials>,
        }
        let config: ClientFile = serde_json::from_slice(bytes)
            .map_err(|_| "The Google OAuth client file is not valid JSON.")?;
        config
            .installed
            .filter(|client| {
                client.client_id.ends_with(".apps.googleusercontent.com")
                    && !client.client_secret.is_empty()
            })
            .ok_or_else(|| "Choose a Google OAuth client JSON for a Desktop app.".to_owned())
    }
}

fn credentials_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DEPLOYERCOASTER_GOOGLE_CLIENT_SECRET") {
        return Some(path.into());
    }
    let configured =
        dirs::config_dir().map(|root| root.join("DeployerCoaster").join("google-oauth.json"));
    configured
        .filter(|path| path.is_file())
        .or_else(|| dirs::download_dir().map(|root| root.join(CLIENT_FILENAME)))
}

fn oauth_client(credentials: &Credentials) -> GoogleClient {
    BasicClient::new(ClientId::new(credentials.client_id.clone()))
        .set_client_secret(ClientSecret::new(credentials.client_secret.clone()))
        .set_auth_type(AuthType::RequestBody)
        .set_auth_uri(
            AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_owned()).unwrap(),
        )
        .set_token_uri(TokenUrl::new("https://oauth2.googleapis.com/token".to_owned()).unwrap())
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not initialize the Google Play connection.".to_owned())
}

#[derive(Clone)]
struct Session {
    credentials: Credentials,
    token: BasicTokenResponse,
    expires_at: Instant,
}

#[derive(Serialize, Deserialize)]
struct SavedSession {
    credentials: Credentials,
    token: BasicTokenResponse,
}

impl Session {
    fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let bytes = serde_json::to_vec(&SavedSession {
            credentials: self.credentials.clone(),
            token: self.token.clone(),
        })
        .map_err(|_| "Could not encode Google Play credentials")?;
        crate::storage::save_credentials(path, &bytes)
    }

    fn load() -> Result<Option<Self>, String> {
        let path = crate::storage::credential_path("google-session.json")?;
        Self::load_from(&path)
    }

    fn load_from(path: &std::path::Path) -> Result<Option<Self>, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Could not read the saved Google Play session.".into()),
        };
        let saved: SavedSession = serde_json::from_slice(&bytes).map_err(
            |_| "Could not load the saved Google Play session. Disconnect and connect again.",
        )?;
        // Force refresh on launch; monotonic expiry cannot survive a process restart.
        Ok(Some(Self {
            credentials: saved.credentials,
            token: saved.token,
            expires_at: Instant::now(),
        }))
    }
    fn new(credentials: Credentials, token: BasicTokenResponse) -> Self {
        let expires_at = Instant::now() + token.expires_in().unwrap_or(Duration::from_secs(3600));
        Self {
            credentials,
            token,
            expires_at,
        }
    }

    fn refresh(&mut self, http: &Client) -> Result<(), String> {
        let refresh_token = self
            .token
            .refresh_token()
            .ok_or("Your Google Play session has expired. Disconnect and connect again.")?;
        let mut token = oauth_client(&self.credentials)
            .exchange_refresh_token(refresh_token)
            .request(http)
            .map_err(|_| "Could not renew your Google Play session. Check your connection, or disconnect and connect again.")?;
        if token.refresh_token().is_none() {
            token.set_refresh_token(Some(refresh_token.clone()));
        }
        *self = Self::new(self.credentials.clone(), token);
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayApp {
    package_name: String,
    #[serde(default)]
    display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppPage {
    #[serde(default)]
    apps: Vec<PlayApp>,
    #[serde(default)]
    next_page_token: String,
}

enum Event {
    Progress(&'static str),
    SignedIn(Box<Session>),
    Apps(Vec<PlayApp>),
    Error(String),
    Finished,
}

struct Job {
    events: Receiver<Event>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct PlayStore {
    session: Option<Session>,
    persistence_path: Option<PathBuf>,
    apps: Vec<PlayApp>,
    loaded: bool,
    job: Option<Job>,
    progress: Option<&'static str>,
    error: Option<String>,
    cancelled: bool,
    icons: crate::app_icons::AppIcons,
    console_apps: Vec<crate::play_console::ConsoleApp>,
    console_sync: Option<crate::console_sync::SyncJob>,
    console_feedback: Option<String>,
}

impl PlayStore {
    pub fn load() -> Self {
        let mut store = match Session::load() {
            Ok(session) => Self {
                session,
                persistence_path: crate::storage::credential_path("google-session.json").ok(),
                ..Self::default()
            },
            Err(error) => Self {
                error: Some(error),
                persistence_path: crate::storage::credential_path("google-session.json").ok(),
                ..Self::default()
            },
        };
        if let Ok(path) = crate::storage::credential_path("google-console-icons.json") {
            match fs::read_to_string(path) {
                Ok(json) => match crate::play_console::parse_console_apps_json(&json) {
                    Ok(apps) => store.console_apps = apps,
                    Err(error) => store.console_feedback = Some(error),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    store.console_feedback = Some("Could not read the saved Console icons.".into())
                }
            }
        }
        store
    }

    pub fn settings_ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        if self.session.is_some() {
            ui.label("Google Play connected");
        }
        self.connection_ui(ui, false);
        self.error_ui(ui);
        ui.add_space(8.0);
        self.console_ui(ui);
    }

    pub fn apps_ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        if self.session.is_some()
            && !self.loaded
            && !self.cancelled
            && self.job.is_none()
            && self.error.is_none()
        {
            self.start(ui.ctx());
        }
        ui.heading("Play Store apps");
        ui.add_space(8.0);
        self.connection_ui(ui, true);
        self.error_ui(ui);
        self.console_ui(ui);
        if self.session.is_some() {
            if self.loaded
                && self.job.is_none()
                && !self.cancelled
                && self.error.is_none()
                && self.icons.needs_start()
            {
                self.icons.ensure_started(
                    crate::app_icons::Store::Play,
                    self.apps
                        .iter()
                        .map(|app| crate::app_icons::IconRequest {
                            key: app.package_name.clone(),
                            artwork_url: self
                                .console_apps
                                .iter()
                                .find(|summary| summary.package_name == app.package_name)
                                .and_then(|summary| summary.icon_url.clone()),
                        })
                        .collect(),
                    ui.ctx(),
                );
            }
            ui.add_space(8.0);
            if self.loaded && self.apps.is_empty() && self.job.is_none() && self.error.is_none() {
                ui.label("No apps are accessible to this Google account.");
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let icons = &mut self.icons;
                        for app in &self.apps {
                            let title = if app.display_name.is_empty() {
                                &app.package_name
                            } else {
                                &app.display_name
                            };
                            ui.horizontal_top(|ui| {
                                icons.ui_icon(ui, &app.package_name, title);
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(title).strong())
                                            .wrap(),
                                    );
                                    if !app.display_name.is_empty() {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&app.package_name).monospace(),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                            });
                            ui.add_space(8.0);
                        }
                    });
            }
        }
    }

    fn connection_ui(&mut self, ui: &mut egui::Ui, show_refresh: bool) {
        if self.session.is_none() {
            if self.job.is_none() {
                if ui
                    .add_sized([180.0, 44.0], egui::Button::new("Connect Play Store"))
                    .clicked()
                {
                    self.icons.refresh();
                    self.start(ui.ctx());
                }
            } else {
                self.progress_ui(ui);
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                if show_refresh
                    && ui
                        .add_enabled(
                            self.job.is_none(),
                            egui::Button::new("Refresh").min_size(egui::vec2(80.0, 44.0)),
                        )
                        .clicked()
                {
                    self.icons.refresh();
                    self.start(ui.ctx());
                }
                if ui
                    .add(egui::Button::new("Disconnect").min_size(egui::vec2(100.0, 44.0)))
                    .clicked()
                {
                    let result =
                        crate::storage::credential_path("google-session.json").and_then(|path| {
                            match fs::remove_file(path) {
                                Ok(()) => Ok(()),
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                    Ok(())
                                }
                                Err(_) => {
                                    Err("Could not remove the saved Google Play session."
                                        .to_owned())
                                }
                            }
                        });
                    match result {
                        Ok(()) => {
                            let persistence_path = self.persistence_path.clone();
                            *self = Self {
                                persistence_path,
                                ..Self::default()
                            };
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            });
            if self.job.is_some() {
                ui.add_space(8.0);
                self.progress_ui(ui);
            }
        }
    }

    fn error_ui(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
            if error.contains("Play Developer Reporting API") {
                ui.hyperlink_to(
                    "Enable Play Developer Reporting API",
                    "https://console.cloud.google.com/apis/library/playdeveloperreporting.googleapis.com",
                );
            }
        }
    }

    fn console_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if self.console_sync.is_some() {
                ui.spinner();
                ui.label("Waiting for Play Console…");
                if ui
                    .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 44.0)))
                    .clicked()
                {
                    self.console_sync = None;
                }
            } else if ui
                .add(egui::Button::new("Sync Console icons…").min_size(egui::vec2(160.0, 44.0)))
                .clicked()
            {
                self.console_feedback = None;
                match crate::console_sync::SyncJob::start(ui.ctx()) {
                    Ok(job) => self.console_sync = Some(job),
                    Err(error) => self.console_feedback = Some(error),
                }
            }
        });
        if let Some(message) = &self.console_feedback {
            ui.add(egui::Label::new(message).wrap());
        }
    }

    fn progress_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label(self.progress.unwrap_or("Connecting…"));
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 44.0)))
                .clicked()
            {
                self.job = None;
                self.progress = None;
                self.cancelled = true;
            }
        });
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        self.error = None;
        self.cancelled = false;
        self.progress = Some(if self.session.is_some() {
            "Loading apps…"
        } else {
            "Opening Google sign-in…"
        });
        let session = self.session.clone();
        let ctx = ctx.clone();
        let (sender, events) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        self.job = Some(Job { events, cancelled });
        thread::spawn(move || {
            let send = |event| {
                if !cancellation.load(Ordering::Relaxed) {
                    let _ = sender.send(event);
                    ctx.request_repaint();
                }
            };
            let result = (|| {
                let http = http_client()?;
                let mut session = match session {
                    Some(session) => session,
                    None => authorize(&http, &cancellation, &send)?,
                };
                check_cancelled(&cancellation)?;
                send(Event::SignedIn(Box::new(session.clone())));
                send(Event::Progress("Loading apps…"));
                let apps = list_apps(&http, &mut session, &cancellation, APPS_URL);
                send(Event::SignedIn(Box::new(session)));
                send(Event::Apps(apps?));
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                send(Event::Error(error));
            }
            send(Event::Finished);
        });
    }

    pub(crate) fn poll(&mut self) {
        if let Some(result) = self.console_sync.as_mut().and_then(|job| job.poll()) {
            self.console_sync = None;
            match result {
                Ok(snapshot) => {
                    let count = snapshot
                        .apps
                        .iter()
                        .filter(|app| app.icon_url.is_some())
                        .count();
                    let saved = crate::storage::credential_path("google-console-icons.json")
                        .and_then(|path| {
                            serde_json::to_vec(&snapshot)
                                .map_err(|_| "Could not encode Console icons.".to_owned())
                                .and_then(|bytes| crate::storage::save_credentials(&path, &bytes))
                        });
                    self.console_apps = snapshot.apps;
                    self.icons.refresh();
                    self.console_feedback = Some(match saved {
                        Ok(()) => format!("Synced {count} Console icons."),
                        Err(_) => format!(
                            "Synced {count} Console icons, but could not save them for next time."
                        ),
                    });
                }
                Err(error) => self.console_feedback = Some(error),
            }
        }
        while let Some(job) = &self.job {
            match job.events.try_recv() {
                Ok(Event::Progress(message)) => self.progress = Some(message),
                Ok(Event::SignedIn(session)) => {
                    if let Some(path) = &self.persistence_path
                        && let Err(error) = session.save(path)
                    {
                        self.error = Some(error);
                    }
                    self.session = Some(*session);
                }
                Ok(Event::Apps(apps)) => {
                    self.apps = apps;
                    self.loaded = true;
                }
                Ok(Event::Error(error)) => self.error = Some(error),
                Ok(Event::Finished) => {
                    self.job = None;
                    self.progress = None;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.progress = None;
                    self.error =
                        Some("The Google Play connection stopped. Please try again.".to_owned());
                }
            }
        }
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Google sign-in cancelled.".to_owned())
    } else {
        Ok(())
    }
}

fn authorize(
    http: &Client,
    cancelled: &AtomicBool,
    send: &impl Fn(Event),
) -> Result<Session, String> {
    let credentials = Credentials::load()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|_| "Could not start the local Google sign-in callback.")?;
    let port = listener
        .local_addr()
        .map_err(|_| "Could not get the sign-in callback port.")?
        .port();
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let client = oauth_client(&credentials).set_redirect_uri(
        RedirectUrl::new(redirect).map_err(|_| "Invalid sign-in callback address.")?,
    );
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(PUBLISHER_SCOPE.to_owned()))
        .add_scope(Scope::new(REPORTING_SCOPE.to_owned()))
        .set_pkce_challenge(challenge)
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent select_account")
        .url();
    check_cancelled(cancelled)?;
    webbrowser::open(url.as_str()).map_err(|_| "Could not open your browser for Google sign-in. Check your default browser and try again.")?;
    send(Event::Progress("Finish signing in in your browser."));
    let code = wait_for_callback(&listener, &state, cancelled, SIGN_IN_TIMEOUT)?;
    check_cancelled(cancelled)?;
    send(Event::Progress("Completing Google sign-in…"));
    let token = client
        .exchange_code(code)
        .set_pkce_verifier(verifier)
        .request(http)
        .map_err(|_| "Could not complete Google sign-in. Check your connection and try again.")?;
    if let Some(scopes) = token.scopes()
        && [PUBLISHER_SCOPE, REPORTING_SCOPE]
            .iter()
            .any(|required| !scopes.iter().any(|scope| scope.as_str() == *required))
    {
        return Err("Google Play access was not fully granted. Connect again and allow both requested permissions.".to_owned());
    }
    Ok(Session::new(credentials, token))
}

enum Callback {
    Code(AuthorizationCode),
    Denied,
}

fn parse_callback(request: &str, expected_state: &CsrfToken) -> Result<Callback, ()> {
    let mut parts = request.split_whitespace();
    if parts.next() != Some("GET") {
        return Err(());
    }
    let target = parts.next().ok_or(())?;
    if !target.starts_with('/') || parts.next() != Some("HTTP/1.1") {
        return Err(());
    }
    let url = Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| ())?;
    if url.path() != CALLBACK_PATH {
        return Err(());
    }
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            return Err(());
        }
    }
    if CsrfToken::new(state.ok_or(())?) != *expected_state {
        return Err(());
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() => Ok(Callback::Code(AuthorizationCode::new(code))),
        (None, Some(_)) => Ok(Callback::Denied),
        _ => Err(()),
    }
}

fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{message}",
        message.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn wait_for_callback(
    listener: &TcpListener,
    state: &CsrfToken,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> Result<AuthorizationCode, String> {
    listener
        .set_nonblocking(true)
        .map_err(|_| "Could not listen for Google sign-in.")?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        check_cancelled(cancelled)?;
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                let mut request = String::new();
                if BufReader::new((&mut stream).take(8192))
                    .read_line(&mut request)
                    .is_err()
                    || !request.ends_with('\n')
                {
                    respond(&mut stream, "400 Bad Request", "Invalid sign-in callback.");
                    continue;
                }
                match parse_callback(&request, state) {
                    Ok(Callback::Code(code)) => {
                        respond(
                            &mut stream,
                            "200 OK",
                            "Google authorization received. Return to DeployerCoaster to finish connecting.",
                        );
                        return Ok(code);
                    }
                    Ok(Callback::Denied) => {
                        respond(
                            &mut stream,
                            "200 OK",
                            "Google sign-in was cancelled. You can return to DeployerCoaster.",
                        );
                        return Err(
                            "Google sign-in was cancelled or denied. Connect again to retry."
                                .to_owned(),
                        );
                    }
                    Err(()) => respond(
                        &mut stream,
                        "400 Bad Request",
                        "Invalid sign-in callback. Continue signing in from DeployerCoaster.",
                    ),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                return Err(
                    "Could not receive the Google sign-in callback. Try connecting again."
                        .to_owned(),
                );
            }
        }
    }
    Err("Google sign-in timed out. Connect again to retry.".to_owned())
}

fn list_apps(
    http: &Client,
    session: &mut Session,
    cancelled: &AtomicBool,
    endpoint: &str,
) -> Result<Vec<PlayApp>, String> {
    if session.expires_at <= Instant::now() + Duration::from_secs(60) {
        session.refresh(http)?;
    }
    let mut apps = Vec::new();
    let mut page_token = String::new();
    let mut seen_pages = HashSet::new();
    let mut refreshed = false;
    loop {
        check_cancelled(cancelled)?;
        let response = http
            .get(endpoint)
            .bearer_auth(session.token.access_token().secret())
            .query(&[("pageSize", "1000"), ("pageToken", page_token.as_str())])
            .send()
            .map_err(|_| "Could not load your Play Store apps. Check your internet connection and retry.")?;
        if response.status() == StatusCode::UNAUTHORIZED && !refreshed {
            session.refresh(http)?;
            refreshed = true;
            continue;
        }
        if !response.status().is_success() {
            return Err(api_error(
                response.status(),
                response.json().unwrap_or_default(),
            ));
        }
        let page: AppPage = response
            .json()
            .map_err(|_| "Google Play returned an invalid app list. Please retry.")?;
        apps.extend(page.apps);
        if page.next_page_token.is_empty() {
            break;
        }
        if !seen_pages.insert(page.next_page_token.clone()) {
            return Err("Google Play returned a repeated page of apps. Please retry.".to_owned());
        }
        page_token = page.next_page_token;
    }
    apps.sort_by_cached_key(|app| (app.display_name.to_lowercase(), app.package_name.clone()));
    let mut packages = HashSet::new();
    apps.retain(|app| packages.insert(app.package_name.clone()));
    Ok(apps)
}

fn api_error(status: StatusCode, body: serde_json::Value) -> String {
    let details = body["error"]["details"].as_array();
    if details.is_some_and(|details| {
        details
            .iter()
            .any(|detail| detail["reason"] == "SERVICE_DISABLED")
    }) {
        return "Enable the Google Play Developer Reporting API in the OAuth client's Google Cloud project, then click Refresh.".to_owned();
    }
    match status {
        StatusCode::UNAUTHORIZED => "Your Google Play session has expired. Disconnect and connect again.",
        StatusCode::FORBIDDEN => "Google Play denied access. Grant the reporting permission when connecting and check this account's Play Console app permissions.",
        StatusCode::TOO_MANY_REQUESTS => "Google Play is receiving too many requests. Wait a moment and refresh again.",
        _ => "Google Play could not load your apps. Please try refreshing again.",
    }.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session() -> Session {
        Session::new(
            Credentials {
                client_id: "test.apps.googleusercontent.com".to_owned(),
                client_secret: "test-client-secret".to_owned(),
            },
            serde_json::from_value(json!({
                "access_token": "test-access-token",
                "refresh_token": "test-refresh-token",
                "token_type": "Bearer",
                "expires_in": 3600,
            }))
            .unwrap(),
        )
    }

    #[test]
    fn saved_session_restores_refresh_token_and_forces_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        session().save(&path).unwrap();
        let restored = Session::load_from(&path).unwrap().unwrap();
        assert_eq!(
            restored.token.refresh_token().unwrap().secret(),
            "test-refresh-token"
        );
        assert!(restored.expires_at <= Instant::now());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    fn test_http() -> Client {
        Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    }

    fn request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut request = String::new();
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            let end = line == "\r\n";
            request.push_str(&line);
            if end {
                break;
            }
        }
        request
    }

    fn app_server(
        pages: Vec<(&'static str, serde_json::Value)>,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}/apps:search", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            for (page_token, body) in pages {
                let (mut stream, _) = listener.accept().unwrap();
                let request = request(&mut stream);
                assert!(
                    request
                        .to_lowercase()
                        .contains("authorization: bearer test-access-token\r\n")
                );
                let target = request.split_whitespace().nth(1).unwrap();
                let url = Url::parse(&format!("http://localhost{target}")).unwrap();
                assert!(
                    url.query_pairs()
                        .any(|(key, value)| key == "pageToken" && value == page_token)
                );
                assert!(
                    url.query_pairs()
                        .any(|(key, value)| key == "pageSize" && value == "1000")
                );
                let body = body.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (endpoint, handle)
    }

    #[test]
    fn credentials_require_a_desktop_client_and_errors_do_not_include_secrets() {
        assert!(Credentials::parse(br#"{"installed":{"client_id":"test.apps.googleusercontent.com","client_secret":"secret"}}"#).is_ok());
        assert!(
            Credentials::parse(br#"{"web":{"client_id":"test","client_secret":"secret"}}"#)
                .is_err()
        );
        let error = Credentials::parse(
            br#"{"installed":{"client_id":"wrong","client_secret":"do-not-leak"}}"#,
        )
        .err()
        .unwrap();
        assert!(!error.contains("do-not-leak"));
    }

    #[test]
    fn callback_rejects_forgery_duplicates_wrong_paths_and_missing_codes() {
        let state = CsrfToken::new("expected-state".to_owned());
        for target in [
            "/oauth/callback?state=wrong&code=secret-code",
            "/oauth/callback?code=secret-code",
            "/oauth/callback?state=expected-state",
            "/oauth/callback?state=expected-state&code=",
            "/oauth/callback?state=expected-state&state=wrong&code=secret-code",
            "/oauth/callback?state=expected-state&code=one&code=two",
            "/oauth/callback?state=expected-state&code=one&error=access_denied",
            "/favicon.ico?state=expected-state&code=secret-code",
        ] {
            assert!(parse_callback(&format!("GET {target} HTTP/1.1\r\n"), &state).is_err());
        }
        let Callback::Code(code) = parse_callback(
            "GET /oauth/callback?state=expected-state&code=code%2Bwith%2Fencoding HTTP/1.1\r\n",
            &state,
        )
        .ok()
        .unwrap() else {
            panic!("Expected an authorization code")
        };
        assert_eq!(code.secret(), "code+with/encoding");
        assert!(matches!(
            parse_callback(
                "GET /oauth/callback?state=expected-state&error=access_denied HTTP/1.1\r\n",
                &state,
            ),
            Ok(Callback::Denied)
        ));
    }

    #[test]
    fn loopback_callback_ignores_invalid_state_and_accepts_the_real_browser_redirect() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let browser = thread::spawn(move || {
            for (state, expected_status) in [("forged", "400 Bad Request"), ("real", "200 OK")] {
                let mut stream = TcpStream::connect(address).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                write!(stream, "GET /oauth/callback?state={state}&code=code-from-browser HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
                let response = request(&mut stream);
                assert!(response.starts_with(&format!("HTTP/1.1 {expected_status}")));
                assert!(response.contains("Cache-Control: no-store"));
                assert!(!response.contains("code-from-browser"));
            }
        });
        let code = wait_for_callback(
            &listener,
            &CsrfToken::new("real".to_owned()),
            &AtomicBool::new(false),
            Duration::from_secs(3),
        )
        .unwrap();
        assert_eq!(code.secret(), "code-from-browser");
        browser.join().unwrap();
    }

    #[test]
    fn callback_cancellation_and_timeout_do_not_wait_for_browser_input() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let state = CsrfToken::new("state".to_owned());
        assert!(
            wait_for_callback(
                &listener,
                &state,
                &AtomicBool::new(true),
                Duration::from_secs(3)
            )
            .unwrap_err()
            .contains("cancelled")
        );
        assert!(
            wait_for_callback(&listener, &state, &AtomicBool::new(false), Duration::ZERO)
                .unwrap_err()
                .contains("timed out")
        );
    }

    #[test]
    fn app_listing_follows_pagination_sorts_and_removes_duplicates() {
        let (endpoint, server) = app_server(vec![
            (
                "",
                json!({"apps":[{"packageName":"com.example.z","displayName":"Zebra"}],"nextPageToken":"page+/2"}),
            ),
            (
                "page+/2",
                json!({"apps":[{"packageName":"com.example.a","displayName":"Alpha"},{"packageName":"com.example.z","displayName":"Zebra"}]}),
            ),
        ]);
        let apps = list_apps(
            &test_http(),
            &mut session(),
            &AtomicBool::new(false),
            &endpoint,
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].display_name, "Alpha");
        assert_eq!(apps[1].display_name, "Zebra");
    }

    #[test]
    fn app_listing_accepts_empty_accounts_and_rejects_repeated_page_tokens() {
        let (endpoint, server) = app_server(vec![("", json!({}))]);
        assert!(
            list_apps(
                &test_http(),
                &mut session(),
                &AtomicBool::new(false),
                &endpoint
            )
            .unwrap()
            .is_empty()
        );
        server.join().unwrap();
        let (endpoint, server) = app_server(vec![
            ("", json!({"nextPageToken":"repeated"})),
            ("repeated", json!({"nextPageToken":"repeated"})),
        ]);
        assert!(
            list_apps(
                &test_http(),
                &mut session(),
                &AtomicBool::new(false),
                &endpoint
            )
            .err()
            .unwrap()
            .contains("repeated page")
        );
        server.join().unwrap();
    }

    #[test]
    fn disabled_api_errors_explain_setup_without_exposing_the_response() {
        let error = api_error(
            StatusCode::FORBIDDEN,
            json!({"error":{
                "message":"private-response-detail",
                "details":[{"reason":"SERVICE_DISABLED"}]
            }}),
        );
        assert!(error.contains("Play Developer Reporting API"));
        assert!(!error.contains("private-response-detail"));
        assert!(api_error(StatusCode::FORBIDDEN, json!({})).contains("permissions"));
    }

    #[test]
    fn list_failure_preserves_the_session_for_retry_and_cancel_discards_pending_events() {
        let (sender, events) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut store = PlayStore {
            job: Some(Job {
                events,
                cancelled: cancelled.clone(),
            }),
            ..Default::default()
        };
        sender.send(Event::SignedIn(Box::new(session()))).unwrap();
        sender
            .send(Event::Error("Enable the API".to_owned()))
            .unwrap();
        sender.send(Event::Finished).unwrap();
        store.poll();
        assert!(store.session.is_some());
        assert_eq!(store.error.as_deref(), Some("Enable the API"));
        assert!(store.job.is_none());
        assert!(cancelled.load(Ordering::Relaxed));
        let (sender, events) = mpsc::channel();
        store.job = Some(Job {
            events,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        store.job = None;
        assert!(sender.send(Event::SignedIn(Box::new(session()))).is_err());
    }

    #[test]
    fn ui_fits_supported_sizes_with_long_titles_packages_and_errors() {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, height),
                )),
                ..Default::default()
            };
            let mut store = PlayStore {
                session: Some(session()),
                loaded: true,
                apps: vec![
                    PlayApp { display_name: "A long application title with several words that needs to wrap on smaller desktop windows".to_owned(), package_name: format!("com.example.{}", "application".repeat(10)) },
                    PlayApp { display_name: String::new(), package_name: "com.example.untitled".to_owned() },
                ],
                error: Some("Google Play denied access. Grant the reporting permission when connecting and check this account's Play Console app permissions.".to_owned()),
                ..Default::default()
            };
            for _ in 0..2 {
                let _ = ctx.run_ui(input.clone(), |ui| {
                    egui::CentralPanel::default_margins().show(ui, |ui| {
                        store.apps_ui(ui);
                        assert!(
                            ui.min_rect().right() <= width,
                            "Horizontal overflow at {width}px"
                        );
                    });
                });
            }
        }
    }
}
