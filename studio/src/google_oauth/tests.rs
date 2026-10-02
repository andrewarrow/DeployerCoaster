use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn mock_api(pages: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/v3/projects:search",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in pages {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (url, server)
}

fn test_session(url: String) -> ConsoleSession {
    ConsoleSession {
        url,
        cookies: "SID=test-session; SAPISID=test-signing-cookie".into(),
        auth_user: "0".into(),
        body: serde_json::json!({
            "requestContext": {"projectId": "old-project"},
            "querySignature": "test-signature",
            "variables": {"projectId": "old-project", "projectNumber": 1}
        }),
    }
}

#[test]
fn console_request_parser_restricts_endpoint_and_never_executes_curl() {
    let command = format!(
        "curl 'https://cloudconsole-pa.clients6.google.com{CONSOLE_PATH}' -H 'Cookie: SID=test-session; SAPISID=test-signing-cookie' -H 'X-Goog-AuthUser: 1' --data-raw '{{\"querySignature\":\"test\",\"variables\":{{}}}}'"
    );
    let session = ConsoleSession::parse_curl(&command).unwrap();
    assert_eq!(session.auth_user, "1");
    assert!(session.authorization().unwrap().starts_with("SAPISIDHASH "));
    assert!(
        ConsoleSession::parse_curl(
            &command.replace("cloudconsole-pa.clients6.google.com", "example.com")
        )
        .is_err()
    );
    assert!(
        ConsoleSession::parse_curl(
            &command.replace("SERVICE_USAGE_GRAPHQL:batchGraphql", "other:method")
        )
        .is_err()
    );
    assert!(
        ConsoleSession::parse_curl(
            &command.replace("--data-raw", "--data-binary @credentials.json")
        )
        .is_err()
    );
}

#[test]
fn branding_uses_each_project_number_and_only_accepts_google_artwork() {
    let session = test_session(format!(
        "https://cloudconsole-pa.clients6.google.com{CONSOLE_PATH}?key=test-key"
    ));
    let mut project = Project {
        project_id: "groupicorn".into(),
        display_name: "Groupicorn".into(),
        name: "projects/123".into(),
    };
    let url = session.branding_url(&project).unwrap();
    assert_eq!(url.host_str(), Some("clientauthconfig.clients6.google.com"));
    assert_eq!(url.path(), "/v1/brands/lookupkey/brand/123");
    assert!(
        url.query_pairs()
            .any(|(name, value)| name == "key" && value == "test-key")
    );
    assert!(
        url.query_pairs()
            .any(|(name, value)| name == "readMask" && value == "iconUrl")
    );
    project.name = "projects/456".into();
    assert!(
        session
            .branding_url(&project)
            .unwrap()
            .path()
            .ends_with("/456")
    );
    project.name = "projects/invalid".into();
    assert!(session.branding_url(&project).is_err());
    for (status, body, expected) in [
        (
            200,
            r#"{"iconUrl":"https://lh3.googleusercontent.com/branding-icon"}"#,
            Some("https://lh3.googleusercontent.com/branding-icon"),
        ),
        (200, r#"{}"#, None),
        (200, r#"{"iconUrl":""}"#, None),
        (
            200,
            r#"{"iconUrl":"https://googleusercontent.com.evil.test/icon"}"#,
            None,
        ),
        (
            200,
            r#"{"iconUrl":"http://lh3.googleusercontent.com/icon"}"#,
            None,
        ),
        (404, "private-provider-message", None),
    ] {
        let (url, server) = mock_api(vec![(status, body.into())]);
        assert_eq!(
            fetch_branding_icon_url(
                &reqwest::blocking::Client::new(),
                &session,
                &url.parse().unwrap()
            )
            .unwrap()
            .as_deref(),
            expected
        );
        let requests = server.join().unwrap();
        assert!(requests[0].to_lowercase().contains("x-goog-authuser: 0"));
    }
    let (url, server) = mock_api(vec![(403, "private-provider-message".into())]);
    assert_eq!(
        fetch_branding_icon_url(
            &reqwest::blocking::Client::new(),
            &session,
            &url.parse().unwrap()
        )
        .unwrap_err(),
        SESSION_ERROR
    );
    server.join().unwrap();
}

#[test]
fn client_list_follows_pages_and_surfaces_graphql_permission_errors() {
    let client = serde_json::json!({"clientId": "test.apps.googleusercontent.com", "displayName": "Web client", "displayType": "CLIENT_TYPE_WEB_APPLICATION", "creationTime": "2026-10-02T12:00:00Z"});
    let response = |clients: serde_json::Value, next: &str| {
        serde_json::json!([{
            "results": [{"data": {"oAuthClientsList": {"data": clients, "nextPageToken": next}}}]
        }])
        .to_string()
    };
    let (url, server) = mock_api(vec![
        (200, response(serde_json::json!([client]), "next")),
        (200, response(serde_json::json!([client]), "")),
    ]);
    let project = Project {
        project_id: "groupicorn".into(),
        display_name: "Groupicorn".into(),
        name: "projects/123".into(),
    };
    let clients = fetch_clients(
        &reqwest::blocking::Client::new(),
        &test_session(url),
        &project,
    )
    .unwrap();
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].type_label(), "Web application");
    let requests = server.join().unwrap();
    assert!(requests[0].contains("\"projectId\":\"groupicorn\""));
    assert!(requests[0].contains("\"projectNumber\":123"));
    assert!(requests[1].contains("\"pageToken\":\"next\""));
    let (url, server) = mock_api(vec![(200, r#"[{"results":[{"data":{"oAuthClientsList":{"data":[]}},"errors":[{"message":"private-provider-message"}]}]}]"#.into())]);
    let error = fetch_clients(
        &reqwest::blocking::Client::new(),
        &test_session(url),
        &project,
    )
    .err()
    .unwrap();
    assert_eq!(error, SESSION_ERROR);
    server.join().unwrap();
}

#[test]
#[ignore = "Uses the local gcloud login and saved Google Cloud Console session for read-only API calls"]
fn live_google_projects_and_groupicorn_clients() {
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let projects = fetch_projects(&http, &access_token().unwrap(), PROJECTS_API).unwrap();
    assert!(!projects.is_empty());
    let project = projects
        .iter()
        .find(|p| p.project_id == "groupicorn")
        .unwrap();
    let clients = fetch_clients(&http, &ConsoleSession::load().unwrap(), project).unwrap();
    assert!(
        clients
            .iter()
            .any(|c| c.display_name == "iOS client 1" && c.type_label() == "iOS")
    );
    assert!(clients.iter().any(|c| c.display_name == "cloudflare"));
    assert!(clients.iter().any(|c| c.display_name == "Web client 1"));
    println!(
        "Verified {} projects and {} Groupicorn OAuth clients.",
        projects.len(),
        clients.len()
    );
}

#[test]
#[ignore = "Uses the local gcloud login and saved Console session for read-only branding and image requests"]
fn live_google_project_branding() {
    let http = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let session = ConsoleSession::load().unwrap();
    let projects = fetch_projects(&http, &access_token().unwrap(), PROJECTS_API).unwrap();
    let cancelled = AtomicBool::new(false);
    let mut count = 0;
    for project in &projects {
        let url = fetch_branding_icon_url(&http, &session, &session.branding_url(project).unwrap())
            .unwrap();
        if project.project_id == "deployercoaster" {
            assert!(url.is_some());
        }
        if let Some(url) = url {
            let image = crate::app_icons::google_artwork(&http, &url, &cancelled).unwrap();
            assert!(image.size[0] > 0 && image.size[1] > 0);
            count += 1;
        }
    }
    assert!(count > 0);
    println!(
        "Verified branding and decoded {count} logos across {} Google projects.",
        projects.len()
    );
}

#[test]
fn projects_follow_pagination_sort_and_deduplicate() {
    let (url, server) = mock_api(vec![
        (200, r#"{"projects":[{"projectId":"zeta","displayName":"Zeta","name":"projects/1"}],"nextPageToken":"next"}"#.into()),
        (200, r#"{"projects":[{"projectId":"alpha","displayName":"Alpha","name":"projects/2"},{"projectId":"zeta","displayName":"Zeta","name":"projects/1"}]}"#.into()),
    ]);
    let projects = fetch_projects(&reqwest::blocking::Client::new(), "test-token", &url).unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].project_id, "alpha");
    let requests = server.join().unwrap();
    assert!(requests[1].contains("pageToken=next"));
    assert!(requests[0].contains("query=state%3AACTIVE"));
    assert!(
        requests[0]
            .to_lowercase()
            .contains("authorization: bearer test-token")
    );
}

#[test]
fn rejects_repeated_pagination_and_reports_errors_without_response_text() {
    let (url, server) = mock_api(vec![
        (200, r#"{"nextPageToken":"repeat"}"#.into()),
        (200, r#"{"nextPageToken":"repeat"}"#.into()),
    ]);
    assert!(
        fetch_projects(&reqwest::blocking::Client::new(), "test-token", &url)
            .err()
            .unwrap()
            .contains("pagination")
    );
    server.join().unwrap();
    for status in [401, 403, 429, 500] {
        let (url, server) = mock_api(vec![(status, "private-provider-message".into())]);
        let error = fetch_projects(&reqwest::blocking::Client::new(), "test-token", &url)
            .err()
            .unwrap();
        assert!(!error.contains("private-provider-message"));
        server.join().unwrap();
    }
}

#[test]
fn project_page_fits_supported_sizes_and_states() {
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        for state in 0..5 {
            let mut page = GoogleOAuth {
                attempted: true,
                branding_attempted: true,
                loaded: state > 0,
                error: (state == 0).then(|| {
                    "Sign in to Google Cloud with gcloud auth login, then refresh projects.".into()
                }),
                ..Default::default()
            };
            if state > 1 {
                page.projects.push(Project {
                    project_id: "a-project-with-a-long-id-12345".into(),
                    display_name: "A project with a long display name for layout checks".into(),
                    name: "projects/123456789012".into(),
                });
            }
            if state >= 3 {
                page.selected = Some(page.projects[0].project_id.clone());
                page.client_project = page.selected.clone();
                page.clients = vec![OAuthClient {
                    client_id:
                        "123456789012-examplelongclientidentifier.apps.googleusercontent.com".into(),
                    display_name: "An OAuth client with a long name".into(),
                    display_type: "CLIENT_TYPE_WEB_APPLICATION".into(),
                    creation_time: "2026-10-02T12:00:00Z".into(),
                }];
            }
            if state == 4 {
                page.icons.insert_image(
                    page.projects[0].project_id.clone(),
                    egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
                );
            }
            let ctx = egui::Context::default();
            crate::style::configure(&ctx);
            for _ in 0..2 {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            page.ui(ui);
                            assert!(
                                ui.min_rect().right() <= width,
                                "Overflow at {width}px in state {state}"
                            );
                        });
                    },
                );
            }
        }
    }
}
