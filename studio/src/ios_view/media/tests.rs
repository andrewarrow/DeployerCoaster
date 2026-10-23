use super::*;
use serde_json::json;

pub(super) fn preview(details: &AppDetails, locale: &str) -> Gallery {
    let mut gallery = Gallery::default();
    gallery.select(Selection {
        app: attr(&details.app, "id").into(),
        version: attr(details.version.as_ref().unwrap(), "id").into(),
        locale: locale.into(),
    });
    let assets: Vec<_> = (1..=3).map(|n| json!({
        "id":format!("screen-{n}"), "fileName":format!("{n:02}-fieldnotes-journal-screenshot.png"),
        "imageAsset":{"templateUrl":format!("https://example.invalid/shot-{n}/{{w}}x{{h}}.{{f}}"),"width":1290,"height":2796},
        "assetDeliveryState":{"state":"COMPLETE"}
    })).collect();
    let poster = json!({
        "id":"preview-1", "fileName":"Fieldnotes app preview.mov",
        "previewFrameImage":{"image":{"templateUrl":"https://example.invalid/preview/{w}x{h}.{f}","width":1290,"height":2796}},
        "videoDeliveryState":{"state":"COMPLETE"}, "videoUrl":"https://example.invalid/preview.mov"
    });
    let placements: Vec<_> = [(media::HEADER_PLACEMENT, "header"), (media::SEARCH_PLACEMENT, "search")]
        .into_iter()
        .map(|(placement_type, name)| PlacedImage {
            placement: json!({"id":format!("{name}-placement"), "placementType":placement_type, "mediaType":"IMAGE", "state":"PARENT_APPROVED"}),
            image: Ok(json!({
                "id":format!("{name}-image"), "fileName":format!("Fieldnotes {name} image.png"), "state":"APPROVED",
                "imageAsset":{"templateUrl":format!("https://example.invalid/{name}/{{w}}x{{h}}.{{f}}"), "width":if name == "header" { 4320 } else { 1920 }, "height":1080}
            })),
        })
        .collect();
    // Local fixture pixels keep rendering and interaction tests independent of the network.
    for (n, (asset, kind)) in assets
        .iter()
        .map(|a| (a, Kind::Screenshot))
        .chain(std::iter::once((&poster, Kind::Preview)))
        .chain(
            placements
                .iter()
                .map(|p| (p.image.as_ref().unwrap(), Kind::LibraryImage)),
        )
        .enumerate()
    {
        let (width, height) = if kind == Kind::LibraryImage {
            (
                320,
                (320 * asset["imageAsset"]["height"].as_u64().unwrap()
                    / asset["imageAsset"]["width"].as_u64().unwrap()) as usize,
            )
        } else {
            (180, 390)
        };
        let background = egui::Color32::from_rgb(245, 243, 236);
        let mut pixels = ColorImage::filled([width, height], background);
        let ink = egui::Color32::from_rgb(70, 80, 67);
        for y in 40..height - 30 {
            for x in 16..width - 16 {
                if (y < 68 && x < 120) || (y > 100 && (y - 100) % 24 < 3 && x < 130 + n * 4) {
                    pixels[(x, y)] = ink;
                }
            }
        }
        gallery.images.insert(
            media::image_url(asset, kind).unwrap().into(),
            CachedImage {
                result: Some(Ok(pixels)),
                ..Default::default()
            },
        );
    }
    gallery.result = Some(Ok(Media {
        screenshots: Ok(vec![AssetSet {
            display_type: "APP_IPHONE_67".into(),
            assets: Ok(assets),
        }]),
        previews: Ok(vec![AssetSet {
            display_type: "IPHONE_67".into(),
            assets: Ok(vec![poster]),
        }]),
        placements: Ok(placements),
    }));
    gallery
}

#[test]
fn selection_changes_cancel_jobs_and_discard_images_from_previous_locales_and_versions() {
    let selection = Selection {
        app: "123".into(),
        version: "draft".into(),
        locale: "en-US".into(),
    };
    for next in [
        Selection {
            locale: "fr-FR".into(),
            ..selection.clone()
        },
        Selection {
            version: "live".into(),
            ..selection.clone()
        },
        Selection {
            app: "456".into(),
            ..selection.clone()
        },
    ] {
        let mut gallery = Gallery::default();
        gallery.select(selection.clone());
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        gallery.job = Some(Job {
            result,
            cancelled: cancelled.clone(),
        });
        sender
            .send(Ok(Media {
                screenshots: Ok(vec![]),
                previews: Ok(vec![]),
                placements: Ok(vec![]),
            }))
            .ok();
        gallery
            .images
            .insert("old-image".into(), CachedImage::default());
        gallery.opened = Some((json!({"id":"old"}), Kind::Screenshot));
        gallery.select(next);
        gallery.poll();
        assert!(cancelled.load(Ordering::Relaxed));
        assert!(gallery.result.is_none());
        assert!(gallery.images.is_empty());
        assert!(gallery.opened.is_none());
    }
}

#[test]
fn keeps_cached_media_when_reopening_the_same_selection() {
    let mut gallery = Gallery::default();
    let selection = Selection {
        app: "123".into(),
        version: "draft".into(),
        locale: "en-US".into(),
    };
    gallery.select(selection.clone());
    gallery.result = Some(Ok(Media {
        screenshots: Ok(vec![]),
        previews: Ok(vec![]),
        placements: Ok(vec![]),
    }));
    gallery.select(selection);
    assert!(gallery.result.is_some());
    assert!(gallery.job.is_none());
}

#[test]
fn cached_textures_follow_the_native_context_and_cache_is_bounded() {
    let mut image = CachedImage {
        result: Some(Ok(ColorImage::filled([2, 2], egui::Color32::WHITE))),
        ..Default::default()
    };
    let first = Context::default();
    let second = Context::default();
    image.texture(&first).unwrap();
    assert!(image.texture.as_ref().unwrap().0 == first);
    image.texture(&second).unwrap();
    assert!(image.texture.as_ref().unwrap().0 == second);
    let mut gallery = Gallery::default();
    for n in 0..40 {
        gallery.images.insert(
            n.to_string(),
            CachedImage {
                result: Some(Err("missing".into())),
                used: n,
                ..Default::default()
            },
        );
    }
    gallery.trim_images();
    assert_eq!(gallery.images.len(), 32);
    assert!(!gallery.images.contains_key("0"));
    assert!(gallery.images.contains_key("39"));
}

#[test]
fn describes_devices_and_preserves_unknown_display_types() {
    assert_eq!(display_name("APP_IPHONE_67"), "iPhone 6.7\"");
    assert_eq!(
        display_name("IMESSAGE_APP_IPAD_PRO_3GEN_129"),
        "iMessage · iPad Pro 12.9\" (3rd generation)"
    );
    assert_eq!(display_name("SOME_FUTURE_DEVICE"), "SOME_FUTURE_DEVICE");
    assert_eq!(
        fit(vec2(100.0, 200.0), vec2(180.0, 228.0)),
        vec2(114.0, 228.0)
    );
}

fn render(
    ctx: &Context,
    gallery: &mut Gallery,
    details: &AppDetails,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let credentials = AppleCredentials::test_credentials("gallery-tests");
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                gallery.ui(
                    ui,
                    details,
                    "en-US",
                    &credentials,
                    Palette::new(ui.visuals().dark_mode),
                    true,
                );
            });
        },
    )
}

fn render_ready(
    ctx: &Context,
    gallery: &mut Gallery,
    details: &AppDetails,
    size: egui::Vec2,
) -> egui::FullOutput {
    let credentials = AppleCredentials::test_credentials("gallery-tests");
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                gallery.ui(
                    ui,
                    details,
                    "en-US",
                    &credentials,
                    Palette::new(ui.visuals().dark_mode),
                    false,
                );
            });
        },
    )
}

fn labels(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

#[test]
fn screenshot_header_actions_fit_at_all_supported_sizes_in_both_themes() {
    let details = super::super::preview::preview_details();
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for size in [
            vec2(390.0, 844.0),
            vec2(768.0, 1024.0),
            vec2(1280.0, 800.0),
            vec2(1440.0, 900.0),
        ] {
            let ctx = Context::default();
            crate::style::configure(&ctx);
            ctx.set_theme(theme);
            let mut gallery = preview(&details, "en-US");
            let output = render(&ctx, &mut gallery, &details, size, vec![]);
            let text = labels(&output);
            for label in ["Screenshots", "Resize", "Reload media"] {
                assert!(text.contains(&label), "Missing {label} at {size:?}");
            }
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape
                    && ["Screenshots", "Resize", "Reload media"].contains(&text.galley.text())
                {
                    assert!(text.pos.x >= 0.0 && text.pos.x + text.galley.size().x <= size.x);
                }
            }
        }
    }
}

#[test]
fn thumbnails_open_an_enlarged_image_and_escape_closes_it_at_all_supported_sizes() {
    let details = super::super::preview::preview_details();
    for size in [
        vec2(390.0, 844.0),
        vec2(768.0, 1024.0),
        vec2(1280.0, 800.0),
        vec2(1440.0, 900.0),
    ] {
        let ctx = Context::default();
        crate::style::configure(&ctx);
        let mut gallery = preview(&details, "en-US");
        let mut output = render(&ctx, &mut gallery, &details, size, vec![]);
        for _ in 0..2 {
            output = render(&ctx, &mut gallery, &details, size, vec![]);
        }
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.brush.is_some() && rect.rect.height() > 100.0 => {
                    Some(rect.rect.center())
                }
                _ => None,
            })
            .expect("gallery image");
        for pressed in [true, false] {
            render(
                &ctx,
                &mut gallery,
                &details,
                size,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(
            gallery.opened.is_some(),
            "thumbnail did not open at {size:?}"
        );
        for _ in 0..3 {
            output = render(&ctx, &mut gallery, &details, size, vec![]);
        }
        assert!(labels(&output).contains(&"Close"));
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == "Close"
            {
                assert!(text.pos.x >= 0.0 && text.pos.x + text.galley.size().x <= size.x);
                assert!(text.pos.y >= 0.0 && text.pos.y + text.galley.size().y <= size.y);
            }
        }
        render(
            &ctx,
            &mut gallery,
            &details,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            gallery.opened.is_none(),
            "escape did not dismiss at {size:?}"
        );
    }
}

#[test]
fn gallery_distinguishes_loading_empty_and_failed_reads_and_poster_processing() {
    let ctx = Context::default();
    crate::style::configure(&ctx);
    let details = super::super::preview::preview_details();
    let mut gallery = preview(&details, "en-US");
    gallery.result = None;
    let output = render(&ctx, &mut gallery, &details, vec2(768.0, 1024.0), vec![]);
    assert!(labels(&output).contains(&"Loading media…"));
    assert!(gallery.job.is_none());
    gallery.result = Some(Ok(Media {
        screenshots: Ok(vec![]),
        previews: Ok(vec![]),
        placements: Ok(vec![]),
    }));
    let output = render(&ctx, &mut gallery, &details, vec2(768.0, 1024.0), vec![]);
    assert!(labels(&output).contains(&"No screenshots for this localization."));
    assert!(labels(&output).contains(&"No app previews for this localization."));
    assert!(labels(&output).contains(&"No header images for this localization."));
    assert!(labels(&output).contains(&"No search images for this localization."));
    gallery.result = Some(Ok(Media {
        screenshots: Err("Access was denied.".into()),
        previews: Ok(vec![]),
        placements: Err("App Store Connect is temporarily unavailable. Please retry.".into()),
    }));
    let output = render(&ctx, &mut gallery, &details, vec2(768.0, 1024.0), vec![]);
    assert!(labels(&output).contains(&"Access was denied."));
    assert!(!labels(&output).contains(&"No screenshots for this localization."));
    assert!(
        labels(&output).contains(&"App Store Connect is temporarily unavailable. Please retry.")
    );
    assert!(!labels(&output).contains(&"No header images for this localization."));
    let poster = json!({"videoDeliveryState":{"state":"COMPLETE"},"previewFrameImage":{"state":{"state":"PROCESSING"}}});
    assert!(processing(&poster, Kind::Preview));
    assert_eq!(delivery_state(&poster, Kind::Preview), "PROCESSING");
}

#[test]
fn image_generation_controls_appear_only_for_confirmed_empty_placements() {
    let details = super::super::preview::preview_details();
    let sizes = [
        vec2(390.0, 844.0),
        vec2(768.0, 1024.0),
        vec2(1280.0, 800.0),
        vec2(1440.0, 900.0),
    ];
    for size in sizes {
        let ctx = Context::default();
        crate::style::configure(&ctx);
        let mut gallery = creative_gallery(&details);
        let Some(Ok(media)) = &mut gallery.result else {
            unreachable!()
        };
        media.placements = Ok(vec![]);
        let output = render(&ctx, &mut gallery, &details, size, vec![]);
        let text = labels(&output);
        assert!(
            text.contains(&"Generate and upload header"),
            "{size:?}: {text:?}"
        );
        assert!(
            text.contains(&"Generate and upload search"),
            "{size:?}: {text:?}"
        );

        let Some(Ok(media)) = &mut gallery.result else {
            unreachable!()
        };
        media.placements = Err("App Store Connect is unavailable.".into());
        let output = render(&ctx, &mut gallery, &details, size, vec![]);
        let text = labels(&output);
        assert!(
            !text.contains(&"Generate and upload header"),
            "{size:?}: {text:?}"
        );
        assert!(
            !text.contains(&"Generate and upload search"),
            "{size:?}: {text:?}"
        );

        let Some(Ok(media)) = &mut gallery.result else {
            unreachable!()
        };
        media.placements = Ok(preview(&details, "en-US")
            .result
            .unwrap()
            .unwrap()
            .placements
            .unwrap());
        let output = render(&ctx, &mut gallery, &details, size, vec![]);
        let text = labels(&output);
        assert!(
            !text.contains(&"Generate and upload header"),
            "{size:?}: {text:?}"
        );
        assert!(
            !text.contains(&"Generate and upload search"),
            "{size:?}: {text:?}"
        );
    }
}

#[test]
fn image_briefs_survive_media_reload_and_clear_when_the_selection_changes() {
    let details = super::super::preview::preview_details();
    let ctx = Context::default();
    crate::style::configure(&ctx);
    let mut gallery = creative_gallery(&details);
    gallery.generation.briefs = ["A warm pulse motif".into(), "A calm search tile".into()];

    gallery.result = None;
    let _ = render(&ctx, &mut gallery, &details, vec2(768.0, 1024.0), vec![]);
    gallery.result = Some(Ok(Media {
        screenshots: Ok(vec![]),
        previews: Ok(vec![]),
        placements: Ok(vec![]),
    }));
    let _ = render(&ctx, &mut gallery, &details, vec2(768.0, 1024.0), vec![]);
    assert_eq!(gallery.generation.briefs[0], "A warm pulse motif");
    assert_eq!(gallery.generation.briefs[1], "A calm search tile");

    gallery.select(Selection {
        app: "another-app".into(),
        version: attr(details.version.as_ref().unwrap(), "id").into(),
        locale: "en-US".into(),
    });
    assert_eq!(gallery.generation.briefs, [String::new(), String::new()]);
}

fn creative_gallery(details: &AppDetails) -> Gallery {
    let mut gallery = preview(details, "en-US");
    let Some(Ok(media)) = &mut gallery.result else {
        unreachable!()
    };
    media.screenshots = Ok(vec![]);
    media.previews = Ok(vec![]);
    gallery
}

#[test]
fn header_and_search_images_open_and_fit_the_viewport_in_both_themes() {
    let details = super::super::preview::preview_details();
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for size in [
            vec2(390.0, 844.0),
            vec2(768.0, 1024.0),
            vec2(1280.0, 800.0),
            vec2(1440.0, 900.0),
        ] {
            let ctx = Context::default();
            crate::style::configure(&ctx);
            ctx.set_theme(theme);
            let mut gallery = creative_gallery(&details);
            for index in 0..2 {
                let mut output = render(&ctx, &mut gallery, &details, size, vec![]);
                for _ in 0..2 {
                    output = render(&ctx, &mut gallery, &details, size, vec![]);
                }
                for label in [
                    "Header images",
                    "Search images",
                    "Fieldnotes header image.png",
                    "Fieldnotes search image.png",
                ] {
                    assert!(
                        labels(&output).contains(&label),
                        "Missing {label} at {size:?}"
                    );
                }
                let rects: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.brush.is_some() => Some(rect.rect),
                        _ => None,
                    })
                    .collect();
                assert_eq!(rects.len(), 2);
                for (rect, aspect) in rects.iter().zip([4.0, 16.0 / 9.0]) {
                    assert!(rect.min.x >= 0.0 && rect.max.x <= size.x);
                    assert!(
                        (rect.width() - rect.height() * aspect).abs() <= 3.0,
                        "Image rect {rect:?} lost its aspect ratio at {size:?}"
                    );
                }
                let position = rects[index].center();
                for pressed in [true, false] {
                    render(
                        &ctx,
                        &mut gallery,
                        &details,
                        size,
                        vec![
                            egui::Event::PointerMoved(position),
                            egui::Event::PointerButton {
                                pos: position,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                assert_eq!(gallery.opened.as_ref().unwrap().1, Kind::LibraryImage);
                for _ in 0..3 {
                    output = render(&ctx, &mut gallery, &details, size, vec![]);
                }
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && text.galley.text() == "Close"
                    {
                        assert!(text.pos.x >= 0.0 && text.pos.x + text.galley.size().x <= size.x);
                        assert!(text.pos.y >= 0.0 && text.pos.y + text.galley.size().y <= size.y);
                    }
                }
                render(
                    &ctx,
                    &mut gallery,
                    &details,
                    size,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert!(gallery.opened.is_none());
            }
        }
    }
}

#[test]
fn library_images_show_processing_and_keep_failures_next_to_the_affected_placement() {
    let details = super::super::preview::preview_details();
    let mut gallery = creative_gallery(&details);
    let Some(Ok(media)) = &mut gallery.result else {
        unreachable!()
    };
    let placements = media.placements.as_mut().unwrap();
    placements[0].image =
        Ok(json!({"id":"processing", "referenceName":"Header artwork", "state":"UPLOAD_COMPLETE"}));
    placements[0].placement["state"] = json!("ASSET_PROCESSING");
    placements[1].image = Err("Access was denied.".into());
    let ctx = Context::default();
    crate::style::configure(&ctx);
    let output = render(&ctx, &mut gallery, &details, vec2(390.0, 844.0), vec![]);
    let text = labels(&output);
    for label in [
        "Processing…",
        "Header artwork",
        "Placement: Asset Processing",
        "Access was denied.",
    ] {
        assert!(text.contains(&label), "Missing {label}: {text:?}");
    }
    assert!(!text.contains(&"No header images for this localization."));
    assert!(!text.contains(&"No search images for this localization."));
}

#[test]
#[ignore = "requires an available offscreen GPU adapter"]
fn render_header_and_search_image_previews() {
    let details = super::super::preview::preview_details();
    let output_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&output_dir).unwrap();
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for [width, height] in [[390, 844], [768, 1024], [1280, 800], [1440, 900]] {
            let ctx = Context::default();
            crate::style::configure(&ctx);
            ctx.set_pixels_per_point(2.0);
            ctx.set_theme(theme);
            let mut gallery = creative_gallery(&details);
            let mut textures = Vec::new();
            let mut latest = None;
            for _ in 0..3 {
                let mut output = render(
                    &ctx,
                    &mut gallery,
                    &details,
                    vec2(width as f32, height as f32),
                    vec![],
                );
                textures.append(&mut output.textures_delta.set);
                latest = Some(output);
            }
            let theme_name = if theme == egui::Theme::Light {
                "light"
            } else {
                "dark"
            };
            let path = output_dir.join(format!("ios-creative-images-{theme_name}-{width}.png"));
            super::super::preview::screenshot(
                &ctx,
                latest.unwrap(),
                textures,
                path.to_str().unwrap(),
                width * 2,
                height * 2,
            );
        }
    }
}

#[test]
#[ignore = "requires an available offscreen GPU adapter"]
fn render_empty_header_and_search_generation_controls() {
    let details = super::super::preview::preview_details();
    let output_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&output_dir).unwrap();
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for [width, height] in [[390, 844], [768, 1024], [1280, 800], [1440, 900]] {
            let ctx = Context::default();
            crate::style::configure(&ctx);
            ctx.set_pixels_per_point(2.0);
            ctx.set_theme(theme);
            let mut gallery = creative_gallery(&details);
            let Some(Ok(media)) = &mut gallery.result else {
                unreachable!()
            };
            media.screenshots = Ok(vec![]);
            media.previews = Ok(vec![]);
            media.placements = Ok(vec![]);
            let mut textures = Vec::new();
            let mut latest = None;
            for _ in 0..3 {
                let mut output = render_ready(
                    &ctx,
                    &mut gallery,
                    &details,
                    vec2(width as f32, height as f32),
                );
                let rendered_labels = labels(&output);
                assert!(rendered_labels.contains(&"Generate and upload header"));
                assert!(rendered_labels.contains(&"Generate and upload search"));
                textures.append(&mut output.textures_delta.set);
                latest = Some(output);
            }
            let theme_name = if theme == egui::Theme::Light {
                "light"
            } else {
                "dark"
            };
            let path = output_dir.join(format!("ios-empty-generation-{theme_name}-{width}.png"));
            super::super::preview::screenshot(
                &ctx,
                latest.unwrap(),
                textures,
                path.to_str().unwrap(),
                width * 2,
                height * 2,
            );
        }
    }
}
