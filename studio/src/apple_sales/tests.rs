use super::*;
use egui::{Pos2, Rect, Shape, vec2};

fn sample_report() -> LoadedReport {
    LoadedReport {
        apps: data::parse_report(concat!(
            "SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\tParent Identifier\n",
            "custom-sku\tA long app name that needs to remain readable on a narrow screen\t1.2.345\t1F\t15\tUS\t101\tiPhone\t\n",
            "custom-sku\tA long app name that needs to remain readable on a narrow screen\t1.2.345\t7F\t2\tGB\t101\tiPhone\t\n",
            "purchase-sku\tPremium subscription\t\tIAY\t3\tCA\t202\t\tcustom-sku\n",
            "unrelated-sku\tUnrelated product\t2.0\t3F\t-1\tDE\t303\tiPad\t\n",
        )).unwrap(),
        cached: true,
        cache_warning: None,
    }
}

fn loaded_sales() -> SalesReports {
    SalesReports {
        credentials: Some(AppleCredentials::test_credentials("test-account")),
        vendor: "12345".into(),
        report: Some(sample_report()),
        attempted: true,
        ..Default::default()
    }
}

fn text_in(output: &egui::FullOutput, expected: &str) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape,
        Shape::Text(text) if text.galley.text() == expected)
    })
}

#[test]
fn report_states_and_expanded_details_fit_supported_sizes_in_both_themes() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for (width, height) in [
            (390.0, 844.0),
            (768.0, 1024.0),
            (1280.0, 800.0),
            (1440.0, 900.0),
        ] {
            for state in ["setup", "loaded", "loading", "error", "empty"] {
                let mut sales = if state == "setup" {
                    SalesReports::default()
                } else {
                    loaded_sales()
                };
                let (sender, receiver) = mpsc::channel();
                if state == "loading" {
                    sales.report = None;
                    sales.job = Some(ReportJob {
                        receiver,
                        cancelled: Arc::new(AtomicBool::new(false)),
                    });
                } else if state == "error" {
                    sales.report = None;
                    sales.error = Some("Apple denied sales report access. Check the API key's App Store Connect permissions.".into());
                } else if state == "empty" {
                    sales.report = Some(LoadedReport {
                        apps: vec![],
                        cached: false,
                        cache_warning: None,
                    });
                }
                let ctx = egui::Context::default();
                crate::style::configure(&ctx);
                ctx.set_theme(theme);
                ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
                let mut icons = AppIcons::default();
                for _ in 0..2 {
                    let output = ctx.run_ui(egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
                        ..Default::default()
                    }, |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                if let Some(report) = &sales.report {
                                    for app in &report.apps {
                                        let id = ui.make_persistent_id(("sales_product", &sales.vendor, &app.sku));
                                        let mut collapse = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
                                        collapse.set_open(true);
                                        collapse.store(ui.ctx());
                                    }
                                }
                                sales.ui(ui, None, &[], &mut icons);
                                assert!(ui.min_rect().right() <= width, "{state} overflow at {width}px: {:?}", ui.min_rect());
                            });
                        });
                    });
                    assert!(text_in(&output, "App Store sales"));
                    match state {
                        "loading" => assert!(text_in(&output, "Loading sales report…")),
                        "empty" => assert!(text_in(&output, "No sales reported for this period.")),
                        "loaded" => {
                            assert!(text_in(&output, "15 downloads · 2 updates · 0 redownloads"))
                        }
                        _ => {}
                    }
                }
                drop(sender);
            }
        }
    }
}

#[test]
fn sales_rows_expand_with_pointer_and_collapse_with_keyboard() {
    let report = sample_report();
    let app = report
        .apps
        .iter()
        .find(|app| app.sku == "purchase-sku")
        .unwrap();
    let ctx = egui::Context::default();
    crate::style::configure(&ctx);
    ctx.style_mut_of(egui::Theme::Light, |style| style.animation_time = 0.0);
    ctx.style_mut_of(egui::Theme::Dark, |style| style.animation_time = 0.0);
    let mut icons = AppIcons::default();
    let mut render = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(390.0, 844.0))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| sales_row(ui, app, "12345", &[], &mut icons));
            },
        )
    };
    let _ = render(vec![]);
    let output = render(vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let Shape::Text(text) = &shape.shape {
                (text.galley.text() == "Premium subscription")
                    .then(|| text.pos + text.galley.size() * 0.5)
            } else {
                None
            }
        })
        .unwrap();
    for pressed in [true, false] {
        let _ = render(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
    }
    assert!(text_in(&render(vec![]), "Countries: CA"));
    assert!(
        ctx.memory(|memory| memory.focused().is_some()),
        "Row did not retain focus after click"
    );
    let _ = render(vec![egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert!(!text_in(&render(vec![]), "Countries: CA"));
}

#[test]
fn app_filter_uses_apple_id_and_parent_sku_instead_of_bundle_id() {
    let mut sales = loaded_sales();
    let ctx = egui::Context::default();
    let mut icons = AppIcons::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        sales.ui(ui, Some(("101", "custom-sku")), &[], &mut icons);
    });
    assert!(text_in(&output, "2 products · 20 units"));
    assert!(!text_in(&output, "Unrelated product"));
}

#[test]
fn navigating_and_changing_accounts_discards_pending_results() {
    let mut sales = loaded_sales();
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    sales.job = Some(ReportJob {
        receiver,
        cancelled: cancelled.clone(),
    });
    sales.select_period(Frequency::Daily, Frequency::Daily.latest(today()));
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(sender.send(Ok(sample_report())).is_err());
    sales.poll();
    assert!(sales.report.is_none());
    assert!(!sales.attempted);

    sales.report = Some(sample_report());
    sales.error = Some("Old account error".into());
    sales.set_account(
        Some(AppleCredentials::test_credentials("different-account")),
        "67890",
    );
    assert!(sales.report.is_none());
    assert!(sales.error.is_none());
    assert_eq!(sales.vendor, "67890");
}

#[test]
fn failed_refresh_keeps_the_current_report_and_currency_amounts_remain_separate() {
    let mut sales = loaded_sales();
    let (sender, receiver) = mpsc::channel();
    sales.job = Some(ReportJob {
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    sender.send(Err("Offline. Please retry.".into())).unwrap();
    sales.poll();
    assert!(sales.report.is_some());
    assert_eq!(sales.error.as_deref(), Some("Offline. Please retry."));
    assert_eq!(
        format_proceeds(&BTreeMap::from([("USD".into(), -109), ("CAD".into(), 225)])),
        "CAD 2.25 · USD −1.09"
    );
}
