use super::*;
use serde_json::json;

fn session() -> Session {
    Session::new(
        Credentials {
            client_id: "test.apps.googleusercontent.com".to_owned(),
            client_secret: "test-client-secret".to_owned(),
        },
        serde_json::from_value(json!({
            "access_token": "test-access-token",
            "refresh_token": "test-refresh-token",
            "token_type": "Bearer",
            "expires_in": 3600,
        }))
        .unwrap(),
    )
}

#[test]
fn saved_session_restores_refresh_token_and_forces_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    session().save(&path).unwrap();
    let restored = Session::load_from(&path).unwrap().unwrap();
    assert_eq!(
        restored.token.refresh_token().unwrap().secret(),
        "test-refresh-token"
    );
    assert!(restored.expires_at <= Instant::now());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn console_credentials_restore_replace_and_clear_saved_values() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings/console-session.json");
    let missing = ConsoleCredentials::load_from(&path).unwrap();
    assert!(missing.url.is_empty());
    assert!(missing.cookies.is_empty());

    let mut credentials = ConsoleCredentials {
        url: "https://play.google.com/console/u/0/developers/123/app-list".into(),
        cookies: "SAPISID=test-cookie; other=value".into(),
        ..ConsoleCredentials::default()
    };
    credentials.save(&path).unwrap();
    let restored = ConsoleCredentials::load_from(&path).unwrap();
    assert_eq!(restored.url, credentials.url);
    assert_eq!(restored.cookies, credentials.cookies);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    credentials.cookies = "SAPISID=replacement".into();
    credentials.save(&path).unwrap();
    assert_eq!(
        ConsoleCredentials::load_from(&path).unwrap().cookies,
        credentials.cookies
    );
    ConsoleCredentials::default().save(&path).unwrap();
    let cleared = ConsoleCredentials::load_from(&path).unwrap();
    assert!(cleared.url.is_empty());
    assert!(cleared.cookies.is_empty());
}

#[test]
fn console_credentials_report_unreadable_and_invalid_files() {
    let directory = tempfile::tempdir().unwrap();
    assert!(ConsoleCredentials::load_from(directory.path()).is_err());
    let path = directory.path().join("console-session.json");
    fs::write(&path, b"invalid json").unwrap();
    assert!(ConsoleCredentials::load_from(&path).is_err());
}

fn test_http() -> Client {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
}

fn request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        let end = line == "\r\n";
        request.push_str(&line);
        if end {
            break;
        }
    }
    request
}

fn app_server(pages: Vec<(&'static str, serde_json::Value)>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("http://{}/apps:search", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        for (page_token, body) in pages {
            let (mut stream, _) = listener.accept().unwrap();
            let request = request(&mut stream);
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer test-access-token\r\n")
            );
            let target = request.split_whitespace().nth(1).unwrap();
            let url = Url::parse(&format!("http://localhost{target}")).unwrap();
            assert!(
                url.query_pairs()
                    .any(|(key, value)| key == "pageToken" && value == page_token)
            );
            assert!(
                url.query_pairs()
                    .any(|(key, value)| key == "pageSize" && value == "1000")
            );
            let body = body.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    (endpoint, handle)
}

#[test]
fn credentials_require_a_desktop_client_and_errors_do_not_include_secrets() {
    assert!(Credentials::parse(br#"{"installed":{"client_id":"test.apps.googleusercontent.com","client_secret":"secret"}}"#).is_ok());
    assert!(
        Credentials::parse(br#"{"web":{"client_id":"test","client_secret":"secret"}}"#).is_err()
    );
    let error =
        Credentials::parse(br#"{"installed":{"client_id":"wrong","client_secret":"do-not-leak"}}"#)
            .err()
            .unwrap();
    assert!(!error.contains("do-not-leak"));
}

#[test]
fn callback_rejects_forgery_duplicates_wrong_paths_and_missing_codes() {
    let state = CsrfToken::new("expected-state".to_owned());
    for target in [
        "/oauth/callback?state=wrong&code=secret-code",
        "/oauth/callback?code=secret-code",
        "/oauth/callback?state=expected-state",
        "/oauth/callback?state=expected-state&code=",
        "/oauth/callback?state=expected-state&state=wrong&code=secret-code",
        "/oauth/callback?state=expected-state&code=one&code=two",
        "/oauth/callback?state=expected-state&code=one&error=access_denied",
        "/favicon.ico?state=expected-state&code=secret-code",
    ] {
        assert!(parse_callback(&format!("GET {target} HTTP/1.1\r\n"), &state).is_err());
    }
    let Callback::Code(code) = parse_callback(
        "GET /oauth/callback?state=expected-state&code=code%2Bwith%2Fencoding HTTP/1.1\r\n",
        &state,
    )
    .ok()
    .unwrap() else {
        panic!("Expected an authorization code")
    };
    assert_eq!(code.secret(), "code+with/encoding");
    assert!(matches!(
        parse_callback(
            "GET /oauth/callback?state=expected-state&error=access_denied HTTP/1.1\r\n",
            &state,
        ),
        Ok(Callback::Denied)
    ));
}

#[test]
fn loopback_callback_ignores_invalid_state_and_accepts_the_real_browser_redirect() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let browser = thread::spawn(move || {
        for (state, expected_status) in [("forged", "400 Bad Request"), ("real", "200 OK")] {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(stream, "GET /oauth/callback?state={state}&code=code-from-browser HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
            let response = request(&mut stream);
            assert!(response.starts_with(&format!("HTTP/1.1 {expected_status}")));
            assert!(response.contains("Cache-Control: no-store"));
            assert!(!response.contains("code-from-browser"));
        }
    });
    let code = wait_for_callback(
        &listener,
        &CsrfToken::new("real".to_owned()),
        &AtomicBool::new(false),
        Duration::from_secs(3),
    )
    .unwrap();
    assert_eq!(code.secret(), "code-from-browser");
    browser.join().unwrap();
}

#[test]
fn callback_cancellation_and_timeout_do_not_wait_for_browser_input() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let state = CsrfToken::new("state".to_owned());
    assert!(
        wait_for_callback(
            &listener,
            &state,
            &AtomicBool::new(true),
            Duration::from_secs(3)
        )
        .unwrap_err()
        .contains("cancelled")
    );
    assert!(
        wait_for_callback(&listener, &state, &AtomicBool::new(false), Duration::ZERO)
            .unwrap_err()
            .contains("timed out")
    );
}

#[test]
fn app_listing_follows_pagination_sorts_and_removes_duplicates() {
    let (endpoint, server) = app_server(vec![
        (
            "",
            json!({"apps":[{"packageName":"com.example.z","displayName":"Zebra"}],"nextPageToken":"page+/2"}),
        ),
        (
            "page+/2",
            json!({"apps":[{"packageName":"com.example.a","displayName":"Alpha"},{"packageName":"com.example.z","displayName":"Zebra"}]}),
        ),
    ]);
    let apps = list_apps(
        &test_http(),
        &mut session(),
        &AtomicBool::new(false),
        &endpoint,
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0].display_name, "Alpha");
    assert_eq!(apps[1].display_name, "Zebra");
}

#[test]
fn app_listing_accepts_empty_accounts_and_rejects_repeated_page_tokens() {
    let (endpoint, server) = app_server(vec![("", json!({}))]);
    assert!(
        list_apps(
            &test_http(),
            &mut session(),
            &AtomicBool::new(false),
            &endpoint
        )
        .unwrap()
        .is_empty()
    );
    server.join().unwrap();
    let (endpoint, server) = app_server(vec![
        ("", json!({"nextPageToken":"repeated"})),
        ("repeated", json!({"nextPageToken":"repeated"})),
    ]);
    assert!(
        list_apps(
            &test_http(),
            &mut session(),
            &AtomicBool::new(false),
            &endpoint
        )
        .err()
        .unwrap()
        .contains("repeated page")
    );
    server.join().unwrap();
}

#[test]
fn disabled_api_errors_explain_setup_without_exposing_the_response() {
    let error = api_error(
        StatusCode::FORBIDDEN,
        json!({"error":{
            "message":"private-response-detail",
            "details":[{"reason":"SERVICE_DISABLED"}]
        }}),
    );
    assert!(error.contains("Play Developer Reporting API"));
    assert!(!error.contains("private-response-detail"));
    assert!(api_error(StatusCode::FORBIDDEN, json!({})).contains("permissions"));
}

#[test]
fn list_failure_preserves_the_session_for_retry_and_cancel_discards_pending_events() {
    let (sender, events) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut store = PlayStore {
        job: Some(Job {
            events,
            cancelled: cancelled.clone(),
        }),
        ..Default::default()
    };
    sender.send(Event::SignedIn(Box::new(session()))).unwrap();
    sender
        .send(Event::Error("Enable the API".to_owned()))
        .unwrap();
    sender.send(Event::Finished).unwrap();
    store.poll();
    assert!(store.session.is_some());
    assert_eq!(store.error.as_deref(), Some("Enable the API"));
    assert!(store.job.is_none());
    assert!(cancelled.load(Ordering::Relaxed));
    let (sender, events) = mpsc::channel();
    store.job = Some(Job {
        events,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    store.job = None;
    assert!(sender.send(Event::SignedIn(Box::new(session()))).is_err());
}

#[test]
fn ui_fits_supported_sizes_with_long_titles_packages_and_errors() {
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };
        let mut store = PlayStore {
            session: Some(session()),
            loaded: true,
            apps: vec![
                PlayApp { display_name: "A long application title with several words that needs to wrap on smaller desktop windows".to_owned(), package_name: format!("com.example.{}", "application".repeat(10)) },
                PlayApp { display_name: String::new(), package_name: "com.example.untitled".to_owned() },
            ],
            error: Some("Google Play denied access. Grant the reporting permission when connecting and check this account's Play Console app permissions.".to_owned()),
            ..Default::default()
        };
        for _ in 0..2 {
            let _ = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default_margins().show(ui, |ui| {
                    store.apps_ui(ui);
                    assert!(
                        ui.min_rect().right() <= width,
                        "Horizontal overflow at {width}px"
                    );
                });
            });
        }
    }
}
