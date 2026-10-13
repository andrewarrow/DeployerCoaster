use super::*;
use egui::{Pos2, Rect, Shape, pos2, vec2};

const LONG_NAME: &str = "A very long app name that should stay on one clipped sidebar row without wrapping or pushing the layout sideways";

fn app(platform: Platform, name: &str, store_id: &str) -> StoreApp {
    StoreApp {
        identifier: store_id.into(),
        name: name.into(),
        store_id: store_id.into(),
        store: match platform {
            Platform::Ios => crate::app_icons::Store::Apple,
            Platform::Android => crate::app_icons::Store::Play,
        },
    }
}

fn status(loading: bool) -> crate::dashboard::StoreStatus {
    crate::dashboard::StoreStatus {
        connected: true,
        loading,
        error: None,
    }
}

fn render(
    ctx: &egui::Context,
    view: &mut StudioView,
    ios_apps: &[StoreApp],
    ios_loading: bool,
    android_apps: &[StoreApp],
    android_loading: bool,
    width: f32,
    height: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
            events,
            ..Default::default()
        },
        |ui| {
            let sidebar_width = width.min(240.0);
            ui.allocate_ui_with_layout(
                vec2(sidebar_width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        view.platform_tree(
                            ui,
                            Platform::Ios,
                            ios_apps.to_vec(),
                            status(ios_loading),
                        );
                        view.platform_tree(
                            ui,
                            Platform::Android,
                            android_apps.to_vec(),
                            status(android_loading),
                        );
                    });
                },
            );
        },
    )
}

fn text_positions(output: &egui::FullOutput, expected: &str) -> Vec<Pos2> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(text) if text.galley.text() == expected => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .collect()
}

fn click(render: &mut impl FnMut(Vec<egui::Event>) -> egui::FullOutput, pos: Pos2) {
    for pressed in [true, false] {
        let _ = render(vec![
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
fn tree_rows_are_unique_truncated_and_fit_supported_viewports_and_themes() {
    let ios = vec![app(Platform::Ios, LONG_NAME, "ios.long")];
    let android = vec![app(Platform::Android, "Android app", "android.app")];
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
            ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
            let mut view = StudioView::default();
            let output = render(
                &ctx,
                &mut view,
                &ios,
                true,
                &android,
                false,
                width,
                height,
                vec![],
            );
            assert_eq!(text_positions(&output, "ios").len(), 1);
            assert_eq!(text_positions(&output, "android").len(), 1);
            assert_eq!(text_positions(&output, LONG_NAME).len(), 1);
            assert_eq!(text_positions(&output, "Android app").len(), 1);
            for shape in &output.shapes {
                if let Shape::Text(text) = &shape.shape {
                    assert!(text.pos.x + text.galley.size().x <= width.min(240.0) + 1.0);
                }
            }
        }
    }
}

#[test]
fn root_clicks_toggle_their_branch_and_app_clicks_select_platform_and_id() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| style.animation_time = 0.0);
    let mut view = StudioView::default();
    let ios = vec![app(Platform::Ios, "iOS app", "ios.app")];
    let android = vec![app(Platform::Android, "Android app", "android.app")];
    let mut draw = |events| {
        render(
            &ctx, &mut view, &ios, false, &android, false, 390.0, 844.0, events,
        )
    };

    let output = draw(vec![]);
    assert_eq!(text_positions(&output, "ios").len(), 1);
    assert_eq!(text_positions(&output, "android").len(), 1);
    click(&mut draw, text_positions(&output, "ios")[0]);
    let output = draw(vec![]);
    drop(draw);
    assert_eq!(view.platform, Platform::Ios);
    assert!(text_positions(&output, "iOS app").is_empty());
    assert_eq!(text_positions(&output, "Android app").len(), 1);

    let mut draw = |events| {
        render(
            &ctx, &mut view, &ios, false, &android, false, 390.0, 844.0, events,
        )
    };
    let output = draw(vec![]);
    click(&mut draw, text_positions(&output, "ios")[0]);
    let output = draw(vec![]);
    click(&mut draw, text_positions(&output, "Android app")[0]);
    let _ = draw(vec![]);
    drop(draw);
    assert_eq!(view.platform, Platform::Android);
    assert_eq!(
        view.selected_app,
        Some((Platform::Android, "android.app".into()))
    );
}

#[test]
fn chevron_toggles_without_changing_platform_and_collapsed_root_keeps_spinner() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| style.animation_time = 0.0);
    let mut view = StudioView {
        platform: Platform::Android,
        ..Default::default()
    };
    let ios = vec![app(Platform::Ios, "iOS app", "ios.app")];
    let android = vec![app(Platform::Android, "Android app", "android.app")];
    let mut draw = |events| {
        render(
            &ctx, &mut view, &ios, false, &android, false, 390.0, 844.0, events,
        )
    };
    let output = draw(vec![]);
    let ios_text = text_positions(&output, "ios")[0];
    let chevron = pos2(
        ctx.style_of(egui::Theme::Light).spacing.indent / 2.0,
        ios_text.y,
    );
    click(&mut draw, chevron);
    let output = draw(vec![]);
    drop(draw);
    assert_eq!(view.platform, Platform::Android);
    assert!(text_positions(&output, "iOS app").is_empty());

    let output = render(
        &ctx,
        &mut view,
        &[],
        true,
        &android,
        false,
        390.0,
        844.0,
        vec![],
    );
    assert!(text_positions(&output, "iOS app").is_empty());
    assert_eq!(text_positions(&output, "Android app").len(), 1);
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        Shape::Path(path) if path.points.len() >= 8
    )));
}

#[test]
fn app_rows_remain_visible_with_a_refresh_error() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    ctx.set_theme(egui::Theme::Light);
    let mut view = StudioView::default();
    let app = app(Platform::Ios, "Cached iOS app", "ios.cached");
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(390.0, 844.0))),
            ..Default::default()
        },
        |ui| {
            view.platform_tree(
                ui,
                Platform::Ios,
                vec![app.clone()],
                crate::dashboard::StoreStatus {
                    connected: true,
                    loading: false,
                    error: Some("Refresh failed. Cached apps are shown.".into()),
                },
            );
        },
    );
    assert_eq!(text_positions(&output, "Cached iOS app").len(), 1);
    assert_eq!(
        text_positions(&output, "Refresh failed. Cached apps are shown.").len(),
        1
    );
}

#[test]
fn compact_navigation_leaves_room_for_details_and_closes_after_selecting_an_app() {
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
            ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
            let mut view = StudioView::default();
            let mut content_rect = Rect::NOTHING;
            let mut draw = |events| {
                ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        view.navigation_panel(ui, |view, ui| {
                            view.platform_tree(
                                ui,
                                Platform::Ios,
                                vec![app(Platform::Ios, "Example app", "123")],
                                status(false),
                            );
                        });
                        egui::CentralPanel::default().show(ui, |ui| {
                            content_rect = ui.max_rect();
                            ui.label("App details");
                        });
                    },
                )
            };
            let _ = draw(vec![]);
            let output = draw(vec![]);
            assert_eq!(
                text_positions(&output, "Example app").len(),
                1,
                "{theme:?} at {width}"
            );
            click(&mut draw, text_positions(&output, "Example app")[0]);
            let output = draw(vec![]);
            if width < 640.0 {
                assert!(text_positions(&output, "Example app").is_empty());
                assert_eq!(text_positions(&output, "Apps").len(), 1);
                click(&mut draw, text_positions(&output, "Apps")[0]);
                let output = draw(vec![]);
                assert_eq!(text_positions(&output, "Example app").len(), 1);
            }
            drop(draw);
            assert_eq!(view.selected_app, Some((Platform::Ios, "123".into())));
            assert!(content_rect.width() >= if width < 640.0 { 370.0 } else { width - 264.0 });
        }
    }
}
