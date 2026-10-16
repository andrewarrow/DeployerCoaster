use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use egui::{Align, ColorImage, Context, Label, Layout, RichText, TextureHandle, Ui, vec2};
use serde_json::Value;

use super::{AppDetails, Job, Palette, attr, documents::readable};
use crate::{
    apple::{
        AppleCredentials,
        api::ParsequeradeApi,
        media::{self, AssetSet, Kind, Media, PlacedImage},
    },
    ios_images::ImageKind,
    style::semibold,
};

mod generation;

#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Eq)]
struct Selection {
    app: String,
    version: String,
    locale: String,
}

#[derive(Default)]
struct CachedImage {
    result: Option<Result<ColorImage, String>>,
    texture: Option<(Context, TextureHandle)>,
    used: u64,
}

struct ImageJob {
    result: mpsc::Receiver<(String, Result<ColorImage, String>)>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for ImageJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub(super) struct Gallery {
    selection: Option<Selection>,
    result: Option<Result<Media, String>>,
    job: Option<Job<Media>>,
    images: HashMap<String, CachedImage>,
    image_job: Option<ImageJob>,
    opened: Option<(Value, Kind)>,
    generation: generation::Generation,
}

impl Gallery {
    #[cfg(test)]
    pub(super) fn preview(details: &AppDetails, locale: &str) -> Self {
        tests::preview(details, locale)
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn cancel(&mut self) {
        self.job = None;
        self.image_job = None;
        self.opened = None;
    }

    fn select(&mut self, selection: Selection) {
        if self.selection.as_ref() != Some(&selection) {
            self.clear();
            self.selection = Some(selection);
        }
    }

    fn poll(&mut self) {
        if let Some(job) = &self.job {
            match job.result.try_recv() {
                Ok(result) => {
                    self.generation.verify(&result);
                    self.result = Some(result);
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.result =
                        Some(Err("Media loading stopped. Retry to load it again.".into()));
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        while let Some(job) = &self.image_job {
            match job.result.try_recv() {
                Ok((url, result)) => {
                    if let Some(image) = self.images.get_mut(&url) {
                        image.result = Some(result);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.image_job = None;
                    break;
                }
            }
        }
    }

    pub(super) fn poll_generation(&mut self, ctx: &Context) {
        if self.generation.poll(ctx) {
            // Read the placement again; a successful agent turn alone is not proof of an upload.
            self.result = None;
            self.job = None;
        }
    }

    fn start(&mut self, credentials: AppleCredentials, ctx: &Context) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.job = Some(Job {
            result,
            cancelled: cancelled.clone(),
        });
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ParsequeradeApi::new(credentials).and_then(|api| {
                media::load(
                    api,
                    &selection.app,
                    &selection.version,
                    &selection.locale,
                    &cancelled,
                )
            });
            if !cancelled.load(Ordering::Relaxed) {
                let _ = sender.send(result);
                ctx.request_repaint();
            }
        });
    }

    fn start_images(&mut self, ctx: &Context) {
        if self.image_job.is_some() {
            return;
        }
        let requests: Vec<_> = self
            .images
            .iter()
            .filter(|(_, image)| image.result.is_none())
            .map(|(url, _)| url.clone())
            .collect();
        if requests.is_empty() {
            return;
        }
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.image_job = Some(ImageJob {
            result,
            cancelled: cancelled.clone(),
        });
        let ctx = ctx.clone();
        thread::spawn(move || {
            let http = media::image_client();
            for url in requests {
                if cancelled.load(Ordering::Relaxed) {
                    break;
                }
                let result = http
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|http| media::fetch_image(http, &url, &cancelled));
                if cancelled.load(Ordering::Relaxed) || sender.send((url, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
            ctx.request_repaint();
        });
    }

    pub(super) fn ui(
        &mut self,
        ui: &mut Ui,
        details: &AppDetails,
        locale: &str,
        credentials: &AppleCredentials,
        palette: Palette,
        refreshing: bool,
    ) {
        for style in [egui::TextStyle::Body, egui::TextStyle::Button] {
            ui.style_mut()
                .text_styles
                .insert(style, egui::FontId::proportional(12.0));
        }
        let Some(version) = &details.version else {
            self.clear();
            ui.label("No iOS version is available.");
            return;
        };
        if !details
            .version_localizations
            .iter()
            .any(|v| attr(v, "locale") == locale)
        {
            self.clear();
            ui.label("No version localization is available for this language.");
            return;
        }
        self.select(Selection {
            app: attr(&details.app, "id").into(),
            version: attr(version, "id").into(),
            locale: locale.into(),
        });
        self.poll();
        if self.result.is_none() && self.job.is_none() && !refreshing {
            self.start(credentials.clone(), ui.ctx());
        }
        let mut retry = false;
        ui.horizontal(|ui| {
            ui.label(semibold("Screenshots").size(14.0).color(palette.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.job.is_some() {
                    ui.spinner();
                } else if ui
                    .add_enabled(!refreshing, egui::Button::new("Reload media"))
                    .clicked()
                {
                    retry = true;
                }
            });
        });
        match &self.result {
            None => {
                ui.label(RichText::new("Loading media…").color(palette.muted));
            }
            Some(Err(error)) => {
                ui.add(Label::new(RichText::new(error).color(palette.error)).wrap());
            }
            Some(Ok(media)) => {
                section(
                    ui,
                    &media.screenshots,
                    Kind::Screenshot,
                    &mut self.images,
                    &mut self.opened,
                    palette,
                );
                ui.add_space(24.0);
                ui.label(semibold("App previews").size(14.0).color(palette.text));
                section(
                    ui,
                    &media.previews,
                    Kind::Preview,
                    &mut self.images,
                    &mut self.opened,
                    palette,
                );
                for (heading, kind, empty) in [
                    (
                        "Header images",
                        ImageKind::Header,
                        "No header images for this localization.",
                    ),
                    (
                        "Search images",
                        ImageKind::Search,
                        "No search images for this localization.",
                    ),
                ] {
                    ui.add_space(24.0);
                    ui.label(semibold(heading).size(14.0).color(palette.text));
                    match &media.placements {
                        Ok(placements) => {
                            let images: Vec<_> = placements
                                .iter()
                                .filter(|image| {
                                    attr(&image.placement, "placementType") == kind.placement()
                                })
                                .collect();
                            if images.is_empty() {
                                ui.label(RichText::new(empty).color(palette.muted));
                            } else {
                                ui.add_space(8.0);
                                tile_grid(ui, &images, 240.0, 280.0, |ui, image| {
                                    placed_image(
                                        ui,
                                        image,
                                        &mut self.images,
                                        &mut self.opened,
                                        palette,
                                    );
                                });
                            }
                            self.generation.ui(
                                ui,
                                kind,
                                details,
                                locale,
                                images.is_empty(),
                                (!refreshing).then_some(credentials),
                            );
                        }
                        Err(error) => {
                            ui.add(Label::new(RichText::new(error).color(palette.error)).wrap());
                        }
                    }
                }
            }
        }
        self.show_opened(ui.ctx(), palette);
        if retry {
            self.result = None;
            self.images.clear();
            self.image_job = None;
            self.opened = None;
            ui.ctx().request_repaint();
        } else {
            self.trim_images();
            if !refreshing {
                self.start_images(ui.ctx());
            }
        }
    }

    fn trim_images(&mut self) {
        if self.images.len() <= 32 {
            return;
        }
        let mut entries: Vec<_> = self
            .images
            .iter()
            .filter(|(_, image)| image.result.is_some())
            .map(|(url, image)| (url.clone(), image.used))
            .collect();
        entries.sort_by_key(|(_, used)| *used);
        for (url, _) in entries.into_iter().take(self.images.len() - 32) {
            self.images.remove(&url);
        }
    }

    fn show_opened(&mut self, ctx: &Context, palette: Palette) {
        let Some((asset, kind)) = self.opened.clone() else {
            return;
        };
        let title = asset_title(&asset, kind);
        let bounds = ctx.content_rect().size() - vec2(48.0, 160.0);
        let size = vec2(bounds.x.clamp(80.0, 960.0), bounds.y.max(80.0));
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("store_media_preview")).show(ctx, |ui| {
            ui.set_max_width(size.x);
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 64.0).max(40.0);
                ui.allocate_ui_with_layout(
                    vec2(width, 24.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add(Label::new(semibold(&title)).truncate())
                            .on_hover_text(&title);
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
            if let Some(url) = media::image_url(&asset, kind)
                && let Some(image) = self.images.get_mut(url.as_str())
                && let Some(texture) = image.texture(ctx)
            {
                let fitted = fit(texture.size_vec2(), size);
                ui.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(fitted)
                        .alt_text(&title),
                );
            }
            asset_info(ui, &asset, kind, palette);
        });
        if close || modal.should_close() {
            self.opened = None;
        }
    }
}

impl CachedImage {
    fn texture(&mut self, ctx: &Context) -> Option<&TextureHandle> {
        let Some(Ok(image)) = &self.result else {
            return None;
        };
        if self
            .texture
            .as_ref()
            .is_none_or(|(context, _)| context != ctx)
        {
            self.texture = Some((
                ctx.clone(),
                ctx.load_texture("store-media", image.clone(), egui::TextureOptions::LINEAR),
            ));
        }
        self.texture.as_ref().map(|(_, texture)| texture)
    }
}

fn section(
    ui: &mut Ui,
    sets: &Result<Vec<AssetSet>, String>,
    kind: Kind,
    images: &mut HashMap<String, CachedImage>,
    opened: &mut Option<(Value, Kind)>,
    palette: Palette,
) {
    let sets = match sets {
        Ok(sets) => sets,
        Err(error) => {
            ui.add(Label::new(RichText::new(error).color(palette.error)).wrap());
            return;
        }
    };
    if sets.is_empty() {
        ui.label(
            RichText::new(match kind {
                Kind::Screenshot => "No screenshots for this localization.",
                Kind::Preview => "No app previews for this localization.",
                Kind::LibraryImage => "No images for this localization.",
            })
            .color(palette.muted),
        );
    }
    for (index, set) in sets.iter().enumerate() {
        ui.push_id((kind as u8, index), |ui| {
            ui.add_space(12.0);
            ui.label(semibold(display_name(&set.display_type)));
            let assets = match &set.assets {
                Ok(assets) => assets,
                Err(error) => {
                    ui.add(Label::new(RichText::new(error).color(palette.error)).wrap());
                    return;
                }
            };
            if assets.is_empty() {
                ui.label(RichText::new("This set is empty.").color(palette.muted));
                return;
            }
            ui.add_space(4.0);
            tile_grid(ui, assets, 148.0, 180.0, |ui, asset| {
                tile(ui, asset, kind, images, opened, palette);
            });
        });
    }
}

fn tile_grid<T>(
    ui: &mut Ui,
    assets: &[T],
    min_width: f32,
    max_width: f32,
    mut render: impl FnMut(&mut Ui, &T),
) {
    let columns = ((ui.available_width() + 16.0) / (min_width + 16.0))
        .floor()
        .max(1.0) as usize;
    let width =
        ((ui.available_width() - 16.0 * (columns - 1) as f32) / columns as f32).min(max_width);
    for row in assets.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for asset in row {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    render(ui, asset);
                });
            }
        });
        ui.add_space(12.0);
    }
}

fn placed_image(
    ui: &mut Ui,
    image: &PlacedImage,
    images: &mut HashMap<String, CachedImage>,
    opened: &mut Option<(Value, Kind)>,
    palette: Palette,
) {
    match &image.image {
        Ok(asset) => tile(ui, asset, Kind::LibraryImage, images, opened, palette),
        Err(error) => {
            let (rect, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), ui.available_width() * 9.0 / 16.0),
                egui::Sense::hover(),
            );
            placeholder(ui, rect, "Image unavailable", palette);
            ui.add(Label::new(RichText::new(error).color(palette.error)).wrap());
        }
    }
    let state = attr(&image.placement, "state");
    let state = state.strip_prefix("PARENT_").unwrap_or(state);
    // The library asset and its use on this listing can have different states.
    if !state.is_empty()
        && image
            .image
            .as_ref()
            .ok()
            .is_none_or(|asset| attr(asset, "state") != state)
    {
        ui.add(
            Label::new(
                RichText::new(format!("Placement: {}", readable(state)))
                    .size(11.0)
                    .color(if state == "FAILED" {
                        palette.error
                    } else {
                        palette.muted
                    }),
            )
            .wrap(),
        );
    }
}

fn tile(
    ui: &mut Ui,
    asset: &Value,
    kind: Kind,
    images: &mut HashMap<String, CachedImage>,
    opened: &mut Option<(Value, Kind)>,
    palette: Palette,
) {
    let title = asset_title(asset, kind);
    let size = vec2(
        ui.available_width(),
        if kind == Kind::LibraryImage {
            media::image_dimensions(asset, kind)
                .map(|[width, height]| ui.available_width() * height as f32 / width as f32)
                .unwrap_or(ui.available_width() * 9.0 / 16.0)
                .clamp(44.0, 228.0)
        } else {
            228.0
        },
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let url = media::image_url(asset, kind);
        let image = url
            .as_ref()
            .map(|url| images.entry(url.to_string()).or_default());
        match image {
            Some(image) => {
                image.used = ui.ctx().cumulative_frame_nr();
                if let Some(texture) = image.texture(ui.ctx()) {
                    let fitted = fit(texture.size_vec2(), size - vec2(4.0, 4.0));
                    let button = egui::Button::image(
                        egui::Image::new(texture)
                            .fit_to_exact_size(fitted)
                            .alt_text(&title),
                    )
                    .frame(false);
                    let response = ui.put(rect, button);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("View {title}"),
                        )
                    });
                    if response.on_hover_text(format!("View {title}")).clicked() {
                        *opened = Some((asset.clone(), kind));
                    }
                } else {
                    let failed = matches!(image.result, Some(Err(_)));
                    placeholder(
                        ui,
                        rect,
                        if failed {
                            "Image unavailable"
                        } else {
                            "Loading image…"
                        },
                        palette,
                    );
                    if failed
                        && ui
                            .put(
                                egui::Rect::from_center_size(
                                    rect.center() + vec2(0.0, 28.0),
                                    vec2(64.0, 28.0),
                                ),
                                egui::Button::new("Retry"),
                            )
                            .clicked()
                    {
                        image.result = None;
                    }
                }
            }
            None => placeholder(
                ui,
                rect,
                if processing(asset, kind) {
                    "Processing…"
                } else {
                    "No image available"
                },
                palette,
            ),
        }
    }
    ui.add(Label::new(RichText::new(&title).color(palette.text)).truncate())
        .on_hover_text(&title);
    asset_info(ui, asset, kind, palette);
}

fn placeholder(ui: &Ui, rect: egui::Rect, label: &str, palette: Palette) {
    ui.painter().rect_filled(rect, 4.0, palette.sidebar);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        palette.muted,
    );
}

fn asset_title(asset: &Value, kind: Kind) -> String {
    let name = attr(asset, "fileName");
    if !name.is_empty() {
        name.into()
    } else {
        let reference = attr(asset, "referenceName");
        if kind == Kind::LibraryImage && !reference.is_empty() {
            return reference.into();
        }
        format!(
            "{} {}",
            match kind {
                Kind::Screenshot => "Screenshot",
                Kind::Preview => "Preview",
                Kind::LibraryImage => "Image",
            },
            attr(asset, "id")
        )
    }
}

fn asset_info(ui: &mut Ui, asset: &Value, kind: Kind, palette: Palette) {
    if let Some([width, height]) = media::image_dimensions(asset, kind) {
        ui.label(
            RichText::new(format!("{width} × {height}"))
                .size(11.0)
                .color(palette.muted),
        );
    }
    let state = delivery_state(asset, kind);
    if !state.is_empty() && state != "COMPLETE" {
        ui.add(
            Label::new(
                RichText::new(readable(state))
                    .size(11.0)
                    .color(if state == "FAILED" {
                        palette.error
                    } else {
                        palette.muted
                    }),
            )
            .wrap(),
        );
    }
    if kind == Kind::Preview
        && let Some(url) = media::secure_url(attr(asset, "videoUrl"))
    {
        ui.hyperlink_to("Open video ↗", url.as_str());
    }
}

fn processing(asset: &Value, kind: Kind) -> bool {
    matches!(
        delivery_state(asset, kind),
        "AWAITING_UPLOAD" | "UPLOAD_COMPLETE" | "PROCESSING" | "ASSET_PROCESSING"
    )
}

fn delivery_state(asset: &Value, kind: Kind) -> &str {
    let state = match kind {
        Kind::Screenshot => attr(&asset["assetDeliveryState"], "state"),
        Kind::Preview => attr(&asset["videoDeliveryState"], "state"),
        Kind::LibraryImage => attr(asset, "state"),
    };
    if kind == Kind::Preview && (state.is_empty() || state == "COMPLETE") {
        let frame_state = attr(&asset["previewFrameImage"]["state"], "state");
        if !frame_state.is_empty() {
            return frame_state;
        }
    }
    state
}

fn fit(image: egui::Vec2, bounds: egui::Vec2) -> egui::Vec2 {
    image * (bounds.x / image.x).min(bounds.y / image.y)
}

fn display_name(value: &str) -> String {
    let (prefix, value) = if let Some(value) = value.strip_prefix("IMESSAGE_APP_") {
        ("iMessage · ", value)
    } else {
        ("", value.strip_prefix("APP_").unwrap_or(value))
    };
    let device = match value {
        "IPHONE_67" => "iPhone 6.7\"",
        "IPHONE_61" => "iPhone 6.1\"",
        "IPHONE_65" => "iPhone 6.5\"",
        "IPHONE_58" => "iPhone 5.8\"",
        "IPHONE_55" => "iPhone 5.5\"",
        "IPHONE_47" => "iPhone 4.7\"",
        "IPHONE_40" => "iPhone 4\"",
        "IPHONE_35" => "iPhone 3.5\"",
        "IPAD_PRO_3GEN_129" => "iPad Pro 12.9\" (3rd generation)",
        "IPAD_PRO_3GEN_11" => "iPad Pro 11\"",
        "IPAD_PRO_129" => "iPad Pro 12.9\"",
        "IPAD_105" => "iPad 10.5\"",
        "IPAD_97" => "iPad 9.7\"",
        "DESKTOP" => "Mac",
        "APPLE_TV" => "Apple TV",
        "APPLE_VISION_PRO" => "Apple Vision Pro",
        "WATCH_ULTRA" => "Apple Watch Ultra",
        "WATCH_SERIES_10" => "Apple Watch Series 10",
        "WATCH_SERIES_7" => "Apple Watch Series 7",
        "WATCH_SERIES_4" => "Apple Watch Series 4",
        "WATCH_SERIES_3" => "Apple Watch Series 3",
        "" => "Other devices",
        _ => value,
    };
    format!("{prefix}{device}")
}
