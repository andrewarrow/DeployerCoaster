use super::*;
use std::collections::VecDeque;

#[test]
fn validates_apple_key_locally_and_keeps_credentials_for_restart() {
    let input = AppleCredentials::test_credentials("issuer");
    let credentials = create_connection(" issuer ", " TESTKEY123 ", &input.private_key).unwrap();
    assert_eq!(credentials.issuer_id, "issuer");
    assert_eq!(credentials.key_id, "TESTKEY123");
    assert_eq!(credentials.private_key, input.private_key);
    assert_eq!(
        credentials.sales_cache_account(),
        input.sales_cache_account()
    );
    let encoded = serde_json::to_value(&credentials).unwrap();
    assert_eq!(encoded["privateKey"], input.private_key);
    for field in ["apiKey", "baseUrl", "connectionId"] {
        assert!(encoded.get(field).is_none());
    }
    let restored = serde_json::from_value(encoded).unwrap();
    assert!(credentials == restored);
    validate_connection(&restored).unwrap();
}

#[test]
fn rejects_invalid_local_keys_without_exposing_the_key() {
    for (issuer, key, pem) in [
        ("issuer", "key", "secret"),
        ("", "key", "secret"),
        ("issuer", "", "secret"),
        (
            "issuer",
            "key",
            "-----BEGIN PRIVATE KEY-----secret-----END PRIVATE KEY-----",
        ),
    ] {
        let error = create_connection(issuer, key, pem).err().unwrap();
        assert!(error.contains("Issuer ID"));
        assert!(!error.contains("secret"));
    }
}

#[test]
fn refuses_paths_that_escape_the_facade() {
    for path in [
        "https://example.com/apps",
        "//example.com/apps",
        "/apps/../users",
        "/apps/%2e%2e/users",
        "/apps\\other",
        "/apps#fragment",
        "/apps//123",
    ] {
        assert!(resource_url(path).is_err(), "accepted {path}");
    }
    assert_eq!(
        resource_url("/apps?cursor=https%3A%2F%2Fexample.com")
            .unwrap()
            .path(),
        "/apps"
    );
}

#[test]
fn sdk_read_errors_keep_status_and_envelope_for_optional_resources() {
    let mut api = ParsequeradeApi::new(AppleCredentials::test_credentials("issuer")).unwrap();
    let (status, value) = api
        .send(Method::GET, "/apps?unsupported=value", None)
        .unwrap();
    assert_eq!(status, 400);
    assert_eq!(value["error"]["code"], "invalid_query");
    let (status, value) = api.send(Method::GET, "/unsupported", None).unwrap();
    assert_eq!(status, 404);
    assert_eq!(value["error"]["code"], "route_not_found");
    // Decoded query values reach SDK validation; Apple-style parameters stay unsupported.
    let (status, _) = api
        .send(Method::GET, "/apps?filter%5Bname%5D=x", None)
        .unwrap();
    assert_eq!(status, 400);
}

#[test]
fn age_rating_writes_use_sdk_validation_and_other_writes_stay_unavailable() {
    let mut api = ParsequeradeApi::new(AppleCredentials::test_credentials("issuer")).unwrap();
    let (status, value) = api
        .send(
            Method::POST,
            "/apps/456/app-infos/info/age-rating",
            Some(&json!({"unsupported":true})),
        )
        .unwrap();
    assert_eq!(status, 400);
    assert_eq!(value["error"]["code"], "invalid_request");
    for (method, path, body) in [
        (Method::POST, "/apps/456", Some(json!({}))),
        (
            Method::POST,
            "/apps/456/app-infos/info/localizations/en-US",
            Some(json!({})),
        ),
        (
            Method::POST,
            "/apps/456/app-infos/info/age-rating?x=1",
            Some(json!({})),
        ),
        (Method::POST, "/apps/456/app-infos/info/age-rating", None),
        (Method::PATCH, "/apps/456", Some(json!({}))),
        (Method::GET, "/apps", Some(json!({}))),
    ] {
        assert!(
            api.send(method, path, body.as_ref())
                .unwrap_err()
                .contains("Only age-rating")
        );
    }
}

#[test]
fn typed_age_rating_response_keeps_explicit_clears_for_the_ui_merge() {
    let rating = serde_json::from_value(json!({
        "id":"rating", "appId":"456", "appInfoId":"info", "advertising":true,
        "kidsAgeBand":null,
    }))
    .unwrap();
    let response = age_rating_response(rating, &json!({"advertising":false,"kidsAgeBand":null}));
    assert_eq!(response["id"], "rating");
    assert_eq!(response["advertising"], true);
    assert_eq!(response.get("kidsAgeBand"), Some(&Value::Null));
    assert!(response.get("gambling").is_none());
}

struct Pages {
    responses: VecDeque<Value>,
    requests: Vec<String>,
}
impl Transport for Pages {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        assert_eq!(method, Method::GET);
        assert!(body.is_none());
        self.requests.push(path.into());
        Ok((200, self.responses.pop_front().unwrap()))
    }
}

#[test]
fn cursor_is_encoded_as_data_and_original_filters_are_preserved() {
    let cursor = "https://example.com/?x=1&cursor=oops+/=";
    let mut pages = Pages {
        requests: vec![],
        responses: VecDeque::from([
            json!({"items":[{"id":"one"}],"nextCursor":cursor}),
            json!({"items":[{"id":"two"}],"nextCursor":null}),
        ]),
    };
    assert_eq!(
        list(
            &mut pages,
            "/apps/123/versions?platform=IOS&limit=200",
            &AtomicBool::new(false)
        )
        .unwrap()
        .len(),
        2
    );
    let next = resource_url(&pages.requests[1]).unwrap();
    let pairs = next
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(pairs["cursor"], cursor);
    assert_eq!(pairs["platform"], "IOS");
    assert_eq!(pairs["limit"], "200");
    assert_eq!(next.path(), "/apps/123/versions");
}

#[test]
fn rejects_bad_pages_and_honors_cancellation_without_network_calls() {
    for response in [
        json!({"data":[]}),
        json!({"items":[],"nextCursor":12}),
        json!({"items":[{"name":"no id"}],"nextCursor":null}),
    ] {
        let mut pages = Pages {
            requests: vec![],
            responses: VecDeque::from([response]),
        };
        assert!(list(&mut pages, "/apps?limit=200", &AtomicBool::new(false)).is_err());
    }
    let mut pages = Pages {
        requests: vec![],
        responses: VecDeque::new(),
    };
    assert!(list(&mut pages, "/apps?limit=200", &AtomicBool::new(true)).is_err());
    assert!(pages.requests.is_empty());
}

#[test]
fn app_list_consumes_flat_pages_sorts_and_deduplicates() {
    let mut pages = Pages {
        requests: vec![],
        responses: VecDeque::from([
            json!({"items":[{"id":"b","name":"Zebra","bundleId":"com.example.b"}],"nextCursor":"next"}),
            json!({"items":[{"id":"a","name":"Alpha","sku":"sku-a"},{"id":"b","name":"Zebra"}],"nextCursor":null}),
        ]),
    };
    let apps = crate::apple::list_apps(&mut pages, &AtomicBool::new(false)).unwrap();
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0].id, "a");
    assert_eq!(apps[0].attributes.sku, "sku-a");
    assert_eq!(apps[1].attributes.bundle_id, "com.example.b");
}

#[test]
fn credential_errors_explain_reconnection_and_never_expose_upstream_details() {
    for (status, code) in [
        (422, "invalid_credentials"),
        (502, "upstream_authentication_failed"),
    ] {
        let error = api_error(
            status,
            &json!({"error": {"code": code, "message": "private upstream detail"}}),
        );
        assert!(error.contains("reconnect"));
        assert!(error.contains("Issuer ID"));
        assert!(!error.contains("private upstream detail"));
    }
}

#[test]
#[ignore = "requires explicitly configured Apple test credentials; reads directly from Apple"]
fn live_parsequerade_reads() {
    let issuer =
        std::env::var("PARSEQUERADE_TEST_ISSUER_ID").expect("set PARSEQUERADE_TEST_ISSUER_ID");
    let key_id = std::env::var("PARSEQUERADE_TEST_KEY_ID").expect("set PARSEQUERADE_TEST_KEY_ID");
    let key_path =
        std::env::var("PARSEQUERADE_TEST_P8_FILE").expect("set PARSEQUERADE_TEST_P8_FILE");
    let key = std::fs::read_to_string(key_path).expect("read test private key");
    let credentials = create_connection(&issuer, &key_id, &key)
        .unwrap_or_else(|error| panic!("Local key: {error}"));
    let mut api = ParsequeradeApi::new(credentials).unwrap();
    let cancelled = AtomicBool::new(false);
    let apps = crate::apple::list_apps(&mut api, &cancelled).unwrap();
    let app = apps.first().expect("Test account has no apps");
    let details = crate::apple::details::load(api, &app.id, &cancelled).unwrap();
    assert!(
        details.warnings.is_empty(),
        "Metadata read failed: {}",
        details.warnings.join("; ")
    );
}
