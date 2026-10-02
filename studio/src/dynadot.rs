use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    fs,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

const API: &str = "https://api.dynadot.com";
const SETTINGS: &str = "https://www.dynadot.com/account/domain/setting/api.html";
const CREDENTIAL_FILE: &str = "dynadot-credentials.json";

// Never derive Debug for credentials or return provider response text in errors.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Credentials {
    api_key: String,
    secret_key: String,
}

impl Credentials {
    fn complete(&self) -> bool {
        !self.api_key.trim().is_empty() && !self.secret_key.trim().is_empty()
    }
}

#[derive(Default)]
pub(crate) struct Dynadot {
    draft: Credentials,
    saved: Option<Credentials>,
    domains: Vec<Domain>,
    job: Option<Receiver<Result<Vec<Domain>, String>>>,
    loaded: bool,
    error: Option<String>,
    feedback: Option<String>,
    search: String,
    hosting_search: String,
    help_texture: Option<(egui::Context, egui::TextureHandle)>,
    show_help: bool,
}

#[derive(Deserialize)]
struct Domain {
    domain_name: String,
    expiration_date: Option<i64>,
    status: Option<String>,
    renew_option: Option<String>,
}

#[derive(Deserialize)]
struct Response {
    code: u16,
    data: Option<DomainPage>,
}

#[derive(Deserialize)]
struct DomainPage {
    domain_info_list: Vec<Domain>,
    pagination_result: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    page: usize,
    has_next_page: serde_json::Value,
}

impl Pagination {
    fn has_next(&self) -> Result<bool, String> {
        match &self.has_next_page {
            serde_json::Value::Bool(value) => Ok(*value),
            serde_json::Value::String(value) if value.eq_ignore_ascii_case("yes") => Ok(true),
            serde_json::Value::String(value) if value.eq_ignore_ascii_case("no") => Ok(false),
            _ => Err("Dynadot returned invalid pagination data.".into()),
        }
    }
}

impl Dynadot {
    pub(crate) fn load() -> Self {
        let result = (|| {
            let path = crate::storage::credential_path(CREDENTIAL_FILE)?;
            match fs::read(path) {
                Ok(bytes) => serde_json::from_slice::<Credentials>(&bytes)
                    .map_err(|_| "Could not load Dynadot credentials.".to_owned()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(Credentials::default())
                }
                Err(_) => Err("Could not read Dynadot credentials.".into()),
            }
        })();
        match result {
            Ok(draft) => Self {
                saved: draft.complete().then(|| draft.clone()),
                draft,
                ..Self::default()
            },
            Err(error) => Self {
                error: Some(error),
                ..Self::default()
            },
        }
    }

    pub(crate) fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.try_recv() {
            Ok(result) => {
                self.job = None;
                match result {
                    Ok(domains) => {
                        self.domains = domains;
                        self.loaded = true;
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.error = Some("Domain sync stopped. Try refreshing again.".into());
            }
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        let Some(credentials) = self.saved.clone() else {
            return;
        };
        self.error = None;
        let (sender, receiver) = mpsc::channel();
        self.job = Some(receiver);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = fetch_domains(&credentials, API);
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn reset(&mut self) {
        // Dropping the receiver prevents a previous account's result from being applied.
        self.job = None;
        self.domains.clear();
        self.loaded = false;
        self.error = None;
        self.search.clear();
        self.hosting_search.clear();
    }

    pub(crate) fn settings_ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        ui.hyperlink_to("Open Dynadot API settings", SETTINGS);
        ui.add(egui::Label::new("In Tools > API, unlock your account and copy the API Production Key and its Secret Key. A restricted key with Domains read access also works.").wrap());
        ui.add(egui::Label::new("Enable the key and allow this computer's public IP if you use IP restrictions. Dynadot says changes can take 10 minutes.").wrap());
        self.help_ui(ui);
        ui.add_space(8.0);
        ui.label("API production key");
        ui.add(
            egui::TextEdit::singleline(&mut self.draft.api_key)
                .password(true)
                .desired_width(ui.available_width()),
        );
        ui.add_space(8.0);
        ui.label("Secret key");
        ui.add(
            egui::TextEdit::singleline(&mut self.draft.secret_key)
                .password(true)
                .desired_width(ui.available_width()),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.draft.complete(),
                    egui::Button::new("Save and connect").min_size(egui::vec2(140.0, 44.0)),
                )
                .clicked()
            {
                let credentials = Credentials {
                    api_key: self.draft.api_key.trim().to_owned(),
                    secret_key: self.draft.secret_key.trim().to_owned(),
                };
                let result = crate::storage::credential_path(CREDENTIAL_FILE).and_then(|path| {
                    let bytes = serde_json::to_vec(&credentials)
                        .map_err(|_| "Could not encode Dynadot credentials".to_owned())?;
                    crate::storage::save_credentials(&path, &bytes)
                });
                match result {
                    Ok(()) => {
                        self.reset();
                        self.draft = credentials.clone();
                        self.saved = Some(credentials);
                        self.feedback = Some("Credentials saved on this device.".into());
                        self.refresh(ui.ctx());
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            if ui
                .add_enabled(
                    self.saved.is_some(),
                    egui::Button::new("Disconnect").min_size(egui::vec2(100.0, 44.0)),
                )
                .clicked()
            {
                let result = crate::storage::credential_path(CREDENTIAL_FILE).and_then(|path| {
                    match fs::remove_file(path) {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(_) => Err("Could not remove Dynadot credentials.".to_owned()),
                    }
                });
                match result {
                    Ok(()) => {
                        self.reset();
                        self.saved = None;
                        self.draft = Credentials::default();
                        self.feedback = Some("Dynadot disconnected.".into());
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        });
        if let Some(feedback) = &self.feedback {
            ui.add(egui::Label::new(feedback).wrap());
        }
        if self.job.is_some() {
            ui.label("Checking connection…");
        } else if self.loaded {
            ui.label(format!("Connected · {} domains", self.domains.len()));
        }
        self.error_ui(ui);
    }

    fn help_ui(&mut self, ui: &mut egui::Ui) {
        if self
            .help_texture
            .as_ref()
            .is_none_or(|(ctx, _)| ctx != ui.ctx())
            && let Ok(image) =
                image::load_from_memory(include_bytes!("../assets/dynadot-api-key-help.png"))
        {
            let limit = ui.ctx().input(|input| input.max_texture_side) as u32;
            let image = if image.width() > limit || image.height() > limit {
                image.resize(limit, limit, image::imageops::FilterType::Triangle)
            } else {
                image
            };
            let rgba = image.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let texture = ui.ctx().load_texture(
                "dynadot-api-help",
                egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                egui::TextureOptions::LINEAR,
            );
            self.help_texture = Some((ui.ctx().clone(), texture));
        }
        if let Some((_, texture)) = &self.help_texture {
            let alt = "Dynadot Tools > API page showing the API Production Key, Secret Key, and restricted API keys.";
            egui::CollapsingHeader::new("Where to find these values").show(ui, |ui| {
                ui.add(
                    egui::Image::new(texture)
                        .max_width(ui.available_width().min(560.0))
                        .alt_text(alt),
                );
                if ui
                    .add(egui::Button::new("View full size").min_size(egui::vec2(120.0, 44.0)))
                    .clicked()
                {
                    self.show_help = true;
                }
            });
            egui::Window::new("Dynadot API settings")
                .open(&mut self.show_help)
                .default_size(egui::vec2(900.0, 540.0))
                .max_size(
                    (ui.ctx().content_rect().size() - egui::vec2(32.0, 64.0))
                        .max(egui::vec2(100.0, 100.0)),
                )
                .scroll([true, true])
                .show(ui.ctx(), |ui| {
                    ui.add(
                        egui::Image::new(texture)
                            .fit_to_original_size(1.0)
                            .alt_text(alt),
                    );
                });
        }
    }

    fn error_ui(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        }
    }

    /// Returns true when the user requests Dynadot settings.
    pub(crate) fn domains_ui(&mut self, ui: &mut egui::Ui) -> bool {
        self.domain_list_ui(ui, false)
    }

    pub(crate) fn hosting_ui(&mut self, ui: &mut egui::Ui) -> bool {
        self.domain_list_ui(ui, true)
    }

    fn domain_list_ui(&mut self, ui: &mut egui::Ui, hosting: bool) -> bool {
        self.poll();
        if self.saved.is_some() && !self.loaded && self.job.is_none() && self.error.is_none() {
            self.refresh(ui.ctx());
        }
        let mut settings = false;
        ui.horizontal_wrapped(|ui| {
            ui.heading(if hosting { "Hosting" } else { "Domains" });
            if self.saved.is_some() {
                if ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::new("Refresh").min_size(egui::vec2(80.0, 44.0)),
                    )
                    .clicked()
                {
                    self.refresh(ui.ctx());
                }
                settings |= ui
                    .add(egui::Button::new("Dynadot settings…").min_size(egui::vec2(140.0, 44.0)))
                    .clicked();
            }
        });
        if self.saved.is_none() {
            ui.label("Connect Dynadot to list your domains.");
            settings |= ui
                .add(egui::Button::new("Connect Dynadot…").min_size(egui::vec2(140.0, 44.0)))
                .clicked();
            self.error_ui(ui);
            return settings;
        }
        self.error_ui(ui);
        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Syncing Dynadot domains…");
            });
        }
        if !self.loaded {
            return settings;
        }
        if self.domains.is_empty() {
            ui.label("There are no domains in this Dynadot account.");
            return settings;
        }
        ui.add_space(8.0);
        ui.label(if hosting {
            "Search emails"
        } else {
            "Search domains"
        });
        let search = if hosting {
            &mut self.hosting_search
        } else {
            &mut self.search
        };
        ui.add(
            egui::TextEdit::singleline(&mut *search).desired_width(ui.available_width().min(400.0)),
        );
        let query = search.trim().to_lowercase();
        let filtered: Vec<_> = self
            .domains
            .iter()
            .filter(|domain| {
                let value = if hosting {
                    format!("support@{}", domain.domain_name)
                } else {
                    domain.domain_name.clone()
                };
                value.to_lowercase().contains(&query)
            })
            .collect();
        ui.label(format!(
            "{} of {} {} · Dynadot",
            filtered.len(),
            self.domains.len(),
            if hosting { "emails" } else { "domains" }
        ));
        if filtered.is_empty() {
            ui.label(if hosting {
                "No emails match your search."
            } else {
                "No domains match your search."
            });
            return settings;
        }
        let wide = ui.available_width() >= 600.0;
        egui::ScrollArea::vertical()
            .id_salt(if hosting {
                "dynadot_hosting"
            } else {
                "dynadot_domains"
            })
            .show(ui, |ui| {
                if hosting {
                    egui::Grid::new("dynadot_hosting_emails")
                        .striped(true)
                        .min_row_height(36.0)
                        .show(ui, |ui| {
                            ui.strong("Email");
                            ui.end_row();
                            for domain in &filtered {
                                ui.add_sized(
                                    [ui.available_width(), 36.0],
                                    egui::Label::new(format!("support@{}", domain.domain_name))
                                        .wrap(),
                                );
                                ui.end_row();
                            }
                        });
                } else if wide {
                    let column_width =
                        (ui.available_width() - ui.spacing().item_spacing.x * 3.0) / 4.0;
                    egui::Grid::new("dynadot_domain_table")
                        .striped(true)
                        .min_row_height(36.0)
                        .show(ui, |ui| {
                            for title in ["Domain", "Expires (UTC)", "Status", "Renewal"] {
                                ui.add_sized(
                                    [column_width, 24.0],
                                    egui::Label::new(egui::RichText::new(title).strong())
                                        .truncate(),
                                );
                            }
                            ui.end_row();
                            for domain in &filtered {
                                for value in [
                                    domain.domain_name.clone(),
                                    expiration(domain.expiration_date),
                                    display_value(domain.status.as_deref()),
                                    display_value(domain.renew_option.as_deref()),
                                ] {
                                    ui.add_sized(
                                        [column_width, 36.0],
                                        egui::Label::new(&value).truncate(),
                                    )
                                    .on_hover_text(value);
                                }
                                ui.end_row();
                            }
                        });
                } else {
                    for domain in &filtered {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&domain.domain_name).strong())
                                .wrap(),
                        );
                        ui.add(
                            egui::Label::new(format!(
                                "Expires {} · {} · {}",
                                expiration(domain.expiration_date),
                                display_value(domain.status.as_deref()),
                                display_value(domain.renew_option.as_deref())
                            ))
                            .wrap(),
                        );
                        ui.add_space(8.0);
                    }
                }
            });
        settings
    }
}

fn display_value(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .map(|value| value.replace('_', " "))
        .unwrap_or_else(|| "—".into())
}

fn expiration(value: Option<i64>) -> String {
    let Some(mut timestamp) = value.filter(|value| *value > 0) else {
        return "—".into();
    };
    // Dynadot timestamps may be milliseconds; accept seconds as well.
    if timestamp >= 100_000_000_000 {
        timestamp /= 1000;
    }
    match time::OffsetDateTime::from_unix_timestamp(timestamp) {
        Ok(date) => format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            date.month() as u8,
            date.day()
        ),
        Err(_) => "—".into(),
    }
}

fn signature(credentials: &Credentials, path: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(credentials.secret_key.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(format!("{}\n{path}\n\n", credentials.api_key).as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

fn provider_error(code: u16) -> String {
    match code {
        401 => "Dynadot authentication failed. Check the API key and its matching secret in settings.",
        403 => "Dynadot denied access. Check Domains read permission, API enabled status, and the IP allowlist.",
        429 => "Dynadot's rate limit was reached. Wait 60 seconds, then refresh.",
        500..=599 => "Dynadot is unavailable. Try refreshing later.",
        _ => "Dynadot could not list domains. Check your API settings and try again.",
    }.into()
}

fn fetch_domains(credentials: &Credentials, base_url: &str) -> Result<Vec<Domain>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not create the Dynadot connection.".to_owned())?;
    let mut domains = Vec::new();
    for page in 1..=10_000 {
        let path =
            format!("/restful/v2/domains?sort=name_asc&page_size=100&page={page}&status=all");
        let response = client
            .get(format!("{base_url}{path}"))
            .bearer_auth(&credentials.api_key)
            .header("Accept", "application/json")
            .header("X-Signature", signature(credentials, &path))
            .send()
            .map_err(|_| {
                "Could not reach Dynadot. Check your connection and try again.".to_owned()
            })?;
        if !response.status().is_success() {
            return Err(provider_error(response.status().as_u16()));
        }
        let response: Response = response
            .json()
            .map_err(|_| "Dynadot returned an invalid domain response.".to_owned())?;
        if response.code != 200 {
            return Err(provider_error(response.code));
        }
        let data = response.data.ok_or("Dynadot returned no domain data.")?;
        if data.pagination_result.page != page {
            return Err("Dynadot returned an unexpected domain page.".into());
        }
        let has_next = data.pagination_result.has_next()?;
        domains.extend(data.domain_info_list);
        if !has_next {
            domains.sort_by(|a, b| a.domain_name.cmp(&b.domain_name));
            domains.dedup_by(|a, b| a.domain_name == b.domain_name);
            return Ok(domains);
        }
    }
    Err("Dynadot returned too many pages. Domain sync was not completed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    fn credentials() -> Credentials {
        Credentials {
            api_key: "test-key".into(),
            secret_key: "test-secret".into(),
        }
    }

    #[test]
    fn signature_matches_independent_hmac_vector() {
        assert_eq!(
            signature(
                &credentials(),
                "/restful/v2/domains?sort=name_asc&page_size=100&page=1&status=all"
            ),
            "g4q067J17EhsYsZRXYdwNWXGpqRq7YkF4n/QY0BjfdQ="
        );
    }

    #[test]
    fn signed_requests_follow_all_pages_and_deduplicate() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for page in 1..=2 {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline, "No request arrived");
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(request).unwrap().to_lowercase();
                let path = format!(
                    "/restful/v2/domains?sort=name_asc&page_size=100&page={page}&status=all"
                );
                assert!(request.starts_with(&format!("get {path} http/1.1")));
                assert!(request.contains("authorization: bearer test-key\r\n"));
                assert!(request.contains(&format!(
                    "x-signature: {}\r\n",
                    signature(&credentials(), &path).to_lowercase()
                )));
                let body = serde_json::json!({
                    "code": 200,
                    "data": {
                        "domain_info_list": [{"domain_name": "example.com", "expiration_date": 1735689600000_i64}, {"domain_name": format!("page{page}.com")}],
                        "pagination_result": {"page": page, "has_next_page": if page == 1 { serde_json::json!("Yes") } else { serde_json::json!(false) }}
                    }
                }).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let domains = fetch_domains(&credentials(), &base_url).unwrap();
        server.join().unwrap();
        assert_eq!(
            domains
                .iter()
                .map(|domain| domain.domain_name.as_str())
                .collect::<Vec<_>>(),
            ["example.com", "page1.com", "page2.com"]
        );
        assert_eq!(expiration(domains[0].expiration_date), "2025-01-01");
    }

    #[test]
    fn credential_changes_discard_previous_account_results() {
        let (sender, receiver) = mpsc::channel();
        let mut dynadot = Dynadot {
            job: Some(receiver),
            ..Dynadot::default()
        };
        dynadot.reset();
        assert!(sender.send(Ok(vec![])).is_err());
        dynadot.poll();
        assert!(!dynadot.loaded);
        assert!(dynadot.domains.is_empty());
    }

    #[test]
    fn pagination_and_dates_handle_provider_variants() {
        for value in [serde_json::json!(true), serde_json::json!("Yes")] {
            assert!(
                Pagination {
                    page: 1,
                    has_next_page: value
                }
                .has_next()
                .unwrap()
            );
        }
        assert!(
            Pagination {
                page: 1,
                has_next_page: serde_json::json!(null)
            }
            .has_next()
            .is_err()
        );
        assert_eq!(expiration(Some(1735689600)), "2025-01-01");
        assert_eq!(expiration(None), "—");
        assert_eq!(expiration(Some(-1)), "—");
        assert_eq!(expiration(Some(0)), "—");
    }

    #[test]
    fn domain_and_hosting_lists_fit_supported_window_sizes() {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            let mut dynadot = Dynadot {
                saved: Some(credentials()),
                loaded: true,
                domains: vec![
                    Domain {
                        domain_name: format!("{}.example.com", "a".repeat(63)),
                        expiration_date: Some(1735689600000),
                        status: Some("transferaway_expired_auction".into()),
                        renew_option: Some("auto_renew".into()),
                    },
                    Domain {
                        domain_name: "example.com".into(),
                        expiration_date: None,
                        status: None,
                        renew_option: None,
                    },
                ],
                ..Dynadot::default()
            };
            let ctx = egui::Context::default();
            crate::style::configure(&ctx);
            for hosting in [false, false, true, true] {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                };
                let _ = ctx.run_ui(input, |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        if hosting {
                            dynadot.hosting_ui(ui);
                        } else {
                            dynadot.domains_ui(ui);
                        }
                        assert!(
                            ui.min_rect().right() <= width,
                            "List overflow at {width}px (hosting: {hosting})"
                        );
                    });
                });
            }
        }
    }
}
