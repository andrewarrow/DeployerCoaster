use super::*;

fn test_codex() -> Codex {
    Codex {
        client: None,
        root: PathBuf::from("/workspace/a-very-long-workspace-directory-name"),
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
fn chat_keeps_composer_visible_and_content_in_bounds() {
    for dark in [true, false] {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            for failed in [false, true] {
                let mut codex = test_codex();
                if failed {
                    codex.error = Some(
                        "A long error explaining that the request could not be completed. "
                            .repeat(20),
                    );
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
                        "composer clipped at {width}x{height}, failed={failed}"
                    );
                }
            }
        }
    }
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
