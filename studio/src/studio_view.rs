use egui::{Button, CentralPanel, Frame, Margin, Panel, Ui};

use crate::{apple::AppleStore, dashboard::StoreApp, play_store::PlayStore};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Platform {
    #[default]
    Ios,
    Android,
}

#[cfg(test)]
mod tests;

impl Platform {
    fn label(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

#[derive(Default)]
pub(crate) struct StudioView {
    platform: Platform,
    selected_app: Option<(Platform, String)>,
}

impl StudioView {
    pub(crate) fn ui(
        &mut self,
        ui: &mut Ui,
        apple_store: &mut AppleStore,
        play_store: &mut PlayStore,
    ) {
        let ctx = ui.ctx().clone();
        apple_store.prepare_dashboard(&ctx);
        play_store.prepare_dashboard(&ctx);

        Panel::left("studio_platform_navigation")
            .exact_size(ui.available_width().min(240.0))
            .resizable(false)
            .frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::same(8)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.platform_tree(
                        ui,
                        Platform::Ios,
                        apple_store.dashboard_apps(),
                        apple_store.dashboard_status(),
                    );
                    self.platform_tree(
                        ui,
                        Platform::Android,
                        play_store.dashboard_apps(),
                        play_store.dashboard_status(),
                    );
                });
            });

        CentralPanel::default().show(ui, |ui| match self.platform {
            Platform::Ios => apple_store
                .app_details_ui(ui, self.selected_app.as_ref().map(|(_, id)| id.as_str())),
            Platform::Android => {
                apple_store.clear_app_details_selection();
                crate::android_view::ui(ui);
            }
        });
    }

    fn platform_tree(
        &mut self,
        ui: &mut Ui,
        platform: Platform,
        mut apps: Vec<StoreApp>,
        status: crate::dashboard::StoreStatus,
    ) {
        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        let id = ui.make_persistent_id(("studio_platform", platform));
        let state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);

        let mut root_clicked = false;
        let mut header = state.show_header(ui, |ui| {
            let selected = self.platform == platform && self.selected_app.is_none();
            let spinner_width = if status.loading { 24.0 } else { 0.0 };
            let response = ui.add_sized(
                [(ui.available_width() - spinner_width).max(0.0), 32.0],
                Button::selectable(selected, "")
                    .left_text(platform.label())
                    .truncate(),
            );
            root_clicked = response.clicked();
            if root_clicked {
                self.platform = platform;
                self.selected_app = None;
            }
            if status.loading {
                ui.add(egui::Spinner::new().size(16.0));
            }
        });
        if root_clicked {
            header.toggle();
        }

        header.body(|ui| {
            if status.loading && apps.is_empty() {
                return;
            }
            if let Some(error) = &status.error {
                ui.label(error).on_hover_text(error);
            }
            if !apps.is_empty() {
                for app in apps {
                    let selected =
                        self.selected_app
                            .as_ref()
                            .is_some_and(|(selected_platform, id)| {
                                *selected_platform == platform && id == &app.store_id
                            });
                    let response = ui
                        .push_id(("studio_app", platform, &app.store_id), |ui| {
                            ui.add_sized(
                                [ui.available_width(), 32.0],
                                Button::selectable(selected, "")
                                    .left_text(&app.name)
                                    .truncate(),
                            )
                        })
                        .inner;
                    response.clone().on_hover_text(&app.name);
                    if response.clicked() {
                        self.platform = platform;
                        self.selected_app = Some((platform, app.store_id));
                    }
                }
                return;
            }
            if status.loading || status.error.is_some() {
                return;
            }
            if !status.connected {
                ui.label("Not connected");
                return;
            }
            ui.label("No apps");
        });
    }
}
