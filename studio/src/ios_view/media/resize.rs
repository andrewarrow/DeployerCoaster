use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use egui::{Context, Label, ProgressBar, RichText, Ui};
use image::{DynamicImage, ImageReader, Rgba, RgbaImage, imageops};

use super::Palette;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Resize {
    job: Option<Receiver<Update>>,
    total: usize,
    completed: usize,
    resized: usize,
    current: Option<String>,
    skipped: Vec<String>,
    errors: Vec<String>,
}

enum Update {
    Started(String),
    Finished(String, Result<Outcome, String>),
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Resized,
    Skipped,
}

#[derive(Debug, PartialEq, Eq)]
enum Device {
    IphoneDuo,
    Iphone,
    Ipad,
}

impl Resize {
    pub(super) fn button(&mut self, ui: &mut Ui) {
        if ui
            .add_enabled(self.job.is_none(), egui::Button::new("Resize"))
            .on_hover_text("Resize selected iPhone and iPad screenshots in place")
            .clicked()
            && let Some(files) = rfd::FileDialog::new()
                .set_title("Resize screenshots in place")
                .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                .pick_files()
        {
            self.start(files, ui.ctx());
        }
    }

    fn start(&mut self, files: Vec<PathBuf>, ctx: &Context) {
        if self.job.is_some() || files.is_empty() {
            return;
        }
        let (sender, result) = mpsc::channel();
        *self = Self {
            job: Some(result),
            total: files.len(),
            ..Self::default()
        };
        let ctx = ctx.clone();
        thread::spawn(move || {
            for file in files {
                let name = file
                    .file_name()
                    .unwrap_or(file.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                if sender.send(Update::Started(name.clone())).is_err() {
                    return;
                }
                ctx.request_repaint();
                let result = resize_file(&file);
                if sender.send(Update::Finished(name, result)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    pub(super) fn poll(&mut self) {
        while let Some(job) = &self.job {
            match job.try_recv() {
                Ok(Update::Started(name)) => self.current = Some(name),
                Ok(Update::Finished(name, result)) => {
                    self.completed += 1;
                    self.current = None;
                    match result {
                        Ok(Outcome::Resized) => self.resized += 1,
                        Ok(Outcome::Skipped) => self.skipped.push(name),
                        Err(error) => self.errors.push(format!("{name}: {error}")),
                    }
                    if self.completed == self.total {
                        self.job = None;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.current = None;
                    self.job = None;
                    self.errors
                        .push("Resizing stopped before all selected files were processed.".into());
                }
            }
        }
    }

    pub(super) fn ui(&self, ui: &mut Ui, palette: Palette) {
        if self.total == 0 {
            return;
        }
        if self.job.is_some() {
            ui.add(
                ProgressBar::new(self.completed as f32 / self.total as f32)
                    .desired_width(ui.available_width().min(480.0))
                    .text(format!("{} / {} files", self.completed, self.total)),
            );
            if let Some(name) = &self.current {
                ui.add(Label::new(format!("Resizing: {name}")).truncate())
                    .on_hover_text(name);
            }
        } else {
            ui.label(format!(
                "Resized {} of {} files in place.",
                self.resized, self.total
            ));
        }
        for name in &self.skipped {
            ui.add(
                Label::new(
                    RichText::new(format!(
                        "{name}: skipped (filename contains neither iPhone nor iPad)."
                    ))
                    .color(palette.muted),
                )
                .wrap(),
            );
        }
        for error in &self.errors {
            ui.add(Label::new(RichText::new(error).color(palette.error)).wrap());
        }
    }
}

impl Device {
    fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        if name.contains("iphone duo") {
            Some(Self::IphoneDuo)
        } else if name.contains("iphone") {
            Some(Self::Iphone)
        } else if name.contains("ipad") {
            Some(Self::Ipad)
        } else {
            None
        }
    }

    fn target_dimensions(&self, source: (u32, u32)) -> (u32, u32) {
        match self {
            Self::Iphone => (1206, 2622),
            Self::Ipad => (2732, 2048),
            Self::IphoneDuo => {
                // App Store screenshot sizes differ from the inner panel's resolution.
                // https://developer.apple.com/help/app-store-connect/reference/app-information/screenshot-specifications/
                let outer = (1398, 2034);
                let inner = (2007, 2853);
                let ratio = f64::from(source.0.min(source.1)) / f64::from(source.0.max(source.1));
                let outer_difference = (ratio - f64::from(outer.0) / f64::from(outer.1)).abs();
                let inner_difference = (ratio - f64::from(inner.0) / f64::from(inner.1)).abs();
                let (width, height) = if outer_difference <= inner_difference {
                    outer
                } else {
                    inner
                };
                if source.0 > source.1 {
                    (height, width)
                } else {
                    (width, height)
                }
            }
        }
    }
}

fn resize_file(path: &Path) -> Result<Outcome, String> {
    let Some(device) = Device::from_path(path) else {
        return Ok(Outcome::Skipped);
    };
    let reader = ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|error| format!("Could not read image: {error}"))?;
    let format = reader.format().ok_or("Image format was not recognized.")?;
    let image = reader
        .decode()
        .map_err(|error| format!("Could not decode image: {error}"))?;
    let (width, height) = device.target_dimensions((image.width(), image.height()));
    let resized = image
        .resize(width, height, imageops::FilterType::Lanczos3)
        .into_rgba8();
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    imageops::replace(
        &mut canvas,
        &resized,
        i64::from((width - resized.width()) / 2),
        i64::from((height - resized.height()) / 2),
    );

    // Encode beside the original and only replace it after the complete image is saved.
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let mut output = tempfile::NamedTempFile::new_in(parent.unwrap_or(Path::new(".")))
        .map_err(|error| format!("Could not create resized image: {error}"))?;
    {
        let mut writer = BufWriter::new(output.as_file_mut());
        DynamicImage::ImageRgba8(canvas)
            .write_to(&mut writer, format)
            .map_err(|error| format!("Could not encode resized image: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("Could not save resized image: {error}"))?;
    }
    let permissions = std::fs::metadata(path)
        .map_err(|error| format!("Could not read original file permissions: {error}"))?
        .permissions();
    output
        .as_file()
        .set_permissions(permissions)
        .map_err(|error| format!("Could not preserve file permissions: {error}"))?;
    output
        .persist(path)
        .map_err(|error| format!("Could not replace original image: {error}"))?;
    Ok(Outcome::Resized)
}
