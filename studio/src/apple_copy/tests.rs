use super::*;
use egui::{Pos2, Rect, Shape, vec2};

fn app(id: &str) -> StoreApp {
    StoreApp {
        identifier: format!("com.example.{id}"),
        name: format!("{id} — an app with a long readable name"),
        store_id: id.into(),
        store: crate::app_icons::Store::Apple,
    }
}

#[test]
fn progress_modal_fits_supported_viewports_and_shows_request_results() {
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
        (390.0, 360.0),
    ] {
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let mut copy = AppCopy {
            progress: Some(Progress {
                context: ctx.clone(),
                source: app("source"),
                target: Some(app("target")),
                steps: vec![
                    Step {
                        label: "Load destination app".into(),
                        request: "GET /v1/apps/target".into(),
                        state: StepState::Success("HTTP 200".into()),
                    },
                    Step {
                        label: "Copy privacy URL (en-US)".into(),
                        request: format!("PATCH /v1/appInfoLocalizations/{}", "long-id".repeat(30)),
                        state: StepState::Failed("HTTP 403: permission denied".into()),
                    },
                    Step {
                        label: "Copy copyright".into(),
                        request: "PATCH /v1/appStoreVersions/target-version".into(),
                        state: StepState::Skipped,
                    },
                ],
                outcome: Some(Err(
                    "Paste stopped. Completed changes have been kept.".into()
                )),
            }),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
                    ..Default::default()
                },
                |ui| {
                    copy.ui(ui.ctx());
                },
            ));
        }
        let output = output.unwrap();
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let Shape::Text(t) = &s.shape {
                    Some(t.galley.text())
                } else {
                    None
                }
            })
            .collect();
        assert!(labels.contains(&"Paste App Settings"));
        assert!(labels.contains(&"1 completed · 1 skipped · 1 failed"));
        assert!(labels.contains(&"Close"));
        for clipped in &output.shapes {
            if let Shape::Text(text) = &clipped.shape {
                let visible =
                    Rect::from_min_size(text.pos, text.galley.size()).intersect(clipped.clip_rect);
                if visible.is_positive() {
                    assert!(
                        visible.left() >= 0.0
                            && visible.right() <= width + 1.0
                            && visible.top() >= 0.0
                            && visible.bottom() <= height + 1.0,
                        "Modal overflow at {width}x{height}: {visible:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn disconnected_job_exits_loading_and_preserves_error() {
    let ctx = egui::Context::default();
    let (sender, receiver) = mpsc::channel();
    drop(sender);
    let mut copy = AppCopy {
        job: Some(Job {
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
        }),
        progress: Some(Progress {
            context: ctx,
            source: app("source"),
            target: None,
            steps: Vec::new(),
            outcome: None,
        }),
        ..Default::default()
    };
    copy.poll();
    assert!(copy.job.is_none());
    assert!(matches!(copy.progress.unwrap().outcome, Some(Err(_))));
}

#[test]
fn dropping_operation_cancels_worker() {
    let (_, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let job = Job {
        receiver,
        cancelled: cancelled.clone(),
    };
    drop(job);
    assert!(cancelled.load(Ordering::Relaxed));
}
