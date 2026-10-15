#[cfg(test)]
mod api;
#[cfg(test)]
mod tests;

#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

#[cfg(test)]
use crate::{apple::AppleCredentials, dashboard::StoreApp};
#[cfg(test)]
#[cfg(test)]
#[derive(Clone)]
enum StepState {
    Pending,
    Running,
    Success(String),
    Failed(String),
    Skipped,
}

#[cfg(not(test))]
#[derive(Default)]
pub(crate) struct AppCopy;

#[cfg(not(test))]
impl AppCopy {
    pub(crate) fn context_menu(
        &mut self,
        response: &egui::Response,
        _app: Option<&crate::dashboard::StoreApp>,
        _credentials: Option<&crate::apple::AppleCredentials>,
    ) {
        response.context_menu(|ui| {
            let reason = "Copy and paste are unavailable because Parsequerade is read-only.";
            ui.add_enabled(false, egui::Button::new("Copy App"))
                .on_disabled_hover_text(reason);
            ui.add_enabled(false, egui::Button::new("Paste"))
                .on_disabled_hover_text(reason);
        });
    }

    pub(crate) fn ui(&mut self, _ctx: &egui::Context) {}
}

#[cfg(test)]
struct Step {
    label: String,
    request: String,
    state: StepState,
}

#[cfg(test)]
enum Event {
    Step {
        label: String,
        request: String,
        state: StepState,
    },
    Status(usize, StepState),
    Done(Result<(), String>),
}

#[cfg(test)]
struct Job {
    receiver: Receiver<Event>,
    cancelled: Arc<AtomicBool>,
}

#[cfg(test)]
impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[cfg(test)]
struct Progress {
    context: egui::Context,
    source: StoreApp,
    target: Option<StoreApp>,
    steps: Vec<Step>,
    outcome: Option<Result<(), String>>,
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct AppCopy {
    job: Option<Job>,
    progress: Option<Progress>,
}

#[cfg(test)]
impl AppCopy {
    pub(crate) fn context_menu(
        &mut self,
        response: &egui::Response,
        _app: Option<&StoreApp>,
        _credentials: Option<&AppleCredentials>,
    ) {
        response.context_menu(|ui| {
            let reason = "Copy and paste are unavailable because Parsequerade is read-only.";
            ui.add_enabled(false, egui::Button::new("Copy App"))
                .on_disabled_hover_text(reason);
            ui.add_enabled(false, egui::Button::new("Paste"))
                .on_disabled_hover_text(reason);
        });
    }

    #[allow(dead_code)]
    fn start(
        &mut self,
        _source: StoreApp,
        _target: Option<StoreApp>,
        _credentials: AppleCredentials,
        _ctx: &egui::Context,
    ) {
        // Keep this second guard even if another caller is added later: copy/paste
        // currently depends on Apple write routes that Parsequerade does not expose.
    }

    fn poll(&mut self) {
        loop {
            let Some(job) = &self.job else {
                break;
            };
            let event = match job.receiver.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => Event::Done(Err("The operation stopped unexpectedly. Check the destination before pasting again.".into())),
            };
            let Some(progress) = &mut self.progress else {
                self.job = None;
                break;
            };
            match event {
                Event::Step {
                    label,
                    request,
                    state,
                } => progress.steps.push(Step {
                    label,
                    request,
                    state,
                }),
                Event::Status(index, state) => {
                    if let Some(step) = progress.steps.get_mut(index) {
                        step.state = state;
                    }
                }
                Event::Done(result) => {
                    progress.outcome = Some(result);
                    self.job = None;
                    break;
                }
            }
        }
    }

    pub(crate) fn ui(&mut self, ctx: &egui::Context) {
        self.poll();
        let Some(progress) = &self.progress else {
            return;
        };
        // The Apple list can also live in a separate native window.
        if progress.context != *ctx {
            return;
        }
        let busy = self.job.is_some();
        let stopping = self
            .job
            .as_ref()
            .is_some_and(|j| j.cancelled.load(Ordering::Relaxed));
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("app_copy_progress")).show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 48.0).clamp(0.0, 620.0);
            ui.set_width(width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.heading(if progress.target.is_some() {
                "Paste App Settings"
            } else {
                "Copy App"
            });
            egui::ScrollArea::vertical()
                .id_salt("app_copy_progress_scroll")
                .max_height((ctx.content_rect().height() - 150.0).max(80.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.label(format!(
                        "From: {} ({})",
                        progress.source.name, progress.source.store_id
                    ));
                    if let Some(target) = &progress.target {
                        ui.label(format!("To: {} ({})", target.name, target.store_id));
                    }
                    ui.add_space(8.0);
                    if busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(if stopping {
                                "Stopping after the current request…"
                            } else {
                                "Copying settings…"
                            });
                        });
                    }
                    if let Some(outcome) = &progress.outcome {
                        match outcome {
                            Ok(()) if progress.target.is_none() => {
                                ui.label("App copied. Right-click another app and choose Paste.");
                            }
                            Ok(()) => {
                                ui.label("Paste complete.");
                            }
                            Err(error) => {
                                ui.colored_label(ui.visuals().error_fg_color, error);
                            }
                        }
                    }
                    let completed = progress
                        .steps
                        .iter()
                        .filter(|s| matches!(s.state, StepState::Success(_)))
                        .count();
                    let skipped = progress
                        .steps
                        .iter()
                        .filter(|s| matches!(s.state, StepState::Skipped))
                        .count();
                    let failed = progress
                        .steps
                        .iter()
                        .filter(|s| matches!(s.state, StepState::Failed(_)))
                        .count();
                    ui.label(format!(
                        "{completed} completed · {skipped} skipped · {failed} failed"
                    ));
                    ui.add_space(8.0);
                    for step in &progress.steps {
                        let (status, color) = match &step.state {
                            StepState::Pending => ("Queued", ui.visuals().weak_text_color()),
                            StepState::Running => ("Running", ui.visuals().text_color()),
                            StepState::Success(_) => ("Done", ui.visuals().text_color()),
                            StepState::Failed(_) => ("Failed", ui.visuals().error_fg_color),
                            StepState::Skipped => ("Skipped", ui.visuals().weak_text_color()),
                        };
                        ui.colored_label(color, format!("{status} — {}", step.label));
                        if !step.request.is_empty() {
                            ui.label(egui::RichText::new(&step.request).small().weak());
                        }
                        match &step.state {
                            StepState::Success(status) => {
                                ui.label(egui::RichText::new(status).small().weak());
                            }
                            StepState::Failed(error) => {
                                ui.colored_label(ui.visuals().error_fg_color, error);
                            }
                            _ => {}
                        }
                        ui.add_space(8.0);
                    }
                });
            ui.add_space(8.0);
            if busy {
                if ui
                    .add_enabled(
                        !stopping,
                        egui::Button::new("Stop").min_size(egui::vec2(80.0, 44.0)),
                    )
                    .clicked()
                {
                    self.job
                        .as_ref()
                        .unwrap()
                        .cancelled
                        .store(true, Ordering::Relaxed);
                }
            } else {
                close = ui
                    .add(egui::Button::new("Close").min_size(egui::vec2(80.0, 44.0)))
                    .clicked();
            }
        });
        if !busy && (close || response.should_close()) {
            self.progress = None;
        }
    }
}
