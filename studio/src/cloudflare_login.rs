use serde_json::json;
use std::{
    io::{Read, Write},
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
                        "Email and password filled. Finish signing in in your browser.".into()
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
                ui.label("Opening Cloudflare login…");
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

// Use the existing default browser session and native controls; never launch
// a separate profile or enable a browser debugging protocol.
fn open_login(email: &str, password: &str) -> Result<(), String> {
    webbrowser::open(LOGIN_URL)
        .map_err(|_| "Could not open Cloudflare in your default browser.".to_owned())?;
    prepare_login(email, password)
}

#[cfg(target_os = "macos")]
fn prepare_login(email: &str, password: &str) -> Result<(), String> {
    let script = format!(
        r#"
        {ui_script}
        ObjC.import('AppKit');
        const url = $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL($.NSURL.URLWithString('{LOGIN_URL}'));
        if (!url) throw Error('Default browser unavailable');
        const id = ObjC.unwrap($.NSBundle.bundleWithURL(url).bundleIdentifier);
        const se = Application('System Events');
        let result = 'unavailable';
        try {{
            const processes = se.processes.whose({{bundleIdentifier: id}})();
            if (processes.length) {{
                processes[0].frontmost = true;
                result = prepareCloudflareLogin(se, processes[0], {credentials});
            }}
        }} catch (error) {{
            result = (error.errorNumber === -25211 || error.errorNumber === -1743)
                ? 'permission' : 'unavailable';
        }}
        result;
    "#,
        ui_script = include_str!("cloudflare_login.js"),
        credentials = json!([email, password])
    );
    match run_jxa(&script)?.trim() {
        "filled" => Ok(()),
        "permission" => Err("Cloudflare opened. Enable Accessibility and Automation access for DeployerCoaster in System Settings → Privacy & Security to select another profile and fill the fields. You can also do this manually in the browser.".into()),
        "focus" => Err("Cloudflare opened. Keep the browser in front while Login selects the profile and fills the fields.".into()),
        _ => Err("Cloudflare opened, but its login controls were unavailable. Click ‘Sign in with another profile’ in the browser, or complete any verification and try Login again.".into()),
    }
}

#[cfg(not(target_os = "macos"))]
fn prepare_login(_email: &str, _password: &str) -> Result<(), String> {
    Err("Cloudflare opened. Select ‘Sign in with another profile’ and enter your credentials in the browser. Automatic form preparation is currently supported on macOS.".into())
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn native_login_selects_other_profile_and_types_only_in_the_verified_form() {
        let script = format!(
            r#"
            {ui_script}
            function scenario(url, frontmost, showSelector) {{
                let selected = false, clicks = 0, focused = null;
                const typed = [];
                function element(attributes) {{
                    const item = {{
                        attributes: {{byName: name => ({{value: () => attributes[name] || ''}})}}
                    }};
                    Object.defineProperty(item, 'focused', {{
                        get: () => () => focused === item,
                        set: value => {{ if (value) focused = item; }}
                    }});
                    return item;
                }}
                const email = element({{AXRole:'AXTextField', AXDescription:'Email'}});
                const password = element({{AXRole:'AXTextField', AXDescription:'Password', AXSubrole:'AXSecureTextField'}});
                const button = element({{AXRole:'AXButton', AXTitle:'Sign in with another profile'}});
                button.click = () => {{ selected = true; clicks++; }};
                const area = element({{AXRole:'AXWebArea', AXURL:url}});
                area.entireContents = () => showSelector && !selected ? [button] : [email, password];
                const window = {{entireContents: () => [area]}};
                const process = {{frontmost: () => frontmost, windows:[window]}};
                const se = {{keystroke: (text, options) => {{
                    if (!options) typed.push([focused === email ? 'email' : 'password', text]);
                }}}};
                const result = prepareCloudflareLogin(se, process, ['support@example.com', 'fake"$()!password']);
                return {{result, clicks, typed}};
            }}
            // Replace the automation delay; no real browser or OS controls are used.
            delay = () => {{}};
            JSON.stringify([
                scenario('https://dash.cloudflare.com/login', true, true),
                scenario('https://dash.cloudflare.com/login?redirect=home', true, false),
                scenario('https://dash.cloudflare.com.evil.example/login', true, true),
                scenario('https://dash.cloudflare.com/login', false, true)
            ]);
        "#,
            ui_script = include_str!("cloudflare_login.js")
        );
        let output = run_jxa(&script).unwrap();
        let cases: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(cases[0]["result"], "filled");
        assert_eq!(cases[0]["clicks"], 1);
        assert_eq!(
            cases[0]["typed"],
            json!([
                ["email", "support@example.com"],
                ["password", "fake\"$()!password"]
            ])
        );
        assert_eq!(cases[1]["result"], "filled");
        assert_eq!(cases[1]["clicks"], 0);
        for index in [2, 3] {
            assert_eq!(cases[index]["clicks"], 0);
            assert_eq!(cases[index]["typed"], json!([]));
        }
        assert_eq!(cases[2]["result"], "unavailable");
        assert_eq!(cases[3]["result"], "focus");
    }
}
