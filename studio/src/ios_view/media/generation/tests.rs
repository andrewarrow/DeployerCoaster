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
