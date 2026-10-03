use super::*;

impl Dashboard {
    pub(super) fn product_header(
        &self,
        ui: &mut Ui,
        product: &Product,
        play: &mut PlayStore,
        apple: &mut AppleStore,
        p: Palette,
        command: &mut Option<Command>,
    ) {
        let wide = ui.available_width() >= 620.0;
        let (rect, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), if wide { 90.0 } else { 144.0 }),
            Sense::hover(),
        );
        paint_product(
            ui,
            Rect::from_min_size(rect.min, vec2(80.0, 80.0)),
            product,
            self.filter,
            play,
            apple,
        );
        let x = rect.left() + 96.0;
        text(
            ui,
            pos2(x, rect.top() + 4.0),
            &product.name,
            26.0,
            true,
            p.text,
            if wide {
                (rect.width() - 370.0).max(160.0)
            } else {
                rect.width() - 96.0
            },
        );
        text(
            ui,
            pos2(x, rect.top() + 40.0),
            &product.identifier,
            12.0,
            false,
            p.muted,
            rect.width() - 96.0,
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("product_platforms")
                .max_rect(Rect::from_min_max(
                    pos2(x, rect.top() + 61.0),
                    pos2(rect.right(), rect.top() + 90.0),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        for listing in &product.listings {
            egui::Frame::NONE
                .fill(p.surface)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 3))
                .show(&mut child, |ui| {
                    ui.label(
                        egui::RichText::new(if listing.store == Store::Apple {
                            "iOS"
                        } else {
                            "Android"
                        })
                        .size(11.0)
                        .color(p.muted),
                    );
                });
        }
        let actions_rect = if wide {
            Rect::from_min_size(
                pos2(rect.right() - 256.0, rect.top() + 4.0),
                vec2(256.0, 36.0),
            )
        } else {
            Rect::from_min_size(rect.min + vec2(0.0, 100.0), vec2(rect.width(), 36.0))
        };
        let mut actions = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("product_actions")
                .max_rect(actions_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        actions.menu_button("•••", |ui| {
            if ui.button("Refresh apps").clicked() {
                play.refresh_dashboard(ui.ctx());
                apple.refresh_dashboard(ui.ctx());
                ui.close();
            }
            if ui.button("Settings…").clicked() {
                *command = Some(Command::Settings);
                ui.close();
            }
            if ui.button("Open workspace…").clicked() {
                *command = Some(Command::OpenWorkspace);
                ui.close();
            }
        });
        if product.listings.len() == 1 {
            if actions.button("Open in Store  ↗").clicked() {
                actions
                    .ctx()
                    .open_url(egui::OpenUrl::new_tab(product.listings[0].url()));
            }
        } else {
            actions.menu_button("Open in Store  ↗", |ui| {
                for listing in &product.listings {
                    if ui
                        .button(if listing.store == Store::Apple {
                            "App Store Connect"
                        } else {
                            "Google Play"
                        })
                        .clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(listing.url()));
                        ui.close();
                    }
                }
            });
        }
        actions.label(egui::RichText::new("● Connected").size(11.0).color(p.green));
    }

    pub(super) fn tabs(&mut self, ui: &mut Ui, p: Palette, command: &mut Option<Command>) {
        egui::Frame::NONE
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(6)
            .inner_margin(3)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    let icon_tabs = ui.available_width() > 820.0;
                    let tab_width = if icon_tabs {
                        (ui.available_width() - 24.0) / 7.0
                    } else {
                        0.0
                    };
                    for (tab, label, icon) in Tab::ALL {
                        let width = if icon_tabs {
                            tab_width
                        } else {
                            ui.painter()
                                .layout_no_wrap(
                                    label.to_owned(),
                                    FontId::proportional(11.0),
                                    p.text,
                                )
                                .size()
                                .x
                                + 24.0
                        };
                        let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
                        let response = control(ui, rect, ("product_tab", label), label);
                        if self.tab == tab || response.hovered() || response.has_focus() {
                            ui.painter().rect_filled(
                                rect,
                                5.0,
                                if self.tab == tab { p.selected } else { p.bg },
                            );
                            if self.tab == tab || response.has_focus() {
                                ui.painter().line_segment(
                                    [rect.left_bottom(), rect.right_bottom()],
                                    Stroke::new(2.0, p.blue),
                                );
                            }
                        }
                        if icon_tabs {
                            icon.paint(ui, rect.min + vec2(8.0, 8.0), 15.0, p.muted);
                        }
                        text(
                            ui,
                            rect.min + vec2(if icon_tabs { 30.0 } else { 12.0 }, 10.0),
                            label,
                            11.0,
                            false,
                            p.text,
                            width - if icon_tabs { 34.0 } else { 16.0 },
                        );
                        if response.clicked() {
                            if tab == Tab::Settings {
                                *command = Some(Command::Settings);
                            } else {
                                self.tab = tab;
                            }
                        }
                    }
                });
            });
    }

    pub(super) fn ecosystem(
        &mut self,
        ui: &mut Ui,
        product: &Product,
        play: &mut PlayStore,
        apple: &mut AppleStore,
        p: Palette,
        command: &mut Option<Command>,
    ) {
        self.github.prepare(ui.ctx());
        egui::Frame::NONE
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(10)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Ecosystem").size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::selectable(self.list_map, "List")
                                    .min_size(vec2(48.0, 28.0)),
                            )
                            .clicked()
                        {
                            self.list_map = true;
                        }
                        if ui
                            .add(
                                egui::Button::selectable(!self.list_map, "Map")
                                    .min_size(vec2(48.0, 28.0)),
                            )
                            .clicked()
                        {
                            self.list_map = false;
                        }
                    });
                });
                ui.label(
                    egui::RichText::new(format!(
                        "Everything that powers {}, connected.",
                        product.name
                    ))
                    .size(12.0)
                    .color(p.muted),
                );
                ui.add_space(8.0);
                if self
                    .github
                    .organization_selector(ui, &product.identifier, &product.name)
                {
                    self.navigation = Section::GitHub;
                }
                ui.add_space(8.0);
                let services = services(
                    product,
                    p,
                    self.github.linked_login(&product.identifier, &product.name),
                );
                if self.list_map || ui.available_width() < 560.0 {
                    for (i, service) in services.iter().enumerate() {
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 92.0), Sense::hover());
                        if service_card(ui, rect, service, p, i, &mut self.github) {
                            service_action(service, product, ui, command);
                        }
                    }
                } else {
                    let (canvas, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 354.0), Sense::hover());
                    let side_width = (canvas.width() * 0.30).min(290.0);
                    let left = canvas.left();
                    let right = canvas.right() - side_width;
                    let top = canvas.top();
                    let positions = [
                        Rect::from_min_size(pos2(left, top), vec2(side_width, 98.0)),
                        Rect::from_min_size(pos2(left, top + 112.0), vec2(side_width, 98.0)),
                        Rect::from_min_size(pos2(right, top), vec2(side_width, 98.0)),
                        Rect::from_min_size(pos2(right, top + 112.0), vec2(side_width, 98.0)),
                        Rect::from_min_size(
                            pos2(left, top + 254.0),
                            vec2((canvas.width() - 24.0) / 3.0, 100.0),
                        ),
                        Rect::from_min_size(
                            pos2(left + (canvas.width() + 12.0) / 3.0, top + 254.0),
                            vec2((canvas.width() - 24.0) / 3.0, 100.0),
                        ),
                        Rect::from_min_size(
                            pos2(left + (canvas.width() + 12.0) * 2.0 / 3.0, top + 254.0),
                            vec2((canvas.width() - 24.0) / 3.0, 100.0),
                        ),
                    ];
                    let core = Rect::from_center_size(
                        pos2(canvas.center().x, top + 123.0),
                        vec2(112.0, 132.0),
                    );
                    for (i, rect) in positions.iter().enumerate() {
                        let (a, b) = match i {
                            0 | 1 => (
                                rect.right_center(),
                                pos2(core.left(), core.top() + 36.0 + i as f32 * 54.0),
                            ),
                            2 | 3 => (
                                pos2(core.right(), core.top() + 36.0 + (i - 2) as f32 * 54.0),
                                rect.left_center(),
                            ),
                            _ => (
                                pos2(core.left() + 16.0 + (i - 4) as f32 * 40.0, core.bottom()),
                                rect.center_top(),
                            ),
                        };
                        let color = if services[i].connected {
                            services[i].color
                        } else {
                            p.muted.gamma_multiply(0.5)
                        };
                        let mid = if i < 4 {
                            vec2((b.x - a.x) * 0.5, 0.0)
                        } else {
                            vec2(0.0, (b.y - a.y) * 0.6)
                        };
                        ui.painter()
                            .add(egui::epaint::CubicBezierShape::from_points_stroke(
                                [a, a + mid, b - mid, b],
                                false,
                                Color32::TRANSPARENT,
                                Stroke::new(1.4, color),
                            ));
                        ui.painter().circle_filled(a, 3.5, color);
                        if services[i].github_org.is_some() {
                            // The final tangent points from the app down into its organization.
                            let direction = mid.normalized();
                            let normal = vec2(-direction.y, direction.x);
                            ui.painter().add(Shape::convex_polygon(
                                vec![
                                    b,
                                    b - direction * 9.0 + normal * 4.5,
                                    b - direction * 9.0 - normal * 4.5,
                                ],
                                color,
                                Stroke::NONE,
                            ));
                        } else {
                            ui.painter().circle_filled(b, 3.5, color);
                        }
                    }
                    ui.painter().rect(
                        core,
                        24.0,
                        p.surface,
                        Stroke::new(1.5, p.blue),
                        StrokeKind::Inside,
                    );
                    paint_product(
                        ui,
                        Rect::from_center_size(core.center() - vec2(0.0, 12.0), vec2(72.0, 72.0)),
                        product,
                        self.filter,
                        play,
                        apple,
                    );
                    text(
                        ui,
                        core.min + vec2(8.0, 100.0),
                        &product.name,
                        13.0,
                        true,
                        p.text,
                        core.width() - 16.0,
                    );
                    for (i, rect) in positions.into_iter().enumerate() {
                        if service_card(ui, rect, &services[i], p, i, &mut self.github) {
                            service_action(&services[i], product, ui, command);
                        }
                    }
                }
            });
    }

    pub(super) fn details(&self, ui: &mut Ui, product: &Product, p: Palette) {
        if ui.available_width() < 640.0 {
            activity(ui, product, p);
            ui.add_space(10.0);
            links(ui, product, p);
        } else {
            ui.columns(2, |columns| {
                activity(&mut columns[0], product, p);
                links(&mut columns[1], product, p);
            });
        }
    }

    pub(super) fn unavailable_tab(&self, ui: &mut Ui, p: Palette) {
        let (title, message) = match self.tab {
            Tab::Releases => (
                "Releases",
                "Release history isn't available from the connected app discovery services.",
            ),
            Tab::Monetization => (
                "Monetization",
                "No revenue service is connected to this app.",
            ),
            Tab::Users => ("Users", "No user data source is connected to this app."),
            _ => ("Overview", "No data is available yet."),
        };
        ui.add_space(8.0);
        ui.heading(title);
        ui.label(egui::RichText::new(message).color(p.muted));
    }
}

pub(super) struct Service {
    pub(super) title: &'static str,
    icon: Icon,
    color: Color32,
    subtitle: String,
    detail: String,
    pub(super) connected: bool,
    store: Option<Store>,
    github_org: Option<String>,
}

pub(super) fn services(product: &Product, p: Palette, github_org: Option<&str>) -> Vec<Service> {
    let mut services = Vec::new();
    for (store, title, icon, color) in [
        (Store::Apple, "App Store", Icon::Apple, p.blue),
        (Store::Play, "Google Play", Icon::Play, p.green),
    ] {
        let listing = product.listings.iter().find(|l| l.store == store);
        services.push(Service {
            title,
            icon,
            color,
            subtitle: listing
                .map(|l| l.name.clone())
                .unwrap_or_else(|| "No linked listing".into()),
            detail: listing.map(|l| l.identifier.clone()).unwrap_or_default(),
            connected: listing.is_some(),
            store: Some(store),
            github_org: None,
        });
    }
    for (title, icon, color) in [
        ("Websites", Icon::Globe, Color32::from_rgb(103, 174, 247)),
        (
            "Domains & DNS",
            Icon::Domain,
            Color32::from_rgb(224, 164, 81),
        ),
        ("Hosting / Backend", Icon::Server, p.blue),
        ("GitHub org", Icon::Users, p.blue),
        (
            "Analytics & Monetization",
            Icon::Chart,
            Color32::from_rgb(174, 123, 238),
        ),
    ] {
        let org = (title == "GitHub org").then_some(github_org).flatten();
        services.push(Service {
            title,
            icon,
            color,
            subtitle: org
                .unwrap_or(if title == "GitHub org" {
                    "Choose an organization"
                } else {
                    "No service linked"
                })
                .into(),
            detail: String::new(),
            connected: org.is_some(),
            store: None,
            github_org: org.map(str::to_owned),
        });
    }
    services
}

fn service_card(
    ui: &Ui,
    rect: Rect,
    service: &Service,
    p: Palette,
    index: usize,
    github: &mut crate::github::GitHub,
) -> bool {
    let response = control(ui, rect, ("ecosystem_service", index), service.title);
    let actionable = service.store.is_some() || service.github_org.is_some();
    ui.painter().rect(
        rect,
        7.0,
        if actionable && (response.hovered() || response.has_focus()) {
            p.selected
        } else {
            p.surface
        },
        Stroke::new(
            1.0,
            if actionable && response.has_focus() {
                p.blue
            } else {
                p.border
            },
        ),
        StrokeKind::Inside,
    );
    let icon_rect = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(34.0, 34.0));
    ui.painter().rect_filled(
        icon_rect,
        6.0,
        service
            .color
            .gamma_multiply(if service.connected { 0.8 } else { 0.3 }),
    );
    service.icon.paint(
        ui,
        icon_rect.min + vec2(7.0, 7.0),
        20.0,
        if service.connected {
            Color32::WHITE
        } else {
            p.muted
        },
    );
    if let Some(login) = &service.github_org {
        github.paint_org_icon(ui, icon_rect, login);
    }
    let x = rect.left() + 55.0;
    let width = rect.width() - 65.0;
    text(
        ui,
        pos2(x, rect.top() + 11.0),
        service.title,
        12.0,
        true,
        p.text,
        width,
    );
    text(
        ui,
        pos2(x, rect.top() + 33.0),
        &service.subtitle,
        11.0,
        false,
        p.muted,
        width,
    );
    if !service.detail.is_empty() {
        text(
            ui,
            pos2(x, rect.top() + 50.0),
            &service.detail,
            10.0,
            false,
            p.muted,
            width,
        );
    }
    let color = if service.connected { p.green } else { p.muted };
    ui.painter()
        .circle_filled(pos2(x + 3.0, rect.bottom() - 14.0), 3.5, color);
    text(
        ui,
        pos2(x + 13.0, rect.bottom() - 20.0),
        if service.connected {
            "Connected"
        } else {
            "Not connected"
        },
        10.0,
        false,
        color,
        width - 13.0,
    );
    if actionable {
        response
            .on_hover_text(if service.github_org.is_some() {
                "Open GitHub organization"
            } else if service.connected {
                "Open store listing"
            } else {
                "Open store settings"
            })
            .clicked()
    } else {
        false
    }
}

fn service_action(service: &Service, product: &Product, ui: &Ui, command: &mut Option<Command>) {
    if let Some(login) = &service.github_org {
        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
            "https://github.com/{login}"
        )));
        return;
    }
    if let Some(listing) = product
        .listings
        .iter()
        .find(|l| Some(l.store) == service.store)
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(listing.url()));
    } else {
        *command = Some(Command::Settings);
    }
}

pub(super) fn metrics(ui: &mut Ui, p: Palette) {
    let width = ui.available_width();
    let columns = if width >= 640.0 { 4 } else { 2 };
    egui::Grid::new("app_metrics")
        .num_columns(columns)
        .spacing(vec2(10.0, 10.0))
        .show(ui, |ui| {
            for (i, (label, icon, color)) in [
                ("Total Users", Icon::Users, Color32::from_rgb(116, 137, 225)),
                ("Revenue (30d)", Icon::Dollar, p.green),
                ("Active Devices", Icon::Phone, p.blue),
                (
                    "Crash Rate",
                    Icon::Activity,
                    Color32::from_rgb(174, 123, 238),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let (rect, _) = ui.allocate_exact_size(
                    vec2((width - (columns - 1) as f32 * 10.0) / columns as f32, 66.0),
                    Sense::hover(),
                );
                ui.painter().rect(
                    rect,
                    7.0,
                    p.surface,
                    Stroke::new(1.0, p.border),
                    StrokeKind::Inside,
                );
                let icon_rect = Rect::from_min_size(rect.min + vec2(10.0, 15.0), vec2(34.0, 34.0));
                ui.painter()
                    .rect_filled(icon_rect, 7.0, color.gamma_multiply(0.3));
                icon.paint(ui, icon_rect.min + vec2(8.0, 8.0), 18.0, color);
                text(
                    ui,
                    rect.min + vec2(54.0, 11.0),
                    label,
                    11.0,
                    false,
                    p.muted,
                    rect.width() - 60.0,
                );
                text(
                    ui,
                    rect.min + vec2(54.0, 29.0),
                    "—",
                    18.0,
                    true,
                    p.text,
                    22.0,
                );
                text(
                    ui,
                    rect.min + vec2(79.0, 36.0),
                    "Not connected",
                    9.0,
                    false,
                    p.muted,
                    rect.width() - 85.0,
                );
                if (i + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
}

fn activity(ui: &mut Ui, product: &Product, p: Palette) {
    egui::Frame::NONE
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(8)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new("Recent Activity").size(13.0).strong());
            ui.add_space(4.0);
            for listing in &product.listings {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
                ui.painter().line_segment(
                    [rect.left_top(), rect.right_top()],
                    Stroke::new(1.0, p.border),
                );
                ui.painter()
                    .circle_filled(rect.min + vec2(10.0, 23.0), 9.0, p.green);
                ui.painter().line_segment(
                    [rect.min + vec2(6.0, 23.0), rect.min + vec2(9.0, 26.0)],
                    Stroke::new(1.8, p.bg),
                );
                ui.painter().line_segment(
                    [rect.min + vec2(9.0, 26.0), rect.min + vec2(15.0, 19.0)],
                    Stroke::new(1.8, p.bg),
                );
                text(
                    ui,
                    rect.min + vec2(30.0, 10.0),
                    if listing.store == Store::Apple {
                        "App loaded from App Store Connect"
                    } else {
                        "App loaded from Google Play"
                    },
                    11.0,
                    false,
                    p.text,
                    rect.width() - 30.0,
                );
                text(
                    ui,
                    rect.min + vec2(30.0, 29.0),
                    &listing.identifier,
                    10.0,
                    false,
                    p.muted,
                    rect.width() - 30.0,
                );
            }
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Release and deployment history isn't connected yet.")
                    .size(11.0)
                    .color(p.muted),
            );
        });
}

fn links(ui: &mut Ui, product: &Product, p: Palette) {
    egui::Frame::NONE
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(8)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new("Key Links").size(13.0).strong());
            ui.add_space(4.0);
            for listing in &product.listings {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
                let label = if listing.store == Store::Apple {
                    "App Store Connect"
                } else {
                    "Google Play"
                };
                let response = control(ui, rect, ("store_link", &listing.store_id), label)
                    .on_hover_text(listing.url());
                ui.painter().line_segment(
                    [rect.left_top(), rect.right_top()],
                    Stroke::new(1.0, p.border),
                );
                if response.hovered() || response.has_focus() {
                    ui.painter().rect_filled(rect, 4.0, p.selected);
                }
                let icon = if listing.store == Store::Apple {
                    Icon::Apple
                } else {
                    Icon::Play
                };
                icon.paint(ui, rect.min + vec2(2.0, 15.0), 18.0, p.blue);
                text(
                    ui,
                    rect.min + vec2(30.0, 9.0),
                    label,
                    11.0,
                    false,
                    p.text,
                    rect.width() - 60.0,
                );
                text(
                    ui,
                    rect.min + vec2(30.0, 28.0),
                    &listing.url(),
                    10.0,
                    false,
                    p.blue,
                    rect.width() - 60.0,
                );
                Icon::External.paint(
                    ui,
                    pos2(rect.right() - 18.0, rect.center().y - 7.0),
                    14.0,
                    p.blue,
                );
                if response.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(listing.url()));
                }
            }
        });
}
