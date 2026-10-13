use super::*;
use std::collections::VecDeque;

fn resource(kind: &str, id: &str, attributes: Value) -> Value {
    json!({"type": kind, "id": id, "attributes": attributes})
}

fn snapshot(prefix: &str) -> Snapshot {
    Snapshot {
        app: resource(
            "apps",
            &format!("{prefix}-app"),
            json!({"name": format!("{prefix} name"), "bundleId": format!("com.{prefix}"), "contentRightsDeclaration": "DOES_NOT_USE_THIRD_PARTY_CONTENT"}),
        ),
        info: json!({"id": format!("{prefix}-info"), "attributes": {"state": "PREPARE_FOR_SUBMISSION"}, "relationships": {"primaryCategory": {"data": {"id": "UTILITIES"}}}}),
        info_localizations: vec![resource(
            "appInfoLocalizations",
            &format!("{prefix}-info-en"),
            json!({"locale": "en-US", "privacyPolicyUrl": "https://example.com/privacy", "name": "Never copy this name", "subtitle": "Never copy this subtitle"}),
        )],
        age: Some(resource(
            "ageRatingDeclarations",
            &format!("{prefix}-age"),
            json!({"advertising": false, "gambling": false, "violenceRealistic": "NONE", "appStoreAgeRating": "FOUR_PLUS", "ageRatingOverride": "NONE"}),
        )),
        versions: vec![Version {
            resource: resource(
                "appStoreVersions",
                &format!("{prefix}-version"),
                json!({"platform": "IOS", "appVersionState": "PREPARE_FOR_SUBMISSION", "versionString": "9.9", "copyright": "Example"}),
            ),
            localizations: vec![resource(
                "appStoreVersionLocalizations",
                &format!("{prefix}-en"),
                json!({"locale": "en-US", "description": "Source description", "keywords": "sample,test", "supportUrl": "https://example.com/help", "marketingUrl": null, "whatsNew": "Never copy release notes"}),
            )],
            review: Some(resource(
                "appStoreReviewDetails",
                &format!("{prefix}-review"),
                json!({"contactFirstName": "Test", "contactLastName": "Person", "contactEmail": "test@example.com", "contactPhone": "12345", "demoAccountPassword": "Never copy password"}),
            )),
        }],
    }
}

#[derive(Default)]
struct Fake {
    expected: VecDeque<(Method, String, u16, Value)>,
    requests: Vec<(Method, String, Option<Value>)>,
}

impl Fake {
    fn response(&mut self, path: &str, body: Value) {
        self.expected
            .push_back((Method::GET, path.into(), 200, body));
    }
    fn load_fixture(&mut self, prefix: &str) {
        let s = snapshot(prefix);
        let version = &s.versions[0];
        self.response(&format!("/v1/apps/{prefix}-app"), json!({"data": s.app}));
        self.response(
            &format!("/v1/apps/{prefix}-app/appInfos?include=primaryCategory&limit=200"),
            json!({"data": [s.info]}),
        );
        self.response(
            &format!("/v1/apps/{prefix}-app/appStoreVersions?limit=200"),
            json!({"data": [version.resource]}),
        );
        self.response(
            &format!("/v1/appInfos/{prefix}-info/appInfoLocalizations?limit=200"),
            json!({"data": s.info_localizations}),
        );
        self.response(
            &format!("/v1/appInfos/{prefix}-info/ageRatingDeclaration"),
            json!({"data": s.age}),
        );
        self.response(
            &format!(
                "/v1/appStoreVersions/{prefix}-version/appStoreVersionLocalizations?limit=200"
            ),
            json!({"data": version.localizations}),
        );
        self.response(
            &format!("/v1/appStoreVersions/{prefix}-version/appStoreReviewDetail"),
            json!({"data": version.review}),
        );
    }
}

impl Transport for Fake {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        self.requests
            .push((method.clone(), path.into(), body.cloned()));
        let (expected_method, expected_path, status, response) =
            self.expected.pop_front().expect("Unexpected API call");
        assert_eq!(method, expected_method);
        let url = checked_url(path).unwrap();
        let normalized = format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        assert_eq!(normalized, expected_path);
        Ok((status, response))
    }
}

#[test]
fn copy_then_paste_loads_all_metadata_before_destination_only_writes() {
    let mut fake = Fake::default();
    fake.load_fixture("source");
    fake.load_fixture("target");
    for path in [
        "apps/target-app",
        "appInfos/target-info",
        "appInfoLocalizations/target-info-en",
        "ageRatingDeclarations/target-age",
        "appStoreVersions/target-version",
        "appStoreReviewDetails/target-review",
        "appStoreVersionLocalizations/target-en",
    ] {
        fake.expected
            .push_back((Method::PATCH, format!("/v1/{path}"), 200, json!({})));
    }
    let cancelled = AtomicBool::new(false);
    let mut events = Vec::new();
    let mut emit = |e| events.push(e);
    let mut session = Session::new(fake, &cancelled, &mut emit);
    let source = session.load("source-app", false).unwrap();
    let target = session.load("target-app", true).unwrap();
    session.apply(plan(&source, &target).unwrap()).unwrap();
    assert!(session.transport.expected.is_empty());
    assert!(
        session.transport.requests[..14]
            .iter()
            .all(|r| r.0 == Method::GET)
    );
    for (_, _, body) in &session.transport.requests[14..] {
        let data = &body.as_ref().unwrap()["data"];
        assert!(data["id"].as_str().unwrap().starts_with("target-"));
        for field in [
            "name",
            "bundleId",
            "sku",
            "versionString",
            "demoAccountPassword",
            "appStoreAgeRating",
            "ageRatingOverride",
            "whatsNew",
            "locale",
        ] {
            assert!(
                data["attributes"].get(field).is_none(),
                "Unexpected {field}"
            );
        }
    }
    drop(session);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Status(_, StepState::Success(_))))
            .count(),
        21
    );
}

#[test]
fn localizations_match_locale_and_create_missing_resources_with_destination_parents() {
    let mut source = snapshot("source");
    source.info_localizations.push(resource(
        "appInfoLocalizations",
        "source-info-fr",
        json!({"locale": "fr-FR", "privacyPolicyUrl": "https://example.com/fr"}),
    ));
    source.versions[0].localizations.push(resource(
        "appStoreVersionLocalizations",
        "source-fr",
        json!({"locale": "fr-FR", "description": "Français"}),
    ));
    let mut target = snapshot("target");
    target.versions[0].review = None;
    target.info_localizations.insert(
        0,
        resource(
            "appInfoLocalizations",
            "target-de",
            json!({"locale": "de-DE"}),
        ),
    );
    let operations: Vec<_> = plan(&source, &target)
        .unwrap()
        .into_iter()
        .filter_map(|p| {
            if let Planned::Write(op) = p {
                Some(op)
            } else {
                None
            }
        })
        .collect();
    let privacy = operations
        .iter()
        .find(|op| op.label == "Copy privacy URL (en-US)")
        .unwrap();
    assert_eq!(privacy.path, "/v1/appInfoLocalizations/target-info-en");
    let created: Vec<_> = operations
        .iter()
        .filter(|op| op.method == Method::POST)
        .collect();
    assert_eq!(created.len(), 3);
    for op in created {
        let data = &op.body["data"];
        assert!(data.get("id").is_none());
        if data["type"] == "appInfoLocalizations" {
            assert_eq!(data["attributes"]["name"], "target name");
            assert_eq!(data["attributes"]["locale"], "fr-FR");
            assert_eq!(
                data["relationships"]["appInfo"]["data"]["id"],
                "target-info"
            );
        } else {
            assert_eq!(
                data["relationships"]["appStoreVersion"]["data"]["id"],
                "target-version"
            );
        }
    }
}

#[test]
fn preflight_rejects_self_paste_noneditable_and_mismatched_platforms() {
    let source = snapshot("source");
    assert!(plan(&source, &source).is_err());
    let mut target = snapshot("target");
    target.versions[0].resource["attributes"]["platform"] = json!("MAC_OS");
    assert!(plan(&source, &target).is_err());
    assert!(
        select_info(
            &[json!({"attributes": {"state": "READY_FOR_DISTRIBUTION"}})],
            true
        )
        .is_err()
    );
    assert!(
        select_versions(
            &[json!({"attributes": {"appVersionState": "IN_REVIEW", "platform": "IOS"}})],
            true
        )
        .is_err()
    );
    let mut fake = Fake::default();
    fake.response("/v1/apps/target-app", json!({"data": target.app}));
    fake.response(
        "/v1/apps/target-app/appInfos?include=primaryCategory&limit=200",
        json!({"data": [{"id": "live-info", "attributes": {"state": "READY_FOR_DISTRIBUTION"}}]}),
    );
    let cancelled = AtomicBool::new(false);
    let mut emit = |_| {};
    let mut session = Session::new(fake, &cancelled, &mut emit);
    assert!(session.load("target-app", true).is_err());
    assert!(
        session
            .transport
            .requests
            .iter()
            .all(|r| r.0 == Method::GET)
    );
}

#[test]
fn version_selection_prefers_editable_then_live_and_uses_matching_platforms() {
    let versions = vec![
        json!({"id":"old", "attributes":{"platform":"IOS", "appVersionState":"REPLACED_WITH_NEW_VERSION", "createdDate":"2024"}}),
        json!({"id":"live", "attributes":{"platform":"IOS", "appVersionState":"READY_FOR_DISTRIBUTION", "createdDate":"2025"}}),
        json!({"id":"draft", "attributes":{"platform":"IOS", "appVersionState":"PREPARE_FOR_SUBMISSION", "createdDate":"2026"}}),
        json!({"id":"mac", "attributes":{"platform":"MAC_OS", "appStoreState":"READY_FOR_SALE"}}),
    ];
    let source = select_versions(&versions, false).unwrap();
    assert_eq!(
        source.iter().map(|v| id(v).unwrap()).collect::<Vec<_>>(),
        ["draft", "mac"]
    );
    assert_eq!(select_versions(&versions, true).unwrap().len(), 1);
}

#[test]
fn failed_write_stops_remaining_calls_and_reports_partial_progress() {
    let source = snapshot("source");
    let target = snapshot("target");
    let mut fake = Fake::default();
    fake.expected
        .push_back((Method::PATCH, "/v1/apps/target-app".into(), 200, json!({})));
    fake.expected.push_back((
        Method::PATCH,
        "/v1/appInfos/target-info".into(),
        403,
        json!({"errors":[{"detail":"Not permitted"}]}),
    ));
    let cancelled = AtomicBool::new(false);
    let mut events = Vec::new();
    let mut emit = |e| events.push(e);
    let mut session = Session::new(fake, &cancelled, &mut emit);
    let error = session.apply(plan(&source, &target).unwrap()).unwrap_err();
    assert!(error.contains("Completed changes have been kept"));
    assert_eq!(session.transport.requests.len(), 2);
    drop(session);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Status(_, StepState::Failed(_))))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Status(_, StepState::Skipped)))
            .count(),
        5
    );
}

#[test]
fn cancellation_between_calls_keeps_completed_changes_and_skips_the_rest() {
    let cancelled = AtomicBool::new(false);
    let mut fake = Fake::default();
    fake.expected
        .push_back((Method::PATCH, "/v1/apps/target-app".into(), 200, json!({})));
    let mut events = Vec::new();
    let mut emit = |e| {
        if matches!(e, Event::Status(_, StepState::Success(_))) {
            cancelled.store(true, Ordering::Relaxed);
        }
        events.push(e);
    };
    let mut session = Session::new(fake, &cancelled, &mut emit);
    assert!(
        session
            .apply(plan(&snapshot("source"), &snapshot("target")).unwrap())
            .is_err()
    );
    assert_eq!(session.transport.requests.len(), 1);
    drop(session);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Status(_, StepState::Skipped)))
            .count(),
        6
    );
}

#[test]
fn pagination_loads_all_locales_and_rejects_foreign_or_repeated_links() {
    let cancelled = AtomicBool::new(false);
    for next in [
        "https://evil.example/v1/apps",
        "https://api.appstoreconnect.apple.com/v1/apps?limit=200",
        "https://api.appstoreconnect.apple.com/v1/users",
    ] {
        let mut fake = Fake::default();
        fake.response(
            "/v1/apps?limit=200",
            json!({"data": [], "links": {"next": next}}),
        );
        let mut emit = |_| {};
        let mut session = Session::new(fake, &cancelled, &mut emit);
        assert!(session.list("Apps", "/v1/apps?limit=200").is_err());
        assert_eq!(session.transport.requests.len(), 1);
    }
    let mut fake = Fake::default();
    fake.response("/v1/appInfos/info/appInfoLocalizations?limit=200", json!({"data": [{"id":"en"}], "links":{"next":"/v1/appInfos/info/appInfoLocalizations?cursor=next"}}));
    fake.response(
        "/v1/appInfos/info/appInfoLocalizations?cursor=next",
        json!({"data": [{"id":"fr"}]}),
    );
    let mut emit = |_| {};
    let mut session = Session::new(fake, &cancelled, &mut emit);
    assert_eq!(
        session
            .list(
                "Locales",
                "/v1/appInfos/info/appInfoLocalizations?limit=200"
            )
            .unwrap()
            .len(),
        2
    );
    assert!(checked_url("https://user@api.appstoreconnect.apple.com/v1/apps").is_err());
}

#[test]
fn absent_values_do_not_clear_destination_settings_and_false_is_copied() {
    let mut source = snapshot("source");
    source.app["attributes"]["contentRightsDeclaration"] = Value::Null;
    source.versions[0].localizations[0]["attributes"]["description"] = json!("");
    let operations = plan(&source, &snapshot("target")).unwrap();
    assert!(matches!(operations[0], Planned::Skip(_)));
    let age = operations
        .iter()
        .find_map(|p| match p {
            Planned::Write(op) if op.label == "Copy age rating declaration" => Some(op),
            _ => None,
        })
        .unwrap();
    assert_eq!(age.body["data"]["attributes"]["advertising"], false);
    let listing = operations
        .iter()
        .find_map(|p| match p {
            Planned::Write(op) if op.label == "Copy IOS listing (en-US)" => Some(op),
            _ => None,
        })
        .unwrap();
    assert!(
        listing.body["data"]["attributes"]
            .get("description")
            .is_none()
    );
}
