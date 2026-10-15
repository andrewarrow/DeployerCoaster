use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use egui::ColorImage;
use reqwest::{Url, blocking::Client};
use serde_json::Value;

use super::{
    api::{self, Transport, id, validate_id},
    details::attr,
};

#[cfg(test)]
mod tests;

const IMAGE_EDGE: u32 = 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Screenshot,
    Preview,
}

impl Kind {
    fn collection(self) -> &'static str {
        match self {
            Self::Screenshot => "screenshot-sets",
            Self::Preview => "preview-sets",
        }
    }

    fn assets(self) -> &'static str {
        match self {
            Self::Screenshot => "screenshots",
            Self::Preview => "previews",
        }
    }

    pub(crate) fn state_field(self) -> &'static str {
        match self {
            Self::Screenshot => "assetDeliveryState",
            Self::Preview => "videoDeliveryState",
        }
    }
}

#[derive(Debug)]
pub(crate) struct AssetSet {
    pub display_type: String,
    pub assets: Result<Vec<Value>, String>,
}

#[derive(Debug)]
pub(crate) struct Media {
    pub screenshots: Result<Vec<AssetSet>, String>,
    pub previews: Result<Vec<AssetSet>, String>,
}

pub(crate) fn load<T: Transport>(
    mut transport: T,
    app_id: &str,
    version_id: &str,
    locale: &str,
    cancelled: &AtomicBool,
) -> Result<Media, String> {
    validate_id(app_id)?;
    validate_id(version_id)?;
    if !(2..=35).contains(&locale.len())
        || !locale.as_bytes()[0].is_ascii_alphanumeric()
        || !locale
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("The selected localization is invalid. Refresh app details.".into());
    }
    let route = format!("/apps/{app_id}/versions/{version_id}/localizations/{locale}");
    let mut load_sets = |kind: Kind| -> Result<Vec<AssetSet>, String> {
        let sets = api::list(
            &mut transport,
            &format!("{route}/{}?limit=200", kind.collection()),
            cancelled,
        )?;
        sets.into_iter()
            .map(|set| {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Media loading cancelled.".into());
                }
                for (field, expected) in [("appId", app_id), ("versionId", version_id)] {
                    if let Some(owner) = set.get(field).filter(|v| !v.is_null())
                        && owner.as_str() != Some(expected)
                    {
                        return Err(
                            "Parsequerade returned media for a different app or version.".into(),
                        );
                    }
                }
                let set_id = id(&set)?;
                Ok(AssetSet {
                    display_type: attr(
                        &set,
                        match kind {
                            Kind::Screenshot => "screenshotDisplayType",
                            Kind::Preview => "previewType",
                        },
                    )
                    .into(),
                    assets: api::list(
                        &mut transport,
                        &format!(
                            "/{}/{set_id}/{}?limit=200",
                            kind.collection(),
                            kind.assets()
                        ),
                        cancelled,
                    ),
                })
            })
            .collect()
    };
    let media = Media {
        screenshots: load_sets(Kind::Screenshot),
        previews: load_sets(Kind::Preview),
    };
    if cancelled.load(Ordering::Relaxed) {
        return Err("Media loading cancelled.".into());
    }
    Ok(media)
}

pub(crate) fn image_dimensions(asset: &Value, kind: Kind) -> Option<[u32; 2]> {
    let image = image_asset(asset, kind);
    let dimension = |field| {
        image[field]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
    };
    Some([dimension("width")?, dimension("height")?])
}

pub(crate) fn image_url(asset: &Value, kind: Kind) -> Option<Url> {
    let template = image_asset(asset, kind)["templateUrl"].as_str()?;
    let [width, height] = image_dimensions(asset, kind)?;
    let scale = (IMAGE_EDGE as f64 / width.max(height) as f64).min(1.0);
    let width = ((width as f64 * scale).round() as u32).max(1);
    let height = ((height as f64 * scale).round() as u32).max(1);
    let url = template
        .replace("{w}", &width.to_string())
        .replace("{h}", &height.to_string())
        .replace("{f}", "png")
        .replace("{c}", "");
    if url.contains(['{', '}']) {
        return None;
    }
    secure_url(&url)
}

fn image_asset(asset: &Value, kind: Kind) -> &Value {
    match kind {
        Kind::Screenshot => &asset["imageAsset"],
        Kind::Preview => &asset["previewFrameImage"]["image"],
    }
}

pub(crate) fn secure_url(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    (url.scheme() == "https"
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

pub(crate) fn image_client() -> Result<Client, String> {
    // CDN image requests never carry the Apple API key or JWT.
    Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not initialize image loading.".into())
}

pub(crate) fn fetch_image(
    http: &Client,
    url: &str,
    cancelled: &AtomicBool,
) -> Result<ColorImage, String> {
    let failed = || "The image could not be loaded. Retry to load it again.".to_owned();
    if cancelled.load(Ordering::Relaxed) {
        return Err("Image loading cancelled.".into());
    }
    let url = secure_url(url).ok_or_else(failed)?;
    let mut response = http.get(url).send().map_err(|_| failed())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_IMAGE_BYTES)
    {
        return Err(failed());
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failed())?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES || cancelled.load(Ordering::Relaxed) {
        return Err(failed());
    }
    decode_image(&bytes)
}

fn decode_image(bytes: &[u8]) -> Result<ColorImage, String> {
    let failed = || "The image format could not be displayed.".to_owned();
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| failed())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader
        .decode()
        .map_err(|_| failed())?
        .thumbnail(IMAGE_EDGE, IMAGE_EDGE)
        .to_rgba8();
    Ok(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}
