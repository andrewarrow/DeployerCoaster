use super::*;
use egui::{Frame, Margin, Pos2, Rect, Shape, vec2};
use serde_json::{Value, json};

pub(super) fn fixture() -> AppDetails {
    AppDetails {
        app: json!({"id": "123", "name": "A long application name for layout testing", "bundleId": "com.example.this.is.a.deliberately.long.bundle.identifier.with.several.segments.to.check.wrapping.behavior", "sku": "example", "primaryLocale": "en-US", "contentRightsDeclaration": "DOES_NOT_USE_THIRD_PARTY_CONTENT"}),
        info: Some(
            json!({"id": "info-draft", "state": "PREPARE_FOR_SUBMISSION", "primaryCategoryId": "UTILITIES", "secondaryCategoryId": "PRODUCTIVITY"}),
        ),
        age_rating: Some(json!({
            "id": "rating-draft",
            "advertising": false,
            "gambling": true,
            "gamblingSimulated": "FREQUENT_OR_INTENSE",
            "ageRatingOverrideV2": "THIRTEEN_PLUS",
            "kidsAgeBand": "SIX_TO_EIGHT",
            "gracRatingClassificationNumber": "GRAC-AB-0123",
            "developerAgeRatingInfoUrl": "https://example.com/age-rating/information/with/a/long/path",
            "horrorOrFearThemes": null,
            "violenceRealistic": "UNRECOGNIZED_FUTURE_VALUE"
        })),
        version: Some(
            json!({"id": "version-draft", "platform": "IOS", "versionString": "2.0", "appVersionState": "PREPARE_FOR_SUBMISSION", "copyright": "2026 Example", "createdDate": "2026-10-13T12:00:00Z"}),
        ),
        versions: vec![
            json!({"id": "version-draft", "platform": "IOS", "versionString": "2.0", "appVersionState": "PREPARE_FOR_SUBMISSION", "createdDate": "2026-10-13T12:00:00Z"}),
            json!({"id": "version-live", "platform": "IOS", "versionString": "1.9.2", "appVersionState": "READY_FOR_SALE", "createdDate": "2026-08-01T12:00:00Z"}),
        ],
        info_localizations: vec![
            json!({"id": "info-en", "locale": "en-US", "name": "A long localized application name with several words", "subtitle": "An English subtitle", "privacyPolicyUrl": "https://example.com/a/very/long/privacy/policy/address/with/a/path/that/keeps/going/and/going/without/a/short/word-break", "privacyChoicesUrl": "https://example.com/a/very/long/privacy/choices/address/with/a/path/that/keeps/going/and/going"}),
            json!({"id": "info-fr", "locale": "fr-FR", "name": "Un nom d'application localisé avec plusieurs mots", "subtitle": "Un sous-titre en français", "privacyPolicyUrl": "https://example.com/fr/politique-de-confidentialite", "privacyChoicesUrl": "https://example.com/fr/choix-de-confidentialite"}),
        ],
        version_localizations: vec![
            json!({"id": "version-en", "locale": "en-US", "description": "An English description with multiple paragraphs.\n\nThis is another paragraph with enough words to exercise wrapping in narrow windows, with a long editorial sentence that runs through the whole width of the content area.", "keywords": "example,app,test", "promotionalText": "Try our latest features", "whatsNew": "New features", "supportUrl": "https://example.com/support", "marketingUrl": "https://example.com/marketing"}),
            json!({"id": "version-fr", "locale": "fr-FR", "description": "Une description en français avec plusieurs paragraphes.\n\nCette deuxième section contient suffisamment de mots pour vérifier le retour à la ligne sur les petits écrans.", "keywords": "exemple,application,test", "promotionalText": "Découvrez les dernières nouveautés", "whatsNew": "De nouvelles fonctionnalités", "supportUrl": "https://example.com/fr/aide", "marketingUrl": "https://example.com/fr"}),
        ],
        review_contact: Some(Ok(Some(json!({
            "id": "contact", "contactFirstName": "Alex", "contactLastName": "Example",
            "contactPhone": "+1 555 0100", "contactEmail": "review@example.com"
        })))),
        ..Default::default()
    }
}

#[test]
fn missing_data_checks_all_languages_and_ignores_optional_fields_and_failed_reads() {
    let mut details = fixture();
    for document in [
        Document::Information,
        Document::Listing,
        Document::Versions,
        Document::Privacy,
    ] {
        assert!(document.missing_data(&details).is_none(), "{document:?}");
    }
    details.info.as_mut().unwrap()["secondaryCategoryId"] = Value::Null;
    for locale in &mut details.version_localizations {
        for field in ["marketingUrl", "promotionalText", "whatsNew"] {
            locale[field] = Value::Null;
        }
    }
    assert!(Document::Information.missing_data(&details).is_none());
    assert!(Document::Listing.missing_data(&details).is_none());

    details.info_localizations[1]["privacyPolicyUrl"] = json!("  \n");
    assert!(Document::Privacy.missing_data(&details).is_some());
    details
        .warnings
        .push("App localizations: Access was denied.".into());
    assert!(Document::Privacy.missing_data(&details).is_none());
    details.app["contentRightsDeclaration"] = Value::Null;
    assert!(Document::Privacy.missing_data(&details).is_some());

    details.version_localizations[1]["description"] = Value::Null;
    assert!(Document::Listing.missing_data(&details).is_some());
    details
        .warnings
        .push("Version localizations: Temporarily unavailable.".into());
    assert!(Document::Listing.missing_data(&details).is_none());
    details.warnings.clear();
    details.info_localizations.pop();
    assert!(Document::Listing.missing_data(&details).is_some());
    details.version = None;
    assert!(Document::Versions.missing_data(&details).is_some());
    details
        .warnings
        .push("iOS version: Access was denied.".into());
    assert!(Document::Versions.missing_data(&details).is_none());

    for (_, documents) in documents::GROUPS {
        for document in *documents {
            if !document.connected() {
                assert!(document.missing_data(&details).is_none(), "{document:?}");
            }
        }
    }
}

#[test]
fn age_rating_answers_accept_false_and_none_but_detect_unanswered_questions() {
    let mut details = super::preview::preview_details();
    assert!(Document::AgeRating.missing_data(&details).is_none());
    details.age_rating.as_mut().unwrap()["gambling"] = json!(false);
    details.age_rating.as_mut().unwrap()["ageRatingOverrideV2"] = Value::Null;
    details.age_rating.as_mut().unwrap()["kidsAgeBand"] = Value::Null;
    assert!(Document::AgeRating.missing_data(&details).is_none());
    details.age_rating.as_mut().unwrap()["advertising"] = Value::Null;
    assert!(Document::AgeRating.missing_data(&details).is_some());
    details.age_rating = None;
    assert!(Document::AgeRating.missing_data(&details).is_some());
    details
        .warnings
        .push("Age rating: Access was denied.".into());
    assert!(Document::AgeRating.missing_data(&details).is_none());
}

#[test]
fn review_checks_arrive_after_details_and_are_cancelled_when_switching_apps() {
    let mut view = IosView::default();
    view.select("123");
    let (sender, result) = mpsc::channel();
    view.job = Some(Job {
        result,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    let (review_sender, result) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    view.review_job = Some(Job {
        result,
        cancelled: cancelled.clone(),
    });
    view.poll();
    assert!(view.cache.is_empty());
    let mut details = fixture();
    details.review_contact = None;
    sender.send(Ok(details)).unwrap();
    view.poll();
    assert!(view.job.is_none());
    assert!(view.review_job.is_some());
    assert!(!cancelled.load(Ordering::Relaxed));
    let details = view.cache["123"].data.as_ref().unwrap();
    assert!(Document::Review.missing_data(details).is_none());

    review_sender.send(Ok(None)).unwrap();
    view.poll();
    assert!(
        Document::Review
            .missing_data(view.cache["123"].data.as_ref().unwrap())
            .is_some()
    );
    let (_, result) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    view.review_job = Some(Job {
        result,
        cancelled: cancelled.clone(),
    });
    view.select("456");
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(view.review_job.is_none());
    assert!(!view.cache.contains_key("456"));
}

#[test]
fn navigator_dots_appear_only_for_confirmed_missing_data_without_moving_rows() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        ctx.set_theme(theme);
        let mut workspace = Workspace::default();
        workspace.filter = "Privacy".into();
        workspace.open(Document::Privacy);
        let mut details = fixture();
        details.app["contentRightsDeclaration"] = Value::Null;
        let mut name_positions = Vec::new();
        for (data, expect_dot) in [
            (None, false),
            (Some(&details), true),
            (Some(&fixture()), false),
        ] {
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.set_width(280.0);
                    chrome::navigator(
                        ui,
                        &mut workspace,
                        data,
                        "Example",
                        Palette::new(ui.visuals().dark_mode),
                    );
                });
            });
            let red = if theme == egui::Theme::Dark {
                Color32::from_rgb(255, 69, 58)
            } else {
                Color32::from_rgb(255, 59, 48)
            };
            let dots: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    Shape::Circle(circle) if circle.fill == red && circle.radius == 5.0 => {
                        Some(circle)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(dots.len(), usize::from(expect_dot));
            let text = text_center(&output, "Privacy & Content Rights");
            if expect_dot {
                assert!(dots[0].center.x < text.x);
                assert!((dots[0].center.y - text.y).abs() < 1.0);
            }
            name_positions.push(text);
        }
        assert!(name_positions.windows(2).all(|p| p[0] == p[1]));
    }
}

fn texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

#[test]
fn selecting_another_app_discards_the_previous_job_and_preserves_its_cache() {
    let mut view = IosView::default();
    view.select("123");
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    let (sender, result) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    view.job = Some(Job {
        result,
        cancelled: cancelled.clone(),
    });
    sender.send(Ok(fixture())).unwrap();
    view.select("456");
    view.poll();
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(view.job.is_none());
    assert!(!view.cache.contains_key("456"));
    assert!(view.cache["123"].data.is_some());
    view.select("123");
    assert!(view.cache["123"].data.is_some());
    view.clear_selection();
    assert!(view.selected.is_none());
}

#[test]
fn failed_refresh_preserves_details_and_success_replaces_them() {
    let mut view = IosView::default();
    view.select("123");
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    let (sender, result) = mpsc::channel();
    view.job = Some(Job {
        result,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    sender.send(Err("Refresh failed".into())).unwrap();
    view.poll();
    assert!(view.cache["123"].data.is_some());
    assert_eq!(view.cache["123"].error.as_deref(), Some("Refresh failed"));
    let (sender, result) = mpsc::channel();
    view.job = Some(Job {
        result,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    let mut new_details = fixture();
    new_details.app["sku"] = json!("updated");
    sender.send(Ok(new_details)).unwrap();
    view.poll();
    assert!(view.cache["123"].error.is_none());
    assert_eq!(
        attr(&view.cache["123"].data.as_ref().unwrap().app, "sku"),
        "updated"
    );
}

fn render_document(
    document: Document,
    width: f32,
    height: f32,
    theme: egui::Theme,
    details: AppDetails,
    edit_description: bool,
) -> (Vec<String>, f32, bool) {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    ctx.set_theme(theme);
    // Exercise the actual main-window toolbar inset as well as the document layout.
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("unified_titlebar"), cfg!(target_os = "macos"));
    });
    let credentials = AppleCredentials::test_credentials("test");
    let mut view = IosView::default();
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(details),
            error: None,
        },
    );
    let details = view.cache["123"].data.as_ref().unwrap();
    let workspace = view.workspaces.entry("123".into()).or_default();
    workspace.reconcile(details);
    workspace.open(document);
    if edit_description {
        let source = details.version_localizations[0].get("description");
        workspace.edit(
            "/apps/123/versions/version-draft/localizations/en-US",
            "description",
            source,
            "An edited local description for review.".into(),
            "Description",
            "Version version-draft",
        );
    }
    let mut content_right = 0.0;
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default()
                .frame(Frame::new().inner_margin(Margin::ZERO))
                .show(ui, |ui| {
                    view.ui(ui, Some(("123", "Example")), Some(&credentials));
                    content_right = ui.min_rect().right();
                });
        },
    );
    let labels = texts(&output).into_iter().map(str::to_owned).collect();
    (labels, content_right, view.job.is_none())
}

#[test]
fn age_rating_shows_read_only_values_and_a_proportional_empty_state() {
    let (labels, _, _) = render_document(
        Document::AgeRating,
        768.0,
        1024.0,
        egui::Theme::Light,
        fixture(),
        false,
    );
    for label in [
        "Rating Overrides",
        "Features",
        "Content",
        "Age rating override",
        "13+",
        "Developer age rating info URL",
        "https://example.com/age-rating/information/with/a/long/path",
        "No",
        "Yes",
        "Frequent or intense",
        "UNRECOGNIZED_FUTURE_VALUE",
    ] {
        assert!(
            labels.iter().any(|text| text.contains(label)),
            "missing {label:?}: {labels:?}"
        );
    }

    let (labels, _, _) = render_document(
        Document::AgeRating,
        390.0,
        844.0,
        egui::Theme::Dark,
        {
            let mut details = fixture();
            details.age_rating = None;
            details
        },
        false,
    );
    assert!(labels.contains(&"No age rating declaration is available.".into()));

    let mut details = fixture();
    details.age_rating = None;
    details
        .warnings
        .push("Age rating: Access was denied.".into());
    let (labels, _, _) = render_document(
        Document::AgeRating,
        390.0,
        844.0,
        egui::Theme::Dark,
        details,
        false,
    );
    assert!(!labels.contains(&"No age rating declaration is available.".into()));
    assert!(Document::AgeRating.connected());
}

#[test]
fn command_w_closes_only_the_active_tab_and_preserves_edits() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let credentials = AppleCredentials::test_credentials("test");
    let mut view = IosView::default();
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    let workspace = view.workspaces.entry("123".into()).or_default();
    workspace.open(Document::Listing);
    workspace.edit(
        "/apps/123/versions/version-draft/localizations/en-US",
        "description",
        None,
        "Unsaved description".into(),
        "Description",
        "Version version-draft",
    );
    view.workspaces.insert("456".into(), Workspace::default());

    // Repeated closes keep the final tab, just like its close button.
    for _ in 0..2 {
        let _ = run_view_events(
            &ctx,
            &mut view,
            &credentials,
            1280.0,
            800.0,
            vec![egui::Event::Key {
                key: egui::Key::W,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        let workspace = &view.workspaces["123"];
        assert_eq!(workspace.tabs, vec![Document::Information]);
        assert_eq!(workspace.active, Document::Information);
        assert_eq!(workspace.edits.len(), 1);
        assert_eq!(view.selected.as_deref(), Some("123"));
        assert_eq!(view.workspaces["456"].tabs.len(), 2);
    }
}

fn run_view_events(
    ctx: &egui::Context,
    view: &mut IosView,
    credentials: &AppleCredentials,
    width: f32,
    height: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
        ..Default::default()
    };
    input.events = events;
    ctx.run_ui(input, |ui| {
        egui::CentralPanel::default()
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                view.ui(ui, Some(("123", "Example")), Some(credentials));
            });
    })
}

fn text_center(output: &egui::FullOutput, target: &str) -> Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Text(text) if text.galley.text() == target => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("could not find text {target:?} in rendered output"))
}

fn click_view_at(
    ctx: &egui::Context,
    view: &mut IosView,
    credentials: &AppleCredentials,
    width: f32,
    height: f32,
    position: Pos2,
) -> egui::FullOutput {
    for pressed in [true, false] {
        let output = run_view_events(
            ctx,
            view,
            credentials,
            width,
            height,
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
        if !pressed {
            return output;
        }
    }
    unreachable!()
}

fn run_version_picker_events(
    ctx: &egui::Context,
    details: &AppDetails,
    action: &mut Option<Action>,
    width: f32,
    height: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
        ..Default::default()
    };
    input.events = events;
    ctx.run_ui(input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            chrome::version_picker(ui, Some(details), false, action);
        });
    })
}

fn click_version_picker_at(
    ctx: &egui::Context,
    details: &AppDetails,
    action: &mut Option<Action>,
    width: f32,
    height: f32,
    position: Pos2,
) -> egui::FullOutput {
    for pressed in [true, false] {
        let output = run_version_picker_events(
            ctx,
            details,
            action,
            width,
            height,
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
        if !pressed {
            return output;
        }
    }
    unreachable!()
}

#[test]
fn workspace_documents_render_at_supported_sizes_and_themes_without_overflow() {
    let documents = [
        (Document::Information, "App Store Categories"),
        (Document::Listing, "App Details"),
        (Document::Privacy, "Privacy Policy"),
        (Document::AgeRating, "Rating Overrides"),
        (Document::Review, "Review Contact"),
        (Document::Versions, "iOS Releases"),
        (Document::Media, "Screenshots & Previews"),
        (Document::Changes, "Local Changes"),
    ];
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            for (document, heading) in documents {
                let (labels, right, cached) = render_document(
                    document,
                    width,
                    height,
                    theme,
                    fixture(),
                    document == Document::Changes,
                );
                assert!(
                    labels.iter().any(|text| text == heading),
                    "missing {heading} at {width}x{height}"
                );
                assert!(
                    right <= width + 1.0,
                    "workspace min rect overflows at {width}x{height}: right={right}"
                );
                assert!(cached, "cached selection must not send requests");
                if document == Document::Information {
                    assert!(labels.iter().any(|text| text == "Bundle ID"));
                }
                if document == Document::Listing {
                    assert!(
                        labels
                            .iter()
                            .any(|text| text
                                == "A long localized application name with several words")
                    );
                }
                if document == Document::Media {
                    assert!(labels.iter().any(|text| text == "Screenshot sets"));
                }
                if document == Document::AgeRating {
                    assert!(!labels.iter().any(|text| text == "Planned resource"));
                    assert!(!labels.iter().any(|text| text == "Planned"));
                }
                if document == Document::Changes {
                    assert!(
                        labels
                            .iter()
                            .any(|text| text == "An edited local description for review.")
                    );
                }
            }
        }
    }
}

#[test]
fn listing_uses_the_selected_locale_content() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let credentials = AppleCredentials::test_credentials("test");
    let mut view = IosView::default();
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    let workspace = view.workspaces.entry("123".into()).or_default();
    workspace.locale = "fr-FR".into();
    workspace.open(Document::Listing);
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(768.0, 1024.0))),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default()
                .frame(Frame::new().inner_margin(Margin::ZERO))
                .show(ui, |ui| {
                    view.ui(ui, Some(("123", "Example")), Some(&credentials));
                });
        },
    );
    let labels = texts(&output);
    assert!(labels.contains(&"Un nom d'application localisé avec plusieurs mots"));
    assert!(labels.contains(&"Un sous-titre en français"));
    assert!(view.job.is_none());
}

#[test]
fn typing_into_copyright_creates_a_draft_and_revert_clears_it() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let credentials = AppleCredentials::test_credentials("test");
    let mut view = IosView::default();
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    let width = 1000.0;
    let height = 1800.0;
    let output = run_view_events(&ctx, &mut view, &credentials, width, height, vec![]);
    let label_position = text_center(&output, "Copyright");
    let input_position = Pos2::new(label_position.x + 220.0, label_position.y);
    click_view_at(&ctx, &mut view, &credentials, width, height, input_position);
    run_view_events(
        &ctx,
        &mut view,
        &credentials,
        width,
        height,
        vec![egui::Event::Text(" Typed".into())],
    );
    let key = (
        "/apps/123/versions/version-draft".to_owned(),
        "copyright".to_owned(),
    );
    assert!(
        view.workspaces["123"].edits[&key].value.ends_with("Typed"),
        "the copyright TextEdit should accept keyboard input"
    );

    view.workspaces
        .get_mut("123")
        .unwrap()
        .open(Document::Changes);
    let changes = run_view_events(&ctx, &mut view, &credentials, width, height, vec![]);
    assert!(texts(&changes).contains(&"2026 Example"));
    assert!(texts(&changes).iter().any(|text| text.ends_with("Typed")));
    let revert = text_center(&changes, "Revert");
    click_view_at(&ctx, &mut view, &credentials, width, height, revert);
    assert!(view.workspaces["123"].edits.is_empty());
}

#[test]
fn localization_menu_switches_to_the_selected_locale() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let credentials = AppleCredentials::test_credentials("test");
    let mut view = IosView::default();
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    view.workspaces
        .entry("123".into())
        .or_default()
        .open(Document::Listing);
    let width = 768.0;
    let height = 1024.0;
    let initial = run_view_events(&ctx, &mut view, &credentials, width, height, vec![]);
    let locale_picker = text_center(&initial, "English (U.S.)");
    click_view_at(&ctx, &mut view, &credentials, width, height, locale_picker);
    let menu = run_view_events(&ctx, &mut view, &credentials, width, height, vec![]);
    let french = text_center(&menu, "French · fr-FR");
    click_view_at(&ctx, &mut view, &credentials, width, height, french);
    assert_eq!(view.workspaces["123"].locale, "fr-FR");
}

#[test]
fn version_picker_requests_the_exact_selected_version() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let details = fixture();
    let mut action = None;
    let width = 500.0;
    let height = 300.0;
    let initial = run_version_picker_events(&ctx, &details, &mut action, width, height, vec![]);
    let picker = text_center(&initial, "iOS  2.0");
    click_version_picker_at(&ctx, &details, &mut action, width, height, picker);
    let menu = run_version_picker_events(&ctx, &details, &mut action, width, height, vec![]);
    let live_version = text_center(&menu, "1.9.2 — Ready For Sale");
    click_version_picker_at(&ctx, &details, &mut action, width, height, live_version);
    assert!(matches!(action, Some(Action::Version(id)) if id == "version-live"));
}

#[test]
fn loading_errors_warnings_and_missing_data_have_compact_feedback() {
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let credentials = AppleCredentials::test_credentials("test");
    let mut view = IosView::default();
    view.select("123");
    let (_sender, result) = mpsc::channel();
    view.job = Some(Job {
        result,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    let input = || egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(768.0, 1024.0))),
        ..Default::default()
    };
    let output = ctx.run_ui(input(), |ui| {
        egui::CentralPanel::default()
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                view.ui(ui, Some(("123", "Example")), Some(&credentials));
            });
    });
    assert!(texts(&output).contains(&"Loading app details…"));
    assert!(view.cache.is_empty());
    view.job = None;
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(AppDetails {
                app: json!({"id": "123", "name": "Example"}),
                warnings: vec!["iOS version: Apple returned no version record.".into()],
                ..Default::default()
            }),
            error: Some("Refresh failed".into()),
        },
    );
    let output = ctx.run_ui(input(), |ui| {
        egui::CentralPanel::default()
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                view.ui(ui, Some(("123", "Example")), Some(&credentials));
            });
    });
    let labels = texts(&output);
    assert!(labels.contains(&"Refresh failed"));
    assert!(labels.contains(&"Previously loaded details are shown."));
    assert!(labels.contains(&"iOS version: Apple returned no version record."));
    view.cache
        .get_mut("123")
        .unwrap()
        .data
        .as_mut()
        .unwrap()
        .warnings
        .clear();
    let output = ctx.run_ui(input(), |ui| {
        egui::CentralPanel::default()
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                view.ui(ui, Some(("123", "Example")), Some(&credentials));
            });
    });
    let labels = texts(&output);
    assert!(labels.contains(&"No app information is available."));
}

#[test]
fn local_edits_are_isolated_by_route_field_locale_and_app_and_revert_cleanly() {
    let details = fixture();
    let en = &details.version_localizations[0];
    let fr = &details.version_localizations[1];
    let mut view = IosView::default();
    let workspace = view.workspaces.entry("123".into()).or_default();
    workspace.reconcile(&details);
    workspace.open(Document::Listing);

    let en_route = "/apps/123/versions/version-draft/localizations/en-US";
    workspace.edit(
        en_route,
        "description",
        en.get("description"),
        "English draft".into(),
        "Description",
        "Version version-draft",
    );
    workspace.locale = "fr-FR".into();
    let fr_route = "/apps/123/versions/version-draft/localizations/fr-FR";
    workspace.edit(
        fr_route,
        "description",
        fr.get("description"),
        "Brouillon français".into(),
        "Description",
        "Version version-draft",
    );
    workspace.edit(
        fr_route,
        "keywords",
        fr.get("keywords"),
        "mots,notes".into(),
        "Keywords",
        "Version version-draft",
    );
    let live_route = "/apps/123/versions/version-live/localizations/fr-FR";
    workspace.edit(
        live_route,
        "description",
        None,
        "Separate version draft".into(),
        "Description",
        "Version version-live",
    );
    assert_eq!(workspace.edits.len(), 4);
    assert!(
        workspace.edits[&(live_route.into(), "description".into())]
            .original
            .is_none()
    );
    let null = Value::Null;
    let null_route = "/apps/123";
    workspace.edit(
        null_route,
        "optionalText",
        Some(&null),
        "Filled value".into(),
        "Optional text",
        "App",
    );
    assert_eq!(
        workspace.edits[&(null_route.into(), "optionalText".into())]
            .original
            .as_ref(),
        Some(&Value::Null)
    );
    workspace.edit(
        null_route,
        "optionalText",
        Some(&null),
        String::new(),
        "Optional text",
        "App",
    );
    assert!(
        !workspace
            .edits
            .contains_key(&(null_route.into(), "optionalText".into()))
    );

    workspace.open(Document::Changes);
    workspace.close(Document::Listing);
    assert!(!workspace.tabs.contains(&Document::Listing));
    assert_eq!(workspace.edits.len(), 4, "closing a tab keeps its edits");
    assert_eq!(workspace.locale, "fr-FR");

    workspace.edit(
        en_route,
        "description",
        en.get("description"),
        en["description"].as_str().unwrap().into(),
        "Description",
        "Version version-draft",
    );
    assert!(
        !workspace
            .edits
            .contains_key(&(en_route.into(), "description".into()))
    );
    assert_eq!(workspace.edits.len(), 3);

    let other = view.workspaces.entry("456".into()).or_default();
    other.open(Document::Listing);
    other.locale = "en-US".into();
    other.edit(
        "/apps/456/versions/version-draft/localizations/en-US",
        "description",
        None,
        "Another app draft".into(),
        "Description",
        "Version version-draft",
    );
    assert_eq!(view.workspaces["123"].edits.len(), 3);
    assert_eq!(view.workspaces["456"].edits.len(), 1);
}

#[test]
fn refreshed_source_preserves_a_draft_and_marks_it_as_changed_in_changes() {
    let mut view = IosView::default();
    view.cache.insert(
        "123".into(),
        Entry {
            data: Some(fixture()),
            error: None,
        },
    );
    {
        let details = view.cache["123"].data.as_ref().unwrap();
        let workspace = view.workspaces.entry("123".into()).or_default();
        workspace.reconcile(details);
        workspace.open(Document::Listing);
        workspace.edit(
            "/apps/123/versions/version-draft/localizations/en-US",
            "description",
            details.version_localizations[0].get("description"),
            "My saved draft".into(),
            "Description",
            "Version version-draft",
        );
    }
    view.cache
        .get_mut("123")
        .unwrap()
        .data
        .as_mut()
        .unwrap()
        .version_localizations[0]["description"] = json!("A newly refreshed server value");
    view.workspaces
        .get_mut("123")
        .unwrap()
        .open(Document::Changes);

    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    let credentials = AppleCredentials::test_credentials("test");
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(768.0, 1024.0))),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default()
                .frame(Frame::new().inner_margin(Margin::ZERO))
                .show(ui, |ui| {
                    view.ui(ui, Some(("123", "Example")), Some(&credentials));
                });
        },
    );
    let labels = texts(&output);
    assert!(labels.contains(&"The source changed since this edit. Your local value is preserved."));
    assert!(labels.contains(&"My saved draft"));
    assert!(labels.contains(&"Current source"));
    assert!(labels.contains(&"A newly refreshed server value"));
    assert_eq!(view.workspaces["123"].edits.len(), 1);
}

#[test]
fn age_rating_empty_background_menu_copies_only_this_section_at_all_sizes() {
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let credentials = AppleCredentials::test_credentials("test");
        let clipboard = AgeRatingClipboard::default();
        let mut view = IosView::with_age_rating_clipboard(clipboard.clone());
        view.cache.insert(
            "123".into(),
            Entry {
                data: Some(fixture()),
                error: None,
            },
        );
        view.workspaces
            .entry("123".into())
            .or_default()
            .open(Document::AgeRating);
        for _ in 0..2 {
            run_view_events(&ctx, &mut view, &credentials, width, height, vec![]);
        }
        // Left margin is blank throughout the scroll viewport, even on narrow windows.
        let pos = Pos2::new(if width >= 700.0 { 246.0 } else { 5.0 }, 250.0);
        for pressed in [true, false] {
            run_view_events(
                &ctx,
                &mut view,
                &credentials,
                width,
                height,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let menu = run_view_events(&ctx, &mut view, &credentials, width, height, vec![]);
        let labels = texts(&menu);
        assert!(labels.contains(&"Copy"), "no Copy at {width}");
        assert!(labels.contains(&"Paste"), "no Paste at {width}");
        assert!(!labels.contains(&"Copy App"));
        let copy = text_center(&menu, "Copy");
        click_view_at(&ctx, &mut view, &credentials, width, height, copy);
        let output = run_view_events(&ctx, &mut view, &credentials, width, height, vec![]);
        assert!(
            texts(&output).contains(&"Age rating copied."),
            "copy failed at {width}"
        );
        assert!(view.job.is_none());
        assert_eq!(clipboard.borrow().as_ref().unwrap()["advertising"], false);
        assert!(clipboard.borrow().as_ref().unwrap().get("id").is_none());
        drop(view);
        // The source window may close before the destination window is opened.
        let destination = IosView::with_age_rating_clipboard(clipboard.clone());
        assert!(clipboard.borrow().is_some());
        drop(destination);
    }
}
