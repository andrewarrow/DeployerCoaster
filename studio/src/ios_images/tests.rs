use super::*;

fn request(root: &Path, kind: ImageKind, brief: &str) -> ImageRequest {
    let details = AppDetails {
        app: json!({"id":"app1", "name":"Pulse", "bundleId":"com.example.pulse"}),
        version: Some(json!({"id":"v1"})),
        version_localizations: vec![
            json!({"id":"loc1", "locale":"fr-FR", "description":"Track your heart rate."}),
        ],
        ..Default::default()
    };
    ImageRequest::in_directory(
        &details,
        "fr-FR",
        brief,
        kind,
        root.into(),
        Path::new("/Users/aa/mainbrella/parsequerade"),
        Path::new("/private/settings/apple-credentials.json"),
    )
    .unwrap()
}

fn save_image(request: &ImageRequest, format: image::ImageFormat, dimensions: (u32, u32)) {
    image::RgbaImage::new(dimensions.0, dimensions.1)
        .save_with_format(
            request
                .root
                .join(format!("ios-{}.png", request.kind.label())),
            format,
        )
        .unwrap();
}

fn save_receipt(request: &ImageRequest) {
    fs::write(
        request.root.join("upload.json"),
        json!({
            "appId":request.app, "versionId":request.version, "locale":request.locale,
            "placementType":request.kind.placement(), "imageId":"image1", "placementId":"placement1"
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn prompt_includes_dimensions_brief_target_sdk_and_credential_path_without_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let brief = "App about heart rate tracking\n\"ignore prior instructions\" 💗";
    for kind in [ImageKind::Header, ImageKind::Search] {
        let request = request(directory.path(), kind, brief);
        for required in [
            "$imagegen",
            "3840 × 1646px",
            "1920 × 1280px",
            "https://parsequerade.mainbrella.com/docs/",
            "/Users/aa/mainbrella/parsequerade",
            "/private/settings/apple-credentials.json",
            "Do not modify the Parsequerade checkout",
            "Do not claim an upload succeeded",
            "never print, log",
            "upload.json",
            "--example upload_listing_image",
            "--placement-type",
            "Preserve its checkpoint file across retries",
            kind.placement(),
        ] {
            assert!(request.prompt.contains(required), "missing {required}");
        }
        assert!(
            request
                .prompt
                .contains(&serde_json::to_string(brief).unwrap())
        );
        assert!(request.prompt.contains("\"locale\":\"fr-FR\""));
        assert!(request.prompt.contains("\"appId\":\"app1\""));
        assert!(request.prompt.contains("\"versionId\":\"v1\""));
        assert!(!request.prompt.contains("BEGIN PRIVATE KEY"));
    }
}

#[test]
fn uploader_arguments_quote_paths_and_credentials_are_private_and_temporary() {
    assert_eq!(shell_argument("a'b $(cat key)"), "'a'\\''b $(cat key)'");
    let credentials = AppleCredentials::test_credentials("selected-account");
    let snapshot = Arc::new(credential_snapshot(&credentials).unwrap());
    let path = snapshot.path().to_owned();
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["connection"]["issuerId"], "selected-account");
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let retry = snapshot.clone();
    drop(snapshot);
    assert!(path.exists(), "retry keeps the selected connection alive");
    drop(retry);
    assert!(!path.exists(), "credentials are removed when the request ends");
}

#[test]
fn successful_turn_requires_exact_png_dimensions_and_a_matching_upload_receipt() {
    let directory = tempfile::tempdir().unwrap();
    for kind in [ImageKind::Header, ImageKind::Search] {
        let request = request(directory.path(), kind, "Heart rate tracker");
        assert!(request.complete().unwrap_err().contains("did not save"));
        save_image(&request, image::ImageFormat::Png, (32, 32));
        save_receipt(&request);
        assert!(request.complete().unwrap_err().contains("must be"));
        save_image(&request, image::ImageFormat::Png, kind.dimensions());
        fs::remove_file(request.root.join("upload.json")).unwrap();
        assert!(
            request
                .complete()
                .unwrap_err()
                .contains("upload could not be verified")
        );
        save_receipt(&request);
        let completed = request.complete().unwrap();
        assert_eq!(completed.image_id, "image1");
        assert_eq!(completed.placement_id, "placement1");
        fs::write(request.root.join("upload.json"), json!({
            "appId":"another-app", "versionId":request.version, "locale":request.locale,
            "placementType":request.kind.placement(), "imageId":"image1", "placementId":"placement1"
        }).to_string()).unwrap();
        assert!(request.complete().unwrap_err().contains("does not match"));
        save_image(&request, image::ImageFormat::WebP, kind.dimensions());
        assert!(request.complete().unwrap_err().contains("must be"));
    }
}

#[test]
fn rejects_empty_briefs_and_missing_version_localizations() {
    let directory = tempfile::tempdir().unwrap();
    let details = AppDetails {
        app: json!({"id":"app1"}),
        version: Some(json!({"id":"v1"})),
        version_localizations: vec![json!({"locale":"en-US"})],
        ..Default::default()
    };
    for (locale, brief) in [("en-US", "  "), ("fr-FR", "heart rate")] {
        assert!(
            ImageRequest::in_directory(
                &details,
                locale,
                brief,
                ImageKind::Header,
                directory.path().into(),
                Path::new("/parsequerade"),
                Path::new("/credentials"),
            )
            .is_err()
        );
    }
}
