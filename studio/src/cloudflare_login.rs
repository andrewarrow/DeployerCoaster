use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

const LOGIN_URL: &str = "https://dash.cloudflare.com/login";
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
                        "Email and password filled. Finish signing in in your private browser window.".into()
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
                ui.label("Opening private Cloudflare login…");
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

// Use native setters and input/change events so React receives the new values.
// Never submit, reuse challenge tokens, or fill outside the intended origin/form.
fn fill_script(email: &str, password: &str) -> String {
    let values = json!([email, password]);
    format!(
        r#"(() => {{
        if (location.origin !== 'https://dash.cloudflare.com' || location.pathname !== '/login') return 'waiting';
        const form = document.querySelector('form[data-testid="login-form"]');
        if (!form) return 'waiting';
        const email = form.querySelector('input[name="email"][type="email"]');
        const password = form.querySelector('input[name="password"][type="password"]');
        if (!email || !password || email.disabled || password.disabled) return 'waiting';
        const values = {values};
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
        for (const [index, input] of [email, password].entries()) {{
            setter.call(input, values[index]);
            input.dispatchEvent(new Event('input', {{bubbles: true}}));
            input.dispatchEvent(new Event('change', {{bubbles: true}}));
        }}
        return email.value === values[0] && password.value === values[1] ? 'filled' : 'waiting';
    }})()"#
    )
}

#[cfg(target_os = "macos")]
fn open_login(email: &str, password: &str) -> Result<(), String> {
    let browser = run_jxa(
        r#"
        ObjC.import('AppKit');
        const url = $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL($.NSURL.URLWithString('https://dash.cloudflare.com/login'));
        if (!url) throw Error('No default browser');
        JSON.stringify({id: ObjC.unwrap($.NSBundle.bundleWithURL(url).bundleIdentifier), path: ObjC.unwrap(url.path)});
    "#,
    )?;
    let browser: Value = serde_json::from_str(browser.trim())
        .map_err(|_| "Could not identify your default browser.".to_owned())?;
    let script = fill_script(email, password);
    match browser["id"].as_str().unwrap_or_default() {
        "org.mozilla.firefox" | "org.mozilla.firefoxdeveloperedition" | "org.mozilla.nightly" => {
            let path = browser["path"].as_str().ok_or("Could not locate Firefox.")?;
            firefox_login(path, &script)
        }
        id @ ("com.google.Chrome" | "com.google.Chrome.beta" | "com.google.Chrome.canary" |
              "org.chromium.Chromium" | "com.brave.Browser" | "com.microsoft.edgemac") => {
            let id = serde_json::to_string(id).unwrap();
            let script = serde_json::to_string(&script).unwrap();
            let result = run_jxa(&format!(r#"
                const browser = Application({id});
                const window = browser.Window({{mode: 'incognito'}});
                browser.windows.push(window);
                if (window.mode() !== 'incognito') throw Error('Private window unavailable');
                const tab = window.activeTab();
                tab.url = '{LOGIN_URL}';
                browser.activate();
                for (let i = 0; i < 100; i++) {{
                    delay(0.5);
                    if (tab.execute({{javascript: {script}}}) === 'filled') {{ 'filled'; break; }}
                    if (i === 99) throw Error('Login form unavailable');
                }}
            "#));
            result.map(|_| ()).map_err(|_| "Could not fill the private browser window. Allow macOS Automation access and enable View → Developer → Allow JavaScript from Apple Events in your browser, then try again.".into())
        }
        _ => Err("Private login autofill supports Firefox, Chrome, Brave, and Edge on macOS. Choose one as your default browser, then try again.".into()),
    }
}

#[cfg(not(target_os = "macos"))]
fn open_login(_email: &str, _password: &str) -> Result<(), String> {
    Err("Private browser login is currently supported on macOS.".into())
}

#[cfg(target_os = "macos")]
fn run_jxa(script: &str) -> Result<String, String> {
    // Script goes through stdin: credentials never appear in process arguments or files.
    let mut child = Command::new("/usr/bin/osascript")
        .env_remove("CF_GREEN")
        .args(["-l", "JavaScript", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Could not start browser automation.".to_owned())?;
    let result = (|| {
        child
            .stdin
            .take()
            .ok_or("Could not contact browser automation.")?
            .write_all(script.as_bytes())
            .map_err(|_| "Could not contact browser automation.")?;
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| "Browser automation stopped.")?
            {
                break status;
            }
            if Instant::now() >= deadline {
                return Err("Browser automation timed out.".into());
            }
            thread::sleep(Duration::from_millis(100));
        };
        if !status.success() {
            return Err("Could not automate your default browser.".into());
        }
        let mut output = String::new();
        child
            .stdout
            .take()
            .ok_or("Browser automation stopped.")?
            .read_to_string(&mut output)
            .map_err(|_| "Browser automation stopped.")?;
        Ok(output)
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn firefox_login(app_path: &str, script: &str) -> Result<(), String> {
    let profile = tempfile::tempdir().map_err(|_| "Could not create a private browser profile.")?;
    std::fs::write(profile.path().join("user.js"),
        "user_pref(\"browser.privatebrowsing.autostart\", true);\nuser_pref(\"signon.rememberSignons\", false);\nuser_pref(\"browser.shell.checkDefaultBrowser\", false);\nuser_pref(\"browser.aboutwelcome.enabled\", false);\n")
        .map_err(|_| "Could not configure the private browser profile.")?;
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map_err(|_| "Could not open browser automation.")?
        .port();
    // LaunchServices gives Firefox its own macOS identity and profile access.
    let mut child = Command::new("/usr/bin/open")
        .env_remove("CF_GREEN")
        .args([
            "-n",
            "-W",
            "-a",
            app_path,
            "--args",
            "--new-instance",
            "--profile",
        ])
        .arg(
            profile
                .path()
                .canonicalize()
                .map_err(|_| "Could not locate the private browser profile.")?,
        )
        .args([
            "--remote-debugging-port",
            &port.to_string(),
            "--private-window",
            LOGIN_URL,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Could not open a private Firefox window.")?;
    let result = fill_firefox(port, script, false);
    // Keep the isolated profile until this browser exits, then remove it.
    // No credentials are written to it, and password saving is disabled.
    thread::spawn(move || {
        let _ = child.wait();
        drop(profile);
    });
    result
}

fn fill_firefox(port: u16, script: &str, close_browser: bool) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    let mut socket = loop {
        if Instant::now() >= deadline {
            return Err("Firefox automation did not start. Try Login again.".into());
        }
        if let Ok(stream) =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500))
        {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            if let Ok((socket, _)) =
                tungstenite::client(format!("ws://127.0.0.1:{port}/session"), stream)
            {
                break socket;
            }
        }
        thread::sleep(Duration::from_millis(200));
    };
    let result = (|| {
        let mut id = 0;
        bidi(
            &mut socket,
            &mut id,
            "session.new",
            json!({"capabilities": {}}),
        )?;
        while Instant::now() < deadline {
            let tree = bidi(
                &mut socket,
                &mut id,
                "browsingContext.getTree",
                json!({"maxDepth": 0}),
            )?;
            for context in tree["contexts"].as_array().into_iter().flatten() {
                let result = bidi(
                    &mut socket,
                    &mut id,
                    "script.evaluate",
                    json!({
                        "expression": script, "target": {"context": context["context"]}, "awaitPromise": false
                    }),
                )?;
                if result["result"]["value"] == "filled" {
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
        Err("Cloudflare's login fields did not become available. Complete any browser challenge, then try Login again.".into())
    })();
    if close_browser {
        let _ = socket.send(tungstenite::Message::Text(
            json!({"id": 999999, "method": "browser.close", "params": {}})
                .to_string()
                .into(),
        ));
    }
    let _ = socket.close(None);
    result
}

fn bidi(
    socket: &mut tungstenite::WebSocket<TcpStream>,
    id: &mut u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    *id += 1;
    socket
        .send(tungstenite::Message::Text(
            json!({"id": *id, "method": method, "params": params})
                .to_string()
                .into(),
        ))
        .map_err(|_| "Could not communicate with Firefox automation.".to_owned())?;
    loop {
        let message = socket
            .read()
            .map_err(|_| "Firefox automation stopped responding.".to_owned())?;
        if let tungstenite::Message::Text(text) = message {
            let response: Value = serde_json::from_str(&text)
                .map_err(|_| "Invalid browser automation response.".to_owned())?;
            if response["id"] == *id {
                if response["type"] == "success" {
                    return Ok(response["result"].clone());
                }
                return Err(
                    "Firefox could not fill the login form. Try again after the page loads.".into(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bidi_skips_events_and_keeps_provider_errors_private() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let _ = socket.read().unwrap();
            for response in [
                json!({"type": "event", "method": "unrelated"}),
                json!({"type": "success", "id": 1, "result": {"value": "ok"}}),
            ] {
                socket
                    .send(tungstenite::Message::Text(response.to_string().into()))
                    .unwrap();
            }
            let _ = socket.read().unwrap();
            socket
                .send(tungstenite::Message::Text(
                    json!({
                        "type": "error", "id": 2, "message": "sensitive provider details"
                    })
                    .to_string()
                    .into(),
                ))
                .unwrap();
        });
        let (mut socket, _) = tungstenite::client(
            format!("ws://{address}/session"),
            TcpStream::connect(address).unwrap(),
        )
        .unwrap();
        let mut id = 0;
        assert_eq!(
            bidi(&mut socket, &mut id, "test", json!({})).unwrap()["value"],
            "ok"
        );
        let error = bidi(&mut socket, &mut id, "test", json!({})).unwrap_err();
        assert!(!error.contains("sensitive"));
        server.join().unwrap();
    }

    #[test]
    #[ignore = "Requires Firefox installed on macOS; runs an isolated headless browser against a local fixture"]
    fn firefox_fills_fields_and_events_without_submitting() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (stop, stopped) = mpsc::channel();
        let server = thread::spawn(move || {
            while stopped.try_recv().is_err() {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request);
                    let body = r#"<form data-testid="login-form"><input type="email" name="email"><input type="password" name="password"><button>Sign in</button></form><script>
                        window.events = [];
                        document.addEventListener('input', e => events.push('input:' + e.target.name));
                        document.addEventListener('change', e => events.push('change:' + e.target.name));
                        document.querySelector('form').addEventListener('submit', e => { window.submitted = true; e.preventDefault(); });
                    </script>"#;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                } else {
                    thread::sleep(Duration::from_millis(20));
                }
            }
        });
        let profile = tempfile::tempdir().unwrap();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut child = Command::new("/usr/bin/open")
            .args([
                "-n",
                "-W",
                "-a",
                "/Applications/Firefox.app",
                "--args",
                "--headless",
                "--new-instance",
                "--profile",
            ])
            .arg(profile.path().canonicalize().unwrap())
            .args([
                "--remote-debugging-port",
                &port.to_string(),
                "--private-window",
                &format!("http://{address}/login"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let guarded_script = fill_script("support@example.com", "quote\"'\\ symbols $() 🦊");
        let script = guarded_script
            .replace("https://dash.cloudflare.com", &format!("http://{address}"))
            .replace("return email.value === values[0]", "return !window.submitted && events.join(',') === 'input:email,change:email,input:password,change:password' && email.value === values[0]");
        // The production script must leave this non-Cloudflare page untouched.
        let script = format!(
            "(() => {{ if (location.pathname !== '/login') return 'waiting'; if (({guarded_script}) !== 'waiting' || events.length !== 0) return 'blocked'; return {script}; }})()"
        );
        let result = fill_firefox(port, &script, true);
        let _ = child.kill();
        let _ = child.wait();
        let _ = stop.send(());
        server.join().unwrap();
        result.unwrap();
    }
}
