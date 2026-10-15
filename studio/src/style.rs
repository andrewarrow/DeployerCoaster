use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::{fs, sync::Arc};

// Load installed UI faces rather than distributing platform font files.
#[cfg(target_os = "macos")]
const FONTS: &[(&str, &str)] = &[(
    "/System/Library/Fonts/SFNS.ttf",
    "/System/Library/Fonts/SFNS.ttf",
)];
#[cfg(target_os = "windows")]
const FONTS: &[(&str, &str)] = &[(
    "C:/Windows/Fonts/segoeui.ttf",
    "C:/Windows/Fonts/seguisb.ttf",
)];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const FONTS: &[(&str, &str)] = &[
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    ),
    (
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Bold.ttf",
    ),
];

pub fn configure(context: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, weight, bold) in [
        ("system-ui", 400.0, false),
        ("system-ui-semibold", 600.0, true),
    ] {
        if let Some(bytes) = FONTS
            .iter()
            .find_map(|(regular, semibold)| fs::read(if bold { semibold } else { regular }).ok())
        {
            let data = FontData::from_owned(bytes);
            #[cfg(target_os = "macos")]
            let data = {
                // Use San Francisco’s text cut and fractional glyph positioning.
                // macOS does not grid-fit UI text; hinting changes its spacing and weight.
                let mut tweak = egui::FontTweak {
                    hinting: Some(false),
                    subpixel_binning: Some(true),
                    ..Default::default()
                };
                tweak.coords.push(b"opsz", 13.0);
                tweak.coords.push(b"wght", weight);
                data.tweak(tweak)
            };
            let _ = weight;
            fonts.font_data.insert(name.into(), Arc::new(data));
            if !bold {
                fonts
                    .families
                    .get_mut(&FontFamily::Proportional)
                    .unwrap()
                    .insert(0, name.into());
            }
        }
    }
    let mut heading = fonts.families[&FontFamily::Proportional].clone();
    if fonts.font_data.contains_key("system-ui-semibold") {
        heading.insert(0, "system-ui-semibold".into());
    }
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), heading);
    #[cfg(target_os = "macos")]
    if let Ok(bytes) = fs::read("/System/Library/Fonts/SFNSMono.ttf") {
        fonts
            .font_data
            .insert("system-mono".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .insert(0, "system-mono".into());
    }
    context.set_fonts(fonts);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*context.style_of(theme)).clone();
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(12.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(11.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(12.0));
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(16.0, FontFamily::Name("semibold".into())),
        );
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 24.0;
        style.visuals.text_options.font_hinting = !cfg!(target_os = "macos");
        style.visuals.text_options.subpixel_binning = cfg!(target_os = "macos");
        if theme == egui::Theme::Dark {
            style.visuals.panel_fill = Color32::from_gray(37);
            style.visuals.window_fill = Color32::from_gray(43);
            style.visuals.faint_bg_color = Color32::from_gray(48);
            style.visuals.extreme_bg_color = Color32::from_gray(26);
            style.visuals.widgets.noninteractive.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(57));
            style.visuals.widgets.inactive.bg_fill = Color32::from_gray(57);
            style.visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(37);
            style.visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(68));
            style.visuals.widgets.hovered.bg_fill = Color32::from_gray(72);
            style.visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(52);
            style.visuals.widgets.active.bg_fill = Color32::from_gray(80);
            style.visuals.widgets.active.weak_bg_fill = Color32::from_gray(58);
            style.visuals.widgets.hovered.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(92));
            style.visuals.widgets.active.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(110));
            style.visuals.selection.bg_fill = Color32::from_rgb(0, 99, 225);
            style.visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
            style.visuals.hyperlink_color = Color32::from_rgb(82, 160, 255);
        } else {
            style.visuals.panel_fill = Color32::from_gray(246);
            style.visuals.window_fill = Color32::WHITE;
            style.visuals.faint_bg_color = Color32::from_gray(240);
            style.visuals.extreme_bg_color = Color32::WHITE;
            style.visuals.widgets.noninteractive.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(218));
            style.visuals.widgets.inactive.bg_fill = Color32::from_gray(239);
            style.visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(246);
            style.visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(215));
            style.visuals.widgets.hovered.bg_fill = Color32::from_gray(229);
            style.visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(234);
            style.visuals.widgets.active.bg_fill = Color32::from_gray(218);
            style.visuals.widgets.active.weak_bg_fill = Color32::from_gray(224);
            style.visuals.widgets.hovered.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(190));
            style.visuals.widgets.active.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_gray(175));
            style.visuals.selection.bg_fill = Color32::from_rgb(0, 99, 225);
            style.visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
            style.visuals.hyperlink_color = Color32::from_rgb(0, 99, 225);
        }
        style.visuals.override_text_color = Some(if theme == egui::Theme::Dark {
            Color32::from_gray(224)
        } else {
            Color32::from_gray(30)
        });
        style.visuals.weak_text_color = Some(if theme == egui::Theme::Dark {
            Color32::from_gray(166)
        } else {
            Color32::from_gray(103)
        });
        style.visuals.widgets.active.bg_stroke =
            egui::Stroke::new(1.0, Color32::from_rgb(10, 132, 255));
        style.visuals.text_edit_bg_color = Some(if theme == egui::Theme::Dark {
            Color32::from_gray(26)
        } else {
            Color32::WHITE
        });
        style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(5);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(5);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(5);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(5);
        context.set_style_of(theme, style);
    }
}

pub(crate) fn semibold(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).font(FontId::new(13.0, FontFamily::Name("semibold".into())))
}
