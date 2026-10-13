const STEP_COUNT: u8 = 9;

pub struct Wizard {
    step: u8,
    app_name: String,
}

impl Default for Wizard {
    fn default() -> Self {
        Self {
            step: 1,
            app_name: String::new(),
        }
    }
}

impl Wizard {
    /// Returns true when the user finishes the final step.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut finished = false;
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(format!("Step {} of {STEP_COUNT}", self.step));
                if self.step == 1 {
                    ui.add_space(24.0);
                    let label = ui.label("App name");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.app_name)
                            .desired_width(f32::INFINITY)
                            .hint_text("Enter an app name"),
                    )
                    .labelled_by(label.id);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.step == STEP_COUNT {
                            if ui.button("Finish").clicked() {
                                finished = true;
                            }
                        } else if ui.button("Next").clicked() {
                            self.step += 1;
                        }
                        if self.step > 1 && ui.button("Back").clicked() {
                            self.step -= 1;
                        }
                    });
                });
            })
            .inner;
        finished
    }
}
