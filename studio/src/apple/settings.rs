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
    // Form fields also accept the original local credential format during migration.
    // Only the validated credentials in `connection` are persisted.
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
    legacy_migration_pending: bool,
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
        let settings = result.unwrap_or_else(|error| Self {
            feedback: Some(error),
            error: true,
            ..Self::default()
        });
        Self::finish_loading(settings)
    }

    fn finish_loading(mut settings: Self) -> Self {
        settings.saved_vendor_number = settings.vendor_number.trim().to_owned();
        settings.saved_credentials = None;
        if settings
            .connection
            .as_ref()
            .is_some_and(|key| key.private_key.is_empty())
        {
            settings.connection = None;
            settings.feedback = Some(
                "Add your Issuer ID, Key ID, and .p8 file again to use Apple directly. The saved hosted connection cannot provide the private key.".into(),
            );
            settings.error = true;
        }
        settings.legacy_migration_pending =
            settings.connection.is_none() && settings.has_key_input();
        settings
    }

    pub(crate) fn credentials(&self) -> Option<AppleCredentials> {
        self.saved_credentials.clone()
    }

    pub(crate) fn saved_vendor_number(&self) -> &str {
        &self.saved_vendor_number
    }

    /// Polls key validation and migrates the original local credential format once.
    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        self.poll_with_save(ctx, |settings| settings.save());
    }

    fn poll_with_save<F>(&mut self, ctx: &egui::Context, save: F)
    where
        F: FnOnce(&Self) -> Result<(), String>,
    {
        self.poll_with_save_and_migration(ctx, save, |settings, ctx| {
            settings.start_create(ctx, JobKind::Migrate);
        });
    }

    fn poll_with_save_and_migration<F, S>(
        &mut self,
        ctx: &egui::Context,
        save: F,
        start_migration: S,
    ) where
        F: FnOnce(&Self) -> Result<(), String>,
        S: FnOnce(&mut Self, &egui::Context),
    {
        let mut save = Some(save);
        let mut start_migration = Some(start_migration);
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(result) => {
                    let kind = self.job_kind.take();
                    self.job = None;
                    match result {
                        Ok(credentials) if kind == Some(JobKind::ValidateSaved) => {
                            self.saved_credentials = Some(credentials);
                            self.feedback = Some("Apple API key saved.".into());
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
                    self.feedback = Some("Apple key validation stopped. Try again.".into());
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
        } else if self.legacy_migration_pending && self.job.is_none() && self.pending_save.is_none()
        {
            self.legacy_migration_pending = false;
            self.migration_attempted = true;
            start_migration
                .take()
                .expect("migration launcher is available")(self, ctx);
        }
    }

    fn has_key_input(&self) -> bool {
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
            crate::apple::api::create_connection(&issuer, &key_id, &private_key)
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
                "The Apple API key could not be saved locally. Retry saving: {error}"
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
        self.feedback = Some("Apple API key saved.".into());
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
            ui.label("Apple API key saved.");
        } else if self.job_kind == Some(JobKind::ValidateSaved) {
            ui.label("Checking saved Apple API key…");
        } else if self.job_kind.is_some() {
            ui.label("Checking Apple API key…");
        } else {
            ui.label("Add your Apple API key to read App Store Connect app listings and metadata.");
        }
        ui.hyperlink_to(
            "Get your API key and IDs in App Store Connect",
            "https://appstoreconnect.apple.com/access/integrations/api",
        );
        ui.add(egui::Label::new(
            "Your Apple key is saved on this device. Studio connects directly to Apple to read app metadata and paste age-rating settings.",
        ).wrap());
        if self.saved_credentials.is_some()
            && !self.editing_connection
            && ui.button("Replace API key…").clicked()
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

        let complete = self.has_key_input();
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if self.connection.is_some()
                && self.saved_credentials.is_none()
                && self.job.is_none()
                && self.pending_save.is_none()
            {
                if ui
                    .add(
                        egui::Button::new("Retry saved key check")
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
                    .add(egui::Button::new("Retry saving key").min_size(egui::vec2(170.0, 44.0)))
                    .clicked()
                {
                    self.persist_pending_connection();
                }
            } else if complete && vendor_valid && self.job.is_none() {
                let label = if self.connection.is_some() {
                    "Replace Apple API key"
                } else {
                    "Save Apple API key"
                };
                if ui
                    .add(egui::Button::new(label).min_size(egui::vec2(180.0, 44.0)))
                    .clicked()
                {
                    self.start_create(ui.ctx(), JobKind::Connect);
                    self.changed = false;
                    self.feedback = Some("Checking Apple API key…".into());
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
        let settings = AppleSettings::finish_loading(serde_json::from_str(json).unwrap());
        assert!(settings.has_key_input());
        assert_eq!(settings.vendor_number, "123");
        assert!(settings.legacy_migration_pending);
        assert!(!settings.migration_attempted);
    }

    fn poll_until_settled(
        settings: &mut AppleSettings,
        save: impl Fn(&AppleSettings) -> Result<(), String>,
    ) {
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            settings.poll_with_save(&ctx, |settings| save(settings));
            if settings.job.is_none() && !settings.legacy_migration_pending {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "local validation stalled"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn local_key_migrates_persists_and_reloads_without_hosted_setup() {
        let key = AppleCredentials::test_credentials("issuer");
        let legacy = serde_json::json!({
            "issuer_id":key.issuer_id, "key_id":key.key_id,
            "private_key":key.private_key, "filename":"test.p8", "vendor_number":"123",
        });
        let mut settings = AppleSettings::finish_loading(serde_json::from_value(legacy).unwrap());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("apple-credentials.json");
        poll_until_settled(&mut settings, |settings| {
            crate::storage::save_credentials(&path, &serde_json::to_vec(settings).unwrap())
        });
        assert!(settings.credentials() == Some(key.clone()));
        assert!(settings.private_key.is_empty());
        let bytes = fs::read(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved["connection"]["privateKey"], key.private_key);
        assert!(saved.get("private_key").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut restored = AppleSettings::finish_loading(serde_json::from_slice(&bytes).unwrap());
        assert!(restored.credentials().is_none());
        poll_until_settled(&mut restored, |_| {
            panic!("saved keys need no migration write")
        });
        assert!(restored.credentials() == Some(key));
        assert_eq!(restored.saved_vendor_number(), "123");
    }

    #[test]
    fn hosted_credentials_request_a_local_key_and_preserve_vendor_number() {
        let hosted = serde_json::json!({
            "connection":{
                "connectionId":"conn_old", "baseUrl":"https://parsequerade.mainbrella.com/v1/connections/conn_old",
                "apiKey":"pq_old", "accountId":"old-account",
            },
            "vendor_number":"123",
        });
        let mut settings = AppleSettings::finish_loading(serde_json::from_value(hosted).unwrap());
        poll_until_settled(&mut settings, |_| panic!("hosted keys cannot be migrated"));
        assert!(settings.credentials().is_none());
        assert!(settings.connection.is_none());
        assert_eq!(settings.saved_vendor_number(), "123");
        assert!(settings.error);
        assert!(settings.feedback.as_deref().unwrap().contains(".p8"));
        assert!(!serde_json::to_string(&settings).unwrap().contains("pq_old"));
    }

    #[test]
    fn newly_entered_complete_key_does_not_start_automatic_migration() {
        let mut settings = AppleSettings {
            issuer_id: "issuer".into(),
            key_id: "key".into(),
            private_key: "pem".into(),
            ..AppleSettings::default()
        };
        let mut launches = 0;

        settings.poll_with_save_and_migration(
            &egui::Context::default(),
            |_| Ok(()),
            |_, _| launches += 1,
        );

        assert_eq!(launches, 0);
        assert!(settings.job.is_none());
        assert!(!settings.migration_attempted);
    }

    #[test]
    fn failed_legacy_migration_is_consumed_and_never_retried_automatically() {
        let json = r#"{"issuer_id":"issuer","key_id":"key","private_key":"pem"}"#;
        let mut settings = AppleSettings::finish_loading(serde_json::from_str(json).unwrap());
        let mut launches = 0;
        let ctx = egui::Context::default();

        for _ in 0..3 {
            settings.poll_with_save_and_migration(
                &ctx,
                |_| Ok(()),
                |settings, _| {
                    launches += 1;
                    let (sender, receiver) = mpsc::channel();
                    sender.send(Err("legacy connection failed".into())).unwrap();
                    settings.job = Some(receiver);
                    settings.job_kind = Some(JobKind::Migrate);
                },
            );
        }

        assert_eq!(launches, 1);
        assert!(!settings.legacy_migration_pending);
        assert!(settings.migration_attempted);
        assert!(settings.job.is_none());
        assert!(
            settings
                .feedback
                .as_deref()
                .unwrap()
                .contains("legacy connection failed")
        );
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

        settings.poll_with_save(&egui::Context::default(), |staged| {
            assert!(staged.connection == Some(created.clone()));
            Ok(())
        });

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
            issuer_id: "replacement issuer".into(),
            key_id: "replacement key".into(),
            private_key: "replacement pem".into(),
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
        assert!(!settings.migration_attempted);
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
            issuer_id: "legacy issuer".into(),
            key_id: "legacy key".into(),
            private_key: "legacy pem".into(),
            legacy_migration_pending: true,
            pending_save: Some(created.clone()),
            ..AppleSettings::default()
        };

        settings.persist_pending_connection_with(|_| Err("disk unavailable".into()));

        assert!(settings.connection == Some(previous.clone()));
        assert!(settings.saved_credentials == Some(previous));
        assert!(settings.pending_save == Some(created));
        let mut migration_started = false;
        settings.poll_with_save_and_migration(
            &egui::Context::default(),
            |_| Ok(()),
            |_, _| migration_started = true,
        );
        assert!(!migration_started);
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
                    let mut keepalive = None;
                    let mut settings = match state {
                        "connecting" => {
                            let (sender, receiver) = mpsc::channel();
                            keepalive = Some(sender);
                            AppleSettings {
                                job_kind: Some(JobKind::Connect),
                                job: Some(receiver),
                                ..AppleSettings::default()
                            }
                        }
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
                    drop(keepalive);
                }
            }
        }
    }
}
