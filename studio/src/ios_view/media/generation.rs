use super::*;
use crate::{
    codex_ui::Codex,
    ios_images::{ImageKind, ImageRequest, UploadedImage},
};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Generation {
    pub(super) briefs: [String; 2],
    task: Option<Task>,
    error: Option<(ImageKind, String)>,
}

struct Task {
    request: ImageRequest,
    codex: Codex,
    receipt: Option<UploadedImage>,
    reload: bool,
    verification: Option<Result<(), String>>,
}

impl Generation {
    pub(super) fn poll(&mut self, ctx: &Context) -> bool {
        let Some(task) = &mut self.task else {
            return false;
        };
        task.codex.poll(ctx);
        if let Some(receipt) = task.codex.take_uploaded_image() {
            task.receipt = Some(receipt);
            task.reload = true;
            task.verification = None;
        }
        std::mem::take(&mut task.reload)
    }

    pub(super) fn verify(&mut self, result: &Result<Media, String>) {
        let Some(task) = &mut self.task else { return };
        let Some(receipt) = &task.receipt else { return };
        task.verification = Some(match result.as_ref().map(|m| &m.placements) {
            Ok(Ok(placements)) if placements.iter().any(|placed| {
                attr(&placed.placement, "id") == receipt.placement_id
                    && attr(&placed.placement, "placementType") == task.request.kind.placement()
                    && attr(&placed.placement, "state") != "PARENT_FAILED"
                    && placed.image.as_ref().is_ok_and(|image| {
                        attr(image, "id") == receipt.image_id && attr(image, "state") != "FAILED"
                    })
            }) => Ok(()),
            Ok(Ok(_)) => Err("The uploaded image could not be found on this listing. Reload media to check again before retrying.".into()),
            _ => Err("Could not check the uploaded image. Reload media to verify it.".into()),
        });
    }

    pub(super) fn ui(
        &mut self,
        ui: &mut Ui,
        kind: ImageKind,
        details: &AppDetails,
        locale: &str,
        missing: bool,
        credentials: Option<&AppleCredentials>,
    ) {
        let running = self
            .task
            .as_ref()
            .is_some_and(|task| task.codex.image_generation_running());
        let active = self
            .task
            .as_ref()
            .is_some_and(|task| task.request.kind == kind);
        if missing && !active {
            let index = if kind == ImageKind::Header { 0 } else { 1 };
            ui.add_space(8.0);
            let label = ui.label("App description");
            ui.add_enabled(
                !running && credentials.is_some(),
                egui::TextEdit::multiline(&mut self.briefs[index])
                    .id_salt(("image_brief", kind.placement()))
                    .hint_text("App about heart rate tracking")
                    .desired_rows(2)
                    .desired_width(ui.available_width().min(600.0)),
            )
            .labelled_by(label.id);
            let (width, height) = kind.dimensions();
            ui.weak(format!("PNG · {width} × {height}px"));
            if ui
                .add_enabled(
                    !running && credentials.is_some() && !self.briefs[index].trim().is_empty(),
                    egui::Button::new(format!("Generate and upload {}", kind.label()))
                        .min_size(vec2(1.0, 44.0)),
                )
                .clicked()
            {
                self.error = None;
                match ImageRequest::prepare(
                    details,
                    locale,
                    &self.briefs[index],
                    kind,
                    credentials.expect("generation is enabled only with Apple credentials"),
                ) {
                    Ok(request) => {
                        let codex = Codex::for_image(request.clone());
                        self.task = Some(Task {
                            request,
                            codex,
                            receipt: None,
                            reload: false,
                            verification: None,
                        });
                    }
                    Err(error) => self.error = Some((kind, error)),
                }
            }
        }
        if let Some((error_kind, error)) = &self.error
            && *error_kind == kind
        {
            ui.add(Label::new(RichText::new(error).color(ui.visuals().error_fg_color)).wrap());
        }
        if let Some(task) = &mut self.task
            && task.request.kind == kind
        {
            ui.add_space(8.0);
            if let Some(verification) = &task.verification {
                match verification {
                    Ok(()) => {
                        ui.label("Image uploaded.");
                    }
                    Err(error) => {
                        ui.add(
                            Label::new(RichText::new(error).color(ui.visuals().error_fg_color))
                                .wrap(),
                        );
                    }
                }
            } else if task.receipt.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking the uploaded image…");
                });
            } else {
                task.codex.image_generation_ui(ui);
            }
            if let Ok(url) = reqwest::Url::from_directory_path(&task.request.root) {
                ui.hyperlink_to("Generated files", url.as_str());
            }
            // A cancelled or failed request can be dismissed to edit its original brief.
            if !task.codex.image_generation_running()
                && task.receipt.is_none()
                && ui
                    .add(egui::Button::new("Edit description").min_size(vec2(1.0, 44.0)))
                    .clicked()
            {
                self.task = None;
            }
        }
    }
}
