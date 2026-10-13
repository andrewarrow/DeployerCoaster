use super::*;
use egui::{Event, FullOutput, Pos2, Rect, Shape, vec2};

fn render(
    wizard: &mut Wizard,
    codex: &mut Codex,
    ctx: &egui::Context,
    size: [f32; 2],
    events: Vec<Event>,
) -> (FullOutput, WizardAction) {
    let mut action = WizardAction::None;
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]))),
            events,
            ..Default::default()
        },
        |ui| {
            action = wizard.ui(ui, codex, Path::new("/unused"));
            assert!(
                ui.min_rect().right() <= size[0],
                "Wizard overflow at {size:?}: {:?}", ui.min_rect()
            );
        },
    );
    (output, action)
}

fn label_position(output: &FullOutput, label: &str) -> Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing label: {label}"))
}

fn click(
    wizard: &mut Wizard,
    codex: &mut Codex,
    ctx: &egui::Context,
    size: [f32; 2],
    pos: Pos2,
) -> (FullOutput, WizardAction) {
    let _ = render(wizard, codex, ctx, size, vec![]);
    let _ = render(
        wizard,
        codex,
        ctx,
        size,
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    render(
        wizard,
        codex,
        ctx,
        size,
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    )
}

fn completed_rewrite(wizard: &Wizard) -> RewrittenDescription {
    RewrittenDescription {
        name: wizard.app_name.trim().into(),
        original: wizard.original_description.clone(),
        description: format!(
            "{} — café 🪴\n{}",
            wizard.app_name.trim(),
            "Fresh product detail. ".repeat(100)
        ),
    }
}

fn saved_review(path: PathBuf) -> Wizard {
    let mut wizard = Wizard::from_path(path);
    wizard.step = 3;
    wizard.app_name = "Example".into();
    wizard.original_description = format!(
        "Original — café 🪴\n{}",
        "Pasted source detail. ".repeat(500)
    );
    wizard.receive_rewritten_description(completed_rewrite(&wizard));
    assert!(wizard.error.is_none());
    wizard
}

#[test]
fn review_step_resumes_with_complete_descriptions_and_create_button() {
    for size in [
        [390.0, 360.0],
        [560.0, 400.0],
        [390.0, 844.0],
        [768.0, 1024.0],
        [1280.0, 800.0],
        [1440.0, 900.0],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings/wizard.json");
        let mut wizard = saved_review(path.clone());
        wizard.step = 2;
        wizard.persist().unwrap();
        let original = wizard.original_description.clone();
        let rewritten = wizard
            .rewritten_description
            .as_ref()
            .unwrap()
            .description
            .clone();
        let mut codex = Codex::test_disconnected(directory.path().into());
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let _ = render(&mut wizard, &mut codex, &ctx, size, vec![]);
        let (output, _) = render(&mut wizard, &mut codex, &ctx, size, vec![]);
        let _ = click(
            &mut wizard,
            &mut codex,
            &ctx,
            size,
            label_position(&output, "Next"),
        );
        drop(wizard);
        drop(codex);

        let mut resumed = Wizard::from_path(path);
        assert_eq!(resumed.step, 3);
        assert_eq!(resumed.app_name, "Example");
        assert_eq!(resumed.original_description, original);
        assert_eq!(
            resumed.rewritten_description.as_ref().unwrap().description,
            rewritten
        );
        assert!(!resumed.website_started);
        assert!(resumed.error.is_none());
        let mut codex = Codex::test_disconnected(directory.path().into());
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let _ = render(&mut resumed, &mut codex, &ctx, size, vec![]);
        let (output, action) = render(&mut resumed, &mut codex, &ctx, size, vec![]);
        for label in [
            "Step 3 of 9",
            "Example",
            "Original description",
            "Rewritten description",
        ] {
            label_position(&output, label);
        }
        let create = label_position(&output, "Create website");
        assert!(create.y < size[1], "Create website clipped at {size:?}");
        assert!(matches!(action, WizardAction::None));
    }
}

#[test]
fn typing_autosaves_inputs_and_invalidates_previous_rewrite() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wizard.json");
    let mut wizard = Wizard::from_path(path.clone());
    let mut codex = Codex::test_disconnected(directory.path().into());
    let ctx = egui::Context::default();
    let size = [560.0, 400.0];
    let _ = render(&mut wizard, &mut codex, &ctx, size, vec![]);
    let (output, _) = render(&mut wizard, &mut codex, &ctx, size, vec![]);
    let _ = click(
        &mut wizard,
        &mut codex,
        &ctx,
        size,
        label_position(&output, "Enter an app name"),
    );
    let (output, _) = render(
        &mut wizard,
        &mut codex,
        &ctx,
        size,
        vec![Event::Text("Example".into())],
    );
    assert_eq!(Wizard::from_path(path.clone()).app_name, "Example");
    let _ = click(
        &mut wizard,
        &mut codex,
        &ctx,
        size,
        label_position(&output, "Paste the original app description"),
    );
    let original = "Original description — café 🪴\nSecond line.";
    let _ = render(
        &mut wizard,
        &mut codex,
        &ctx,
        size,
        vec![Event::Text(original.into())],
    );
    assert_eq!(
        Wizard::from_path(path.clone()).original_description,
        original
    );

    wizard.receive_rewritten_description(completed_rewrite(&wizard));
    wizard.website_started = true;
    wizard.persist().unwrap();
    let _ = render(
        &mut wizard,
        &mut codex,
        &ctx,
        size,
        vec![Event::Text(" Changed.".into())],
    );
    let resumed = Wizard::from_path(path);
    assert!(resumed.original_description.contains("Changed."));
    assert_eq!(resumed.original_description, wizard.original_description);
    assert!(resumed.rewritten_description.is_none());
    assert!(!resumed.website_started);
}

#[test]
fn back_navigation_and_finish_are_saved() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wizard.json");
    let mut wizard = saved_review(path.clone());
    let mut codex = Codex::test_disconnected(directory.path().into());
    let ctx = egui::Context::default();
    let size = [560.0, 400.0];
    for step in [2, 1] {
        let _ = render(&mut wizard, &mut codex, &ctx, size, vec![]);
        let (output, _) = render(&mut wizard, &mut codex, &ctx, size, vec![]);
        let _ = click(
            &mut wizard,
            &mut codex,
            &ctx,
            size,
            label_position(&output, "Back"),
        );
        let resumed = Wizard::from_path(path.clone());
        assert_eq!(resumed.step, step);
        assert_eq!(resumed.app_name, "Example");
        assert_eq!(resumed.rewritten_description.is_some(), step == 2);
    }

    wizard = saved_review(path.clone());
    wizard.step = STEP_COUNT;
    wizard.website_started = true;
    wizard.persist().unwrap();
    let _ = render(&mut wizard, &mut codex, &ctx, size, vec![]);
    let (output, _) = render(&mut wizard, &mut codex, &ctx, size, vec![]);
    let (_, action) = click(
        &mut wizard,
        &mut codex,
        &ctx,
        size,
        label_position(&output, "Finish"),
    );
    assert!(matches!(action, WizardAction::Finished));
    let resumed = Wizard::from_path(path);
    assert_eq!(resumed.step, 1);
    assert!(resumed.app_name.is_empty());
    assert!(resumed.original_description.is_empty());
    assert!(resumed.rewritten_description.is_none());
}

#[test]
fn interrupted_and_stale_rewrites_resume_with_retry_available() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wizard.json");
    let mut wizard = saved_review(path.clone());
    let saved = fs::read(&path).unwrap();
    let mut stale = completed_rewrite(&wizard);
    stale.original = "Another draft".into();
    wizard.receive_rewritten_description(stale);
    assert_eq!(fs::read(&path).unwrap(), saved);

    for stale in [false, true] {
        let mut state: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        if stale {
            state["rewritten_description"]["original"] = "Another draft".into();
        } else {
            state["step"] = 2.into();
            state["rewritten_description"] = serde_json::Value::Null;
        }
        fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
        let mut resumed = Wizard::from_path(path.clone());
        assert_eq!(resumed.step, 2);
        assert_eq!(resumed.app_name, wizard.app_name);
        assert_eq!(resumed.original_description, wizard.original_description);
        assert!(resumed.rewritten_description.is_none());
        let mut codex = Codex::test_disconnected(directory.path().into());
        let ctx = egui::Context::default();
        let _ = render(&mut resumed, &mut codex, &ctx, [560.0, 400.0], vec![]);
        let (output, _) = render(&mut resumed, &mut codex, &ctx, [560.0, 400.0], vec![]);
        label_position(&output, "Retry rewrite");
    }
}

#[test]
fn storage_errors_preserve_saved_file_and_current_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wizard.json");
    fs::write(&path, b"invalid json").unwrap();
    let mut wizard = Wizard::from_path(path.clone());
    assert!(wizard.error.is_some());
    wizard.app_name = "Example".into();
    wizard.original_description = "Keep my inputs".into();
    wizard.receive_rewritten_description(completed_rewrite(&wizard));
    assert!(wizard.error.is_some());
    assert_eq!(fs::read(path).unwrap(), b"invalid json");
    assert_eq!(wizard.original_description, "Keep my inputs");
    assert!(wizard.rewritten_description.is_some());

    let blocked = directory.path().join("blocked");
    let mut wizard = saved_review(blocked.join("wizard.json"));
    fs::remove_file(blocked.join("wizard.json")).unwrap();
    fs::remove_dir(&blocked).unwrap();
    fs::write(&blocked, b"not a directory").unwrap();
    wizard.step = STEP_COUNT;
    let mut codex = Codex::test_disconnected(directory.path().into());
    let ctx = egui::Context::default();
    let size = [560.0, 400.0];
    let _ = render(&mut wizard, &mut codex, &ctx, size, vec![]);
    let (output, _) = render(&mut wizard, &mut codex, &ctx, size, vec![]);
    let (_, action) = click(
        &mut wizard,
        &mut codex,
        &ctx,
        size,
        label_position(&output, "Finish"),
    );
    assert!(matches!(action, WizardAction::None));
    assert_eq!(wizard.step, STEP_COUNT);
    assert_eq!(wizard.app_name, "Example");
    assert!(wizard.rewritten_description.is_some());
    assert!(wizard.error.is_some());
}
