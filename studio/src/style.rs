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
                // Match Cubacadabra's text cut; SFNS defaults to the display cut.
                let mut tweak = egui::FontTweak {
                    hinting: Some(true),
                    subpixel_binning: Some(false),
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
    context.set_fonts(fonts);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*context.style_of(theme)).clone();
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(20.0, FontFamily::Name("semibold".into())),
        );
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 32.0;
        style.visuals.text_options.font_hinting = true;
        style.visuals.text_options.subpixel_binning = false;
        style.visuals.override_text_color = Some(if theme == egui::Theme::Dark {
            Color32::from_gray(235)
        } else {
            Color32::from_gray(30)
        });
        style.visuals.text_edit_bg_color = Some(if theme == egui::Theme::Dark {
            Color32::from_gray(42)
        } else {
            Color32::WHITE
        });
        style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
        context.set_style_of(theme, style);
    }
}
