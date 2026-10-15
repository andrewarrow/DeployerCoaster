use super::*;
use serde_json::json;

#[test]
fn clipboard_contains_only_age_rating_fields_and_preserves_false_and_optional_clears() {
    let mut details = super::super::tests::fixture();
    details.age_rating.as_mut().unwrap()["kidsAgeBand"] = Value::Null;
    details.age_rating.as_mut().unwrap()["appStoreAgeRating"] = json!("FOUR_PLUS");
    details.age_rating.as_mut().unwrap()["unknownFutureField"] = json!(true);
    let copied = snapshot(&details).unwrap();
    assert_eq!(copied["advertising"], false);
    assert_eq!(copied["kidsAgeBand"], Value::Null);
    assert!(copied.get("horrorOrFearThemes").is_none());
    for field in [
        "id",
        "appId",
        "appInfoId",
        "appStoreAgeRating",
        "unknownFutureField",
        "copyright",
        "name",
    ] {
        assert!(copied.get(field).is_none(), "copied {field}");
    }
    details.age_rating = Some(json!({"id":"only-id", "advertising":null}));
    assert!(snapshot(&details).is_none());
}

struct Fake {
    calls: Vec<(Method, String, Value)>,
    response: (u16, Value),
}
impl Transport for Fake {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        self.calls
            .push((method, path.into(), body.unwrap().clone()));
        Ok(self.response.clone())
    }
}

#[test]
fn paste_posts_only_copied_attributes_to_destination_and_validates_response() {
    let values = snapshot(&super::super::tests::fixture()).unwrap();
    for (status, response, success) in [
        (
            200,
            json!({"id":"target-rating", "appId":"456", "appInfoId":"target-info", "advertising":false}),
            true,
        ),
        (
            200,
            json!({"id":"source-rating", "appId":"123", "appInfoId":"source-info"}),
            false,
        ),
        (
            200,
            json!({"id":"target-rating", "appId":"456", "appInfoId":"other-info"}),
            false,
        ),
        (200, json!({"advertising":false}), false),
        (
            403,
            json!({"error":{"code":"upstream_forbidden", "message":"secret"}}),
            false,
        ),
        (503, json!({"error":{"code":"upstream_unavailable"}}), false),
    ] {
        let mut fake = Fake {
            calls: vec![],
            response: (status, response),
        };
        let result = save(&mut fake, "456", "target-info", "target-rating", &values);
        assert_eq!(result.is_ok(), success);
        assert_eq!(
            fake.calls,
            vec![(
                Method::POST,
                "/apps/456/app-infos/target-info/age-rating".into(),
                values.clone()
            )]
        );
        if status == 403 {
            let message = result.unwrap_err();
            assert!(message.contains("App Store Connect permissions"));
            assert!(!message.contains("secret"));
        }
    }
}

#[test]
fn paste_completion_updates_only_target_age_rating_even_after_switching_apps() {
    let mut view = IosView::default();
    for id in ["123", "456"] {
        view.cache.insert(
            id.into(),
            Entry {
                data: Some(super::super::tests::fixture()),
                error: None,
            },
        );
    }
    view.select("123");
    let (sender, result) = mpsc::channel();
    view.age_copy.paste = Some(Paste {
        app_id: "456".into(),
        info_id: "info-draft".into(),
        result,
    });
    view.age_copy.poll(&mut view.cache);
    assert!(view.age_copy.pasting("456"));
    let rating =
        json!({"id":"target-rating", "appId":"456", "appInfoId":"info-draft", "advertising":true});
    sender.send(Ok(rating.clone())).unwrap();
    view.age_copy.poll(&mut view.cache);
    assert!(!view.age_copy.pasting("456"));
    assert_eq!(view.selected.as_deref(), Some("123"));
    let target = view.cache["456"].data.as_ref().unwrap();
    let mut expected = super::super::tests::fixture().age_rating.unwrap();
    expected
        .as_object_mut()
        .unwrap()
        .extend(rating.as_object().unwrap().clone());
    assert_eq!(target.age_rating.as_ref().unwrap(), &expected);
    assert_eq!(target.app, super::super::tests::fixture().app);
    assert_eq!(target.version, super::super::tests::fixture().version);
    assert_eq!(
        target.version_localizations,
        super::super::tests::fixture().version_localizations
    );
    assert_eq!(
        view.cache["123"].data.as_ref().unwrap().age_rating,
        super::super::tests::fixture().age_rating
    );
}

#[test]
fn failed_paste_keeps_destination_and_clipboard_for_retry() {
    let mut cache = HashMap::from([(
        "456".into(),
        Entry {
            data: Some(super::super::tests::fixture()),
            error: None,
        },
    )]);
    let mut copy = AgeCopy {
        clipboard: std::rc::Rc::new(std::cell::RefCell::new(snapshot(
            &super::super::tests::fixture(),
        ))),
        ..Default::default()
    };
    let before = copy.clipboard.borrow().clone();
    let (sender, result) = mpsc::channel();
    copy.paste = Some(Paste {
        app_id: "456".into(),
        info_id: "info-draft".into(),
        result,
    });
    sender.send(Err("Access denied.".into())).unwrap();
    copy.poll(&mut cache);
    assert_eq!(
        cache["456"].data.as_ref().unwrap().age_rating,
        super::super::tests::fixture().age_rating
    );
    assert_eq!(*copy.clipboard.borrow(), before);
    assert!(copy.feedback["456"].is_err());
}
