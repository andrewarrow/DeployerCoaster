use super::AppleCredentials;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

#[derive(Default, Serialize, Deserialize)]
pub struct AppleSettings {
    #[serde(default)]
    connection: Option<AppleCredentials>,
    // These fields are read only to migrate the old local format. They are never written again.
    #[serde(default, skip_serializing)]
    issuer_id: String,
    #[serde(default, skip_serializing)]
    key_id: String,
    #[serde(default, skip_serializing)]
    private_key: String,
    #[serde(default, skip_serializing)]
    filename: String,
    #[serde(default)]
    vendor_number: String,
    #[serde(skip)]
    feedback: Option<String>,
    #[serde(skip)]
    error: bool,
    #[serde(skip)]
    changed: bool,
    #[serde(skip)]
    saved_credentials: Option<AppleCredentials>,
    #[serde(skip)]
    saved_vendor_number: String,
    #[serde(skip)]
    api_key_help_texture: Option<(egui::Context, egui::TextureHandle)>,
    #[serde(skip)]
    show_api_key_help: bool,
    #[serde(skip)]
    editing_connection: bool,
    #[serde(skip)]
    job: Option<Receiver<Result<AppleCredentials, String>>>,
    #[serde(skip)]
    job_kind: Option<JobKind>,
    #[serde(skip)]
    migration_attempted: bool,
    #[serde(skip)]
    saved_validation_attempted: bool,
    #[serde(skip)]
    pending_save: Option<AppleCredentials>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    ValidateSaved,
    Connect,
    Migrate,
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
        settings.saved_vendor_number = settings.vendor_number.trim().to_owned();
        settings.saved_credentials = None;
        settings
    }

    pub(crate) fn credentials(&self) -> Option<AppleCredentials> {
        self.saved_credentials.clone()
    }

    pub(crate) fn saved_vendor_number(&self) -> &str {
        &self.saved_vendor_number
    }

    /// Polls network work and starts a single automatic migration for a legacy local key.
    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        self.poll_with_save(ctx, |settings| settings.save());
    }

    fn poll_with_save<F>(&mut self, ctx: &egui::Context, save: F)
    where
        F: FnOnce(&Self) -> Result<(), String>,
    {
        let mut save = Some(save);
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(result) => {
                    let kind = self.job_kind.take();
                    self.job = None;
                    match result {
                        Ok(credentials) if kind == Some(JobKind::ValidateSaved) => {
                            self.saved_credentials = Some(credentials);
                            self.feedback = Some("Saved Parsequerade connection.".into());
                            self.error = false;
                        }
                        Ok(credentials) => {
                            self.pending_save = Some(credentials);
                            self.persist_pending_connection_with(
                                save.take().expect("save closure is available"),
                            );
                        }
                        Err(error) => {
                            self.feedback = Some(error);
                            self.error = true;
                        }
                    }
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(120));
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.job_kind = None;
                    self.feedback = Some("The Parsequerade connection stopped. Try again.".into());
                    self.error = true;
                }
            }
        }

        if self.connection.is_some()
            && self.saved_credentials.is_none()
            && !self.saved_validation_attempted
            && self.job.is_none()
        {
            let connection = self.connection.clone().expect("checked above");
            self.saved_validation_attempted = true;
            self.start_job(ctx, JobKind::ValidateSaved, move || {
                crate::apple::api::validate_connection(&connection)?;
                Ok(connection)
            });
        } else if self.connection.is_none()
            && !self.migration_attempted
            && self.has_legacy_credentials()
            && self.job.is_none()
        {
            self.migration_attempted = true;
            self.start_create(ctx, JobKind::Migrate);
        }
    }

    fn has_legacy_credentials(&self) -> bool {
        !self.issuer_id.trim().is_empty()
            && !self.key_id.trim().is_empty()
            && !self.private_key.trim().is_empty()
    }

    fn start_job<F>(&mut self, ctx: &egui::Context, kind: JobKind, work: F)
    where
        F: FnOnce() -> Result<AppleCredentials, String> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        self.job = Some(receiver);
        self.job_kind = Some(kind);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(work());
            ctx.request_repaint();
        });
    }

    fn start_create(&mut self, ctx: &egui::Context, kind: JobKind) {
        let issuer = self.issuer_id.trim().to_owned();
        let key_id = self.key_id.trim().to_owned();
        let private_key = self.private_key.clone();
        self.start_job(ctx, kind, move || {
            let credentials = crate::apple::api::create_connection(&issuer, &key_id, &private_key)?;
            crate::apple::api::validate_connection(&credentials)?;
            Ok(credentials)
        });
    }

    fn persist_pending_connection(&mut self) {
        self.persist_pending_connection_with(|settings| settings.save());
    }

    fn persist_pending_connection_with<F>(&mut self, save: F)
    where
        F: FnOnce(&Self) -> Result<(), String>,
    {
        let Some(credentials) = self.pending_save.clone() else {
            return;
        };
        let previous = self.connection.replace(credentials.clone());
        let result = save(self);
        if let Err(error) = result {
            self.connection = previous;
            self.feedback = Some(format!(
                "The connection was created, but could not be saved locally. Retry saving to avoid creating another connection: {error}"
            ));
            self.error = true;
            return;
        }

        self.pending_save = None;
        self.saved_credentials = Some(credentials);
        self.saved_vendor_number = self.vendor_number.trim().to_owned();
        self.issuer_id.clear();
        self.key_id.clear();
        self.private_key.clear();
        self.filename.clear();
        self.editing_connection = false;
        self.changed = false;
        self.feedback = Some("Saved Parsequerade connection.".into());
        self.error = false;
    }

    fn save(&self) -> Result<(), String> {
        let path = crate::storage::credential_path("apple-credentials.json")?;
        let bytes =
            serde_json::to_vec(self).map_err(|_| "Could not encode Apple settings".to_owned())?;
        crate::storage::save_credentials(&path, &bytes)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll(ui.ctx());
        ui.label(egui::RichText::new("App Store Connect").strong());
        if self.saved_credentials.is_some() {
            ui.label("Saved Parsequerade connection · read-only app listings and metadata.");
        } else if self.job_kind == Some(JobKind::ValidateSaved) {
            ui.label("Checking saved Parsequerade connection…");
        } else if self.job_kind.is_some() {
            ui.label("Connecting securely through Parsequerade…");
        } else {
            ui.label(
                "Connect through Parsequerade to read App Store Connect app listings and metadata.",
            );
        }
        ui.hyperlink_to(
            "Get your API key and IDs in App Store Connect",
            "https://appstoreconnect.apple.com/access/integrations/api",
        );
        ui.add(egui::Label::new(
            "Your Apple key is sent securely to Parsequerade, which stores the connection for read-only access.",
        ).wrap());
        if self.saved_credentials.is_some()
            && !self.editing_connection
            && ui.button("Replace connection…").clicked()
        {
            self.editing_connection = true;
        }
        self.api_key_help_ui(ui);

        if self.saved_credentials.is_none()
            || self.editing_connection
            || self.job_kind == Some(JobKind::ValidateSaved)
        {
            ui.add_space(8.0);
            let issuer_label = ui.label("Issuer ID");
            self.changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.issuer_id)
                        .desired_width(ui.available_width())
                        .hint_text("Paste Issuer ID"),
                )
                .labelled_by(issuer_label.id)
                .changed();
            ui.add_space(8.0);
            let key_label = ui.label("Key ID");
            self.changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.key_id)
                        .desired_width(ui.available_width())
                        .hint_text("Paste Key ID"),
                )
                .labelled_by(key_label.id)
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
                            "Choose a PEM private key (.p8) downloaded from App Store Connect."
                                .into(),
                        );
                        self.error = true;
                    }
                }
            }
        }

        ui.add_space(8.0);
        let vendor_label = ui.label("Vendor number");
        self.changed |= ui
            .add(
                egui::TextEdit::singleline(&mut self.vendor_number)
                    .desired_width(ui.available_width())
                    .hint_text("Enter vendor number"),
            )
            .labelled_by(vendor_label.id)
            .changed();
        ui.hyperlink_to(
            "Find your vendor number in App Store Connect",
            "https://appstoreconnect.apple.com/itc/payments_and_financial_reports",
        );
        let vendor_valid = self
            .vendor_number
            .trim()
            .bytes()
            .all(|byte| byte.is_ascii_digit());
        if !vendor_valid {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Vendor number must contain only digits.",
            );
        }

        let complete = self.has_legacy_credentials();
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if self.connection.is_some()
                && self.saved_credentials.is_none()
                && self.job.is_none()
                && self.pending_save.is_none()
            {
                if ui
                    .add(
                        egui::Button::new("Retry saved connection check")
                            .min_size(egui::vec2(190.0, 44.0)),
                    )
                    .clicked()
                {
                    self.saved_validation_attempted = false;
                    self.feedback = None;
                    self.error = false;
                }
            }
            if self.pending_save.is_some() {
                if ui
                    .add(
                        egui::Button::new("Retry save connection")
                            .min_size(egui::vec2(170.0, 44.0)),
                    )
                    .clicked()
                {
                    self.persist_pending_connection();
                }
            } else if complete && vendor_valid && self.job.is_none() {
                let label = if self.connection.is_some() {
                    "Reconnect via Parsequerade"
                } else {
                    "Connect via Parsequerade"
                };
                if ui
                    .add(egui::Button::new(label).min_size(egui::vec2(180.0, 44.0)))
                    .clicked()
                {
                    self.start_create(ui.ctx(), JobKind::Connect);
                    self.changed = false;
                    self.feedback = Some("Connecting through Parsequerade…".into());
                    self.error = false;
                }
            }
            if self.saved_credentials.is_some()
                && vendor_valid
                && self.changed
                && self.job.is_none()
            {
                if ui
                    .add(egui::Button::new("Save vendor number").min_size(egui::vec2(160.0, 44.0)))
                    .clicked()
                {
                    let old = std::mem::replace(
                        &mut self.saved_vendor_number,
                        self.vendor_number.trim().to_owned(),
                    );
                    match self.save() {
                        Ok(()) => {
                            self.changed = false;
                            self.feedback = Some("Vendor number saved.".into());
                            self.error = false;
                        }
                        Err(error) => {
                            self.saved_vendor_number = old;
                            self.feedback = Some(error);
                            self.error = true;
                        }
                    }
                }
            }
            if self.job_kind.is_some() {
                ui.spinner();
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
                image::load_from_memory(include_bytes!("../../assets/apple-api-key-help.png"))
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
                ui.add(egui::Label::new(
                    "In App Store Connect, open Users and Access > Integrations > Team Keys. Copy the Issuer ID shown above the key list. Create or select a key for its Key ID, and choose Download API Key to save the .p8 file. Apple only lets you download a private key once.",
                ).wrap());
                ui.add(egui::Label::new(
                    egui::RichText::new("Example only — copy the IDs from your own account.").small().weak(),
                ).wrap());
                if let Some((_, texture)) = &self.api_key_help_texture {
                    ui.add_space(4.0);
                    ui.add(egui::Image::new(texture).max_width(ui.available_width().min(560.0)).alt_text(
                        "App Store Connect Team Keys page with arrows pointing to the Issuer ID, add key button, and Key ID column.",
                    ));
                    if ui.add(egui::Button::new("View full size").min_size(egui::vec2(120.0, 44.0))).clicked() {
                        self.show_api_key_help = true;
                    }
                }
            });

        if let Some((_, texture)) = &self.api_key_help_texture {
            egui::Window::new("App Store Connect Team Keys")
                .open(&mut self.show_api_key_help)
                .default_size(egui::vec2(900.0, 500.0))
                .max_size((ui.ctx().content_rect().size() - egui::vec2(32.0, 64.0)).max(egui::vec2(100.0, 100.0)))
                .scroll([true, true])
                .show(ui.ctx(), |ui| {
                    ui.add(egui::Image::new(texture).fit_to_original_size(1.0).alt_text(
                        "App Store Connect Team Keys page with arrows pointing to the Issuer ID, add key button, and Key ID column.",
                    ));
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_never_writes_legacy_private_key() {
        let settings = AppleSettings {
            issuer_id: "issuer".into(),
            key_id: "key".into(),
            private_key: "-----BEGIN PRIVATE KEY-----secret".into(),
            filename: "AuthKey.p8".into(),
            vendor_number: "123".into(),
            ..AppleSettings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("private_key"));
        assert!(!json.contains("BEGIN PRIVATE KEY"));
        assert!(!json.contains("issuer"));
    }

    #[test]
    fn legacy_settings_load_and_are_marked_for_migration() {
        let json = r#"{"issuer_id":"issuer","key_id":"key","private_key":"pem","filename":"old.p8","vendor_number":"123"}"#;
        let settings: AppleSettings = serde_json::from_str(json).unwrap();
        assert!(settings.has_legacy_credentials());
        assert_eq!(settings.vendor_number, "123");
        assert!(!settings.migration_attempted);
    }

    #[test]
    fn successful_connect_job_persists_before_publishing_credentials() {
        let previous = AppleCredentials::test_credentials("previous");
        let created = AppleCredentials::test_credentials("created");
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(created.clone())).unwrap();
        let mut settings = AppleSettings {
            connection: Some(previous.clone()),
            saved_credentials: Some(previous),
            saved_vendor_number: "123".into(),
            vendor_number: "123".into(),
            job: Some(receiver),
            job_kind: Some(JobKind::Connect),
            saved_validation_attempted: true,
            ..AppleSettings::default()
        };

        settings.poll_with_save(&egui::Context::default(), |_| Ok(()));

        assert!(settings.connection == Some(created.clone()));
        assert!(settings.saved_credentials == Some(created.clone()));
        assert!(settings.credentials() == Some(created));
        assert!(settings.pending_save.is_none());
        assert_eq!(settings.saved_vendor_number, "123");
    }

    #[test]
    fn failed_connect_job_keeps_saved_credentials_and_does_not_retry_automatically() {
        let saved = AppleCredentials::test_credentials("saved");
        let (sender, receiver) = mpsc::channel();
        sender.send(Err("connection failed".into())).unwrap();
        let mut settings = AppleSettings {
            connection: Some(saved.clone()),
            saved_credentials: Some(saved.clone()),
            saved_validation_attempted: true,
            job: Some(receiver),
            job_kind: Some(JobKind::Connect),
            ..AppleSettings::default()
        };
        let ctx = egui::Context::default();

        settings.poll_with_save(&ctx, |_| panic!("failed jobs must not persist"));
        settings.poll_with_save(&ctx, |_| panic!("failed jobs must not persist"));

        assert!(settings.saved_credentials == Some(saved.clone()));
        assert!(settings.credentials() == Some(saved));
        assert!(settings.job.is_none());
        assert!(
            settings
                .feedback
                .as_deref()
                .unwrap()
                .contains("connection failed")
        );
    }

    #[test]
    fn persistence_failure_retains_created_credentials_for_save_retry() {
        let previous = AppleCredentials::test_credentials("previous");
        let created = AppleCredentials::test_credentials("created");
        let mut settings = AppleSettings {
            connection: Some(previous.clone()),
            saved_credentials: Some(previous.clone()),
            pending_save: Some(created.clone()),
            ..AppleSettings::default()
        };

        settings.persist_pending_connection_with(|_| Err("disk unavailable".into()));

        assert!(settings.connection == Some(previous.clone()));
        assert!(settings.saved_credentials == Some(previous));
        assert!(settings.pending_save == Some(created));
        assert!(
            settings
                .feedback
                .as_deref()
                .unwrap()
                .contains("disk unavailable")
        );
    }

    #[test]
    fn settings_render_across_viewports_themes_and_connection_states() {
        let dimensions = [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ];
        let themes = [egui::Theme::Light, egui::Theme::Dark];
        for state in ["connecting", "error", "connected"] {
            for theme in themes {
                for (width, height) in dimensions {
                    let ctx = egui::Context::default();
                    crate::style::configure(&ctx);
                    ctx.set_theme(theme);
                    let mut settings = match state {
                        "connecting" => AppleSettings {
                            job_kind: Some(JobKind::Connect),
                            job: Some(mpsc::channel().1),
                            ..AppleSettings::default()
                        },
                        "error" => AppleSettings {
                            feedback: Some("Could not connect. Please retry.".into()),
                            error: true,
                            ..AppleSettings::default()
                        },
                        "connected" => {
                            let credentials = AppleCredentials::test_credentials("connected");
                            AppleSettings {
                                connection: Some(credentials.clone()),
                                saved_credentials: Some(credentials),
                                ..AppleSettings::default()
                            }
                        }
                        _ => unreachable!(),
                    };
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        ..Default::default()
                    };
                    let _ = ctx.run_ui(input, |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| settings.ui(ui));
                            assert!(
                                ui.min_rect().right() <= width,
                                "{state} settings overflow at {width}px in {theme:?}"
                            );
                        });
                    });
                }
            }
        }
    }
}
