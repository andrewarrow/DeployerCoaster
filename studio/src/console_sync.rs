use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use oauth2::CsrfToken;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SYNC_PATH: &str = "/studio-console-sync";
const ICONS_PATH: &str = "/icons";
const SYNC_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Snapshot {
    pub developer_id: String,
    pub apps: Vec<crate::play_console::ConsoleApp>,
}

pub(crate) struct SyncJob {
    receiver: Receiver<Result<Snapshot, String>>,
    cancelled: Arc<AtomicBool>,
    received: bool,
}

impl SyncJob {
    pub(crate) fn start(ctx: &egui::Context) -> Result<Self, String> {
        let extension_root = crate::console_extension::install()?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "Could not start the Play Console sync bridge.".to_owned())?;
        let token = CsrfToken::new_random_len(32);
        let browser_url = format!(
            "http://127.0.0.1:{}/studio-console-sync#token={}",
            listener
                .local_addr()
                .map_err(|_| "Could not start the Play Console sync bridge.".to_owned())?
                .port(),
            token.secret()
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::channel();
        let repaint_context = ctx.clone();
        thread::spawn(move || {
            let result = serve(
                listener,
                &token,
                &worker_cancelled,
                SYNC_TIMEOUT,
                extension_root,
            );
            let _ = sender.send(result);
            repaint_context.request_repaint();
        });

        if webbrowser::open(&browser_url).is_err() {
            cancelled.store(true, Ordering::Relaxed);
            return Err("Could not open the Play Console sync page in your browser.".to_owned());
        }

        Ok(Self {
            receiver,
            cancelled,
            received: false,
        })
    }

    pub(crate) fn poll(&mut self) -> Option<Result<Snapshot, String>> {
        if self.received {
            return None;
        }
        match self.receiver.try_recv() {
            Ok(result) => {
                self.received = true;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.received = true;
                Some(Err(
                    "The Play Console sync bridge stopped unexpectedly.".to_owned()
                ))
            }
        }
    }
}

impl Drop for SyncJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn serve(
    listener: TcpListener,
    token: &CsrfToken,
    cancelled: &AtomicBool,
    timeout: Duration,
    extension_root: PathBuf,
) -> Result<Snapshot, String> {
    listener
        .set_nonblocking(true)
        .map_err(|_| "Could not listen for Play Console sync data.".to_owned())?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Play Console sync was cancelled.".to_owned());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                match handle_request(&mut stream, token, &extension_root) {
                    Ok(Some(snapshot)) => return Ok(snapshot),
                    Ok(None) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err("Could not receive Play Console sync data.".to_owned()),
        }
    }
    Err("Play Console sync timed out. Connect again to retry.".to_owned())
}

fn handle_request(
    stream: &mut TcpStream,
    token: &CsrfToken,
    extension_root: &PathBuf,
) -> Result<Option<Snapshot>, String> {
    let request = match read_request(stream) {
        Ok(request) => request,
        Err(RequestError::TooLarge) => {
            response(
                stream,
                "413 Payload Too Large",
                "Request is too large.",
                None,
            );
            return Ok(None);
        }
        Err(RequestError::Invalid) => {
            response(stream, "400 Bad Request", "Invalid request.", None);
            return Ok(None);
        }
    };
    let mut request_parts = request.request_line.split_whitespace();
    let (method, path) = match (
        request_parts.next(),
        request_parts.next(),
        request_parts.next(),
        request_parts.next(),
    ) {
        (Some(method), Some(path), Some("HTTP/1.1" | "HTTP/1.0"), None) => (method, path),
        _ => {
            response(stream, "400 Bad Request", "Invalid request.", None);
            return Ok(None);
        }
    };

    if method == "GET" && path == SYNC_PATH {
        page_response(stream, extension_root);
        return Ok(None);
    }
    if method == "OPTIONS" && path == ICONS_PATH {
        let origin = request.headers.get("origin").map(String::as_str);
        if !origin.is_some_and(is_extension_origin) {
            response(stream, "403 Forbidden", "Extension origin required.", None);
        } else {
            options_response(stream, origin.unwrap());
        }
        return Ok(None);
    }
    if method != "POST" || path != ICONS_PATH {
        response(stream, "404 Not Found", "Not found.", None);
        return Ok(None);
    }

    let origin = request.headers.get("origin").map(String::as_str);
    if origin.is_some_and(|origin| !is_extension_origin(origin)) {
        response(stream, "403 Forbidden", "Extension origin required.", None);
        return Ok(None);
    }
    let authorization = request.headers.get("authorization").map(String::as_str);
    let expected = format!("Bearer {}", token.secret());
    if !authorization
        .is_some_and(|received| constant_time_eq(received.as_bytes(), expected.as_bytes()))
    {
        response(
            stream,
            "401 Unauthorized",
            "Invalid sync token.",
            origin.filter(|o| is_extension_origin(o)),
        );
        return Ok(None);
    }
    if request.headers.get("content-type").is_none_or(|value| {
        value
            .split(';')
            .next()
            .is_none_or(|media_type| !media_type.trim().eq_ignore_ascii_case("application/json"))
    }) {
        response(
            stream,
            "400 Bad Request",
            "JSON content type required.",
            origin,
        );
        return Ok(None);
    }

    match parse_snapshot(&request.body) {
        Ok(snapshot) => {
            let body = format!("{{\"received\":{}}}", snapshot.apps.len());
            let cors = origin.map_or_else(String::new, |origin| {
                format!("Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\n")
            });
            response_with_headers(
                stream,
                "200 OK",
                body.as_bytes(),
                &cors,
                Some("application/json; charset=utf-8"),
            );
            Ok(Some(snapshot))
        }
        Err(message) => {
            response(stream, "400 Bad Request", &message, origin);
            Ok(None)
        }
    }
}

fn parse_snapshot(body: &[u8]) -> Result<Snapshot, String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| "The Play Console app metadata is not valid JSON.".to_owned())?;
    let developer_id = value
        .get("developer_id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty() && id.len() <= 32 && id.bytes().all(|byte| byte.is_ascii_digit())
        })
        .ok_or_else(|| "The Play Console developer ID is invalid.".to_owned())?;
    let apps = crate::play_console::parse_console_apps(&value)?;
    Ok(Snapshot {
        developer_id: developer_id.to_owned(),
        apps,
    })
}

struct Request {
    request_line: String,
    headers: std::collections::HashMap<String, String>,
    body: Vec<u8>,
}

enum RequestError {
    Invalid,
    TooLarge,
}

fn read_request(stream: &mut TcpStream) -> Result<Request, RequestError> {
    let mut bytes = Vec::with_capacity(4096);
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        if bytes.len() >= MAX_HEADERS + 4 {
            return Err(RequestError::TooLarge);
        }
        let read = stream
            .read(&mut buffer)
            .map_err(|_| RequestError::Invalid)?;
        if read == 0 {
            return Err(RequestError::Invalid);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if position > MAX_HEADERS {
                return Err(RequestError::TooLarge);
            }
            break position + 4;
        }
    };
    let header_text =
        std::str::from_utf8(&bytes[..header_end - 4]).map_err(|_| RequestError::Invalid)?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().ok_or(RequestError::Invalid)?.to_owned();
    let mut headers = std::collections::HashMap::new();
    let mut content_length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(RequestError::Invalid)?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || headers.contains_key(&name) {
            return Err(RequestError::Invalid);
        }
        let value = value.trim().to_owned();
        if name == "transfer-encoding" {
            return Err(RequestError::Invalid);
        }
        if name == "content-length" {
            content_length = Some(value.parse::<usize>().map_err(|_| RequestError::Invalid)?);
        }
        headers.insert(name, value);
    }
    let expected_length = content_length.unwrap_or(0);
    if expected_length > MAX_BODY {
        return Err(RequestError::TooLarge);
    }
    let mut body = bytes[header_end..].to_vec();
    if body.len() > expected_length {
        body.truncate(expected_length);
    }
    while body.len() < expected_length {
        let read = stream
            .read(&mut buffer)
            .map_err(|_| RequestError::Invalid)?;
        if read == 0 {
            return Err(RequestError::Invalid);
        }
        let take = read.min(expected_length - body.len());
        body.extend_from_slice(&buffer[..take]);
    }
    Ok(Request {
        request_line,
        headers,
        body,
    })
}

fn page_response(stream: &mut TcpStream, extension_root: &PathBuf) {
    let chrome_path = html_escape(&extension_root.join("chrome").display().to_string());
    let firefox_path = html_escape(
        &extension_root
            .join("firefox/manifest.json")
            .display()
            .to_string(),
    );
    let page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Connect Play Console</title></head><body><main><h1>Connect Play Console</h1><p id=\"studio-console-status\">Install the DeployerCoaster bridge extension, then reload this page.</p><p>Chromium: open Extensions, enable Developer mode, then choose Load unpacked and select <code>{chrome_path}</code>.</p><p>Firefox: open <code>about:debugging</code>, choose Load Temporary Add-on, then select <code>{firefox_path}</code>.</p></main></body></html>"
    );
    let headers = "Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; base-uri 'none'; form-action 'none'\r\n";
    response_with_headers(stream, "200 OK", page.as_bytes(), headers, None);
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn options_response(stream: &mut TcpStream, origin: &str) {
    let headers = format!(
        "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: authorization, content-type\r\nVary: Origin\r\n"
    );
    response_with_headers(stream, "204 No Content", b"", &headers, None);
}

fn response(stream: &mut TcpStream, status: &str, body: &str, origin: Option<&str>) {
    let cors = origin.map_or_else(String::new, |origin| {
        format!("Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\n")
    });
    response_with_headers(
        stream,
        status,
        body.as_bytes(),
        &cors,
        Some("text/plain; charset=utf-8"),
    );
}

fn response_with_headers(
    stream: &mut TcpStream,
    status: &str,
    body: &[u8],
    extra_headers: &str,
    content_type: Option<&str>,
) {
    let content_type =
        content_type.map_or_else(String::new, |value| format!("Content-Type: {value}\r\n"));
    let header = format!(
        "HTTP/1.1 {status}\r\n{content_type}{extra_headers}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn is_extension_origin(origin: &str) -> bool {
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    matches!(scheme, "chrome-extension" | "moz-extension")
        && !authority.is_empty()
        && !authority.contains('/')
        && !authority
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::atomic::AtomicBool;

    fn test_server() -> (
        SocketAddr,
        Arc<AtomicBool>,
        thread::JoinHandle<Result<Snapshot, String>>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::spawn(move || {
            serve(
                listener,
                &CsrfToken::new("test-token".to_owned()),
                &worker_cancelled,
                Duration::from_secs(3),
                PathBuf::from("/tmp/deployercoaster-test-extension"),
            )
        });
        (address, cancelled, worker)
    }

    fn post(address: SocketAddr, authorization: &str, body: &[u8]) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "POST /icons HTTP/1.1\r\nHost: localhost\r\nAuthorization: {authorization}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nOrigin: chrome-extension://test\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        read_response(&mut stream)
    }

    fn read_response(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 2048];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => bytes.extend_from_slice(&buffer[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("could not read test response: {error}"),
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn accepts_sanitized_sync_payload() {
        let (address, _, worker) = test_server();
        let body = br#"{"developer_id":"12345","apps":[{"display_name":"Example","package_name":"example.app","icon_url":"https://lh3.googleusercontent.com/icon"}],"cookies":"ignored"}"#;
        let response = post(address, "Bearer test-token", body);
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response:?}");
        let snapshot = worker.join().unwrap().unwrap();
        assert_eq!(snapshot.developer_id, "12345");
        assert_eq!(snapshot.apps.len(), 1);
        assert_eq!(snapshot.apps[0].package_name, "example.app");
    }

    #[test]
    fn rejects_forged_token_then_accepts_valid_request() {
        let (address, _, worker) = test_server();
        let body = br#"{"developer_id":"123","apps":[]}"#;
        let response = post(address, "Bearer forged", body);
        assert!(
            response.starts_with("HTTP/1.1 401 Unauthorized"),
            "{response:?}"
        );
        let response = post(address, "Bearer test-token", body);
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response:?}");
        assert!(worker.join().unwrap().is_ok());
    }

    #[test]
    fn rejects_oversized_body_without_ending_job() {
        let (address, _, worker) = test_server();
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "POST /icons HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        )
        .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let response = read_response(&mut stream);
        assert!(
            response.starts_with("HTTP/1.1 413 Payload Too Large"),
            "{response:?}"
        );
        drop(stream);
        let response = post(
            address,
            "Bearer test-token",
            br#"{"developer_id":"9","apps":[]}"#,
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response:?}");
        assert!(worker.join().unwrap().is_ok());
    }

    #[test]
    fn rejects_invalid_snapshot_fields() {
        assert!(parse_snapshot(br#"{"developer_id":"x","apps":[]}"#).is_err());
        assert!(parse_snapshot(br#"{"developer_id":"123","apps":[{}]}"#).is_err());
    }

    #[test]
    fn cancellation_stops_idle_listener_quickly() {
        let (address, cancelled, worker) = test_server();
        let start = Instant::now();
        cancelled.store(true, Ordering::Relaxed);
        let result = worker.join().unwrap();
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        drop(TcpStream::connect(address));
    }
}
