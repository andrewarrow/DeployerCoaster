use egui::{Button, CentralPanel, RichText, Ui, vec2};

use crate::{
    apple::{AppleApp, AppleStore, catalog},
    commands::Command,
};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct StudioView {
    account: Option<String>,
    requested_app: Option<AppleApp>,
    recents: Vec<AppleApp>,
    recents_path: Option<std::path::PathBuf>,
    picking: bool,
    search: String,
    logo: Option<egui::TextureHandle>,
    error: Option<String>,
}

impl StudioView {
    pub(crate) fn open(&mut self) {
        self.picking = true;
        self.search.clear();
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn take_app_request(&mut self) -> Option<AppleApp> {
        self.requested_app.take()
    }

    fn set_account(&mut self, account: Option<String>) {
        if self.account != account {
            self.recents_path = account.as_deref().and_then(catalog::recents_path);
            self.recents = catalog::load_recents(self.recents_path.as_deref());
            self.account = account;
            self.requested_app = None;
            self.error = None;
        }
    }

    fn select(&mut self, app: AppleApp) {
        self.error =
            catalog::remember(&mut self.recents, app.clone(), self.recents_path.as_deref()).err();
        self.requested_app = Some(app);
        self.picking = false;
    }

    pub(crate) fn ui(&mut self, ui: &mut Ui, apple: &mut AppleStore) -> Option<Command> {
        self.set_account(apple.selection_account());
        if self.picking
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.picking = false;
        }
        let mut command = None;
        if self.picking {
            apple.prepare_app_list(ui.ctx());
        }
        CentralPanel::default().show(ui, |ui| {
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = ui.available_width().min(640.0);
                    let top = ((ui.available_height() - 430.0).max(28.0) * 0.42).min(180.0);
                    ui.add_space(top);
                    ui.horizontal(|ui| {
                        ui.add_space(((ui.available_width() - width) * 0.5).max(0.0));
                        ui.allocate_ui_with_layout(
                            vec2(width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(width);
                                if self.picking {
                                    command = self.picker(ui, apple);
                                } else {
                                    self.start_screen(ui);
                                }
                            },
                        );
                    });
                });
        });
        command
    }

    fn start_screen(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            if self.logo.is_none()
                && let Ok(image) = image::load_from_memory(crate::metadata::LOGO_BYTES)
            {
                let image = image.into_rgba8();
                self.logo = Some(ui.ctx().load_texture(
                    "startup_logo",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        &image,
                    ),
                    egui::TextureOptions::LINEAR,
                ));
            }
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::from_texture(logo).fit_to_exact_size(vec2(48.0, 48.0)));
            }
            ui.add_space(12.0);
            ui.heading(crate::metadata::APP_NAME);
            ui.weak("Open an app to begin");
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - ui.spacing().item_spacing.x) * 0.5;
            if ui
                .add_sized(
                    [width, 44.0],
                    Button::new(
                        RichText::new("Open App…").color(ui.visuals().selection.stroke.color),
                    )
                    .fill(ui.visuals().selection.bg_fill),
                )
                .clicked()
            {
                self.open();
            }
            // Creating apps is intentionally a no-op until that workflow is implemented.
            let _ = ui.add_sized([width, 44.0], Button::new("Create New…"));
        });
        ui.add_space(24.0);
        ui.label(RichText::new("Recent Apps").strong());
        if self.recents.is_empty() {
            ui.weak("Your opened apps will appear here.");
        } else {
            for app in self.recents.clone() {
                if app_row(ui, &app) {
                    self.select(app);
                }
            }
        }
    }

    fn picker(&mut self, ui: &mut Ui, apple: &mut AppleStore) -> Option<Command> {
        ui.horizontal(|ui| {
            ui.heading("Open App");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_sized([72.0, 44.0], Button::new("Cancel")).clicked() {
                    self.picking = false;
                }
            });
        });
        let status = apple.dashboard_status();
        if !status.connected {
            ui.label("Connect your Apple account through Parsequerade to load iOS apps.");
            return ui
                .add_sized([160.0, 44.0], Button::new("Apple settings…"))
                .clicked()
                .then_some(Command::AppleSettings);
        }
        ui.horizontal(|ui| {
            let label = ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .desired_width(ui.available_width())
                    .hint_text("App name, bundle ID, or App ID"),
            )
            .labelled_by(label.id);
        });
        if status.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading iOS apps…");
            });
        }
        if let Some(error) = &status.error {
            ui.add(
                egui::Label::new(RichText::new(error).color(ui.visuals().error_fg_color)).wrap(),
            );
            if ui.add_sized([80.0, 44.0], Button::new("Retry")).clicked() {
                apple.refresh_dashboard(ui.ctx());
            }
        }
        let search = self.search.trim().to_lowercase();
        let apps: Vec<_> = apple
            .picker_apps()
            .iter()
            .filter(|app| {
                app.title().to_lowercase().contains(&search)
                    || app.attributes.bundle_id.to_lowercase().contains(&search)
                    || app.id.to_lowercase().contains(&search)
            })
            .cloned()
            .collect();
        if apps.is_empty() && !status.loading && status.error.is_none() {
            ui.label(if search.is_empty() {
                "No iOS apps are available to this account."
            } else {
                "No apps match your search."
            });
        }
        for app in apps {
            if app_row(ui, &app) {
                self.select(app);
            }
        }
        None
    }
}

fn app_row(ui: &mut Ui, app: &AppleApp) -> bool {
    ui.push_id(&app.id, |ui| {
        let subtitle = if app.attributes.bundle_id.is_empty() {
            &app.id
        } else {
            &app.attributes.bundle_id
        };
        let response = ui.add_sized([ui.available_width(), 56.0], Button::new(""));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("Open {} ({subtitle})", app.title()),
            )
        });
        let text_rect = response.rect.shrink2(vec2(12.0, 6.0));
        let painter = ui.painter().with_clip_rect(text_rect);
        for (text, offset, color, size) in [
            (app.title(), -9.0, ui.visuals().text_color(), 14.0),
            (
                subtitle.as_str(),
                10.0,
                ui.visuals().weak_text_color(),
                12.0,
            ),
        ] {
            let galley = egui::WidgetText::from(RichText::new(text).size(size).color(color))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    text_rect.width(),
                    egui::TextStyle::Body,
                );
            let position = egui::pos2(
                text_rect.left(),
                text_rect.center().y + offset - galley.size().y * 0.5,
            );
            painter.galley(position, galley, color);
        }
        response
            .on_hover_text(format!("{}\n{}\nApp ID: {}", app.title(), subtitle, app.id))
            .clicked()
    })
    .inner
}
