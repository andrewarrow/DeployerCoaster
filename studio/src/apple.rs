use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{StatusCode, Url, blocking::Client};

const APPS_URL: &str = "https://api.appstoreconnect.apple.com/v1/apps?limit=200";

// Credentials deliberately have no Debug implementation.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AppleCredentials {
    issuer_id: String,
    key_id: String,
    private_key: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct AppleSettings {
    issuer_id: String,
    key_id: String,
    private_key: String,
    filename: String,
    #[serde(skip)]
    feedback: Option<String>,
    #[serde(skip)]
    error: bool,
    #[serde(skip)]
    changed: bool,
    #[serde(skip)]
    saved_credentials: Option<AppleCredentials>,
    #[serde(skip)]
    api_key_help_texture: Option<(egui::Context, egui::TextureHandle)>,
    #[serde(skip)]
    show_api_key_help: bool,
}

impl AppleSettings {
    pub fn load() -> Self {
        let result = (|| {
            let path = crate::storage::credential_path("apple-credentials.json")?;
            match fs::read(path) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|_| "Could not load Apple settings.".to_owned()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
                Err(_) => Err("Could not read Apple settings.".to_owned()),
            }
        })();
        let mut settings = result.unwrap_or_else(|error| Self {
            feedback: Some(error),
            error: true,
            ..Self::default()
        });
        settings.saved_credentials = settings.current_credentials();
        settings
    }

    fn current_credentials(&self) -> Option<AppleCredentials> {
        if self.issuer_id.trim().is_empty()
            || self.key_id.trim().is_empty()
            || self.private_key.is_empty()
        {
            return None;
        }
        Some(AppleCredentials {
            issuer_id: self.issuer_id.trim().to_owned(),
            key_id: self.key_id.trim().to_owned(),
            private_key: self.private_key.clone(),
        })
    }

    pub(crate) fn credentials(&self) -> Option<AppleCredentials> {
        self.saved_credentials.clone()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("App Store Connect").strong());
        ui.hyperlink_to(
            "Get your API key and IDs in App Store Connect",
            "https://appstoreconnect.apple.com/access/integrations/api",
        );
        ui.add(
            egui::Label::new(
                "Download a Team Key (.p8), then paste its Issuer ID and Key ID below.",
            )
            .wrap(),
        );
        self.api_key_help_ui(ui);
        ui.add_space(8.0);
        ui.label("Issuer ID");
        self.changed |= ui
            .add(
                egui::TextEdit::singleline(&mut self.issuer_id)
                    .desired_width(ui.available_width())
                    .hint_text("Paste Issuer ID"),
            )
            .changed();
        ui.add_space(8.0);
        ui.label("Key ID");
        self.changed |= ui
            .add(
                egui::TextEdit::singleline(&mut self.key_id)
                    .desired_width(ui.available_width())
                    .hint_text("Paste Key ID"),
            )
            .changed();
        ui.add_space(8.0);
        ui.label("P8 private key");
        if !self.filename.is_empty() {
            ui.add(egui::Label::new(&self.filename).wrap());
        }
        if ui
            .add(
                egui::Button::new(if self.private_key.is_empty() {
                    "Choose .p8 file…"
                } else {
                    "Replace .p8 file…"
                })
                .min_size(egui::vec2(140.0, 44.0)),
            )
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Apple private key", &["p8"])
                .pick_file()
        {
            match fs::read_to_string(&path) {
                Ok(key)
                    if key.trim().starts_with("-----BEGIN PRIVATE KEY-----")
                        && key.trim().ends_with("-----END PRIVATE KEY-----") =>
                {
                    self.private_key = key;
                    self.filename = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.changed = true;
                    self.feedback = None;
                }
                _ => {
                    self.feedback = Some(
                        "Choose a PEM private key (.p8) downloaded from App Store Connect.".into(),
                    );
                    self.error = true;
                }
            }
        }
        ui.add_space(8.0);
        let complete = !self.issuer_id.trim().is_empty()
            && !self.key_id.trim().is_empty()
            && !self.private_key.is_empty();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    complete && self.changed,
                    egui::Button::new("Save Apple settings").min_size(egui::vec2(150.0, 44.0)),
                )
                .clicked()
            {
                self.issuer_id = self.issuer_id.trim().to_owned();
                self.key_id = self.key_id.trim().to_owned();
                let result =
                    crate::storage::credential_path("apple-credentials.json").and_then(|path| {
                        let bytes = serde_json::to_vec(self)
                            .map_err(|_| "Could not encode Apple settings".to_owned())?;
                        crate::storage::save_credentials(&path, &bytes)
                    });
                self.error = result.is_err();
                self.feedback = Some(match result {
                    Ok(()) => {
                        self.changed = false;
                        self.saved_credentials = self.current_credentials();
                        "Apple settings saved. Connection has not been verified.".into()
                    }
                    Err(error) => error,
                });
            }
            if complete && !self.changed {
                ui.label("Credentials saved locally");
            }
        });
        if let Some(message) = &self.feedback {
            let color = if self.error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.add(egui::Label::new(egui::RichText::new(message).color(color)).wrap());
        }
    }

    fn api_key_help_ui(&mut self, ui: &mut egui::Ui) {
        if self
            .api_key_help_texture
            .as_ref()
            .is_none_or(|(ctx, _)| ctx != ui.ctx())
        {
            if let Ok(image) =
                image::load_from_memory(include_bytes!("../assets/apple-api-key-help.png"))
            {
                let image = image.into_rgba8();
                self.api_key_help_texture = Some((
                    ui.ctx().clone(),
                    ui.ctx().load_texture(
                        "app-store-connect-api-key-help",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width() as usize, image.height() as usize],
                            &image,
                        ),
                        egui::TextureOptions::LINEAR,
                    ),
                ));
            }
        }

        egui::CollapsingHeader::new("Where to find these values")
            .id_salt("apple_api_key_help")
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        "In App Store Connect, open Users and Access > Integrations > Team Keys. Copy the Issuer ID shown above the key list. Create or select a key for its Key ID, and choose Download API Key to save the .p8 file. Apple only lets you download a private key once.",
                    )
                    .wrap(),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new("Example only — copy the IDs from your own account.")
                            .small()
                            .weak(),
                    )
                    .wrap(),
                );
                if let Some((_, texture)) = &self.api_key_help_texture {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Image::new(texture)
                            .max_width(ui.available_width().min(560.0))
                            .alt_text("App Store Connect Team Keys page with arrows pointing to the Issuer ID, add key button, and Key ID column."),
                    );
                    if ui
                        .add(
                            egui::Button::new("View full size")
                                .min_size(egui::vec2(120.0, 44.0)),
                        )
                        .clicked()
                    {
                        self.show_api_key_help = true;
                    }
                }
            });

        if let Some((_, texture)) = &self.api_key_help_texture {
            egui::Window::new("App Store Connect Team Keys")
                .open(&mut self.show_api_key_help)
                .default_size(egui::vec2(900.0, 500.0))
                .max_size(
                    (ui.ctx().content_rect().size() - egui::vec2(32.0, 64.0))
                        .max(egui::vec2(100.0, 100.0)),
                )
                .scroll([true, true])
                .show(ui.ctx(), |ui| {
                    ui.add(
                        egui::Image::new(texture)
                            .fit_to_original_size(1.0)
                            .alt_text("App Store Connect Team Keys page with arrows pointing to the Issuer ID, add key button, and Key ID column."),
                    );
                });
        }
    }
}

#[derive(Deserialize)]
struct AppleApp {
    id: String,
    #[serde(default)]
    attributes: AppAttributes,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppAttributes {
    #[serde(default)]
    name: String,
    #[serde(default)]
    bundle_id: String,
}

#[derive(Deserialize)]
struct AppPage {
    data: Vec<AppleApp>,
    #[serde(default)]
    links: PageLinks,
}

#[derive(Default, Deserialize)]
struct PageLinks {
    next: Option<String>,
}

struct AppJob {
    result: Receiver<Result<Vec<AppleApp>, String>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for AppJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct AppleStore {
    credentials: Option<AppleCredentials>,
    apps: Vec<AppleApp>,
    loaded: bool,
    job: Option<AppJob>,
    error: Option<String>,
    cancelled: bool,
    icons: crate::app_icons::AppIcons,
}

impl AppleStore {
    pub(crate) fn dashboard_apps(&self) -> Vec<crate::dashboard::StoreApp> {
        self.apps
            .iter()
            .map(|app| crate::dashboard::StoreApp {
                identifier: if app.attributes.bundle_id.is_empty() {
                    format!("apple:{}", app.id)
                } else {
                    app.attributes.bundle_id.clone()
                },
                name: if app.attributes.name.is_empty() {
                    app.id.clone()
                } else {
                    app.attributes.name.clone()
                },
                store_id: app.id.clone(),
                store: crate::app_icons::Store::Apple,
            })
            .collect()
    }

    pub(crate) fn dashboard_status(&self) -> crate::dashboard::StoreStatus {
        crate::dashboard::StoreStatus {
            connected: self.credentials.is_some(),
            loading: self.job.is_some(),
            error: self.error.clone(),
        }
    }

    pub(crate) fn prepare_dashboard(&mut self, ctx: &egui::Context) {
        self.poll();
        if self.credentials.is_some()
            && !self.loaded
            && !self.cancelled
            && self.job.is_none()
            && self.error.is_none()
        {
            self.start(ctx);
        }
        if self.loaded && self.job.is_none() && self.icons.needs_start() {
            self.icons.ensure_started(
                crate::app_icons::Store::Apple,
                self.apps
                    .iter()
                    .filter(|app| !app.attributes.bundle_id.is_empty())
                    .map(|app| crate::app_icons::IconRequest {
                        key: app.attributes.bundle_id.clone(),
                        artwork_url: None,
                    })
                    .collect(),
                ctx,
            );
        }
    }

    pub(crate) fn refresh_dashboard(&mut self, ctx: &egui::Context) {
        if self.credentials.is_some() {
            self.icons.refresh();
            self.start(ctx);
        }
    }

    pub(crate) fn has_dashboard_icon(&mut self, key: &str) -> bool {
        self.icons.has_icon(key)
    }

    pub(crate) fn paint_dashboard_icon(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        key: &str,
        title: &str,
    ) {
        self.icons.paint_icon(ui, rect, key, title);
    }

    pub(crate) fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.result.try_recv() {
            Ok(Ok(apps)) => {
                self.apps = apps;
                self.loaded = true;
                self.job = None;
            }
            Ok(Err(error)) => {
                self.error = Some(error);
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.error = Some("The Apple connection stopped. Please refresh again.".into());
                self.job = None;
            }
        }
    }

    // Only saved credentials are used. Replacing a key invalidates the old account's list/job.
    pub(crate) fn set_credentials(&mut self, credentials: Option<AppleCredentials>) {
        if self.credentials != credentials {
            *self = Self {
                credentials,
                ..Self::default()
            };
        }
    }

    /// Returns true when the user asks to edit Apple credentials in Settings.
    pub fn apps_ui(&mut self, ui: &mut egui::Ui) -> bool {
        self.poll();
        ui.heading("App Store Connect apps");
        ui.add_space(8.0);
        if self.credentials.is_none() {
            ui.add(
                egui::Label::new(
                    "Add your Issuer ID, Key ID, and .p8 key in Apple settings to load apps.",
                )
                .wrap(),
            );
            return ui
                .add(egui::Button::new("Apple settings…").min_size(egui::vec2(140.0, 44.0)))
                .clicked();
        }
        if !self.loaded && !self.cancelled && self.job.is_none() && self.error.is_none() {
            self.start(ui.ctx());
        }
        let mut settings = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::new("Refresh").min_size(egui::vec2(80.0, 44.0)),
                )
                .clicked()
            {
                self.icons.refresh();
                self.start(ui.ctx());
            }
            settings = ui
                .add(egui::Button::new("Apple settings…").min_size(egui::vec2(140.0, 44.0)))
                .clicked();
        });
        if self.job.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label("Loading apps…");
                if ui
                    .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 44.0)))
                    .clicked()
                {
                    self.job = None;
                    // Keep automatic loading from restarting a cancelled request.
                    self.cancelled = true;
                }
            });
        }
        if self.loaded
            && self.job.is_none()
            && !self.cancelled
            && self.error.is_none()
            && self.icons.needs_start()
        {
            self.icons.ensure_started(
                crate::app_icons::Store::Apple,
                self.apps
                    .iter()
                    .filter(|app| !app.attributes.bundle_id.is_empty())
                    .map(|app| crate::app_icons::IconRequest {
                        key: app.attributes.bundle_id.clone(),
                        artwork_url: None,
                    })
                    .collect(),
                ui.ctx(),
            );
        }
        if let Some(error) = &self.error {
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        }
        ui.add_space(8.0);
        if self.loaded && self.job.is_none() && self.error.is_none() && self.apps.is_empty() {
            ui.label("No apps are accessible to this Apple API key.");
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let icons = &mut self.icons;
                    for app in &self.apps {
                        let title = if app.attributes.name.is_empty() {
                            &app.id
                        } else {
                            &app.attributes.name
                        };
                        ui.horizontal_top(|ui| {
                            icons.ui_icon(ui, &app.attributes.bundle_id, title);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(title).strong()).wrap(),
                                );
                                if !app.attributes.bundle_id.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&app.attributes.bundle_id)
                                                .monospace(),
                                        )
                                        .wrap(),
                                    );
                                }
                                ui.label(format!("App ID: {}", app.id));
                            });
                        });
                        ui.add_space(8.0);
                    }
                });
        }
        settings
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(credentials) = self.credentials.clone() else {
            return;
        };
        if self.job.is_some() {
            return;
        }
        self.error = None;
        self.cancelled = false;
        let ctx = ctx.clone();
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        self.job = Some(AppJob { result, cancelled });
        thread::spawn(move || {
            let result = (|| {
                let http = Client::builder()
                    .https_only(true)
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|_| "Could not initialize the Apple connection.".to_owned())?;
                let token = generate_token(&credentials)?;
                list_apps(&http, &token, &cancellation, APPS_URL)
            })();
            if !cancellation.load(Ordering::Relaxed) {
                let _ = sender.send(result);
                ctx.request_repaint();
            }
        });
    }
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    iat: u64,
    exp: u64,
    aud: &'static str,
}

fn generate_token(credentials: &AppleCredentials) -> Result<String, String> {
    let key = EncodingKey::from_ec_pem(credentials.private_key.as_bytes())
        .map_err(|_| "The saved .p8 key is invalid. Replace it in Apple settings.".to_owned())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Check your system clock before connecting to Apple.".to_owned())?
        .as_secs();
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(credentials.key_id.clone());
    encode(
        &header,
        &Claims {
            iss: &credentials.issuer_id,
            iat: now,
            exp: now + 20 * 60,
            aud: "appstoreconnect-v1",
        },
        &key,
    )
    .map_err(|_| "Could not sign the Apple request. Check your .p8 key in Apple settings.".into())
}

fn list_apps(
    http: &Client,
    token: &str,
    cancelled: &AtomicBool,
    endpoint: &str,
) -> Result<Vec<AppleApp>, String> {
    let endpoint = Url::parse(endpoint).map_err(|_| "Invalid Apple app list URL.".to_owned())?;
    let mut next = Some(endpoint.clone());
    let mut pages = HashSet::new();
    let mut apps = Vec::new();
    while let Some(url) = next {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Apple app loading cancelled.".into());
        }
        // Follow Apple's pagination URLs, but never forward the bearer token to another origin.
        if url.origin() != endpoint.origin()
            || url.path() != endpoint.path()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Apple returned an unexpected app list URL. Please refresh again.".into());
        }
        if !pages.insert(url.as_str().to_owned()) {
            return Err("Apple returned a repeated page of apps. Please refresh again.".into());
        }
        let response = http
            .get(url)
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .map_err(|_| {
                "Could not reach App Store Connect. Check your connection and refresh again."
                    .to_owned()
            })?;
        if !response.status().is_success() {
            return Err(api_error(response.status()));
        }
        let page: AppPage = response
            .json()
            .map_err(|_| "Apple returned an invalid app list. Please refresh again.".to_owned())?;
        apps.extend(page.data);
        next = page
            .links
            .next
            .filter(|url| !url.is_empty())
            .map(|url| {
                endpoint
                    .join(&url)
                    .map_err(|_| "Apple returned an invalid pagination URL.".to_owned())
            })
            .transpose()?;
    }
    apps.sort_by_cached_key(|app| {
        (
            app.attributes.name.to_lowercase(),
            app.attributes.bundle_id.clone(),
            app.id.clone(),
        )
    });
    let mut ids = HashSet::new();
    apps.retain(|app| ids.insert(app.id.clone()));
    Ok(apps)
}

fn api_error(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "Apple could not verify your API key. Check the Issuer ID, Key ID, .p8 key, and system clock in Settings.",
        StatusCode::FORBIDDEN => "Apple denied access. Check that this API key has permission to view your apps in App Store Connect.",
        StatusCode::TOO_MANY_REQUESTS => "Apple is receiving too many requests. Wait a moment and refresh again.",
        _ => "App Store Connect could not load your apps. Please refresh again.",
    }.to_owned()
}
