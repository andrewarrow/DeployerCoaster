use super::*;
use egui::{Frame, Margin, Pos2, Rect, vec2};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor, wgpu};
use serde_json::json;
use std::{fs, path::PathBuf};

const APP_ID: &str = "6743921801";

pub(super) fn preview_details() -> AppDetails {
    AppDetails {
        app: json!({
            "id": APP_ID,
            "name": "Fieldnotes",
            "bundleId": "com.fieldnotes.ios",
            "sku": "fieldnotes-ios",
            "primaryLocale": "en-US",
            "contentRightsDeclaration": "DOES_NOT_USE_THIRD_PARTY_CONTENT"
        }),
        info: Some(json!({
            "id": "info-draft",
            "state": "PREPARE_FOR_SUBMISSION",
            "primaryCategoryId": "PRODUCTIVITY",
            "secondaryCategoryId": "LIFESTYLE"
        })),
        age_rating: Some(json!({
            "id": "rating-24",
            "appId": APP_ID,
            "appInfoId": "info-draft",
            "advertising": false,
            "alcoholTobaccoOrDrugUseOrReferences": "NONE",
            "contests": "NONE",
            "gambling": false,
            "gamblingSimulated": "NONE",
            "gunsOrOtherWeapons": "NONE",
            "healthOrWellnessTopics": true,
            "kidsAgeBand": "NINE_TO_ELEVEN",
            "lootBox": false,
            "medicalOrTreatmentInformation": "NONE",
            "messagingAndChat": true,
            "parentalControls": false,
            "profanityOrCrudeHumor": "INFREQUENT_OR_MILD",
            "ageAssurance": false,
            "sexualContentGraphicAndNudity": "NONE",
            "sexualContentOrNudity": "NONE",
            "socialMedia": false,
            "socialMediaAgeRestricted": false,
            "horrorOrFearThemes": "NONE",
            "matureOrSuggestiveThemes": "INFREQUENT_OR_MILD",
            "unrestrictedWebAccess": false,
            "userGeneratedContent": true,
            "violenceCartoonOrFantasy": "NONE",
            "violenceRealisticProlongedGraphicOrSadistic": "NONE",
            "violenceRealistic": "NONE",
            "ageRatingOverrideV2": "THIRTEEN_PLUS",
            "koreaAgeRatingOverride": "ALL",
            "gracRatingClassificationNumber": "GRAC-0000-0000",
            "developerAgeRatingInfoUrl": "https://fieldnotes.app/legal/age-rating-information"
        })),
        version: Some(json!({
            "id": "release-24",
            "platform": "IOS",
            "versionString": "2.4.0",
            "appVersionState": "PREPARE_FOR_SUBMISSION",
            "copyright": "2026 Fieldnotes Studio",
            "createdDate": "2026-10-13T12:00:00Z"
        })),
        versions: vec![
            json!({
                "id": "release-24",
                "platform": "IOS",
                "versionString": "2.4.0",
                "appVersionState": "PREPARE_FOR_SUBMISSION",
                "createdDate": "2026-10-13T12:00:00Z"
            }),
            json!({
                "id": "release-23",
                "platform": "IOS",
                "versionString": "2.3.1",
                "appVersionState": "READY_FOR_SALE",
                "createdDate": "2026-08-21T09:30:00Z"
            }),
        ],
        info_localizations: vec![
            json!({
                "id": "info-en-us",
                "locale": "en-US",
                "name": "Fieldnotes — Everyday Journal",
                "subtitle": "A little space for your thoughts",
                "privacyPolicyUrl": "https://fieldnotes.app/privacy",
                "privacyChoicesUrl": "https://fieldnotes.app/privacy/choices"
            }),
            json!({
                "id": "info-fr-fr",
                "locale": "fr-FR",
                "name": "Fieldnotes — Journal quotidien",
                "subtitle": "Un peu d'espace pour vos pensées",
                "privacyPolicyUrl": "https://fieldnotes.app/fr/privacy",
                "privacyChoicesUrl": "https://fieldnotes.app/fr/privacy/choices"
            }),
        ],
        version_localizations: vec![
            json!({
                "id": "listing-en-us",
                "locale": "en-US",
                "description": "Keep a record of the moments you want to remember. Fieldnotes brings your writing, photos, and ideas together in one simple journal.\n\nMake it your own with daily prompts, flexible tags, and a calendar that helps you look back. Your entries stay private, with secure sync across your devices.\n\nWhether you're capturing a quick thought or settling in for a longer reflection, there's always room for another page.",
                "keywords": "journal,diary,notes,writing,reflection,mindfulness",
                "promotionalText": "Meet your new daily writing habit. Now with photo journals and quick capture.",
                "whatsNew": "• Add multiple photos to any entry.\n• Find past entries faster with improved search.\n• More reliable sync when you're offline.",
                "supportUrl": "https://fieldnotes.app/support",
                "marketingUrl": "https://fieldnotes.app"
            }),
            json!({
                "id": "listing-fr-fr",
                "locale": "fr-FR",
                "description": "Gardez une trace des moments que vous souhaitez retenir. Fieldnotes réunit vos écrits, vos photos et vos idées dans un journal simple et élégant.\n\nPersonnalisez votre expérience avec des suggestions quotidiennes, des tags flexibles et un calendrier pour retrouver vos souvenirs. Vos notes restent privées et se synchronisent en toute sécurité.",
                "keywords": "journal,intime,notes,ecriture,reflexion,bien-etre",
                "promotionalText": "Adoptez une nouvelle habitude d'écriture, avec des journaux photo et une capture rapide.",
                "whatsNew": "• Ajoutez plusieurs photos à chaque entrée.\n• Retrouvez vos souvenirs plus vite grâce à la recherche améliorée.",
                "supportUrl": "https://fieldnotes.app/fr/support",
                "marketingUrl": "https://fieldnotes.app/fr"
            }),
        ],
        review_contact: Some(Ok(Some(json!({
            "id": "review", "contactFirstName": "Alex", "contactLastName": "Example",
            "contactPhone": "+1 555 0100", "contactEmail": "review@fieldnotes.app"
        })))),
        ..Default::default()
    }
}

pub(super) fn screenshot(
    ctx: &egui::Context,
    output: egui::FullOutput,
    textures: Vec<(egui::TextureId, egui::epaint::ImageDelta)>,
    path: &str,
    width: u32,
    height: u32,
) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ios-workspace-preview"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = Renderer::new(&device, format, RendererOptions::default());
    for (id, delta) in &textures {
        renderer.update_texture(&device, &queue, *id, delta);
    }
    let screen = ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point: output.pixels_per_point,
    };
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let mut encoder = device.create_command_encoder(&Default::default());
    let buffers = renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
    let view = texture.create_view(&Default::default());
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
    }
    let padded_row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit(buffers.into_iter().chain([encoder.finish()]));
    let (tx, rx) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks(padded_row as usize) {
        pixels.extend_from_slice(&row[..(width * 4) as usize]);
    }
    image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8).unwrap();
}

#[test]
#[ignore = "requires an available offscreen GPU adapter"]
fn render_ios_workspace_previews() {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    fs::create_dir_all(&output_dir).unwrap();
    let credentials = AppleCredentials::test_credentials("preview");

    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for (document, widths) in [
            (Document::Information, &[390, 768, 1280, 1440][..]),
            (Document::Listing, &[390, 768, 1280, 1440][..]),
            (Document::AgeRating, &[390, 768, 1280, 1440][..]),
            (Document::Media, &[390, 768, 1280, 1440][..]),
            (Document::Privacy, &[1280][..]),
        ] {
            for &width in widths {
                let height = if width == 390 {
                    844
                } else if width == 768 {
                    1024
                } else if width == 1280 {
                    800
                } else {
                    900
                };
                let ctx = egui::Context::default();
                crate::style::configure(&ctx);
                // Inspect text and system symbol rendering at native Retina scale.
                ctx.set_pixels_per_point(2.0);
                ctx.set_theme(theme);
                ctx.style_mut_of(theme, |style| style.animation_time = 0.0);

                let mut view = IosView::default();
                view.select(APP_ID);
                let mut details = preview_details();
                if document == Document::Privacy {
                    details.info_localizations[1]["privacyPolicyUrl"] = serde_json::Value::Null;
                    details.age_rating = None;
                }
                view.cache.insert(
                    APP_ID.into(),
                    Entry {
                        data: Some(details),
                        error: None,
                    },
                );
                let details = view.cache.get(APP_ID).unwrap().data.as_ref().unwrap();
                if document == Document::Media {
                    view.media = media::Gallery::preview(details, "en-US");
                }
                let workspace = view.workspaces.entry(APP_ID.into()).or_default();
                workspace.reconcile(details);
                workspace.open(document);
                if document == Document::Privacy {
                    workspace.filter = "Trust & capabilities".into();
                }
                if document == Document::Listing {
                    let source = details.version_localizations[0].get("description");
                    workspace.edit(
                        &format!("/apps/{APP_ID}/versions/release-24/localizations/en-US"),
                        "description",
                        source,
                        format!(
                            "{}\n\nFind the right entry faster with the new search filters.",
                            source
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                        ),
                        "Description",
                        "iOS 2.4.0",
                    );
                }

                let mut textures = Vec::new();
                let mut latest = None;
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                Pos2::ZERO,
                                vec2(width as f32, height as f32),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            egui::CentralPanel::default()
                                .frame(Frame::new().inner_margin(Margin::ZERO))
                                .show(ui, |ui| {
                                    view.ui(ui, Some((APP_ID, "Fieldnotes")), Some(&credentials));
                                });
                        },
                    );
                    textures.append(&mut output.textures_delta.set);
                    latest = Some(output);
                }
                let theme_name = if theme == egui::Theme::Light {
                    "light"
                } else {
                    "dark"
                };
                let document_name = match document {
                    Document::Information => "information",
                    Document::Listing => "listing",
                    Document::AgeRating => "age-rating",
                    Document::Media => "media",
                    Document::Privacy => "missing-data",
                    _ => unreachable!(),
                };
                let path = output_dir.join(format!(
                    "ios-workspace-{document_name}-{theme_name}-{width}.png"
                ));
                screenshot(
                    &ctx,
                    latest.unwrap(),
                    textures,
                    path.to_str().unwrap(),
                    width * 2,
                    height * 2,
                );
                println!("Saved {}", path.display());
            }
        }
    }
}
