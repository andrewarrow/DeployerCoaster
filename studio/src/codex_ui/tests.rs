use super::*;

fn test_codex() -> Codex {
    Codex {
        client: None,
        root: PathBuf::from("/workspace/a-very-long-workspace-directory-name"),
        website_request: None,
        created_website: None,
        image_request: None,
        uploaded_image: None,
        description_rewrite: None,
        rewrite_response: None,
        rewritten_description: None,
        auto_send: false,
        account: Some(ChatGptAccount {
            email: Some("a-long-account-address@example.com".into()),
            plan_type: Some("plus".into()),
        }),
        checking: false,
        login_pending: false,
        available: true,
        ready: true,
        opening: false,
        busy: false,
        cancelling: false,
        activity: String::new(),
        error: None,
        messages: vec![ChatMessage { user: false, text: "A streamed response with a long_identifier_that_should_wrap_even_on_narrow_screens_1234567890.\n".repeat(80) }],
        streaming_message: None,
        draft: String::new(),
        sent_draft: None,
        model: "gpt-5.6-luna",
        effort: "medium",
        open: true,
    }
}

#[test]
fn rewrite_retry_requests_saved_inputs_after_restart() {
    let mut codex = Codex::test_disconnected(PathBuf::from("/unused"));
    let ctx = egui::Context::default();
    let mut button = egui::Pos2::ZERO;
    let mut retry = false;
    for phase in 0..4 {
        let events = if phase >= 2 {
            vec![
                egui::Event::PointerMoved(button),
                egui::Event::PointerButton {
                    pos: button,
                    button: egui::PointerButton::Primary,
                    pressed: phase == 2,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        } else {
            vec![]
        };
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(560.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| retry |= codex.description_rewrite_ui(ui),
        );
        if phase < 2 {
            button = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Retry rewrite" => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .expect("Retry must be available without an in-memory request");
        }
    }
    assert!(retry);
    assert!(codex.description_rewrite.is_none());
    assert!(!codex.auto_send && !codex.busy);
}

#[test]
fn chat_keeps_composer_visible_and_content_in_bounds() {
    for dark in [true, false] {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            for state in ["ready", "failed", "queued"] {
                let mut codex = test_codex();
                if state == "failed" {
                    codex.error = Some(
                        "A long error explaining that the request could not be completed. "
                            .repeat(20),
                    );
                } else if state == "queued" {
                    codex.account = None;
                    codex.ready = false;
                    codex.auto_send = true;
                    codex.draft =
                        "A long website creation prompt with reference paths and requirements.\n"
                            .repeat(40);
                }
                let ctx = egui::Context::default();
                crate::style::configure(&ctx);
                ctx.set_theme(if dark {
                    egui::ThemePreference::Dark
                } else {
                    egui::ThemePreference::Light
                });
                for _ in 0..2 {
                    let output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            egui::Panel::top("connection").show(ui, |ui| codex.toolbar(ui));
                            if width < 1000.0 {
                                egui::CentralPanel::default().show(ui, |ui| {
                                    codex.chat_ui(ui, None);
                                    assert!(
                                        ui.min_rect().right() <= width,
                                        "overflow at {width}px: {:?}",
                                        ui.min_rect()
                                    );
                                });
                            } else {
                                egui::Panel::right("chat").exact_size(360.0).show(ui, |ui| {
                                    codex.chat_ui(ui, None);
                                    assert!(
                                        ui.min_rect().right() <= width,
                                        "overflow at {width}px: {:?}",
                                        ui.min_rect()
                                    );
                                });
                                egui::CentralPanel::default().show(ui, |_| {});
                            }
                        },
                    );
                    let send = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == "Send" => Some(text),
                            _ => None,
                        })
                        .expect("composer must render");
                    assert!(
                        send.pos.y + send.galley.size().y <= height,
                        "composer clipped at {width}x{height}, state={state}"
                    );
                }
            }
        }
    }
}

#[test]
fn website_waits_for_sign_in_and_readiness_then_sends_once_in_its_own_directory() {
    use crate::codex::CodexCommand;

    let mut codex = test_codex();
    let (client, requests, events) = CodexClient::test_channels();
    codex.client = Some(client);
    codex.root = PathBuf::from("/sites/New Name");
    codex.website_request = Some(WebsiteRequest {
        root: codex.root.clone(),
        prompt: "Create the website with logo, images, and a privacy policy.".into(),
    });
    codex.draft = "Create the website with logo, images, and a privacy policy.".into();
    codex.account = None;
    codex.ready = false;
    codex.auto_send = true;
    let ctx = egui::Context::default();

    codex.set_root(Path::new("/unrelated/workspace"));
    assert_eq!(codex.root, Path::new("/sites/New Name"));
    codex.poll(&ctx);
    assert!(requests.try_recv().is_err());
    assert!(!codex.draft.is_empty());

    events
        .send(CodexEvent::LoginCompleted(ChatGptAccount {
            email: None,
            plan_type: None,
        }))
        .unwrap();
    codex.poll(&ctx);
    assert!(matches!(
        requests.try_recv().unwrap(),
        CodexCommand::OpenChat
    ));
    assert!(requests.try_recv().is_err());
    events.send(CodexEvent::ChatReady).unwrap();
    codex.poll(&ctx);
    let CodexCommand::SendChatMessage { message, .. } = requests.try_recv().unwrap() else {
        panic!("website request was not sent");
    };
    assert!(message.contains("logo, images, and a privacy policy"));
    assert!(message.contains("No workspace document is open"));
    assert!(codex.busy && !codex.auto_send);
    assert!(codex.draft.is_empty());
    codex.poll(&ctx);
    assert!(requests.try_recv().is_err());

    events
        .send(CodexEvent::ChatError("Creation failed".into()))
        .unwrap();
    assert!(
        !codex.poll(&ctx),
        "website work must not reload an unrelated workspace"
    );
    assert!(!codex.busy && !codex.auto_send);
    assert!(codex.draft.contains("logo, images, and a privacy policy"));
    codex.poll(&ctx);
    assert!(
        requests.try_recv().is_err(),
        "a failed request must not replay automatically"
    );
    codex.return_to_workspace();
    assert!(codex.website_request.is_none());
}

#[test]
fn website_creation_preserves_active_work_and_unsent_drafts() {
    let mut codex = test_codex();
    assert!(codex.can_create_website().is_ok());
    codex.busy = true;
    assert!(codex.can_create_website().is_err());
    codex.busy = false;
    codex.auto_send = true;
    assert!(codex.can_create_website().is_err());
    codex.auto_send = false;
    codex.draft = "My draft".into();
    assert!(codex.can_create_website().is_err());
    assert_eq!(codex.draft, "My draft");
}

#[test]
fn unavailable_clears_active_state_and_keeps_draft_and_history() {
    let mut codex = test_codex();
    codex.busy = true;
    codex.cancelling = true;
    codex.opening = true;
    codex.login_pending = true;
    codex.draft = "Next question".into();
    codex.unavailable("Codex stopped".into());
    assert!(!codex.busy && !codex.cancelling && !codex.opening && !codex.login_pending);
    assert!(!codex.ready && !codex.available);
    assert!(codex.account.is_none());
    assert_eq!(codex.draft, "Next question");
    assert_eq!(codex.messages.len(), 1);
}

#[test]
fn workspace_context_identifies_document_without_reading_credentials() {
    let message = contextual_message(
        "Rename this workspace",
        Some(Path::new("/workspace/example.dcstudio")),
    );
    assert!(message.contains("/workspace/example.dcstudio"));
    assert!(message.contains("Preserve its format and unknown fields"));
    assert!(message.ends_with("User request:\nRename this workspace"));
    assert!(contextual_message("Help", None).contains("No workspace document is open"));
}

fn image_request() -> ImageRequest {
    ImageRequest {
        root: PathBuf::from("/tmp/generated-ios-image"),
        prompt: "Generate and upload a header image based on the heart-rate brief.".into(),
        app: "app1".into(),
        version: "version1".into(),
        locale: "en-US".into(),
        kind: crate::ios_images::ImageKind::Header,
        _credentials: None,
    }
}

#[test]
fn image_generation_reuses_ready_chatgpt_session_with_the_generation_model() {
    use crate::codex::CodexCommand;

    let mut codex = test_codex();
    let (client, requests, events) = CodexClient::test_channels();
    codex.client = Some(client);
    codex.image_request = Some(image_request());
    codex.queue_generation(codex.image_request.as_ref().unwrap().prompt.clone());
    events
        .send(CodexEvent::AccountStatus(codex.account.clone()))
        .unwrap();
    events.send(CodexEvent::ChatReady).unwrap();

    codex.poll(&egui::Context::default());
    let CodexCommand::SendChatMessage {
        message,
        model,
        reasoning_effort,
    } = requests.try_recv().unwrap()
    else {
        panic!("image request was not sent");
    };
    assert!(message.contains("header image"));
    assert_eq!(model, "gpt-6.1-sol");
    assert_eq!(reasoning_effort, "xhigh");
    assert!(
        requests.try_recv().is_err(),
        "a ready session needs no login/open command"
    );
    assert!(codex.busy && !codex.auto_send);
}

#[test]
fn failed_or_interrupted_image_turn_never_reports_an_uploaded_image() {
    for event in [
        CodexEvent::ChatError("Upload failed".into()),
        CodexEvent::ChatTurnCancelled,
    ] {
        let mut codex = test_codex();
        let (client, requests, events) = CodexClient::test_channels();
        codex.client = Some(client);
        codex.image_request = Some(image_request());
        codex.busy = true;
        codex.sent_draft = Some(image_request().prompt);
        codex.draft.clear();
        events.send(event).unwrap();

        codex.poll(&egui::Context::default());
        assert!(codex.take_uploaded_image().is_none());
        assert!(!codex.busy);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn settings_show_compact_connection_actions_without_the_chat_toggle() {
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        let ctx = egui::Context::default();
        let mut codex = test_codex();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, height),
                )),
                ..Default::default()
            },
            |ui| codex.settings_ui(ui),
        );
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"Sign in with another ChatGPT account"));
        assert!(!labels.contains(&"Codex chat"));
        assert!(!labels.contains(&"Close Codex chat"));
        assert!(
            labels.iter().all(|label| !label.is_empty()),
            "{width}x{height}"
        );
    }

    let ctx = egui::Context::default();
    let mut codex = test_codex();
    codex.account = None;
    codex.available = false;
    codex.error = Some("Codex could not start".into());
    let output = ctx.run_ui(Default::default(), |ui| codex.settings_ui(ui));
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(labels.contains(&"Retry Codex connection"));

    let mut codex = test_codex();
    codex.login_pending = true;
    codex.error = Some("The previous sign-in attempt failed and needs a retry.".into());
    let output = ctx.run_ui(Default::default(), |ui| codex.settings_ui(ui));
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(labels.contains(&"Signing in with ChatGPT…"));
    assert!(labels.iter().any(|label| {
        label.contains("previous sign-in attempt failed")
    }));
    assert!(!labels.contains(&"a-long-account-address@example.com"));
}
