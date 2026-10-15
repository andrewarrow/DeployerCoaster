use super::*;
use egui::{Pos2, Rect, Shape};
use serde_json::json;

const LONG_NAME: &str = "An exceptionally long iOS app name that must fit on a single row without expanding the chooser beyond the window";

fn app(id: &str, name: &str) -> AppleApp {
    serde_json::from_value(json!({"id": id, "name": name, "bundleId": "com.example.app"})).unwrap()
}

fn render(
    ctx: &egui::Context,
    view: &mut StudioView,
    apple: &mut AppleStore,
    size: [f32; 2],
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size.into())),
            events,
            ..Default::default()
        },
        |ui| {
            view.ui(ui, apple);
            assert!(
                ui.min_rect().right() <= size[0] + 1.0,
                "Horizontal overflow at {size:?}"
            );
        },
    )
}

fn text_position(output: &egui::FullOutput, expected: &str) -> Option<Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        Shape::Text(text) if text.galley.text().contains(expected) => {
            Some(text.pos + text.galley.size() * 0.5)
        }
        _ => None,
    })
}

fn click(draw: &mut impl FnMut(Vec<egui::Event>) -> egui::FullOutput, pos: Pos2) {
    for pressed in [true, false] {
        draw(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
    }
}

#[test]
fn startup_and_picker_fit_all_supported_sizes_and_themes() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for size in [
            [390.0, 844.0],
            [768.0, 1024.0],
            [1280.0, 800.0],
            [1440.0, 900.0],
        ] {
            let ctx = egui::Context::default();
            crate::style::configure(&ctx);
            ctx.set_theme(theme);
            let mut view = StudioView::default();
            let mut apple = AppleStore::test_apps(vec![app("123", LONG_NAME)]);
            let _ = render(&ctx, &mut view, &mut apple, size, vec![]);
            view.recents = vec![app("123", LONG_NAME)];
            let output = render(&ctx, &mut view, &mut apple, size, vec![]);
            for text in ["Open App…", "Create New…", "Recent Apps", LONG_NAME] {
                assert!(
                    text_position(&output, text).is_some(),
                    "Missing {text} at {size:?}"
                );
            }
            view.open();
            let output = render(&ctx, &mut view, &mut apple, size, vec![]);
            for text in ["Open App", "Cancel", "Search", LONG_NAME] {
                assert!(
                    text_position(&output, text).is_some(),
                    "Missing {text} at {size:?}"
                );
            }
            view.search = "no-match".into();
            let output = render(&ctx, &mut view, &mut apple, size, vec![]);
            assert!(text_position(&output, "No apps match your search.").is_some());
        }
    }
}

#[test]
fn open_uses_picker_and_cancel_returns_to_start() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let mut view = StudioView::default();
    let mut apple = AppleStore::default();
    let mut draw = |events| render(&ctx, &mut view, &mut apple, [390.0, 844.0], events);
    let output = draw(vec![]);
    click(&mut draw, text_position(&output, "Open App…").unwrap());
    let output = draw(vec![]);
    assert!(text_position(&output, "Apple settings…").is_some());
    click(&mut draw, text_position(&output, "Cancel").unwrap());
    let output = draw(vec![]);
    assert!(text_position(&output, "Recent Apps").is_some());
}

#[test]
fn create_new_requests_the_wizard() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let mut view = StudioView::default();
    let mut apple = AppleStore::default();
    let mut commands = Vec::new();
    let mut draw = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(390.0, 844.0))),
                events,
                ..Default::default()
            },
            |ui| {
                if let Some(command) = view.ui(ui, &mut apple) {
                    commands.push(command);
                }
            },
        )
    };
    let output = draw(vec![]);
    click(&mut draw, text_position(&output, "Create New…").unwrap());
    assert!(matches!(commands.as_slice(), [Command::Wizard]));
}

#[test]
fn picker_and_recent_rows_request_windows_and_persist_recents() {
    let directory = tempfile::tempdir().unwrap();
    for recent in [false, true] {
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let mut apple = AppleStore::test_apps(vec![app("123", "Example App")]);
        let mut view = StudioView::default();
        view.set_account(apple.selection_account());
        view.recents_path = Some(directory.path().join("recents.json"));
        if recent {
            view.recents = vec![app("123", "Example App")];
        } else {
            view.open();
        }
        // Render only selection controls, so this test never requests live app details.
        {
            let mut draw = |events| {
                ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(768.0, 1024.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if view.requested_app.is_none() {
                            if recent {
                                view.start_screen(ui);
                            } else {
                                view.picker(ui, &mut apple);
                            }
                        }
                    },
                )
            };
            let _ = draw(vec![]);
            let output = draw(vec![]);
            click(&mut draw, text_position(&output, "Example App").unwrap());
        }
        assert!(!view.picking);
        assert_eq!(
            catalog::load_recents(view.recents_path.as_deref())[0].id,
            "123"
        );
        let selected = view.take_app_request().unwrap();
        assert_eq!(selected.id, "123");
        assert_eq!(selected.title(), "Example App");
        assert!(view.take_app_request().is_none());
        // The chooser remains available for opening the next app.
        let output = render(&ctx, &mut view, &mut apple, [390.0, 844.0], vec![]);
        assert!(text_position(&output, "Recent Apps").is_some());
        view.select(app("456", "Another App"));
        assert_eq!(view.take_app_request().unwrap().id, "456");
        view.select(app("123", "Example App"));
        view.set_account(Some("different-account".into()));
        assert!(view.take_app_request().is_none());
    }
}
