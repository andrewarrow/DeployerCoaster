use std::path::Path;

use crate::{
    codex_ui::Codex,
    description::{DescriptionRewriteRequest, RewrittenDescription},
    website::WebsiteRequest,
};

const STEP_COUNT: u8 = 9;

pub(crate) enum WizardAction {
    None,
    Finished,
    WebsiteStarted,
}

pub struct Wizard {
    step: u8,
    app_name: String,
    original_description: String,
    rewritten_description: Option<RewrittenDescription>,
    website_started: bool,
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
            error: None,
        }
    }
}

impl Wizard {
    pub(crate) fn receive_rewritten_description(&mut self, result: RewrittenDescription) {
        if result.name == self.app_name.trim() && result.original == self.original_description {
            self.rewritten_description = Some(result);
            self.error = None;
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
        let request = WebsiteRequest::prepare(&rewritten.name, &rewritten.description, root)?;
        codex.create_website(request);
        self.website_started = true;
        self.error = None;
        self.step = 4;
        Ok(())
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, codex: &mut Codex, root: &Path) -> WizardAction {
        let mut action = WizardAction::None;
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_max_width(740.0);
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
                                    self.error = None;
                                }
                            }
                            2 => {
                                ui.heading("Rewrite description");
                                if self.rewritten_description.is_some() {
                                    ui.label("Description rewritten. Ready to review.");
                                } else {
                                    codex.description_rewrite_ui(ui);
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
                                if self.website_started {
                                    ui.label("Website creation started.");
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
                        } else if self.step == 3 && !self.website_started {
                            "Create website"
                        } else {
                            "Next"
                        };
                        let enabled = match self.step {
                            1 => !self.app_name.trim().is_empty() && !self.original_description.trim().is_empty(),
                            2 | 3 => self.rewritten_description.is_some(),
                            _ => true,
                        };
                        if ui.add_enabled(enabled, egui::Button::new(label).min_size(egui::vec2(80.0, 44.0))).clicked() {
                            match self.step {
                                1 => {
                                    if let Err(error) = self.start_rewrite(codex, root) {
                                        self.error = Some(error);
                                    }
                                }
                                3 if !self.website_started => {
                                    match self.create_website(codex, root) {
                                        Ok(()) => action = WizardAction::WebsiteStarted,
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                                STEP_COUNT => action = WizardAction::Finished,
                                _ => self.step += 1,
                            }
                        }
                        if self.step > 1 && ui.add(egui::Button::new("Back").min_size(egui::vec2(80.0, 44.0))).clicked() {
                            if self.step == 2 {
                                codex.abandon_description_rewrite();
                                self.rewritten_description = None;
                            }
                            self.error = None;
                            self.step -= 1;
                        }
                    });
                });
            })
            });
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
