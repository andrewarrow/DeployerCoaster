use super::*;
use serde_json::json;
use std::path::PathBuf;

fn task(kind: ImageKind) -> Task {
    Task {
        request: ImageRequest {
            root: PathBuf::from("/tmp/generated"),
            prompt: String::new(),
            app: "app1".into(),
            version: "version1".into(),
            locale: "en-US".into(),
            kind,
            _credentials: None,
        },
        codex: Codex::test_disconnected(PathBuf::from("/tmp/generated")),
        receipt: Some(UploadedImage {
            image_id: "image1".into(),
            placement_id: "placement1".into(),
        }),
        reload: false,
        verification: None,
    }
}

fn media(placement_type: &str, placement_state: &str, image_id: &str, image_state: &str) -> Media {
    Media {
        screenshots: Ok(vec![]),
        previews: Ok(vec![]),
        placements: Ok(vec![PlacedImage {
            placement: json!({"id":"placement1", "placementType":placement_type, "state":placement_state}),
            image: Ok(json!({"id":image_id, "state":image_state})),
        }]),
    }
}

fn task_for_recovery(root: &std::path::Path) -> Task {
    let mut task = task(ImageKind::Header);
    task.request.root = root.to_owned();
    task.codex = Codex::test_disconnected(root.to_owned());
    task.receipt = None;
    task
}

fn write_recovery_artifacts(root: &std::path::Path) {
    let (width, height) = ImageKind::Header.dimensions();
    image::RgbaImage::new(width, height)
        .save_with_format(root.join("ios-header.png"), image::ImageFormat::Png)
        .unwrap();
    std::fs::write(
        root.join("upload.json"),
        json!({
            "appId":"app1",
            "versionId":"version1",
            "locale":"en-US",
            "placementType":ImageKind::Header.placement(),
            "imageId":"image1",
            "placementId":"placement1"
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn verifies_only_the_matching_successful_placement_and_image() {
    let mut generation = Generation {
        task: Some(task(ImageKind::Header)),
        ..Default::default()
    };

    generation.verify(&Ok(media(
        ImageKind::Header.placement(),
        "PARENT_APPROVED",
        "image1",
        "APPROVED",
    )));
    assert_eq!(generation.task.as_ref().unwrap().verification, Some(Ok(())));

    generation.verify(&Ok(media(
        ImageKind::Search.placement(),
        "PARENT_APPROVED",
        "image1",
        "APPROVED",
    )));
    assert!(
        generation
            .task
            .as_ref()
            .unwrap()
            .verification
            .as_ref()
            .unwrap()
            .is_err()
    );

    generation.verify(&Ok(media(
        ImageKind::Header.placement(),
        "PARENT_FAILED",
        "image1",
        "APPROVED",
    )));
    assert!(
        generation
            .task
            .as_ref()
            .unwrap()
            .verification
            .as_ref()
            .unwrap()
            .is_err()
    );

    generation.verify(&Ok(media(
        ImageKind::Header.placement(),
        "PARENT_APPROVED",
        "image1",
        "FAILED",
    )));
    assert!(
        generation
            .task
            .as_ref()
            .unwrap()
            .verification
            .as_ref()
            .unwrap()
            .is_err()
    );

    generation.verify(&Err("read failed".into()));
    assert!(
        generation
            .task
            .as_ref()
            .unwrap()
            .verification
            .as_ref()
            .unwrap()
            .is_err()
    );
}

#[test]
fn reload_recovers_a_valid_receipt_after_codex_stops_and_verifies_its_ids() {
    for (placement_type, image_id, succeeds) in [
        (ImageKind::Header.placement(), "image1", true),
        (ImageKind::Search.placement(), "image1", false),
        (ImageKind::Header.placement(), "another-image", false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        write_recovery_artifacts(directory.path());
        let mut generation = Generation {
            task: Some(task_for_recovery(directory.path())),
            ..Default::default()
        };

        generation.verify(&Ok(media(
            placement_type,
            "PARENT_APPROVED",
            image_id,
            "APPROVED",
        )));

        let task = generation.task.as_ref().unwrap();
        assert_eq!(
            task.receipt
                .as_ref()
                .map(|receipt| receipt.placement_id.as_str()),
            Some("placement1")
        );
        assert_eq!(
            task.receipt
                .as_ref()
                .map(|receipt| receipt.image_id.as_str()),
            Some("image1")
        );
        assert_eq!(task.verification.as_ref().unwrap().is_ok(), succeeds);
    }
}

#[test]
fn reload_keeps_the_failed_codex_state_when_no_valid_receipt_exists() {
    let directory = tempfile::tempdir().unwrap();
    let mut generation = Generation {
        task: Some(task_for_recovery(directory.path())),
        ..Default::default()
    };

    generation.verify(&Ok(media(
        ImageKind::Header.placement(),
        "PARENT_APPROVED",
        "image1",
        "APPROVED",
    )));

    let task = generation.task.as_ref().unwrap();
    assert!(task.receipt.is_none());
    assert!(task.verification.is_none());
}
