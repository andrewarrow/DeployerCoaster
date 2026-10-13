use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    codex_ui::Codex,
    description::{
        DescriptionRewriteRequest, RewrittenDescription, validate_rewritten_description,
    },
    website::WebsiteRequest,
};

const STEP_COUNT: u8 = 9;

pub(crate) enum WizardAction {
    None,
    Finished,
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Wizard {
    step: u8,
    app_name: String,
    original_description: String,
    rewritten_description: Option<RewrittenDescription>,
    website_started: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    website_created: Option<PathBuf>,
    #[serde(skip)]
    path: Option<PathBuf>,
    #[serde(skip)]
    error: Option<String>,
}

impl Default for Wizard {
    fn default() -> Self {
        Self {
            step: 1,
            app_name: String::new(),
            original_description: String::new(),
            rewritten_description: None,
            website_started: false,
            website_created: None,
            path: None,
            error: None,
        }
    }
}

impl Wizard {
    pub(crate) fn load() -> Self {
        match crate::storage::credential_path("wizard.json") {
            Ok(path) => Self::from_path(path),
            Err(error) => {
                log::warn!("Could not locate wizard storage: {error}");
                Self {
                    error: Some("Could not locate local storage for wizard progress.".into()),
                    ..Default::default()
                }
            }
        }
    }

    pub(crate) fn from_path(path: PathBuf) -> Self {
        let result = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes)
                .map_err(|error| error.to_string()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        };
        match result {
            Ok(mut wizard) => {
                wizard.path = Some(path);
                wizard.step = wizard.step.clamp(1, STEP_COUNT);
                if wizard.rewritten_description.as_ref().is_some_and(|rewritten| {
                    rewritten.name != wizard.app_name.trim()
                        || rewritten.original != wizard.original_description
                        || validate_rewritten_description(&rewritten.name, &rewritten.description)
                            .is_err()
                }) {
                    wizard.rewritten_description = None;
                    wizard.website_started = false;
                    wizard.website_created = None;
                }
                if wizard.app_name.trim().is_empty() || wizard.original_description.trim().is_empty() {
                    wizard.step = 1;
                    wizard.rewritten_description = None;
                    wizard.website_started = false;
                    wizard.website_created = None;
                } else if wizard.step >= 3 && wizard.rewritten_description.is_none() {
                    wizard.step = 2;
                    wizard.website_started = false;
                    wizard.website_created = None;
                }
                let slug = wizard.app_name.trim().to_lowercase().replace(' ', "-");
                if !wizard.website_started || wizard.website_created.as_ref().is_some_and(|root| {
                    root.file_name().is_none_or(|name| name != slug.as_str())
                }) {
                    wizard.website_created = None;
                }
                if wizard.step > 4 && wizard.website_created.is_none() {
                    wizard.step = 4;
                }
                wizard
            }
            Err(error) => {
                log::warn!("Could not load {}: {error}", path.display());
                Self {
                    error: Some("Could not load saved wizard progress. Check the local wizard file and restart the app.".into()),
                    ..Default::default()
                }
            }
        }
    }

    fn persist(&self) -> Result<(), String> {
        let path = self.path.as_deref().ok_or("Local storage is unavailable.")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        crate::storage::atomic_write(path, &bytes)
    }

    fn save_progress(&mut self) -> bool {
        if let Err(error) = self.persist() {
            log::warn!("Could not save wizard progress: {error}");
            self.error = Some("Could not save wizard progress locally. Check storage permissions and try again.".into());
            return false;
        }
        true
    }

    pub(crate) fn receive_rewritten_description(&mut self, result: RewrittenDescription) {
        if result.name == self.app_name.trim() && result.original == self.original_description {
            self.rewritten_description = Some(result);
            self.error = None;
            self.save_progress();
        }
    }

    pub(crate) fn receive_created_website(&mut self, root: PathBuf) {
        let slug = self.app_name.trim().to_lowercase().replace(' ', "-");
        if self.website_started && root.file_name().is_some_and(|name| name == slug.as_str()) {
            self.website_created = Some(root);
            self.error = None;
            self.save_progress();
        }
    }

    fn start_rewrite(&mut self, codex: &mut Codex, root: &Path) -> Result<(), String> {
        codex.can_rewrite_description()?;
        let request = DescriptionRewriteRequest::new(
            self.app_name.trim(),
            &self.original_description,
        )?;
        codex.rewrite_description(request, root);
        self.rewritten_description = None;
        self.website_started = false;
        self.website_created = None;
        self.error = None;
        self.step = 2;
        Ok(())
    }

    fn create_website(&mut self, codex: &mut Codex, root: &Path) -> Result<(), String> {
        let rewritten = self
            .rewritten_description
            .as_ref()
            .ok_or("Rewrite the description before creating the website.")?;
        codex.can_create_website()?;
        let request = if self.website_started {
            WebsiteRequest::resume(&rewritten.name, &rewritten.description, root)?
        } else {
            WebsiteRequest::prepare(&rewritten.name, &rewritten.description, root)?
        };
        codex.create_website(request);
        self.website_started = true;
        self.website_created = None;
        self.error = None;
        Ok(())
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, codex: &mut Codex, root: &Path) -> WizardAction {
        let mut action = WizardAction::None;
        let mut changed = false;
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(740.0));
                ui.label(format!("Step {} of {STEP_COUNT}", self.step));
                ui.add_space(8.0);
                // Reserve the footer so long pasted descriptions never push
                // navigation outside the window.
                let content_height = (ui.available_height() - 60.0).max(0.0);
                egui::ScrollArea::vertical()
                    .id_salt(("wizard_content", self.step))
                    .max_height(content_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match self.step {
                            1 => {
                                let label = ui.label("App name");
                                let name = ui.add(
                                    egui::TextEdit::singleline(&mut self.app_name)
                                        .desired_width(f32::INFINITY)
                                        .hint_text("Enter an app name"),
                                ).labelled_by(label.id);
                                let label = ui.label("Original description");
                                let description = egui::ScrollArea::vertical()
                                    .id_salt("wizard_description_editor")
                                    .max_height(160.0)
                                    .auto_shrink([false, true])
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::multiline(&mut self.original_description)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(5)
                                                .hint_text("Paste the original app description"),
                                        ).labelled_by(label.id)
                                    }).inner;
                                if name.changed() || description.changed() {
                                    self.rewritten_description = None;
                                    self.website_started = false;
                                    self.website_created = None;
                                    self.error = None;
                                    changed = true;
                                }
                            }
                            2 => {
                                ui.heading("Rewrite description");
                                if self.rewritten_description.is_some() {
                                    ui.label("Description rewritten. Ready to review.");
                                } else if codex.description_rewrite_ui(ui) {
                                    match self.start_rewrite(codex, root) {
                                        Ok(()) => changed = true,
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                            }
                            3 => {
                                ui.heading(&self.app_name);
                                let preview_height = ((content_height - 112.0) / 2.0)
                                    .clamp(64.0, 160.0);
                                ui.label("Original description");
                                description_preview(ui, "wizard_original", &self.original_description, preview_height);
                                if let Some(rewritten) = &self.rewritten_description {
                                    ui.label("Rewritten description");
                                    ui.weak(format!("{} / 3999 characters", rewritten.description.chars().count()));
                                    description_preview(ui, "wizard_rewritten", &rewritten.description, preview_height);
                                }
                            }
                            4 => {
                                ui.heading("Website");
                                if self.website_created.is_some() {
                                    ui.label("Website created. Ready to continue.");
                                } else if self.website_started && codex.website_creation_ui(ui) {
                                    match self.create_website(codex, root) {
                                        Ok(()) => changed = true,
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                            }
                            _ => {}
                        }
                        if let Some(error) = &self.error {
                            ui.add(egui::Label::new(
                                egui::RichText::new(error).color(ui.visuals().error_fg_color),
                            ).wrap());
                        }
                    });
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.step == STEP_COUNT {
                            "Finish"
                        } else if self.step == 4 && !self.website_started {
                            "Create website"
                        } else {
                            "Next"
                        };
                        let enabled = match self.step {
                            1 => !self.app_name.trim().is_empty() && !self.original_description.trim().is_empty(),
                            2 | 3 => self.rewritten_description.is_some(),
                            4 => if self.website_started {
                                self.website_created.is_some()
                            } else {
                                self.rewritten_description.is_some()
                            },
                            _ => true,
                        };
                        if ui.add_enabled(enabled, egui::Button::new(label).min_size(egui::vec2(80.0, 44.0))).clicked() {
                            match self.step {
                                1 => {
                                    match self.start_rewrite(codex, root) {
                                        Ok(()) => changed = true,
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                                4 if !self.website_started => {
                                    match self.create_website(codex, root) {
                                        Ok(()) => {
                                            changed = true;
                                        }
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                                STEP_COUNT => {
                                    let mut finished = Self { path: self.path.clone(), ..Default::default() };
                                    if finished.save_progress() {
                                        *self = finished;
                                        action = WizardAction::Finished;
                                    } else {
                                        self.error = finished.error;
                                    }
                                }
                                _ => {
                                    self.step += 1;
                                    changed = true;
                                }
                            }
                        }
                        if self.step > 1 && ui.add(egui::Button::new("Back").min_size(egui::vec2(80.0, 44.0))).clicked() {
                            if self.step == 2 || self.step == 4 {
                                codex.abandon_wizard_generation();
                            }
                            if self.step == 2 {
                                self.rewritten_description = None;
                                self.website_started = false;
                                self.website_created = None;
                            }
                            self.error = None;
                            self.step -= 1;
                            changed = true;
                        }
                    });
                });
            });
        if changed {
            self.save_progress();
        }
        action
    }
}

fn description_preview(ui: &mut egui::Ui, id: &str, description: &str, height: f32) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add(egui::Label::new(description).selectable(true).wrap());
        });
}

#[cfg(test)]
mod tests;
