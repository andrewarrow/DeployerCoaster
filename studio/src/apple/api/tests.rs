use super::*;
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};

fn server(status: &str, body: Value) -> (String, thread::JoinHandle<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let status = status.to_owned();
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut headers = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            headers.push_str(&line);
        }
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes).unwrap();
        let request_body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        let body = body.to_string();
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        (headers, request_body)
    });
    (endpoint, task)
}

#[test]
fn exchanges_apple_key_for_validated_facade_credentials() {
    let credentials = AppleCredentials::test_credentials("conn_example");
    let (endpoint, task) = server("201 Created", serde_json::to_value(credentials).unwrap());
    let result = create_connection_with(
        &Client::new(),
        &format!("{endpoint}/v1/connections"),
        "issuer",
        "key",
        "private PEM",
    )
    .unwrap();
    assert_eq!(result.api_key, "pq_test-key");
    assert_eq!(result.account_id.len(), 64);
    let (headers, body) = task.join().unwrap();
    assert!(headers.starts_with("POST /v1/connections "));
    assert!(!headers.to_lowercase().contains("authorization:"));
    assert_eq!(
        body,
        json!({"issuerId":"issuer", "keyId":"key", "privateKey":"private PEM"})
    );
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(!encoded.contains("private PEM"));
}

#[test]
fn setup_rejects_untrusted_returned_base_url_and_redacts_errors() {
    let mut credentials = AppleCredentials::test_credentials("conn_example");
    credentials.base_url = "https://example.com/v1/connections/conn_example".into();
    let (endpoint, task) = server("201 Created", serde_json::to_value(credentials).unwrap());
    assert!(create_connection_with(&Client::new(), &endpoint, "issuer", "key", "secret").is_err());
    task.join().unwrap();
    let (endpoint, task) = server(
        "401 Unauthorized",
        json!({"error":{"code":"invalid_credentials", "message":"secret"}}),
    );
    let error = create_connection_with(&Client::new(), &endpoint, "issuer", "key", "secret")
        .err()
        .unwrap();
    assert!(error.contains("Reconnect"));
    assert!(!error.contains("secret"));
    task.join().unwrap();
}

#[test]
fn sends_facade_key_to_connection_scoped_resource_without_apple_envelopes() {
    let (endpoint, task) = server("200 OK", json!({"id":"123", "name":"Example"}));
    let mut credentials = AppleCredentials::test_credentials("conn_example");
    credentials.base_url = format!("{endpoint}/v1/connections/conn_example");
    let mut api = ParsequeradeApi {
        http: Client::new(),
        credentials,
    };
    let (status, app) = api.send(Method::GET, "/apps/123", None).unwrap();
    assert_eq!(status, 200);
    assert_eq!(app["name"], "Example");
    let (headers, body) = task.join().unwrap();
    assert!(headers.starts_with("GET /v1/connections/conn_example/apps/123 "));
    assert!(
        headers
            .to_lowercase()
            .contains("authorization: bearer pq_test-key")
    );
    assert_eq!(body, Value::Null);
    assert!(
        api.send(Method::PATCH, "/apps/123", Some(&json!({})))
            .unwrap_err()
            .contains("read-only")
    );
}

#[test]
fn refuses_paths_that_escape_the_connection() {
    let base = format!("{SERVICE}/v1/connections/conn_example");
    for path in [
        "https://example.com/apps",
        "//example.com/apps",
        "/apps/../users",
        "/apps/%2e%2e/users",
        "/apps\\other",
        "/apps#fragment",
        "/apps//123",
    ] {
        assert!(resource_url(&base, path).is_err(), "accepted {path}");
    }
    assert_eq!(
        resource_url(&base, "/apps?cursor=https%3A%2F%2Fexample.com")
            .unwrap()
            .origin(),
        Url::parse(SERVICE).unwrap().origin()
    );
    let mut credentials = AppleCredentials::test_credentials("conn_example");
    for url in [
        "http://parsequerade.mainbrella.com/v1/connections/conn_example",
        "https://user@parsequerade.mainbrella.com/v1/connections/conn_example",
        "https://parsequerade.mainbrella.com/v1/connections/another",
    ] {
        credentials.base_url = url.into();
        assert!(validate_connection(&credentials).is_err());
    }
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
    let next = resource_url(SERVICE, &pages.requests[1]).unwrap();
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
#[ignore = "requires explicitly configured Apple test credentials; creates and revokes a temporary hosted connection"]
fn live_parsequerade_reads() {
    let issuer =
        std::env::var("PARSEQUERADE_TEST_ISSUER_ID").expect("set PARSEQUERADE_TEST_ISSUER_ID");
    let key_id = std::env::var("PARSEQUERADE_TEST_KEY_ID").expect("set PARSEQUERADE_TEST_KEY_ID");
    let key_path =
        std::env::var("PARSEQUERADE_TEST_P8_FILE").expect("set PARSEQUERADE_TEST_P8_FILE");
    let key = std::fs::read_to_string(key_path).expect("read test private key");
    let credentials = create_connection(&issuer, &key_id, &key)
        .unwrap_or_else(|error| panic!("Connection setup: {error}"));
    let result = (|| {
        let mut api = ParsequeradeApi::new(credentials.clone())?;
        let cancelled = AtomicBool::new(false);
        let apps = crate::apple::list_apps(&mut api, &cancelled)?;
        let app = apps.first().ok_or("Test account has no apps")?;
        let details = crate::apple::details::load(api, &app.id, &cancelled)?;
        if !details.warnings.is_empty() {
            return Err(format!(
                "Metadata read failed: {}",
                details.warnings.join("; ")
            ));
        }
        println!(
            "Verified hosted connection, {} app listings, and selected app metadata/localizations.",
            apps.len()
        );
        Ok::<_, String>(())
    })();
    let cleanup = client()
        .unwrap()
        .delete(&credentials.base_url)
        .bearer_auth(&credentials.api_key)
        .send();
    assert_eq!(
        cleanup
            .expect("revoke temporary connection")
            .status()
            .as_u16(),
        204,
        "temporary connection revocation failed"
    );
    result.unwrap_or_else(|error| panic!("Live reads: {error}"));
}
