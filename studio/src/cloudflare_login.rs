use serde_json::{Value, json};
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

const LOGIN_URL: &str = "https://dash.cloudflare.com/login";
const BROWSER_API: &str = "http://127.0.0.1:9001";
const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(crate) struct CloudflareLogin {
    job: Option<Receiver<Result<(), String>>>,
    message: Option<Result<String, String>>,
}

impl CloudflareLogin {
    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn start(&mut self, email: String, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        self.message = None;
        let (sender, receiver) = mpsc::channel();
        self.job = Some(receiver);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| {
                let password = std::env::var("CF_GREEN")
                    .ok()
                    .filter(|value| !value.is_empty())
                    .ok_or("Set CF_GREEN in the app's environment, then try Login again.")?;
                open_login(&email, &password)
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(result) => {
                    self.job = None;
                    self.message = Some(result.map(|()| {
                        "Email and password filled. Finish signing in in wkdomains.".into()
                    }));
                }
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.message = Some(Err("Browser login stopped. Try again.".into()));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Opening Cloudflare login in wkdomains…");
            });
        }
        if let Some(message) = &self.message {
            match message {
                Ok(message) => {
                    ui.label(message);
                }
                Err(message) => {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                }
            }
        }
    }
}

fn open_login(email: &str, password: &str) -> Result<(), String> {
    prepare_login(BROWSER_API, email, password)
}

fn is_login_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("dash.cloudflare.com")
            && url.port_or_known_default() == Some(443)
            && url.path() == "/login"
    })
}

struct BrowserApi {
    client: reqwest::blocking::Client,
    base: String,
}

impl BrowserApi {
    fn new(base: &str) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "Could not connect to the wkdomains browser.".to_owned())?;
        Ok(Self {
            client,
            base: base.into(),
        })
    }

    fn request(&self, path: &str, body: Option<Value>) -> Result<Value, String> {
        let url = format!("{}{path}", self.base);
        let request = match body {
            Some(body) => self.client.post(url).json(&body),
            None => self.client.get(url),
        };
        let response = request.send()
            .map_err(|_| "Could not reach wkdomains. Open the browser with its HTTP API on port 9001, then try Login again.".to_owned())?;
        if !response.status().is_success() {
            return Err("wkdomains could not complete the browser action. Check the page and try Login again.".into());
        }
        // Never include provider response bodies or credential values in errors.
        let result: Value = response
            .json()
            .map_err(|_| "wkdomains returned an invalid browser response.".to_owned())?;
        if result.get("ok") == Some(&Value::Bool(false)) {
            return Err("wkdomains could not complete the browser action. Check the page and try Login again.".into());
        }
        Ok(result)
    }

    fn verify_page(&self) -> Result<(), String> {
        let page = self.request("/api/v1/page", None)?;
        verify_login_page(&page)
    }

    fn action(&self, body: Value) -> Result<(), String> {
        self.verify_page()?;
        let result = self.request("/api/v1/action", Some(body))?;
        if result["ok"] != true {
            return Err(
                "wkdomains did not confirm the browser action. Check the page and try again."
                    .into(),
            );
        }
        verify_login_page(&result)
    }
}

fn verify_login_page(page: &Value) -> Result<(), String> {
    if page["url"].as_str().is_some_and(is_login_url) {
        Ok(())
    } else {
        Err("The wkdomains browser left Cloudflare's login page. Try Login again.".into())
    }
}

fn prepare_login(base: &str, email: &str, password: &str) -> Result<(), String> {
    let browser = BrowserApi::new(base)?;
    let navigation = browser.request(
        "/api/v1/navigate",
        Some(json!({"url": LOGIN_URL, "mode": "hard"})),
    )?;
    verify_login_page(&navigation)?;
    let deadline = Instant::now() + TIMEOUT;
    let mut selected_another_profile = false;
    while Instant::now() < deadline {
        let snapshot = browser.request("/api/v1/snapshot", None)?;
        verify_login_page(&snapshot)?;
        let elements = snapshot["elements"].as_array();
        let elements = elements.map(Vec::as_slice).unwrap_or_default();
        if !selected_another_profile
            && elements.iter().any(|element| {
                element["role"] == "button"
                    && (element["label"] == "Sign in with another profile"
                        || element["text"] == "Sign in with another profile")
                    && element["disabled"] != true
            })
        {
            browser.action(json!({"type": "click", "role": "button", "name": "Sign in with another profile", "exact": true}))?;
            selected_another_profile = true;
            continue;
        }
        let field_exists = |name: &str| {
            elements.iter().any(|element| {
                element["tag"] == "input" && element["name"] == name && element["disabled"] != true
            })
        };
        if field_exists("email") && field_exists("password") {
            for (name, value) in [("email", email), ("password", password)] {
                browser.action(json!({
                    "type": "fill",
                    "selector": format!("form[data-testid=\"login-form\"] input[name=\"{name}\"]"),
                    "value": value
                }))?;
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err("Cloudflare's login form did not appear in wkdomains. Complete any verification in the browser, then try Login again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
    };

    struct Step {
        method: &'static str,
        path: &'static str,
        body: Option<Value>,
        response: Value,
        status: u16,
    }

    fn step(
        method: &'static str,
        path: &'static str,
        body: Option<Value>,
        response: Value,
    ) -> Step {
        Step {
            method,
            path,
            body,
            response,
            status: 200,
        }
    }

    fn serve(steps: Vec<Step>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for step in steps {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with(&format!("{} {} ", step.method, step.path)));
                let mut length = 0;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                if let Some(expected) = step.body {
                    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), expected);
                } else {
                    assert!(body.is_empty());
                }
                let body = step.response.to_string();
                write!(stream, "HTTP/1.1 {} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", step.status, body.len()).unwrap();
            }
        });
        (format!("http://{address}"), server)
    }

    fn navigation() -> Step {
        step(
            "POST",
            "/api/v1/navigate",
            Some(json!({"url": LOGIN_URL, "mode": "hard"})),
            json!({"ok": true, "url": LOGIN_URL}),
        )
    }

    fn page() -> Step {
        step("GET", "/api/v1/page", None, json!({"url": LOGIN_URL}))
    }

    fn fill(name: &str, value: &str) -> Step {
        step(
            "POST",
            "/api/v1/action",
            Some(json!({
                "type": "fill", "selector": format!("form[data-testid=\"login-form\"] input[name=\"{name}\"]"), "value": value
            })),
            json!({"ok": true, "url": LOGIN_URL}),
        )
    }

    fn form() -> Step {
        step(
            "GET",
            "/api/v1/snapshot",
            None,
            json!({"url": LOGIN_URL, "elements": [
                {"tag":"input", "name":"email", "type":"email"},
                {"tag":"input", "name":"password", "type":"text"}
            ]}),
        )
    }

    #[test]
    fn browser_api_selects_another_profile_and_fills_without_submitting() {
        let password = "fake\"$()!password";
        let (base, server) = serve(vec![
            navigation(),
            step(
                "GET",
                "/api/v1/snapshot",
                None,
                json!({"url": LOGIN_URL, "elements": [
                    {"role":"button", "label":"Sign in with another profile"}
                ]}),
            ),
            page(),
            step(
                "POST",
                "/api/v1/action",
                Some(
                    json!({"type":"click", "role":"button", "name":"Sign in with another profile", "exact":true}),
                ),
                json!({"ok":true, "url":LOGIN_URL}),
            ),
            form(),
            page(),
            fill("email", "support@example.com"),
            page(),
            fill("password", password),
        ]);
        prepare_login(&base, "support@example.com", password).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn browser_api_stops_before_filling_if_the_page_changes() {
        let (base, server) = serve(vec![
            navigation(),
            form(),
            step(
                "GET",
                "/api/v1/page",
                None,
                json!({"url":"https://dash.cloudflare.com.evil.example/login"}),
            ),
        ]);
        let error = prepare_login(&base, "support@example.com", "fake-password").unwrap_err();
        assert!(error.contains("left Cloudflare"));
        server.join().unwrap();
    }

    #[test]
    fn browser_api_errors_do_not_expose_credentials_or_response_details() {
        let mut password_step = fill("password", "fake-secret");
        password_step.response =
            json!({"ok":false, "error":"fake-secret sensitive-provider-details"});
        let (base, server) = serve(vec![
            navigation(),
            form(),
            page(),
            fill("email", "support@example.com"),
            page(),
            password_step,
        ]);
        let error = prepare_login(&base, "support@example.com", "fake-secret").unwrap_err();
        assert!(!error.contains("fake-secret"));
        assert!(!error.contains("sensitive-provider-details"));
        server.join().unwrap();
    }

    #[test]
    #[ignore = "Navigates the running wkdomains browser and fills Cloudflare using CF_GREEN; never submits"]
    fn running_browser_fills_cloudflare() {
        let password = std::env::var("CF_GREEN").expect("CF_GREEN must be set");
        open_login("support@cubacadabra.com", &password).unwrap();
    }
}
