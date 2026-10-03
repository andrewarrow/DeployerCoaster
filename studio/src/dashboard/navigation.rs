use super::*;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    Overview,
    Apps,
    Websites,
    Domains,
    Hosting,
    GitHub,
    GoogleOAuth,
    Analytics,
    Deployments,
    Team,
    Settings,
}

impl Section {
    // Reorder this list to change navigation on every screen size. Routes use
    // Section values, so their behavior is independent of their position.
    pub(super) const ALL: &'static [(Self, &'static str, Icon)] = &[
        (Self::Overview, "Overview", Icon::Home),
        (Self::Apps, "My Apps", Icon::Apps),
        (Self::Websites, "Websites", Icon::Globe),
        (Self::Domains, "Domains", Icon::Domain),
        (Self::Hosting, "Hosting", Icon::Server),
        (Self::GitHub, "GitHub orgs", Icon::Users),
        (Self::GoogleOAuth, "Google OAuth", Icon::Shield),
        (Self::Analytics, "Analytics", Icon::Chart),
        (Self::Deployments, "Deployments", Icon::Deploy),
        (Self::Team, "Team", Icon::Users),
        (Self::Settings, "Settings", Icon::Settings),
    ];

    pub(super) fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(section, _, _)| *section == self)
            .expect("every section has a navigation entry")
            .1
    }

    pub(super) fn has_dedicated_page(self) -> bool {
        matches!(
            self,
            Self::Domains | Self::Hosting | Self::GitHub | Self::GoogleOAuth | Self::Analytics
        )
    }

    pub(super) fn is_product_section(self) -> bool {
        matches!(self, Self::Overview | Self::Apps)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Overview,
    Releases,
    Analytics,
    Monetization,
    Users,
    Infrastructure,
    Settings,
}

impl Tab {
    pub(super) const ALL: [(Self, &'static str, Icon); 7] = [
        (Self::Overview, "Overview", Icon::Apps),
        (Self::Releases, "Releases", Icon::Deploy),
        (Self::Analytics, "Analytics", Icon::Chart),
        (Self::Monetization, "Monetization", Icon::Dollar),
        (Self::Users, "Users", Icon::Users),
        (Self::Infrastructure, "Infrastructure", Icon::Server),
        (Self::Settings, "Settings", Icon::Settings),
    ];
}

impl Dashboard {
    pub(super) fn select_section(&mut self, section: Section) -> Option<Command> {
        if section == Section::Settings {
            return Some(Command::Settings);
        }
        self.navigation = section;
        self.tab = if section == Section::Analytics {
            Tab::Analytics
        } else {
            Tab::Overview
        };
        None
    }

    pub(super) fn sidebar(&mut self, ui: &mut Ui, count: usize, p: Palette) -> Option<Command> {
        let mut command = None;
        if self.logo.is_none() {
            if let Ok(logo) = image::load_from_memory(crate::metadata::LOGO_BYTES) {
                let logo = logo.into_rgba8();
                self.logo = Some(ui.ctx().load_texture(
                    "dashboard-logo",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [logo.width() as usize, logo.height() as usize],
                        &logo,
                    ),
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        let (brand, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
        if let Some(logo) = &self.logo {
            egui::Image::new(logo).paint_at(
                ui,
                Rect::from_min_size(brand.min + vec2(-2.0, 4.0), vec2(40.0, 40.0)),
            );
        }
        text(
            ui,
            brand.min + vec2(46.0, 6.0),
            "DeployerCoaster",
            18.0,
            true,
            p.text,
            brand.width() - 46.0,
        );
        text(
            ui,
            brand.min + vec2(46.0, 32.0),
            "Apps. Web. Infrastructure.",
            11.0,
            false,
            p.muted,
            brand.width() - 46.0,
        );
        let available_height = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("navigation_items")
            .max_height((available_height - 230.0).max(160.0))
            .show(ui, |ui| {
                for &(section, label, icon) in Section::ALL {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
                    let response = control(ui, rect, ("navigation", section), label);
                    if self.navigation == section || response.hovered() || response.has_focus() {
                        ui.painter().rect_filled(
                            rect,
                            6.0,
                            if self.navigation == section {
                                p.selected
                            } else {
                                p.surface
                            },
                        );
                        if response.has_focus() {
                            ui.painter().rect_stroke(
                                rect,
                                6.0,
                                Stroke::new(1.0, p.blue),
                                StrokeKind::Inside,
                            );
                        }
                    }
                    icon.paint(ui, rect.min + vec2(12.0, 11.0), 18.0, p.muted);
                    text(
                        ui,
                        rect.min + vec2(42.0, 12.0),
                        label,
                        13.0,
                        false,
                        p.text,
                        rect.width() - 70.0,
                    );
                    if section == Section::Apps {
                        let center = pos2(rect.right() - 18.0, rect.center().y);
                        ui.painter().circle_filled(center, 12.0, p.border);
                        ui.painter().text(
                            center,
                            Align2::CENTER_CENTER,
                            count.to_string(),
                            FontId::proportional(11.0),
                            p.text,
                        );
                    }
                    if response.clicked() {
                        command = self.select_section(section);
                    }
                }
            });
        ui.add_space(16.0);
        egui::Frame::NONE
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(8)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Quick Actions")
                        .size(11.0)
                        .color(p.muted),
                );
                for (label, icon, action) in [
                    ("Connect Service", Icon::Link, Command::Settings),
                    ("Apple Apps", Icon::Apps, Command::Apple),
                    ("Android Apps", Icon::Play, Command::Android),
                    ("Open Workspace", Icon::Box, Command::OpenWorkspace),
                ] {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                    let response = control(ui, rect, label, label);
                    ui.painter().rect(
                        rect,
                        5.0,
                        if response.hovered() || response.has_focus() {
                            p.selected
                        } else {
                            p.surface
                        },
                        Stroke::new(1.0, p.border),
                        StrokeKind::Inside,
                    );
                    icon.paint(ui, rect.min + vec2(10.0, 9.0), 16.0, p.muted);
                    text(
                        ui,
                        rect.min + vec2(36.0, 10.0),
                        label,
                        11.0,
                        false,
                        p.text,
                        rect.width() - 42.0,
                    );
                    if response.clicked() {
                        command = Some(action);
                    }
                }
            });
        let bottom = ui.max_rect().bottom();
        if bottom - ui.cursor().top() > 76.0 {
            ui.add_space(bottom - ui.cursor().top() - 76.0);
        }
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, p.border),
        );
        Icon::Shield.paint(ui, rect.min + vec2(10.0, 22.0), 22.0, p.blue);
        text(
            ui,
            rect.min + vec2(44.0, 18.0),
            "Local workspace",
            12.0,
            true,
            p.text,
            rect.width() - 44.0,
        );
        text(
            ui,
            rect.min + vec2(44.0, 37.0),
            "Credentials saved on this device",
            10.0,
            false,
            p.muted,
            rect.width() - 44.0,
        );
        command
    }

    pub(super) fn service_page(&self, ui: &mut Ui, p: Palette, command: &mut Option<Command>) {
        let message = match self.navigation {
            Section::Websites => "No websites are connected yet.",
            Section::Deployments => "No deployment sources are connected yet.",
            Section::Team => "Team management isn't available yet.",
            _ => return,
        };
        ui.heading(self.navigation.label());
        ui.add_space(8.0);
        ui.label(egui::RichText::new(message).color(p.muted));
        if self.navigation != Section::Team {
            ui.add_space(12.0);
            if ui.button("View connections").clicked() {
                *command = Some(Command::Settings);
            }
        }
    }
}
