use super::*;
use serde_json::json;
use std::collections::VecDeque;

const ROUTE: &str = "/apps/123/versions/version-draft/localizations/en-US";

struct Mock(VecDeque<(Method, Option<Value>, u16, Value)>);

impl Transport for Mock {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let (expected, expected_body, status, value) =
            self.0.pop_front().expect("unexpected request");
        assert_eq!(method, expected);
        assert_eq!(path, ROUTE);
        assert_eq!(body, expected_body.as_ref());
        Ok((status, value))
    }
}

fn workspace() -> Workspace {
    let mut workspace = Workspace::default();
    workspace.open(Document::Listing);
    workspace.locale = "en-US".into();
    let details = crate::ios_view::tests::fixture();
    workspace.edit(
        ROUTE,
        "description",
        details.version_localizations[0].get("description"),
        "Updated description".into(),
        "Description",
        "iOS 2.0",
    );
    workspace.edit(
        ROUTE,
        "marketingUrl",
        details.version_localizations[0].get("marketingUrl"),
        String::new(),
        "Marketing URL",
        "iOS 2.0",
    );
    workspace
}

#[test]
fn saves_one_localization_with_only_changed_fields_including_clears() {
    let workspace = workspace();
    let current = crate::ios_view::tests::fixture().version_localizations[0].clone();
    let mut updated = current.clone();
    updated["description"] = json!("Updated description");
    updated["marketingUrl"] = json!("");
    let mut api = Mock(VecDeque::from([
        (Method::GET, None, 200, current),
        (
            Method::PATCH,
            Some(json!({"description":"Updated description", "marketingUrl":""})),
            200,
            updated.clone(),
        ),
    ]));
    let results = save_edits(&workspace.edits, |route, edits| {
        save_resource(&mut api, route, edits)
    });
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, ROUTE);
    assert_eq!(results[0].1.as_ref().unwrap(), &updated);
    assert!(api.0.is_empty());
}

#[test]
fn external_changes_stop_writes_and_already_saved_values_need_no_patch() {
    for already_saved in [false, true] {
        let mut workspace = workspace();
        workspace
            .edits
            .retain(|(_, field), _| field == "description");
        let mut current = crate::ios_view::tests::fixture().version_localizations[0].clone();
        current["description"] = json!(if already_saved {
            "Updated description"
        } else {
            "Someone else's edit"
        });
        let mut api = Mock(VecDeque::from([(Method::GET, None, 200, current)]));
        let result = save_edits(&workspace.edits, |route, edits| {
            save_resource(&mut api, route, edits)
        });
        assert_eq!(result[0].1.is_ok(), already_saved);
        assert!(api.0.is_empty());
    }
}

#[test]
fn rejected_or_unconfirmed_writes_preserve_the_result_as_an_error() {
    for status in [403, 409, 422, 503, 200] {
        let workspace = workspace();
        let current = crate::ios_view::tests::fixture().version_localizations[0].clone();
        let mut api = Mock(VecDeque::from([
            (Method::GET, None, 200, current),
            (
                Method::PATCH,
                Some(json!({"description":"Updated description", "marketingUrl":""})),
                status,
                json!({"id":"wrong-id"}),
            ),
        ]));
        let results = save_edits(&workspace.edits, |route, edits| {
            save_resource(&mut api, route, edits)
        });
        assert!(results[0].1.is_err());
        assert!(api.0.is_empty());
    }
}

#[test]
fn partial_save_clears_only_confirmed_edits_and_updates_the_correct_app() {
    let mut workspace = workspace();
    workspace.locale = "fr-FR".into();
    let french_route = ROUTE.replace("en-US", "fr-FR");
    let details = crate::ios_view::tests::fixture();
    workspace.edit(
        &french_route,
        "description",
        details.version_localizations[1].get("description"),
        "French local edit".into(),
        "Description",
        "iOS 2.0",
    );
    let mut cache = HashMap::from([
        (
            "123".into(),
            Entry {
                data: Some(crate::ios_view::tests::fixture()),
                error: None,
            },
        ),
        (
            "456".into(),
            Entry {
                data: Some(crate::ios_view::tests::fixture()),
                error: None,
            },
        ),
    ]);
    let (sender, result) = mpsc::channel();
    let mut metadata = Metadata::default();
    metadata.save = Some(Save {
        app_id: "123".into(),
        edits: workspace.edits.clone(),
        result,
    });
    let mut workspaces = HashMap::from([("123".into(), workspace)]);
    let mut updated = details.version_localizations[0].clone();
    updated["description"] = json!("Updated description");
    updated["marketingUrl"] = json!("");
    sender
        .send(vec![
            (ROUTE.into(), Ok(updated)),
            (french_route.clone(), Err("Access denied.".into())),
        ])
        .unwrap();
    metadata.poll(&mut cache, &mut workspaces);
    assert!(metadata.save.is_none());
    let edits = &workspaces["123"].edits;
    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[&(french_route, "description".into())].value,
        "French local edit"
    );
    assert_eq!(
        cache["123"].data.as_ref().unwrap().version_localizations[0]["description"],
        "Updated description"
    );
    assert_eq!(
        cache["456"].data.as_ref().unwrap().version_localizations,
        details.version_localizations
    );
    assert!(metadata.feedback["123"].is_err());
}

#[test]
fn disconnected_save_retains_edits_for_retry() {
    let workspace = workspace();
    let (sender, result) = mpsc::channel();
    drop(sender);
    let mut metadata = Metadata::default();
    metadata.save = Some(Save {
        app_id: "123".into(),
        edits: workspace.edits.clone(),
        result,
    });
    let mut workspaces = HashMap::from([("123".into(), workspace)]);
    metadata.poll(&mut HashMap::new(), &mut workspaces);
    assert_eq!(workspaces["123"].edits.len(), 2);
    assert!(
        metadata.feedback["123"]
            .as_ref()
            .unwrap_err()
            .contains(UNCONFIRMED)
    );
}

#[test]
fn save_control_and_feedback_fit_small_and_large_windows() {
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
            let mut metadata = Metadata::default();
            metadata.feedback.insert("123".into(), Err("Apple rejected these changes. Check text lengths, URLs, and the selected version’s requirements.".into()));
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        metadata.controls(
                            ui,
                            &workspace(),
                            "123",
                            false,
                            Palette::new(ui.visuals().dark_mode),
                        );
                        assert!(ui.min_rect().right() <= width);
                    });
                },
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Save")));
        }
    }
}
