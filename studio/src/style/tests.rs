use super::*;

#[test]
#[cfg(target_os = "macos")]
fn localized_text_has_glyphs_in_labels_headings_and_editors() {
    let context = egui::Context::default();
    configure(&context);
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        ui.fonts_mut(|fonts| {
            for family in [
                FontFamily::Proportional,
                FontFamily::Name("semibold".into()),
                FontFamily::Monospace,
            ] {
                let font = FontId::new(13.0, family);
                for text in [
                    "הדפסת תמונות שָׁלוֹם",
                    "طباعة الصور",
                    "写真プリント 照片打印 사진 인쇄",
                    "फ़ोटो प्रिंट",
                    "Фотопечать Εκτύπωση φωτογραφιών café",
                ] {
                    for character in text.chars() {
                        assert!(
                            fonts.has_glyph(&font, character),
                            "{:?} is missing {character:?} (U+{:04X})",
                            font.family,
                            character as u32
                        );
                    }
                    let galley = fonts.layout(text.to_owned(), font.clone(), Color32::WHITE, 300.0);
                    assert_eq!(galley.text(), text);
                    assert!(galley.size().x > 0.0);
                    assert!(galley.size().x.is_finite());
                }
            }
        });
    });
}
