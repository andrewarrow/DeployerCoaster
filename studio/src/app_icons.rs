use std::{
    collections::{HashMap, HashSet},
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Ui};
use reqwest::{Url, blocking::Client};

const ICON_SIZE: f32 = 48.0;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Store {
    Apple,
    Play,
}

#[derive(Clone)]
pub struct IconRequest {
    pub key: String,
}

struct DecodedIcon {
    key: String,
    image: ColorImage,
}

struct Job {
    receiver: Receiver<DecodedIcon>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct CachedIcon {
    image: Option<ColorImage>,
    texture: Option<(Context, TextureHandle)>,
}

#[derive(Default)]
pub struct AppIcons {
    icons: HashMap<String, CachedIcon>,
    attempted: HashSet<String>,
    job: Option<Job>,
    started: bool,
}

impl AppIcons {
    pub fn needs_start(&self) -> bool {
        !self.started && self.job.is_none()
    }

    /// Begins one sequential best-effort lookup for each key after the app list loads.
    pub fn ensure_started(&mut self, store: Store, requests: Vec<IconRequest>, ctx: &Context) {
        if self.job.is_some() || self.started {
            return;
        }
        self.started = true;
        let requests: Vec<_> = requests
            .into_iter()
            .filter(|request| {
                !request.key.trim().is_empty() && self.attempted.insert(request.key.clone())
            })
            .collect();
        if requests.is_empty() {
            return;
        }
        self.start(store, requests, ctx);
    }

    /// Invalidates the previous batch so an explicit list refresh retries missing artwork too.
    pub fn refresh(&mut self) {
        self.job = None;
        self.started = false;
        self.attempted.clear();
    }

    pub fn poll(&mut self) {
        while let Some(job) = &self.job {
            match job.receiver.try_recv() {
                Ok(icon) => {
                    self.icons.insert(
                        icon.key,
                        CachedIcon {
                            image: Some(icon.image),
                            texture: None,
                        },
                    );
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    break;
                }
            }
        }
    }

    pub fn ui_icon(&mut self, ui: &mut Ui, key: &str, title: &str) {
        self.poll();
        let icon = self.icons.entry(key.to_owned()).or_default();
        if let Some(image) = &icon.image {
            let context = ui.ctx().clone();
            if icon
                .texture
                .as_ref()
                .is_none_or(|(old_context, _)| old_context != &context)
            {
                icon.texture = Some((
                    context,
                    ui.ctx().load_texture(
                        format!("app-icon-{key}"),
                        image.clone(),
                        TextureOptions::LINEAR,
                    ),
                ));
            }
        }
        if let Some((_, texture)) = &icon.texture {
            ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(egui::vec2(ICON_SIZE, ICON_SIZE))
                    .corner_radius(6.0),
            );
        } else {
            let initial = title
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            let background = ui.visuals().widgets.inactive.bg_fill;
            let foreground = ui.visuals().weak_text_color();
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ICON_SIZE, ICON_SIZE), egui::Sense::hover());
            ui.painter().rect_filled(rect, 6.0, background);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initial,
                egui::FontId::proportional(20.0),
                foreground,
            );
        }
    }

    fn start(&mut self, store: Store, requests: Vec<IconRequest>, ctx: &Context) {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        let context = ctx.clone();
        self.job = Some(Job {
            receiver,
            cancelled,
        });
        thread::spawn(move || {
            let Ok(http) = Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
            else {
                return;
            };
            for request in requests {
                if cancellation.load(Ordering::Relaxed) {
                    break;
                }
                let image = match store {
                    Store::Apple => apple_icon(&http, &request.key, &cancellation),
                    Store::Play => play_icon(&http, &request.key, &cancellation),
                };
                if let Some(image) = image {
                    if sender
                        .send(DecodedIcon {
                            key: request.key,
                            image,
                        })
                        .is_err()
                    {
                        break;
                    }
                    context.request_repaint();
                }
            }
        });
    }
}

fn apple_icon(http: &Client, bundle_id: &str, cancelled: &AtomicBool) -> Option<ColorImage> {
    let mut url = Url::parse("https://itunes.apple.com/lookup").ok()?;
    url.query_pairs_mut().append_pair("bundleId", bundle_id);
    let bytes = limited_get(http, url, MAX_PAGE_BYTES, cancelled)?;
    let response: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let artwork = apple_artwork_url(&response, bundle_id)?;
    let url = Url::parse(&artwork).ok()?;
    let host = url.host_str()?;
    if url.scheme() != "https" || !(host == "mzstatic.com" || host.ends_with(".mzstatic.com")) {
        return None;
    }
    decode_image(limited_get(http, url, MAX_IMAGE_BYTES, cancelled)?)
}

fn play_icon(http: &Client, package_name: &str, cancelled: &AtomicBool) -> Option<ColorImage> {
    let mut url = Url::parse("https://play.google.com/store/apps/details").ok()?;
    url.query_pairs_mut()
        .append_pair("id", package_name)
        .append_pair("hl", "en")
        .append_pair("gl", "US");
    let page = limited_get(http, url, MAX_PAGE_BYTES, cancelled)?;
    let artwork = og_image_url(std::str::from_utf8(&page).ok()?)?;
    let url = Url::parse(&artwork).ok()?;
    let host = url.host_str()?;
    if url.scheme() != "https"
        || !(host == "googleusercontent.com" || host.ends_with(".googleusercontent.com"))
    {
        return None;
    }
    decode_image(limited_get(http, url, MAX_IMAGE_BYTES, cancelled)?)
}

fn limited_get(http: &Client, url: Url, limit: usize, cancelled: &AtomicBool) -> Option<Vec<u8>> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let mut response = http.get(url).send().ok()?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > limit as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > limit || cancelled.load(Ordering::Relaxed) {
        return None;
    }
    Some(bytes)
}

fn decode_image(bytes: Vec<u8>) -> Option<ColorImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let rgba = decoded.thumbnail(128, 128).to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

fn apple_artwork_url(response: &serde_json::Value, bundle_id: &str) -> Option<String> {
    let result = response.get("results")?.as_array()?.iter().find(|result| {
        result.get("bundleId").and_then(serde_json::Value::as_str) == Some(bundle_id)
    })?;
    ["artworkUrl512", "artworkUrl100", "artworkUrl60"]
        .into_iter()
        .find_map(|field| {
            result
                .get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
}

/// Extracts and decodes Open Graph image metadata from a public Play listing.
fn og_image_url(html: &str) -> Option<String> {
    for tag in html.match_indices("<meta") {
        let rest = &html[tag.0..];
        let end = rest.find('>')?;
        let attributes = parse_attributes(&rest[5..end]);
        if attributes
            .get("property")
            .is_some_and(|value| value.eq_ignore_ascii_case("og:image"))
        {
            return attributes
                .get("content")
                .map(|value| decode_entities(value));
        }
    }
    None
}

fn parse_attributes(mut input: &str) -> HashMap<String, String> {
    let mut attributes = HashMap::new();
    while !input.is_empty() {
        input = input.trim_start();
        if input.is_empty() {
            break;
        }
        let name_end = input
            .find(|character: char| character.is_whitespace() || character == '=')
            .unwrap_or(input.len());
        if name_end == 0 {
            input = &input[1..];
            continue;
        }
        let name = input[..name_end].to_ascii_lowercase();
        input = input[name_end..].trim_start();
        if let Some(after_equals) = input.strip_prefix('=') {
            input = after_equals.trim_start();
            if let Some(quote) = input
                .chars()
                .next()
                .filter(|character| *character == '\'' || *character == '"')
            {
                input = &input[quote.len_utf8()..];
                if let Some(end) = input.find(quote) {
                    attributes.insert(name, input[..end].to_owned());
                    input = &input[end + quote.len_utf8()..];
                } else {
                    break;
                }
            } else {
                let end = input.find(char::is_whitespace).unwrap_or(input.len());
                attributes.insert(name, input[..end].trim_end_matches('/').to_owned());
                input = &input[end..];
            }
        }
    }
    attributes
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&#x26;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_og_image_attributes_in_either_order_and_quote_style() {
        assert_eq!(
            og_image_url(
                "<meta property=\"og:image\" content=\"https://example.test/icon?a=1&amp;b=2\">"
            ),
            Some("https://example.test/icon?a=1&b=2".into())
        );
        assert_eq!(
            og_image_url("<meta content='https://example.test/icon' property='og:image'>"),
            Some("https://example.test/icon".into())
        );
    }

    #[test]
    fn ignores_other_meta_tags_and_missing_content() {
        assert_eq!(
            og_image_url(
                "<meta property=\"og:title\" content=\"App\"><meta property=\"og:image\">"
            ),
            None
        );
    }

    #[test]
    fn decodes_png_without_exceeding_the_thumbnail_dimensions() {
        let bytes = image::DynamicImage::new_rgb8(8, 4);
        let mut encoded = std::io::Cursor::new(Vec::new());
        bytes
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_image(encoded.into_inner()).unwrap();
        assert_eq!(decoded.size, [128, 64]);
    }

    #[test]
    fn decodes_jpeg_artwork() {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(16, 16)
            .write_to(&mut encoded, image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(decode_image(encoded.into_inner()).unwrap().size, [128, 128]);
    }

    #[test]
    fn selects_apple_artwork_only_for_the_requested_bundle_and_uses_fallback_fields() {
        let response = serde_json::json!({"results": [
            {"bundleId": "other.app", "artworkUrl512": "https://is1-ssl.mzstatic.com/other.jpg"},
            {"bundleId": "com.example.app", "artworkUrl100": "https://is2-ssl.mzstatic.com/right.jpg"}
        ]});
        assert_eq!(
            apple_artwork_url(&response, "com.example.app").as_deref(),
            Some("https://is2-ssl.mzstatic.com/right.jpg")
        );
        assert_eq!(apple_artwork_url(&response, "missing.app"), None);
    }

    #[test]
    fn cached_image_rebuilds_its_texture_for_a_new_native_context() {
        let mut icons = AppIcons::default();
        icons.icons.insert(
            "test.app".into(),
            CachedIcon {
                image: Some(ColorImage::new([2, 2], vec![egui::Color32::WHITE; 4])),
                texture: None,
            },
        );
        let first_context = Context::default();
        let _ = first_context.run_ui(egui::RawInput::default(), |ui| {
            icons.ui_icon(ui, "test.app", "Test");
        });
        let first = icons.icons["test.app"].texture.as_ref().unwrap().0.clone();
        let second_context = Context::default();
        let _ = second_context.run_ui(egui::RawInput::default(), |ui| {
            icons.ui_icon(ui, "test.app", "Test");
        });
        let second = &icons.icons["test.app"].texture.as_ref().unwrap().0;
        assert_ne!(first, *second);
    }
}
