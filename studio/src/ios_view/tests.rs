use super::*;
use egui::{Pos2, Rect, Shape, vec2};
use serde_json::json;

fn fixture() -> AppDetails {
    AppDetails {
        app: json!({"id": "123", "attributes": {"bundleId": "com.example.long.application.identifier", "sku": "example", "primaryLocale": "en-US"}}),
        info: Some(
            json!({"attributes": {"state": "PREPARE_FOR_SUBMISSION"}, "relationships": {"primaryCategory": {"data": {"id": "UTILITIES"}}}}),
        ),
        version: Some(
            json!({"attributes": {"versionString": "2.0", "appVersionState": "PREPARE_FOR_SUBMISSION", "copyright": "2026 Example", "createdDate": "2026-10-13T12:00:00Z"}}),
        ),
        info_localizations: vec![
            json!({"attributes": {"locale": "en-US", "name": "A long localized application name with several words", "subtitle": "An English subtitle", "privacyPolicyUrl": "https://example.com/a/very/long/privacy/policy/address"}}),
        ],
        version_localizations: vec![
            json!({"attributes": {"locale": "en-US", "description": "An English description with multiple paragraphs.\n\nThis is another paragraph with enough words to exercise wrapping in narrow windows.", "keywords": "example,app,test", "promotionalText": "Try our latest features", "whatsNew": "New features", "supportUrl": "https://example.com/support"}}),
        ],
        ..Default::default()
    }
}

fn texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

#[test]
fn selecting_another_app_discards_the_previous_job_and_preserves_its_cache() {
    let mut view = IosView::default();
    view.select("123");
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    let (sender, result) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    view.job = Some(Job {
        result,
        cancelled: cancelled.clone(),
    });
    sender.send(Ok(fixture())).unwrap();
    view.select("456");
    view.poll();
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(view.job.is_none());
    assert!(!view.cache.contains_key("456"));
    assert!(view.cache["123"].data.is_some());
    view.select("123");
    assert!(view.cache["123"].data.is_some());
    view.clear_selection();
    assert!(view.selected.is_none());
}

#[test]
fn failed_refresh_preserves_details_and_success_replaces_them() {
    let mut view = IosView::default();
    view.select("123");
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    let (sender, result) = mpsc::channel();
    view.job = Some(Job {
        result,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    sender.send(Err("Refresh failed".into())).unwrap();
    view.poll();
    assert!(view.cache["123"].data.is_some());
    assert_eq!(view.cache["123"].error.as_deref(), Some("Refresh failed"));
    let (sender, result) = mpsc::channel();
    view.job = Some(Job {
        result,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    let mut new_details = fixture();
    new_details.app["attributes"]["sku"] = json!("updated");
    sender.send(Ok(new_details)).unwrap();
    view.poll();
    assert!(view.cache["123"].error.is_none());
    assert_eq!(
        attr(&view.cache["123"].data.as_ref().unwrap().app, "sku"),
        "updated"
    );
}

#[test]
fn details_wrap_within_the_right_pane_at_supported_sizes_and_themes() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            let ctx = egui::Context::default();
            crate::style::configure(&ctx);
            ctx.set_theme(theme);
            let mut view = IosView::default();
            view.cache.insert(
                "123".into(),
                Entry {
                    data: Some(fixture()),
                    error: None,
                },
            );
            let credentials = AppleCredentials::test_credentials("test");
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
                    ..Default::default()
                },
                |ui| {
                    egui::Panel::left("tree").exact_size(240.0).show(ui, |_| {});
                    egui::CentralPanel::default().show(ui, |ui| {
                        view.ui(
                            ui,
                            Some(("123", "An application with a long name that wraps")),
                            Some(&credentials),
                        );
                    });
                },
            );
            let labels = texts(&output);
            assert!(labels.contains(&"Bundle ID"));
            assert!(labels.contains(&"en-US"));
            assert!(labels.contains(&"English localizations"));
            assert!(labels.contains(&"An English subtitle"));
            assert!(
                view.job.is_none(),
                "cached selection must not send requests"
            );
            for shape in &output.shapes {
                if let Shape::Text(text) = &shape.shape {
                    assert!(
                        text.pos.x + text.galley.size().x <= width + 1.0,
                        "overflow at {width}: {} (right {})",
                        text.galley.text(),
                        text.pos.x + text.galley.size().x
                    );
                }
            }
        }
    }
}

#[test]
fn loading_errors_and_empty_localizations_have_compact_feedback() {
    let ctx = egui::Context::default();
    let credentials = AppleCredentials::test_credentials("test");
    let mut view = IosView::default();
    view.select("123");
    let (_sender, result) = mpsc::channel();
    view.job = Some(Job {
        result,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    let input = || egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(768.0, 1024.0))),
        ..Default::default()
    };
    let output = ctx.run_ui(input(), |ui| {
        view.ui(ui, Some(("123", "Example")), Some(&credentials))
    });
    assert!(texts(&output).contains(&"Loading app details…"));
    assert!(view.cache.is_empty());
    view.job = None;
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(AppDetails {
                app: json!({"id": "123"}),
                ..Default::default()
            }),
            error: Some("Refresh failed".into()),
        },
    );
    let output = ctx.run_ui(input(), |ui| {
        view.ui(ui, Some(("123", "Example")), Some(&credentials))
    });
    let labels = texts(&output);
    assert!(labels.contains(&"Refresh failed"));
    assert!(labels.contains(&"Previously loaded details are shown."));
    assert!(labels.contains(&"No English localization is available."));
    assert!(labels.contains(&"No iOS App Store version is available."));
}
