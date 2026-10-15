use super::*;
use serde_json::json;
use std::collections::VecDeque;

#[derive(Default)]
struct Fake {
    responses: VecDeque<(String, u16, Value)>,
    requests: Vec<String>,
}

impl Fake {
    fn response(&mut self, path: &str, data: Value) {
        self.responses.push_back((
            path.into(),
            200,
            if data.is_array() {
                json!({"items": data, "nextCursor": null})
            } else {
                data
            },
        ));
    }

    fn app(&mut self) {
        self.response("/apps/123", json!({"id": "123", "name": "Example", "bundleId": "com.example.app", "sku": "example", "primaryLocale": "fr-FR"}));
    }
}

impl Transport for &mut Fake {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        assert_eq!(method, Method::GET);
        assert!(body.is_none());
        self.requests.push(path.into());
        let (expected, status, value) = self.responses.pop_front().expect("unexpected API call");
        assert_eq!(path, expected);
        Ok((status, value))
    }
}

fn resource(id: &str, attributes: Value) -> Value {
    {
        let mut resource = attributes;
        resource["id"] = json!(id);
        resource
    }
}

#[test]
fn loads_main_fields_and_all_locales_from_preferred_ios_version() {
    let mut fake = Fake::default();
    fake.app();
    fake.response(
        "/apps/123/app-infos?limit=200",
        json!([
            resource("old-info", json!({"state": "REPLACED_WITH_NEW_INFO"})),
            resource("live-info", json!({"state": "READY_FOR_DISTRIBUTION"})),
            resource("draft-info", json!({"state": "PREPARE_FOR_SUBMISSION"}))
        ]),
    );
    fake.response("/apps/123/app-infos/draft-info/localizations?limit=200", json!([
        resource("info-fr", json!({"locale": "fr-FR", "name": "French name"})),
        resource("info-gb", json!({"locale": "en-GB", "subtitle": "British subtitle"})),
        resource("info-us", json!({"locale": "en-US", "name": "English name", "subtitle": "English subtitle", "privacyPolicyUrl": "https://example.com/privacy"}))
    ]));
    fake.response("/apps/123/versions?platform=IOS&limit=200", json!([
        resource("mac", json!({"platform": "MAC_OS", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-10-01"})),
        resource("live", json!({"platform": "IOS", "appVersionState": "READY_FOR_DISTRIBUTION"})),
        resource("draft-old", json!({"platform": "IOS", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-09-01"})),
        resource("draft-new", json!({"platform": "IOS", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-10-01", "versionString": "2.0"}))
    ]));
    fake.response("/apps/123/versions/draft-new/localizations?limit=200", json!([
        resource("loc-de", json!({"locale": "de-DE"})),
        resource("loc-us", json!({"locale": "en-US", "description": "English description", "keywords": "example,test", "promotionalText": "Try it", "whatsNew": "New features", "supportUrl": "https://example.com/help", "marketingUrl": null}))
    ]));
    let details = load(&mut fake, "123", &AtomicBool::new(false)).unwrap();
    assert_eq!(attr(&details.app, "bundleId"), "com.example.app");
    assert_eq!(attr(&details.app, "sku"), "example");
    assert_eq!(attr(&details.app, "primaryLocale"), "fr-FR");
    assert_eq!(details.info.as_ref().unwrap()["id"], "draft-info");
    assert_eq!(details.version.as_ref().unwrap()["id"], "draft-new");
    assert_eq!(
        attr(details.version.as_ref().unwrap(), "versionString"),
        "2.0"
    );
    assert_eq!(details.info_localizations.len(), 3);
    assert_eq!(attr(&details.info_localizations[0], "locale"), "en-US");
    assert_eq!(attr(&details.info_localizations[1], "locale"), "en-GB");
    assert_eq!(attr(&details.info_localizations[2], "locale"), "fr-FR");
    assert_eq!(
        attr(&details.info_localizations[0], "subtitle"),
        "English subtitle"
    );
    assert_eq!(details.version_localizations.len(), 2);
    assert_eq!(
        attr(&details.version_localizations[0], "description"),
        "English description"
    );
    assert_eq!(
        attr(&details.version_localizations[0], "whatsNew"),
        "New features"
    );
    assert_eq!(attr(&details.version_localizations[1], "locale"), "de-DE");
    assert_eq!(details.versions.len(), 3);
    assert_eq!(details.versions[0]["id"], "draft-new");
    assert_eq!(details.versions[1]["id"], "draft-old");
    assert_eq!(details.versions[2]["id"], "live");
    assert!(details.warnings.is_empty());
    assert!(fake.responses.is_empty());
}

#[test]
fn follows_localization_pages_and_keeps_all_locales_in_locale_order() {
    let mut fake = Fake::default();
    let first = "/apps/123/app-infos/info/localizations?limit=200";
    let next = "/apps/123/app-infos/info/localizations?limit=200&cursor=opaque%2B%2F%3D";
    fake.responses.push_back((
        first.into(),
        200,
        json!({
            "items": [resource("fr", json!({"locale": "fr-FR"}))], "nextCursor": "opaque+/="
        }),
    ));
    fake.response(
        next,
        json!([resource(
            "en",
            json!({"locale": "en-AU", "name": "English"})
        )]),
    );
    let cancelled = AtomicBool::new(false);
    let mut reader = Reader {
        transport: &mut fake,
        cancelled: &cancelled,
    };
    let result = sort_localizations(reader.list(first).unwrap());
    assert_eq!(result.len(), 2);
    assert_eq!(attr(&result[0], "locale"), "en-AU");
    assert_eq!(attr(&result[1], "locale"), "fr-FR");
    assert!(fake.responses.is_empty());
}

#[test]
fn explicit_version_selection_uses_the_requested_ios_version() {
    let mut fake = Fake::default();
    fake.app();
    fake.response("/apps/123/app-infos?limit=200", json!([]));
    fake.response(
        "/apps/123/versions?platform=IOS&limit=200",
        json!([
            resource("preferred", json!({"platform": "IOS", "appVersionState": "PREPARE_FOR_SUBMISSION"})),
            resource("chosen", json!({"platform": "IOS", "appVersionState": "READY_FOR_SALE", "versionString": "1.0"}))
        ]),
    );
    fake.response(
        "/apps/123/versions/chosen/localizations?limit=200",
        json!([
            resource("loc-fr", json!({"locale": "fr-FR"})),
            resource("loc-us", json!({"locale": "en-US"}))
        ]),
    );

    let details =
        load_for_version(&mut fake, "123", Some("chosen"), &AtomicBool::new(false)).unwrap();

    assert_eq!(details.version.as_ref().unwrap()["id"], "chosen");
    assert_eq!(details.versions[0]["id"], "preferred");
    assert_eq!(details.version_localizations.len(), 2);
    assert_eq!(attr(&details.version_localizations[0], "locale"), "en-US");
    assert_eq!(attr(&details.version_localizations[1], "locale"), "fr-FR");
    assert!(fake.responses.is_empty());
}

#[test]
fn explicit_version_selection_rejects_invalid_or_foreign_ids() {
    let cancelled = AtomicBool::new(false);
    let mut invalid = Fake::default();
    assert!(load_for_version(&mut invalid, "123", Some("../versions/other"), &cancelled).is_err());
    assert!(invalid.requests.is_empty());

    let mut foreign = Fake::default();
    foreign.app();
    foreign.response("/apps/123/app-infos?limit=200", json!([]));
    foreign.response(
        "/apps/123/versions?platform=IOS&limit=200",
        json!([resource(
            "owned-version",
            json!({"platform": "IOS", "appVersionState": "READY_FOR_SALE"})
        )]),
    );
    let error = load_for_version(&mut foreign, "123", Some("other-version"), &cancelled)
        .err()
        .unwrap();
    assert!(error.contains("unavailable for this app"));
    assert_eq!(foreign.requests.len(), 3);
    assert!(foreign.responses.is_empty());
}

#[test]
fn explicit_version_selection_errors_when_version_list_is_unavailable() {
    let mut fake = Fake::default();
    fake.app();
    fake.response("/apps/123/app-infos?limit=200", json!([]));
    fake.responses.push_back((
        "/apps/123/versions?platform=IOS&limit=200".into(),
        403,
        json!({"errors": [{"detail": "raw server message"}]}),
    ));

    let error = load_for_version(&mut fake, "123", Some("requested"), &AtomicBool::new(false))
        .err()
        .unwrap();

    assert!(error.contains("Could not load iOS versions"));
    assert!(error.contains("Access was denied"));
    assert!(!error.contains("raw server message"));
    assert!(fake.responses.is_empty());
}

#[test]
fn loads_and_orders_version_summaries_across_pages() {
    let mut fake = Fake::default();
    fake.app();
    fake.response("/apps/123/app-infos?limit=200", json!([]));
    fake.responses.push_back((
        "/apps/123/versions?platform=IOS&limit=200".into(),
        200,
        json!({
            "items": [resource("live", json!({"platform": "IOS", "appVersionState": "READY_FOR_SALE"}))],
            "nextCursor": "page-2"
        }),
    ));
    fake.response(
        "/apps/123/versions?platform=IOS&limit=200&cursor=page-2",
        json!([
            resource("mac", json!({"platform": "MAC_OS", "appVersionState": "PREPARE_FOR_SUBMISSION"})),
            resource("draft", json!({"platform": "IOS", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-10-01"}))
        ]),
    );
    fake.response(
        "/apps/123/versions/draft/localizations?limit=200",
        json!([]),
    );

    let details = load(&mut fake, "123", &AtomicBool::new(false)).unwrap();

    assert_eq!(details.version.as_ref().unwrap()["id"], "draft");
    assert_eq!(details.versions.len(), 2);
    assert_eq!(details.versions[0]["id"], "draft");
    assert_eq!(details.versions[1]["id"], "live");
    assert!(fake.responses.is_empty());
}

#[test]
fn keeps_base_details_and_other_sections_when_optional_requests_fail() {
    let mut fake = Fake::default();
    fake.app();
    fake.responses.push_back((
        "/apps/123/app-infos?limit=200".into(),
        403,
        json!({"errors": [{"detail": "raw server message"}]}),
    ));
    fake.response(
        "/apps/123/versions?platform=IOS&limit=200",
        json!([resource(
            "live",
            json!({"platform": "IOS", "appVersionState": "READY_FOR_SALE"})
        )]),
    );
    fake.response("/apps/123/versions/live/localizations?limit=200", json!([]));
    let details = load(&mut fake, "123", &AtomicBool::new(false)).unwrap();
    assert_eq!(details.app["id"], "123");
    assert_eq!(details.version.as_ref().unwrap()["id"], "live");
    assert!(details.info.is_none());
    assert_eq!(details.warnings.len(), 1);
    assert!(details.warnings[0].contains("Access was denied"));
    assert!(!details.warnings[0].contains("raw server message"));
}

#[test]
fn new_apps_without_metadata_or_versions_still_load() {
    let mut fake = Fake::default();
    fake.app();
    fake.response("/apps/123/app-infos?limit=200", json!([]));
    fake.response("/apps/123/versions?platform=IOS&limit=200", json!([]));
    let details = load(&mut fake, "123", &AtomicBool::new(false)).unwrap();
    assert!(details.info.is_none());
    assert!(details.version.is_none());
    assert!(details.warnings.is_empty());
}

#[test]
fn refuses_repeated_cursors_before_sending_another_request() {
    let path = "/apps/123/app-infos?limit=200";
    let mut fake = Fake::default();
    for page in [path, "/apps/123/app-infos?limit=200&cursor=again"] {
        fake.responses.push_back((
            page.into(),
            200,
            json!({"items": [], "nextCursor": "again"}),
        ));
    }
    let cancelled = AtomicBool::new(false);
    let mut reader = Reader {
        transport: &mut fake,
        cancelled: &cancelled,
    };
    assert!(reader.list(path).unwrap_err().contains("repeated page"));
    assert_eq!(fake.requests.len(), 2);
}

#[test]
fn rejects_cancelled_requests_invalid_ids_and_authentication_failures() {
    let mut fake = Fake::default();
    assert!(load(&mut fake, "123", &AtomicBool::new(true)).is_err());
    assert!(load(&mut fake, "../apps", &AtomicBool::new(false)).is_err());
    assert!(fake.requests.is_empty());
    fake.responses
        .push_back(("/apps/123".into(), 401, Value::Null));
    let error = load(&mut fake, "123", &AtomicBool::new(false))
        .err()
        .unwrap();
    assert!(error.contains("Reconnect"));
    assert_eq!(fake.requests.len(), 1);
}
