use super::*;
use std::collections::VecDeque;

fn version(number: &str, date: &str) -> Value {
    json!({"id": "new-draft", "appId": "123", "platform": "IOS", "versionString": number,
        "createdDate": date, "appVersionState": "PREPARE_FOR_SUBMISSION"})
}

#[test]
fn increments_only_the_last_number_of_the_most_recent_ios_version() {
    for (current, expected) in [
        ("1.0.1", "1.0.2"),
        ("1.2.7", "1.2.8"),
        ("1.2.9", "1.2.10"),
        ("2.0", "2.1"),
        ("9", "10"),
    ] {
        assert_eq!(next_version(&[version(current, "")]).unwrap(), expected);
    }
    let mut mac = version("99.0", "2026-12-01");
    mac["platform"] = json!("MAC_OS");
    assert_eq!(
        next_version(&[
            version("1.2.7", "2026-10-15"),
            version("8.0", "2026-01-01"),
            mac
        ])
        .unwrap(),
        "1.2.8"
    );
    // Missing/tied dates use numeric ordering, never lexicographic or selected order.
    assert_eq!(
        next_version(&[version("1.2.9", ""), version("1.2.10", "")]).unwrap(),
        "1.2.11"
    );
    for invalid in ["", "1.", "1..2", "1.2-beta", "1.2.18446744073709551615"] {
        assert!(next_version(&[version(invalid, "")]).is_err());
    }
    assert!(next_version(&[]).is_err());
}

struct Mock {
    steps: VecDeque<(Method, String, Option<Value>, u16, Value)>,
}

impl Transport for Mock {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let (expected_method, expected_path, expected_body, status, value) =
            self.steps.pop_front().expect("unexpected request");
        assert_eq!(method, expected_method);
        assert_eq!(path, expected_path);
        assert_eq!(body, expected_body.as_ref());
        Ok((status, value))
    }
}

#[test]
fn reads_every_page_then_posts_once_and_preserves_failures() {
    for status in [201, 409, 503] {
        let returned = if status == 201 {
            version("1.2.8", "2026-10-16")
        } else {
            json!({"error": {"code": "write_outcome_unknown"}})
        };
        let mut transport = Mock {
            steps: VecDeque::from([
                (
                    Method::GET,
                    "/apps/123/versions?platform=IOS&limit=200".into(),
                    None,
                    200,
                    json!({"items": [version("1.2.6", "2026-10-01")], "nextCursor": "next-page"}),
                ),
                (
                    Method::GET,
                    "/apps/123/versions?platform=IOS&limit=200&cursor=next-page".into(),
                    None,
                    200,
                    json!({"items": [version("1.2.7", "2026-10-15")], "nextCursor": null}),
                ),
                (
                    Method::POST,
                    "/apps/123/versions".into(),
                    Some(json!({"platform": "IOS", "versionString": "1.2.8"})),
                    status,
                    returned,
                ),
            ]),
        };
        let result = create(&mut transport, "123");
        assert_eq!(result.is_ok(), status == 201);
        assert!(transport.steps.is_empty());
    }
}

#[test]
fn confirmed_creation_selects_draft_even_when_refresh_fails_and_app_changes() {
    for refresh_ok in [true, false] {
        let mut view = IosView::default();
        view.select("456");
        view.cache.insert(
            "123".into(),
            Entry {
                data: Some(crate::ios_view::tests::fixture()),
                error: None,
            },
        );
        let (sender, result) = mpsc::channel();
        view.creation = Some(Creation {
            app_id: "123".into(),
            result,
        });
        let new = version("2.1", "2026-10-16");
        let mut refreshed = crate::ios_view::tests::fixture();
        refreshed.version = Some(new.clone());
        refreshed.versions.insert(0, new.clone());
        refreshed.version_localizations.clear();
        refreshed.review_contact = None;
        sender
            .send(Ok(Created {
                version: new,
                details: if refresh_ok {
                    Ok(refreshed)
                } else {
                    Err("Offline".into())
                },
            }))
            .ok()
            .unwrap();
        view.poll();
        let details = view.cache["123"].data.as_ref().unwrap();
        assert_eq!(
            attr(details.version.as_ref().unwrap(), "versionString"),
            "2.1"
        );
        assert!(details.version_localizations.is_empty());
        assert!(details.review_contact.is_none());
        assert!(view.creation.is_none());
        assert_eq!(view.selected.as_deref(), Some("456"));
        assert!(!view.cache.contains_key("456"));
    }
}

#[test]
fn failed_creation_preserves_selected_version_and_localizations() {
    let mut view = IosView::default();
    view.select("123");
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(crate::ios_view::tests::fixture()),
            error: None,
        },
    );
    let (sender, result) = mpsc::channel();
    view.creation = Some(Creation {
        app_id: "123".into(),
        result,
    });
    sender.send(Err("Creation failed".into())).ok().unwrap();
    view.poll();
    let entry = &view.cache["123"];
    assert_eq!(
        attr(entry.data.as_ref().unwrap().version.as_ref().unwrap(), "id"),
        "version-draft"
    );
    assert_eq!(entry.data.as_ref().unwrap().version_localizations.len(), 2);
    assert_eq!(entry.error.as_deref(), Some("Creation failed"));
}
