use super::*;
use egui::{Frame, Margin, Pos2, Rect, Shape, vec2};
use serde_json::{Value, json};

fn fixture() -> AppDetails {
    AppDetails {
        app: json!({"id": "123", "name": "A long application name for layout testing", "bundleId": "com.example.this.is.a.deliberately.long.bundle.identifier.with.several.segments.to.check.wrapping.behavior", "sku": "example", "primaryLocale": "en-US", "contentRightsDeclaration": "DOES_NOT_USE_THIRD_PARTY_CONTENT"}),
        info: Some(
            json!({"id": "info-draft", "state": "PREPARE_FOR_SUBMISSION", "primaryCategoryId": "UTILITIES", "secondaryCategoryId": "PRODUCTIVITY"}),
        ),
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
        ..Default::default()
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
