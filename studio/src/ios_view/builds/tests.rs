use super::*;
use std::collections::VecDeque;

type Step = (Method, String, Option<Value>, u16, Value);
struct Mock(VecDeque<Step>);
impl Transport for Mock {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let (expected_method, expected_path, expected_body, status, value) =
            self.0.pop_front().expect("unexpected request");
        assert_eq!(method, expected_method);
        assert_eq!(path, expected_path);
        assert_eq!(body, expected_body.as_ref());
        Ok((status, value))
    }
}

fn build() -> Value {
    json!({"id":"build1", "appId":"123", "version":"42", "processingState":"VALID", "expired":false,
        "uploadedDate":"2026-10-16T01:02:03Z", "expirationDate":"2027-01-14T01:02:03Z"})
}

fn scope() -> Scope {
    Scope {
        app_id: "123".into(),
        version_id: "version-draft".into(),
    }
}

#[test]
fn lists_every_page_for_the_exact_selected_version_and_reads_its_attached_build() {
    let path = "/apps/123/builds?platform=IOS&sort=-uploadedDate&limit=200&prereleaseVersion=2.0";
    let mut api = Mock(VecDeque::from([
        (
            Method::GET,
            path.into(),
            None,
            200,
            json!({"items":[build()],"nextCursor":"next"}),
        ),
        (
            Method::GET,
            format!("{path}&cursor=next"),
            None,
            200,
            json!({"items":[],"nextCursor":null}),
        ),
        (
            Method::GET,
            "/apps/123/versions/version-draft/build".into(),
            None,
            200,
            build(),
        ),
    ]));
    let details = crate::ios_view::tests::fixture();
    let loaded = load(
        &mut api,
        "123",
        details.version.as_ref(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(loaded.items.len(), 1);
    assert_eq!(loaded.attached.as_deref(), Some("build1"));
    assert!(api.0.is_empty());
}

#[test]
fn empty_list_and_unattached_version_are_distinct_from_read_failures() {
    for status in [404, 403, 503] {
        let mut api = Mock(VecDeque::from([
            (
                Method::GET,
                "/apps/123/builds?platform=IOS&sort=-uploadedDate&limit=200&prereleaseVersion=2.0"
                    .into(),
                None,
                200,
                json!({"items":[],"nextCursor":null}),
            ),
            (
                Method::GET,
                "/apps/123/versions/version-draft/build".into(),
                None,
                status,
                json!({}),
            ),
        ]));
        let details = crate::ios_view::tests::fixture();
        let result = load(
            &mut api,
            "123",
            details.version.as_ref(),
            &AtomicBool::new(false),
        );
        assert_eq!(result.is_ok(), status == 404);
        if let Ok(loaded) = result {
            assert!(loaded.items.is_empty());
            assert!(loaded.attached.is_none());
        }
    }
    let mut api = Mock(VecDeque::new());
    assert!(load(&mut api, "123", None, &AtomicBool::new(true)).is_err());
    assert!(api.0.is_empty());
}

#[test]
fn attaches_only_the_requested_version_and_requires_confirmation() {
    for status in [200, 409, 503] {
        for confirmed in [true, false] {
            let mut api = Mock(VecDeque::from([(
                Method::PATCH,
                "/apps/123/versions/version-draft/build".into(),
                Some(json!({"buildId":"build1"})),
                status,
                if confirmed {
                    json!({"id":"version-draft","appId":"123","buildId":"build1"})
                } else {
                    json!({"error":{"code":"write_outcome_unknown"}})
                },
            )]));
            assert_eq!(
                attach(&mut api, "123", "version-draft", "build1").is_ok(),
                status == 200 && confirmed
            );
            assert!(api.0.is_empty());
        }
    }
}

#[test]
fn completion_updates_the_original_app_and_version_after_selection_changes() {
    let mut builds = Builds::default();
    let mut cache = HashMap::from([(
        "123".into(),
        Entry {
            data: Some(crate::ios_view::tests::fixture()),
            error: None,
        },
    )]);
    // Another version is now selected while an earlier write finishes.
    let details = cache.get_mut("123").unwrap().data.as_mut().unwrap();
    details.version = Some(details.versions[1].clone());
    let (sender, result) = mpsc::channel();
    builds.attachment = Some(Attachment {
        scope: scope(),
        result,
    });
    builds.scope = Some(Scope {
        app_id: "456".into(),
        version_id: "other".into(),
    });
    builds.data = Some(Available {
        items: vec![],
        attached: None,
    });
    sender.send(Ok("build1".into())).unwrap();
    builds.poll(&mut cache);
    let details = cache["123"].data.as_ref().unwrap();
    assert_eq!(details.versions[0]["buildId"], "build1");
    assert!(details.version.as_ref().unwrap().get("buildId").is_none());
    assert!(builds.data.as_ref().unwrap().attached.is_none());
    assert!(builds.attachment.is_none());
}

#[test]
fn failed_write_requires_refresh_and_scope_changes_cancel_only_reads() {
    let mut builds = Builds {
        scope: Some(scope()),
        data: Some(Available {
            items: vec![build()],
            attached: None,
        }),
        ..Default::default()
    };
    let (sender, result) = mpsc::channel();
    builds.attachment = Some(Attachment {
        scope: scope(),
        result,
    });
    sender.send(Err(UNCONFIRMED.into())).unwrap();
    builds.poll(&mut HashMap::new());
    assert!(builds.data.is_none());
    assert_eq!(builds.error.as_deref(), Some(UNCONFIRMED));
    let (_sender, result) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    builds.load = Some(Job {
        result,
        cancelled: cancelled.clone(),
    });
    let (_sender, result) = mpsc::channel();
    builds.attachment = Some(Attachment {
        scope: scope(),
        result,
    });
    builds.clear();
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(builds.attaching("123"));
    assert!(builds.data.is_none());
    assert!(builds.scope.is_none());
}

#[test]
fn build_eligibility_handles_processing_expired_internal_and_missing_values() {
    assert!(unavailable_reason(&build()).is_none());
    for (field, value, expected) in [
        ("expired", json!(true), "Expired"),
        ("expired", Value::Null, "Build availability unknown"),
        ("processingState", json!("PROCESSING"), "Processing"),
        ("processingState", json!("FAILED"), "Processing failed"),
        ("processingState", json!("INVALID"), "Invalid build"),
        (
            "processingState",
            json!("FUTURE_STATE"),
            "Build availability unknown",
        ),
        (
            "buildAudienceType",
            json!("INTERNAL_ONLY"),
            "Internal testing only",
        ),
    ] {
        let mut candidate = build();
        candidate[field] = value;
        assert_eq!(unavailable_reason(&candidate), Some(expected));
    }
}

#[test]
fn builds_render_all_states_without_horizontal_overflow_at_supported_sizes() {
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            for state in ["ready", "attached", "empty", "error", "released", "loading"] {
                let ctx = egui::Context::default();
                crate::style::configure(&ctx);
                ctx.set_theme(theme);
                let mut details = crate::ios_view::tests::fixture();
                if state == "released" {
                    details.version.as_mut().unwrap()["appVersionState"] =
                        json!("READY_FOR_DISTRIBUTION");
                }
                let mut candidate = build();
                candidate["version"] =
                    json!("a-deliberately-long-build-number-with-many-segments-to-check-wrapping");
                let mut builds = Builds {
                    scope: Some(scope()),
                    data: Some(Available {
                        items: if state == "empty" {
                            vec![]
                        } else {
                            vec![candidate]
                        },
                        attached: if state == "attached" {
                            Some("build1".into())
                        } else {
                            None
                        },
                    }),
                    ..Default::default()
                };
                if state == "error" {
                    builds.error = Some("Builds could not be loaded. Please refresh again.".into());
                }
                let (_sender, result) = mpsc::channel();
                if state == "loading" {
                    builds.load = Some(Job {
                        result,
                        cancelled: Arc::new(AtomicBool::new(false)),
                    });
                }
                let mut right = 0.0;
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
                            builds.ui(
                                ui,
                                &details,
                                &AppleCredentials::test_credentials("test"),
                                Palette::new(ui.visuals().dark_mode),
                                false,
                            );
                            right = ui.min_rect().right();
                        });
                    },
                );
                assert!(right <= width + 1.0, "{state} overflow at {width}: {right}");
                let texts: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some(text.galley.text()),
                        _ => None,
                    })
                    .collect();
                let expected = match state {
                    "empty" => "No builds are available",
                    "error" => "Builds could not be loaded",
                    "released" => "Select a Prepare",
                    "attached" => "Attached",
                    "loading" => "Loading builds",
                    _ => "Attach",
                };
                assert!(
                    texts.iter().any(|text| text.contains(expected)),
                    "{state}: {texts:?}"
                );
            }
        }
    }
}
