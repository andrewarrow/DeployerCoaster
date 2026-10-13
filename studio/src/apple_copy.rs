mod api;
#[cfg(test)]
mod tests;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

use crate::{apple::AppleCredentials, dashboard::StoreApp};
use api::{AppleApi, Session, Snapshot};

#[derive(Clone)]
enum StepState {
    Pending,
    Running,
    Success(String),
    Failed(String),
    Skipped,
}

struct Step {
    label: String,
    request: String,
    state: StepState,
}

enum Event {
    Step {
        label: String,
        request: String,
        state: StepState,
    },
    Status(usize, StepState),
    Copied(Box<Snapshot>),
    Done(Result<(), String>),
}

struct Job {
    receiver: Receiver<Event>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

struct CopiedApp {
    app: StoreApp,
    snapshot: Snapshot,
}

struct Progress {
    context: egui::Context,
    source: StoreApp,
    target: Option<StoreApp>,
    steps: Vec<Step>,
    outcome: Option<Result<(), String>>,
}

#[derive(Default)]
pub(crate) struct AppCopy {
    copied: Option<CopiedApp>,
    job: Option<Job>,
    progress: Option<Progress>,
}

impl AppCopy {
    pub(crate) fn context_menu(
        &mut self,
        response: &egui::Response,
        app: Option<&StoreApp>,
        credentials: Option<&AppleCredentials>,
    ) {
        response.context_menu(|ui| {
            let enabled = app.is_some() && credentials.is_some() && self.job.is_none();
            if ui
                .add_enabled(enabled, egui::Button::new("Copy App"))
                .clicked()
            {
                let app = app.unwrap().clone();
                ui.ctx().copy_text(app.store_id.clone());
                self.start(app, None, credentials.unwrap().clone(), ui.ctx());
                ui.close();
            }
            let paste_reason = if app.is_none() {
                "Copy and paste settings is available for App Store Connect apps.".to_owned()
            } else if credentials.is_none() {
                "Connect an Apple account in Settings first.".into()
            } else if self.job.is_some() {
                "Wait for the current operation to finish.".into()
            } else if let Some(copied) = &self.copied {
                if Some(&copied.app.store_id) == app.map(|a| &a.store_id) {
                    "Choose a different destination app.".into()
                } else {
                    String::new()
                }
            } else {
                "Copy an app first.".into()
            };
            let paste = ui.add_enabled(paste_reason.is_empty(), egui::Button::new("Paste"));
            if paste_reason.is_empty() {
                let source = &self.copied.as_ref().unwrap().app;
                if paste
                    .on_hover_text(format!("Copy settings from {} to this app", source.name))
                    .clicked()
                {
                    self.start(
                        source.clone(),
                        app.cloned(),
                        credentials.unwrap().clone(),
                        ui.ctx(),
                    );
                    ui.close();
                }
            } else {
                paste.on_disabled_hover_text(paste_reason);
            }
        });
    }

    fn start(
        &mut self,
        source: StoreApp,
        target: Option<StoreApp>,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
        if self.job.is_some() {
            return;
        }
        let snapshot = if target.is_some() {
            self.copied.as_ref().map(|c| c.snapshot.clone())
        } else {
            // Failed copies must not leave an older app available as the paste source.
            self.copied = None;
            None
        };
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.job = Some(Job {
            receiver,
            cancelled: cancelled.clone(),
        });
        self.progress = Some(Progress {
            context: ctx.clone(),
            source: source.clone(),
            target: target.clone(),
            steps: Vec::new(),
            outcome: None,
        });
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut emit = |event| {
                let _ = sender.send(event);
                ctx.request_repaint();
            };
            let result = (|| {
                let api = AppleApi::new(credentials)?;
                let mut session = Session::new(api, &cancelled, &mut emit);
                if let Some(target) = target {
                    let source = snapshot.ok_or_else(|| "Copy an app first.".to_owned())?;
                    let destination = session.load(&target.store_id, true)?;
                    let plan = api::plan(&source, &destination)?;
                    session.apply(plan)
                } else {
                    let snapshot = session.load(&source.store_id, false)?;
                    (session.emit)(Event::Copied(Box::new(snapshot)));
                    Ok(())
                }
            })();
            emit(Event::Done(result));
        });
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
                Event::Copied(snapshot) => {
                    self.copied = Some(CopiedApp {
                        app: progress.source.clone(),
                        snapshot: *snapshot,
                    });
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
