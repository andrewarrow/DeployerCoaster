use std::{fs, time::Duration};

use super::*;

#[test]
fn detects_device_case_insensitively_from_the_filename_only() {
    for name in [
        "Simulator Screenshot - iPhone 17 Pro - 2026-10-23.png",
        "01-IPHONE-home.PNG",
        "iphone.jpeg",
    ] {
        assert_eq!(Device::from_path(Path::new(name)), Some(Device::Iphone));
    }
    for name in ["Simulator Screenshot - iPad Pro.png", "01-IPAD-home.webp"] {
        assert_eq!(Device::from_path(Path::new(name)), Some(Device::Ipad));
    }
    for name in ["Screenshot iPhone Duo.png", "01-IPHONE DUO-home.PNG"] {
        assert_eq!(Device::from_path(Path::new(name)), Some(Device::IphoneDuo));
    }
    assert_eq!(
        Device::from_path(Path::new("/iPhone/screenshots/home.png")),
        None
    );
    assert_eq!(Device::from_path(Path::new("other.png")), None);
}

#[test]
fn duo_uses_the_official_display_and_orientation_for_each_example() {
    for (name, source) in [
        (
            "Screenshot iPhone Duo 10-23-2026 at 4.02.54 AM.png",
            (1398, 2034),
        ),
        (
            "Screenshot iPhone Duo 10-23-2026 at 4.03.23 AM.png",
            (2853, 2007),
        ),
        (
            "Screenshot iPhone Duo 10-23-2026 at 4.03.26 AM.png",
            (2853, 2007),
        ),
        (
            "Screenshot iPhone Duo 10-23-2026 at 4.03.31 AM.png",
            (2007, 2853),
        ),
    ] {
        let device = Device::from_path(Path::new(name)).unwrap();
        assert_eq!(device.target_dimensions(source), source, "{name}");
    }
}

#[test]
fn duo_recognizes_scaled_outer_and_inner_captures_in_both_orientations() {
    for (source, target) in [
        ((699, 1017), (1398, 2034)),
        ((1017, 699), (2034, 1398)),
        ((669, 951), (2007, 2853)),
        ((951, 669), (2853, 2007)),
        // Physical inner panel captures need the App Store screenshot dimensions.
        ((1878, 2670), (2007, 2853)),
        ((2670, 1878), (2853, 2007)),
    ] {
        assert_eq!(Device::IphoneDuo.target_dimensions(source), target);
    }
    // The originally requested fixed sizes continue to apply to other devices.
    assert_eq!(Device::Iphone.target_dimensions((2853, 2007)), (1206, 2622));
    assert_eq!(Device::Ipad.target_dimensions((1398, 2034)), (2732, 2048));
}

#[test]
fn resizes_in_place_with_the_correct_dimensions_aspect_ratio_and_centered_black_padding() {
    let directory = tempfile::tempdir().unwrap();
    let white = Rgba([255, 255, 255, 255]);
    let black = Rgba([0, 0, 0, 255]);
    for (name, source, target, bounds) in [
        // An odd padding height leaves the extra pixel below the centered image.
        ("iPhone.png", (4, 2), (1206, 2622), (0, 1009, 1206, 603)),
        ("iPad.png", (2, 4), (2732, 2048), (854, 0, 1024, 2048)),
        (
            "iPhone Duo landscape.png",
            (951, 669),
            (2853, 2007),
            (0, 0, 2853, 2007),
        ),
        // Also exercise downscaling a screenshot larger than its target.
        (
            "iPhone-large.png",
            (1208, 2624),
            (1206, 2622),
            (0, 1, 1206, 2620),
        ),
    ] {
        let path = directory.path().join(name);
        RgbaImage::from_pixel(source.0, source.1, white)
            .save(&path)
            .unwrap();
        assert_eq!(resize_file(&path), Ok(Outcome::Resized));
        let result = image::open(&path).unwrap().into_rgba8();
        assert_eq!(result.dimensions(), target);
        let (x, y, width, height) = bounds;
        assert_eq!(*result.get_pixel(x, y), white);
        assert_eq!(*result.get_pixel(x + width - 1, y + height - 1), white);
        if x > 0 {
            assert_eq!(*result.get_pixel(x - 1, target.1 / 2), black);
            assert_eq!(*result.get_pixel(x + width, target.1 / 2), black);
        }
        if y > 0 {
            assert_eq!(*result.get_pixel(target.0 / 2, y - 1), black);
            assert_eq!(*result.get_pixel(target.0 / 2, y + height), black);
        }
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 4);
}

#[test]
fn skipped_and_corrupt_files_keep_their_original_bytes() {
    let directory = tempfile::tempdir().unwrap();
    for (name, skipped) in [("home.png", true), ("iPhone-broken.png", false)] {
        let path = directory.path().join(name);
        fs::write(&path, b"original file bytes").unwrap();
        let result = resize_file(&path);
        if skipped {
            assert_eq!(result, Ok(Outcome::Skipped));
        } else {
            assert!(result.is_err());
        }
        assert_eq!(fs::read(path).unwrap(), b"original file bytes");
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn progress_tracks_the_current_file_and_all_outcomes_without_stopping_on_errors() {
    let (sender, result) = mpsc::channel();
    let mut resize = Resize {
        job: Some(result),
        total: 3,
        ..Default::default()
    };
    sender
        .send(Update::Started("iPhone-broken.png".into()))
        .unwrap();
    resize.poll();
    assert_eq!(resize.completed, 0);
    assert_eq!(resize.current.as_deref(), Some("iPhone-broken.png"));
    sender
        .send(Update::Finished(
            "iPhone-broken.png".into(),
            Err("Could not decode image.".into()),
        ))
        .unwrap();
    sender.send(Update::Started("home.png".into())).unwrap();
    resize.poll();
    assert_eq!(resize.completed, 1);
    assert_eq!(resize.errors.len(), 1);
    assert_eq!(resize.current.as_deref(), Some("home.png"));
    sender
        .send(Update::Finished("home.png".into(), Ok(Outcome::Skipped)))
        .unwrap();
    sender.send(Update::Started("iPad.png".into())).unwrap();
    resize.poll();
    assert_eq!(resize.completed, 2);
    assert_eq!(resize.skipped, ["home.png"]);
    assert_eq!(resize.current.as_deref(), Some("iPad.png"));
    sender
        .send(Update::Finished("iPad.png".into(), Ok(Outcome::Resized)))
        .unwrap();
    resize.poll();
    assert_eq!(resize.completed, 3);
    assert_eq!(resize.resized, 1);
    assert!(resize.job.is_none());
    assert!(resize.current.is_none());
}

#[test]
fn batch_resizing_continues_after_failures_and_survives_gallery_selection_changes() {
    let directory = tempfile::tempdir().unwrap();
    let broken = directory.path().join("iPhone-broken.png");
    let skipped = directory.path().join("home.png");
    let valid = directory.path().join("iPhone.png");
    fs::write(&broken, b"broken image").unwrap();
    fs::write(&skipped, b"unchanged").unwrap();
    RgbaImage::from_pixel(2, 2, Rgba([255, 255, 255, 255]))
        .save(&valid)
        .unwrap();

    let mut gallery = super::super::Gallery::default();
    gallery.resize.start(
        vec![broken.clone(), skipped.clone(), valid.clone()],
        &Context::default(),
    );
    gallery.clear();
    assert!(gallery.resize.job.is_some());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while gallery.resize.job.is_some() {
        assert!(std::time::Instant::now() < deadline, "resizing timed out");
        gallery.resize.poll();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(gallery.resize.completed, 3);
    assert_eq!(gallery.resize.resized, 1);
    assert_eq!(gallery.resize.skipped, ["home.png"]);
    assert_eq!(gallery.resize.errors.len(), 1);
    assert_eq!(image::image_dimensions(valid).unwrap(), (1206, 2622));
    assert_eq!(fs::read(broken).unwrap(), b"broken image");
    assert_eq!(fs::read(skipped).unwrap(), b"unchanged");
}

#[test]
fn a_disconnected_worker_reports_an_interrupted_batch() {
    let (sender, result) = mpsc::channel();
    let mut resize = Resize {
        job: Some(result),
        total: 2,
        current: Some("iPhone.png".into()),
        ..Default::default()
    };
    drop(sender);
    resize.poll();
    assert!(resize.job.is_none());
    assert!(resize.current.is_none());
    assert_eq!(
        resize.errors,
        ["Resizing stopped before all selected files were processed."]
    );
}
