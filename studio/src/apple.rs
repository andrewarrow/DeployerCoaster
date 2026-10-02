use serde::{Deserialize, Serialize};
use std::fs;

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
        result.unwrap_or_else(|error| Self {
            feedback: Some(error),
            error: true,
            ..Self::default()
        })
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
}
