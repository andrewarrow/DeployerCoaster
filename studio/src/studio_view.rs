use egui::{Button, CentralPanel, Frame, Margin, Panel, Ui};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Platform {
    #[default]
    Ios,
    Android,
}

#[derive(Default)]
pub(crate) struct StudioView {
    platform: Platform,
}

impl StudioView {
    pub(crate) fn ui(&mut self, ui: &mut Ui) {
        Panel::left("studio_platform_navigation")
            .exact_size(160.0)
            .resizable(false)
            .frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::same(8)))
            .show(ui, |ui| {
                for (platform, label) in [(Platform::Ios, "ios"), (Platform::Android, "android")] {
                    if ui
                        .add_sized(
                            [ui.available_width(), 44.0],
                            Button::selectable(self.platform == platform, "").left_text(label),
                        )
                        .clicked()
                    {
                        self.platform = platform;
                    }
                }
            });

        CentralPanel::default().show(ui, |ui| match self.platform {
            Platform::Ios => crate::ios_view::ui(ui),
            Platform::Android => crate::android_view::ui(ui),
        });
    }
}
