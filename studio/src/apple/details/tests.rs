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
        self.responses
            .push_back((path.into(), 200, json!({"data": data})));
    }

    fn app(&mut self) {
        self.response("/v1/apps/123", json!({"id": "123", "attributes": {
            "name": "Example", "bundleId": "com.example.app", "sku": "example", "primaryLocale": "fr-FR"
        }}));
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
    json!({"id": id, "attributes": attributes})
}

#[test]
fn loads_main_fields_and_all_english_strings_from_preferred_ios_version() {
    let mut fake = Fake::default();
    fake.app();
    fake.response(
        "/v1/apps/123/appInfos?limit=200",
        json!([
            resource("old-info", json!({"state": "REPLACED_WITH_NEW_INFO"})),
            resource("live-info", json!({"state": "READY_FOR_DISTRIBUTION"})),
            resource("draft-info", json!({"state": "PREPARE_FOR_SUBMISSION"}))
        ]),
    );
    fake.response("/v1/appInfos/draft-info/appInfoLocalizations?limit=200", json!([
        resource("info-fr", json!({"locale": "fr-FR", "name": "French name"})),
        resource("info-gb", json!({"locale": "en-GB", "subtitle": "British subtitle"})),
        resource("info-us", json!({"locale": "en-US", "name": "English name", "subtitle": "English subtitle", "privacyPolicyUrl": "https://example.com/privacy"}))
    ]));
    fake.response("/v1/apps/123/appStoreVersions?filter[platform]=IOS&limit=200", json!([
        resource("mac", json!({"platform": "MAC_OS", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-10-01"})),
        resource("live", json!({"platform": "IOS", "appVersionState": "READY_FOR_DISTRIBUTION"})),
        resource("draft-old", json!({"platform": "IOS", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-09-01"})),
        resource("draft-new", json!({"platform": "IOS", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-10-01", "versionString": "2.0"}))
    ]));
    fake.response("/v1/appStoreVersions/draft-new/appStoreVersionLocalizations?limit=200", json!([
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
    assert_eq!(details.info_localizations.len(), 2);
    assert_eq!(attr(&details.info_localizations[0], "locale"), "en-US");
    assert_eq!(attr(&details.info_localizations[1], "locale"), "en-GB");
    assert_eq!(
        attr(&details.info_localizations[0], "subtitle"),
        "English subtitle"
    );
    assert_eq!(details.version_localizations.len(), 1);
    assert_eq!(
        attr(&details.version_localizations[0], "description"),
        "English description"
    );
    assert_eq!(
        attr(&details.version_localizations[0], "whatsNew"),
        "New features"
    );
    assert!(details.warnings.is_empty());
    assert!(fake.responses.is_empty());
}

#[test]
fn follows_localization_pages_before_filtering_for_english() {
    let mut fake = Fake::default();
    let first = "/v1/appInfos/info/appInfoLocalizations?limit=200";
    let next =
        "https://api.appstoreconnect.apple.com/v1/appInfos/info/appInfoLocalizations?cursor=2";
    fake.responses.push_back((
        first.into(),
        200,
        json!({
            "data": [resource("fr", json!({"locale": "fr-FR"}))], "links": {"next": next}
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
    let result = english(reader.list(first).unwrap());
    assert_eq!(result.len(), 1);
    assert_eq!(attr(&result[0], "locale"), "en-AU");
    assert!(fake.responses.is_empty());
}

#[test]
fn keeps_base_details_and_other_sections_when_optional_requests_fail() {
    let mut fake = Fake::default();
    fake.app();
    fake.responses.push_back((
        "/v1/apps/123/appInfos?limit=200".into(),
        403,
        json!({"errors": [{"detail": "raw server message"}]}),
    ));
    fake.response(
        "/v1/apps/123/appStoreVersions?filter[platform]=IOS&limit=200",
        json!([resource(
            "live",
            json!({"platform": "IOS", "appVersionState": "READY_FOR_SALE"})
        )]),
    );
    fake.response(
        "/v1/appStoreVersions/live/appStoreVersionLocalizations?limit=200",
        json!([]),
    );
    let details = load(&mut fake, "123", &AtomicBool::new(false)).unwrap();
    assert_eq!(details.app["id"], "123");
    assert_eq!(details.version.as_ref().unwrap()["id"], "live");
    assert!(details.info.is_none());
    assert_eq!(details.warnings.len(), 1);
    assert!(details.warnings[0].contains("Apple denied access"));
    assert!(!details.warnings[0].contains("raw server message"));
}

#[test]
fn new_apps_without_metadata_or_versions_still_load() {
    let mut fake = Fake::default();
    fake.app();
    fake.response("/v1/apps/123/appInfos?limit=200", json!([]));
    fake.response(
        "/v1/apps/123/appStoreVersions?filter[platform]=IOS&limit=200",
        json!([]),
    );
    let details = load(&mut fake, "123", &AtomicBool::new(false)).unwrap();
    assert!(details.info.is_none());
    assert!(details.version.is_none());
    assert!(details.warnings.is_empty());
}

#[test]
fn refuses_unsafe_or_repeated_pagination_before_sending_another_request() {
    let path = "/v1/apps/123/appInfos?limit=200";
    for next in [
        "https://example.com/v1/apps/123/appInfos?cursor=2",
        "https://api.appstoreconnect.apple.com/v1/apps/other/appInfos?cursor=2",
        "https://user@api.appstoreconnect.apple.com/v1/apps/123/appInfos?cursor=2",
        "https://api.appstoreconnect.apple.com/v1/apps/123/appInfos?cursor=2#fragment",
        path,
    ] {
        let mut fake = Fake::default();
        fake.responses.push_back((
            path.into(),
            200,
            json!({"data": [], "links": {"next": next}}),
        ));
        let cancelled = AtomicBool::new(false);
        let mut reader = Reader {
            transport: &mut fake,
            cancelled: &cancelled,
        };
        assert!(reader.list(path).is_err());
        assert_eq!(fake.requests.len(), 1);
    }
}

#[test]
fn rejects_cancelled_requests_invalid_ids_and_authentication_failures() {
    let mut fake = Fake::default();
    assert!(load(&mut fake, "123", &AtomicBool::new(true)).is_err());
    assert!(load(&mut fake, "../apps", &AtomicBool::new(false)).is_err());
    assert!(fake.requests.is_empty());
    fake.responses
        .push_back(("/v1/apps/123".into(), 401, Value::Null));
    let error = load(&mut fake, "123", &AtomicBool::new(false))
        .err()
        .unwrap();
    assert!(error.contains("API key"));
    assert_eq!(fake.requests.len(), 1);
}
