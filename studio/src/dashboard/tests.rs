use super::*;

#[test]
fn analytics_shows_sales_without_requiring_apps_and_opens_apple_settings() {
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        let mut dashboard = Dashboard::default();
        dashboard.select_section(Section::Analytics);
        dashboard.mobile_detail = true;
        let mut play = PlayStore::default();
        let mut apple = AppleStore::default();
        let mut dynadot = crate::dynadot::Dynadot::default();
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let mut button = Pos2::ZERO;
        let mut action = None;
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
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    action = dashboard.ui(ui, &mut play, &mut apple, &mut dynadot, true);
                    assert!(
                        ui.min_rect().right() <= width,
                        "Analytics overflow at {width}px"
                    );
                },
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(text) if text.galley.text() == "App Store sales")));
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(text) if text.galley.text() == "Search apps or identifiers…" || text.galley.text() == "‹  My Apps")));
            if phase < 2 {
                button = output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let Shape::Text(text) = &shape.shape {
                            (text.galley.text() == "Apple settings…")
                                .then(|| text.pos + text.galley.size() * 0.5)
                        } else {
                            None
                        }
                    })
                    .expect("Sales setup button must be visible");
            }
        }
        assert!(matches!(action, Some(Command::AppleSettings)));
    }
}

#[test]
fn google_oauth_navigation_opens_page_at_supported_sizes() {
    fn label_position(output: &egui::FullOutput, label: &str) -> Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let Shape::Text(text) = &shape.shape {
                    (text.galley.text() == label).then(|| text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing label: {label}"))
    }

    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        let mut dashboard = Dashboard {
            google_oauth: crate::google_oauth::GoogleOAuth::test_connection(),
            ..Default::default()
        };
        let mut play = PlayStore::default();
        let mut apple = AppleStore::default();
        let mut dynadot = crate::dynadot::Dynadot::default();
        let ctx = egui::Context::default();
        crate::style::configure(&ctx);
        let mut render = |dashboard: &mut Dashboard, events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    dashboard.ui(ui, &mut play, &mut apple, &mut dynadot, true);
                    assert!(ui.min_rect().right() <= width, "Overflow at {width}px");
                },
            )
        };
        let mut click = |dashboard: &mut Dashboard, pos: Pos2| {
            let _ = render(dashboard, vec![]);
            for pressed in [true, false] {
                let _ = render(
                    dashboard,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            render(dashboard, vec![])
        };
        let mut output = click(&mut dashboard, pos2(width - 1.0, height - 1.0));
        if width < 1100.0 {
            let pos = label_position(&output, "Overview");
            output = click(&mut dashboard, pos);
        }
        let pos = label_position(&output, "Google OAuth");
        let _ = click(&mut dashboard, pos);
        assert_eq!(
            dashboard.navigation,
            Section::GoogleOAuth,
            "OAuth click failed at {width}px"
        );
        // A selected app's mobile back button must not replace OAuth navigation.
        dashboard.mobile_detail = true;
        output = click(&mut dashboard, pos2(width - 1.0, height - 1.0));
        label_position(&output, "Google OAuth");
        label_position(&output, "Refresh projects");
        label_position(
            &output,
            "No active Google Cloud projects are visible to this account.",
        );
        assert!(!output.shapes.iter().any(|shape| {
            matches!(&shape.shape, Shape::Text(text) if text.galley.text() == "Search apps or identifiers…" || text.galley.text() == "‹  My Apps")
        }), "App library appeared on OAuth page at {width}px");

        if width >= 1100.0 {
            let pos = label_position(&output, "Analytics");
            output = click(&mut dashboard, pos);
            assert_eq!(dashboard.navigation, Section::Analytics);
            assert!(dashboard.tab == Tab::Analytics);
            let pos = label_position(&output, "Deployments");
            output = click(&mut dashboard, pos);
            assert_eq!(dashboard.navigation, Section::Deployments);
            label_position(&output, "No deployment sources are connected yet.");
        }
    }
}

#[test]
fn ecosystem_shows_org_arrow_and_fits_map_and_list_sizes() {
    let product = Product {
        identifier: "com.cubacadabra.app".into(),
        name: "cubacadabra".into(),
        listings: Vec::new(),
    };
    for (width, height) in [
        (390.0, 844.0),
        (768.0, 1024.0),
        (1280.0, 800.0),
        (1440.0, 900.0),
    ] {
        for linked in [false, true] {
            for list_map in [false, true] {
                let mut dashboard = Dashboard {
                    github: crate::github::GitHub::test_connection(if linked {
                        "cubacadabra"
                    } else {
                        "other-org"
                    }),
                    list_map,
                    ..Default::default()
                };
                let mut play = PlayStore::default();
                let mut apple = AppleStore::default();
                let ctx = egui::Context::default();
                crate::style::configure(&ctx);
                for _ in 0..2 {
                    let mut org_color = Color32::TRANSPARENT;
                    let output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))),
                            ..Default::default()
                        },
                        |ui| {
                            egui::CentralPanel::default().show(ui, |ui| {
                                let palette = Palette::new(ui);
                                org_color = palette.blue;
                                let items = services(
                                    &product,
                                    palette,
                                    dashboard
                                        .github
                                        .linked_login(&product.identifier, &product.name),
                                );
                                assert_eq!(items.len(), 7);
                                assert_eq!(items[5].title, "GitHub org");
                                assert_eq!(items[5].connected, linked);
                                dashboard.ecosystem(
                                    ui, &product, &mut play, &mut apple, palette, &mut None,
                                );
                                assert!(ui.min_rect().right() <= width, "Overflow at {width}px");
                            });
                        },
                    );
                    if !list_map && width >= 768.0 {
                        let arrows = output.shapes.iter().filter(|shape| {
                            matches!(&shape.shape, Shape::Path(path) if path.points.len() == 3 && path.closed && path.fill == org_color)
                        }).count();
                        assert_eq!(
                            arrows,
                            usize::from(linked),
                            "Incorrect org arrow at {width}px"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn product_artwork_uses_selected_store_and_falls_back_to_available_icon() {
    let products = products(vec![
        StoreApp {
            identifier: "example.app".into(),
            name: "Example".into(),
            store_id: "123".into(),
            store: Store::Apple,
        },
        StoreApp {
            identifier: "example.app".into(),
            name: "Example".into(),
            store_id: "example.app".into(),
            store: Store::Play,
        },
    ]);
    let product = &products[0];
    assert!(
        product_icon_listing(product, Store::Play, |_| true)
            .unwrap()
            .store
            == Store::Play
    );
    assert!(
        product_icon_listing(product, Store::Apple, |_| true)
            .unwrap()
            .store
            == Store::Apple
    );
    assert!(
        product_icon_listing(product, Store::Apple, |l| l.store == Store::Play)
            .unwrap()
            .store
            == Store::Play
    );
    assert!(
        product_icon_listing(product, Store::Play, |_| false)
            .unwrap()
            .store
            == Store::Play
    );
}

#[test]
fn refresh_control_receives_pointer_click_in_app_list_header() {
    for width in [272.0, 390.0] {
        let ctx = egui::Context::default();
        let mut rect = Rect::NOTHING;
        let mut clicked = false;
        for phase in 0..3 {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 844.0))),
                ..Default::default()
            };
            if phase > 0 {
                input.events = vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::PointerButton {
                        pos: rect.center(),
                        button: egui::PointerButton::Primary,
                        pressed: phase == 1,
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
            }
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default_margins().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("My Apps");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let response = refresh_control(ui);
                            rect = response.rect;
                            clicked |= response.clicked();
                        });
                    });
                });
            });
        }
        assert!(clicked, "Refresh did not receive a click at {width}px");
    }
}

#[test]
fn store_listings_merge_by_identifier_and_keep_distinct_apps() {
    let apps = products(vec![
        StoreApp {
            identifier: "dev.test.one".into(),
            name: "One".into(),
            store_id: "123".into(),
            store: Store::Apple,
        },
        StoreApp {
            identifier: "dev.test.one".into(),
            name: "One".into(),
            store_id: "dev.test.one".into(),
            store: Store::Play,
        },
        StoreApp {
            identifier: "dev.test.two".into(),
            name: "One".into(),
            store_id: "dev.test.two".into(),
            store: Store::Play,
        },
    ]);
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0].listings.len(), 2);
    let mut dashboard = Dashboard {
        search: "TEST.ONE".into(),
        filter: Filter::Android,
        ..Default::default()
    };
    assert!(dashboard.matches(&apps[0]));
    assert!(!dashboard.matches(&apps[1]));
    dashboard.search.clear();
    dashboard.filter = Filter::Apple;
    assert!(!dashboard.matches(&apps[1]));
}
