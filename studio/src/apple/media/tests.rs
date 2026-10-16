use super::*;
use reqwest::Method;
use serde_json::json;
use std::collections::VecDeque;

#[derive(Default)]
struct Fake(VecDeque<(String, u16, Value)>);

impl Fake {
    fn page(&mut self, path: &str, items: Value) {
        self.0.push_back((
            path.into(),
            200,
            json!({"items": items, "nextCursor": null}),
        ));
    }

    fn placements(&mut self, locale: &str, items: Value) {
        self.page(
            &format!("/apps/123/versions/draft/localizations/{locale}/placements?placementType={HEADER_PLACEMENT},{SEARCH_PLACEMENT}&limit=200"),
            items,
        );
    }

    fn empty_sets(&mut self) {
        self.page(
            "/apps/123/versions/draft/localizations/en-US/screenshot-sets?limit=200",
            json!([]),
        );
        self.page(
            "/apps/123/versions/draft/localizations/en-US/preview-sets?limit=200",
            json!([]),
        );
    }
}

impl Transport for &mut Fake {
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        assert_eq!(method, Method::GET);
        assert!(body.is_none());
        let (expected, status, value) = self.0.pop_front().expect("unexpected request");
        assert_eq!(path, expected);
        Ok((status, value))
    }
}

#[test]
fn loads_exact_locale_and_preserves_screenshot_and_preview_order_across_pages() {
    let mut fake = Fake::default();
    fake.page("/apps/123/versions/draft/localizations/fr-FR/screenshot-sets?limit=200", json!([
        {"id":"phone", "appId":"123", "versionId":"draft", "screenshotDisplayType":"APP_IPHONE_67"}
    ]));
    fake.0.push_back((
        "/screenshot-sets/phone/screenshots?limit=200".into(),
        200,
        json!({"items":[{"id":"shot-z", "fileName":"first.png"}],"nextCursor":"next+="}),
    ));
    fake.page(
        "/screenshot-sets/phone/screenshots?limit=200&cursor=next%2B%3D",
        json!([{"id":"shot-a", "fileName":"second.png"}]),
    );
    fake.page(
        "/apps/123/versions/draft/localizations/fr-FR/preview-sets?limit=200",
        json!([
            {"id":"videos", "previewType":"IPHONE_67"}
        ]),
    );
    fake.page(
        "/preview-sets/videos/previews?limit=200",
        json!([{"id":"video", "videoDeliveryState":{"state":"PROCESSING"}}]),
    );
    fake.placements("fr-FR", json!([]));
    let result = load(&mut fake, "123", "draft", "fr-FR", &AtomicBool::new(false)).unwrap();
    let sets = result.screenshots.unwrap();
    assert_eq!(sets[0].display_type, "APP_IPHONE_67");
    let screenshots = sets[0].assets.as_ref().unwrap();
    assert_eq!(screenshots[0]["id"], "shot-z");
    assert_eq!(screenshots[1]["id"], "shot-a");
    assert_eq!(
        result.previews.unwrap()[0].assets.as_ref().unwrap()[0]["id"],
        "video"
    );
    assert!(fake.0.is_empty());
}

#[test]
fn a_failed_set_does_not_hide_other_media_and_permission_errors_are_not_empty_states() {
    let mut fake = Fake::default();
    fake.page(
        "/apps/123/versions/draft/localizations/en-US/screenshot-sets?limit=200",
        json!([
            {"id":"phone", "screenshotDisplayType":"APP_IPHONE_67"},
            {"id":"tablet", "screenshotDisplayType":"APP_IPAD_PRO_3GEN_129"}
        ]),
    );
    fake.0.push_back((
        "/screenshot-sets/phone/screenshots?limit=200".into(),
        403,
        json!({"error":{"message":"private upstream details"}}),
    ));
    fake.page(
        "/screenshot-sets/tablet/screenshots?limit=200",
        json!([{"id":"tablet-shot"}]),
    );
    fake.0.push_back((
        "/apps/123/versions/draft/localizations/en-US/preview-sets?limit=200".into(),
        429,
        json!({}),
    ));
    fake.placements("en-US", json!([]));
    let result = load(&mut fake, "123", "draft", "en-US", &AtomicBool::new(false)).unwrap();
    let screenshots = result.screenshots.unwrap();
    assert!(
        screenshots[0]
            .assets
            .as_ref()
            .unwrap_err()
            .contains("Access was denied")
    );
    assert!(
        !screenshots[0]
            .assets
            .as_ref()
            .unwrap_err()
            .contains("private")
    );
    assert_eq!(
        screenshots[1].assets.as_ref().unwrap()[0]["id"],
        "tablet-shot"
    );
    assert!(result.previews.unwrap_err().contains("request limit"));
}

#[test]
fn rejects_invalid_selection_and_cancellation_before_requesting_media() {
    for (app, version, locale) in [
        ("../123", "draft", "en-US"),
        ("123", "draft/other", "en-US"),
        ("123", "draft", "en-US/../../other"),
        ("123", "draft", ""),
    ] {
        assert!(
            load(
                &mut Fake::default(),
                app,
                version,
                locale,
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    assert!(
        load(
            &mut Fake::default(),
            "123",
            "draft",
            "en-US",
            &AtomicBool::new(true)
        )
        .is_err()
    );
}

#[test]
fn rejects_foreign_media_before_loading_its_images() {
    let mut fake = Fake::default();
    fake.page(
        "/apps/123/versions/draft/localizations/en-US/screenshot-sets?limit=200",
        json!([{"id":"foreign", "versionId":"live"}]),
    );
    fake.page(
        "/apps/123/versions/draft/localizations/en-US/preview-sets?limit=200",
        json!([]),
    );
    fake.placements("en-US", json!([]));
    let result = load(&mut fake, "123", "draft", "en-US", &AtomicBool::new(false)).unwrap();
    assert!(
        result
            .screenshots
            .as_ref()
            .err()
            .unwrap()
            .contains("different app or version")
    );
    assert!(fake.0.is_empty());
}

#[test]
fn builds_bounded_image_urls_for_screenshots_and_nested_preview_posters() {
    let image = json!({"templateUrl":"https://is1-ssl.mzstatic.com/image/{w}x{h}{c}.{f}", "width":1290, "height":2796});
    for (asset, kind) in [
        (json!({"imageAsset":image}), Kind::Screenshot),
        (json!({"previewFrameImage":{"image":image}}), Kind::Preview),
        (json!({"imageAsset":image}), Kind::LibraryImage),
    ] {
        assert_eq!(image_dimensions(&asset, kind), Some([1290, 2796]));
        assert_eq!(
            image_url(&asset, kind).unwrap().as_str(),
            "https://is1-ssl.mzstatic.com/image/472x1024.png"
        );
    }
    for template in [
        "file:///tmp/{w}.png",
        "http://example.com/{w}.png",
        "https://user:password@example.com/{w}.png",
        "https://example.com/{unsupported}.png",
    ] {
        let asset = json!({"imageAsset":{"templateUrl":template,"width":100,"height":100}});
        assert!(image_url(&asset, Kind::Screenshot).is_none());
    }
    assert!(image_url(&json!({"imageAsset":{"templateUrl":"https://example.com/{w}.png","width":0,"height":100}}), Kind::Screenshot).is_none());
    assert!(
        image_url(
            &json!({"previewFrameImage":{"state":{"state":"PROCESSING"}}}),
            Kind::Preview
        )
        .is_none()
    );
}

#[test]
fn loads_header_and_search_images_across_pages_and_reuses_shared_image_metadata() {
    let mut fake = Fake::default();
    fake.empty_sets();
    fake.0.push_back((
        format!("/apps/123/versions/draft/localizations/en-US/placements?placementType={HEADER_PLACEMENT},{SEARCH_PLACEMENT}&limit=200"),
        200,
        json!({"items":[{"id":"header", "appId":"123", "versionId":"draft", "placementType":HEADER_PLACEMENT, "mediaType":"IMAGE", "imageId":"shared", "state":"PARENT_APPROVED"}], "nextCursor":"more+="}),
    ));
    fake.page(
        &format!("/apps/123/versions/draft/localizations/en-US/placements?placementType={HEADER_PLACEMENT},{SEARCH_PLACEMENT}&limit=200&cursor=more%2B%3D"),
        json!([
            {"id":"search", "placementType":SEARCH_PLACEMENT, "mediaType":"IMAGE", "imageId":"shared"},
            {"id":"video", "placementType":HEADER_PLACEMENT, "mediaType":"VIDEO", "videoId":"video1"},
            {"id":"screenshot", "placementType":"APP_SCREENSHOT", "imageId":"screen1"}
        ]),
    );
    fake.0.push_back(("/library-images/shared".into(), 200, json!({
        "id":"shared", "fileName":"brand.png", "state":"APPROVED",
        "imageAsset":{"templateUrl":"https://example.test/{w}x{h}.{f}", "width":1920, "height":1080}
    })));
    let media = load(&mut fake, "123", "draft", "en-US", &AtomicBool::new(false)).unwrap();
    let placements = media.placements.unwrap();
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].placement["id"], "header");
    assert_eq!(placements[1].placement["id"], "search");
    assert_eq!(placements[0].image, placements[1].image);
    assert_eq!(
        image_url(placements[0].image.as_ref().unwrap(), Kind::LibraryImage)
            .unwrap()
            .as_str(),
        "https://example.test/1024x576.png"
    );
    assert!(fake.0.is_empty());
}

#[test]
fn image_failures_are_local_and_placement_failures_are_not_empty_results() {
    let mut fake = Fake::default();
    fake.empty_sets();
    fake.placements("en-US", json!([
        {"id":"denied", "placementType":HEADER_PLACEMENT, "mediaType":"IMAGE", "imageId":"private"},
        {"id":"missing", "placementType":SEARCH_PLACEMENT, "mediaType":"IMAGE"},
        {"id":"invalid", "placementType":HEADER_PLACEMENT, "imageId":"../bad"},
        {"id":"wrong", "placementType":SEARCH_PLACEMENT, "imageId":"expected"},
        {"id":"valid", "placementType":SEARCH_PLACEMENT, "imageId":"ok"}
    ]));
    fake.0.push_back((
        "/library-images/private".into(),
        403,
        json!({"error":{"message":"private upstream details"}}),
    ));
    fake.0.push_back((
        "/library-images/expected".into(),
        200,
        json!({"id":"other"}),
    ));
    fake.0.push_back((
        "/library-images/ok".into(),
        200,
        json!({"id":"ok", "state":"UPLOAD_COMPLETE"}),
    ));
    let media = load(&mut fake, "123", "draft", "en-US", &AtomicBool::new(false)).unwrap();
    let placements = media.placements.unwrap();
    let error = placements[0].image.as_ref().unwrap_err();
    assert!(error.contains("Access was denied"));
    assert!(!error.contains("private upstream"));
    assert!(
        placements[1]
            .image
            .as_ref()
            .unwrap_err()
            .contains("No image is linked")
    );
    assert!(placements[2].image.is_err());
    assert!(
        placements[3]
            .image
            .as_ref()
            .unwrap_err()
            .contains("different library image")
    );
    assert_eq!(
        placements[4].image.as_ref().unwrap()["state"],
        "UPLOAD_COMPLETE"
    );
    assert!(fake.0.is_empty());

    let mut fake = Fake::default();
    fake.empty_sets();
    fake.0.push_back((
        format!("/apps/123/versions/draft/localizations/en-US/placements?placementType={HEADER_PLACEMENT},{SEARCH_PLACEMENT}&limit=200"),
        403,
        json!({}),
    ));
    let media = load(&mut fake, "123", "draft", "en-US", &AtomicBool::new(false)).unwrap();
    assert!(media.placements.unwrap_err().contains("Access was denied"));
    assert!(media.screenshots.is_ok());
    assert!(media.previews.is_ok());
}

#[test]
fn rejects_foreign_placements_before_following_image_links() {
    let mut fake = Fake::default();
    fake.empty_sets();
    fake.placements(
        "en-US",
        json!([
            {"id":"valid", "placementType":HEADER_PLACEMENT, "imageId":"image1"},
            {"id":"foreign", "appId":"456", "placementType":SEARCH_PLACEMENT, "imageId":"image2"}
        ]),
    );
    let media = load(&mut fake, "123", "draft", "en-US", &AtomicBool::new(false)).unwrap();
    assert!(
        media
            .placements
            .unwrap_err()
            .contains("different app or version")
    );
    assert!(fake.0.is_empty());
}

#[test]
fn decodes_images_without_distorting_their_aspect_ratio_and_rejects_invalid_bytes() {
    let image = image::DynamicImage::new_rgba8(1200, 600);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    assert_eq!(decode_image(bytes.get_ref()).unwrap().size, [1024, 512]);
    assert!(decode_image(b"not an image").is_err());
}
