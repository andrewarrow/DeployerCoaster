use egui::{
    Align2, Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, pos2, vec2,
};

use crate::{app_icons::Store, apple::AppleStore, commands::Command, play_store::PlayStore};

#[derive(Clone)]
pub(crate) struct StoreApp {
    pub identifier: String,
    pub name: String,
    pub store_id: String,
    pub store: Store,
}

impl StoreApp {
    fn url(&self) -> String {
        match self.store {
            Store::Apple => format!("https://appstoreconnect.apple.com/apps/{}", self.store_id),
            Store::Play => {
                let mut url =
                    reqwest::Url::parse("https://play.google.com/store/apps/details").unwrap();
                url.query_pairs_mut().append_pair("id", &self.store_id);
                url.to_string()
            }
        }
    }
}

pub(crate) struct StoreStatus {
    pub connected: bool,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
struct Product {
    identifier: String,
    name: String,
    listings: Vec<StoreApp>,
}

fn products(listings: Vec<StoreApp>) -> Vec<Product> {
    let mut products: Vec<Product> = Vec::new();
    for listing in listings {
        if let Some(product) = products
            .iter_mut()
            .find(|p| p.identifier == listing.identifier)
        {
            if product.name == product.identifier && listing.name != listing.identifier {
                product.name = listing.name.clone();
            }
            product.listings.push(listing);
        } else {
            products.push(Product {
                identifier: listing.identifier.clone(),
                name: listing.name.clone(),
                listings: vec![listing],
            });
        }
    }
    products.sort_by_cached_key(|p| (p.name.to_lowercase(), p.identifier.clone()));
    products
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Apple,
    Android,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
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
    const ALL: [(Self, &'static str, Icon); 7] = [
        (Self::Overview, "Overview", Icon::Apps),
        (Self::Releases, "Releases", Icon::Deploy),
        (Self::Analytics, "Analytics", Icon::Chart),
        (Self::Monetization, "Monetization", Icon::Dollar),
        (Self::Users, "Users", Icon::Users),
        (Self::Infrastructure, "Infrastructure", Icon::Server),
        (Self::Settings, "Settings", Icon::Settings),
    ];
}

#[derive(Default)]
pub(crate) struct Dashboard {
    search: String,
    filter: Filter,
    selected: Option<String>,
    tab: Tab,
    navigation: usize,
    list_map: bool,
    mobile_detail: bool,
    logo: Option<egui::TextureHandle>,
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    sidebar: Color32,
    surface: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    selected: Color32,
    blue: Color32,
    green: Color32,
}

impl Palette {
    fn new(ui: &Ui) -> Self {
        if ui.visuals().dark_mode {
            Self {
                bg: Color32::from_rgb(23, 25, 28),
                sidebar: Color32::from_rgb(29, 32, 36),
                surface: Color32::from_rgb(29, 32, 37),
                border: Color32::from_rgb(47, 52, 59),
                text: Color32::from_rgb(239, 241, 245),
                muted: Color32::from_rgb(160, 168, 181),
                selected: Color32::from_rgb(35, 49, 67),
                blue: Color32::from_rgb(55, 148, 255),
                green: Color32::from_rgb(75, 224, 146),
            }
        } else {
            Self {
                bg: Color32::from_rgb(248, 249, 251),
                sidebar: Color32::from_rgb(237, 240, 245),
                surface: Color32::WHITE,
                border: Color32::from_rgb(213, 219, 229),
                text: Color32::from_rgb(30, 36, 46),
                muted: Color32::from_rgb(89, 100, 119),
                selected: Color32::from_rgb(221, 234, 252),
                blue: Color32::from_rgb(25, 100, 205),
                green: Color32::from_rgb(22, 126, 70),
            }
        }
    }
}

impl Dashboard {
    pub(crate) fn ui(
        &mut self,
        ui: &mut Ui,
        play: &mut PlayStore,
        apple: &mut AppleStore,
        show_sidebar: bool,
    ) -> Option<Command> {
        let palette = Palette::new(ui);
        let apps = products(
            apple
                .dashboard_apps()
                .into_iter()
                .chain(play.dashboard_apps())
                .collect(),
        );
        let apple_status = apple.dashboard_status();
        let play_status = play.dashboard_status();
        let filtered: Vec<_> = apps.iter().filter(|p| self.matches(p)).collect();
        if self
            .selected
            .as_ref()
            .is_none_or(|key| !filtered.iter().any(|p| &p.identifier == key))
        {
            self.selected = filtered.first().map(|p| p.identifier.clone());
        }
        let selected = filtered
            .iter()
            .find(|p| Some(&p.identifier) == self.selected.as_ref())
            .copied();
        let width = ui.available_width();
        let compact = width < 720.0;
        let mut command = None;

        if show_sidebar && width >= 1100.0 {
            egui::Panel::left("workspace_navigation")
                .exact_size(212.0)
                .resizable(false)
                .frame(
                    egui::Frame::NONE
                        .fill(palette.sidebar)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ui, |ui| {
                    command = self.sidebar(ui, apps.len(), palette);
                });
        } else {
            egui::Panel::top("compact_navigation")
                .frame(
                    egui::Frame::NONE
                        .fill(palette.sidebar)
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if compact && self.mobile_detail {
                            if ui.button("‹  My Apps").clicked() {
                                self.mobile_detail = false;
                            }
                        } else {
                            ui.label(egui::RichText::new("DeployerCoaster").strong());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_sized([44.0, 36.0], egui::Button::new("⚙"))
                                .on_hover_text("Settings")
                                .clicked()
                            {
                                command = Some(Command::Settings);
                            }
                        });
                    });
                });
        }

        if !compact {
            egui::Panel::left("app_library")
                .exact_size(if width < 1100.0 { 264.0 } else { 304.0 })
                .resizable(false)
                .frame(
                    egui::Frame::NONE
                        .fill(palette.bg)
                        .inner_margin(egui::Margin::same(16)),
                )
                .show(ui, |ui| {
                    if self.app_list(
                        ui,
                        &apps,
                        &filtered,
                        &apple_status,
                        &play_status,
                        play,
                        apple,
                        palette,
                    ) {
                        command = Some(Command::Settings);
                    }
                });
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(palette.bg)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| {
                if compact && !self.mobile_detail {
                    if self.app_list(
                        ui,
                        &apps,
                        &filtered,
                        &apple_status,
                        &play_status,
                        play,
                        apple,
                        palette,
                    ) {
                        command = Some(Command::Settings);
                    }
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("product_detail")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.navigation > 1 && self.navigation != 6 && self.navigation != 8 {
                                self.service_page(ui, palette, &mut command);
                            } else if let Some(product) = selected {
                                self.product_header(
                                    ui,
                                    product,
                                    play,
                                    apple,
                                    palette,
                                    &mut command,
                                );
                                ui.add_space(16.0);
                                self.tabs(ui, palette, &mut command);
                                ui.add_space(12.0);
                                match self.tab {
                                    Tab::Overview => {
                                        self.ecosystem(
                                            ui,
                                            product,
                                            play,
                                            apple,
                                            palette,
                                            &mut command,
                                        );
                                        ui.add_space(10.0);
                                        metrics(ui, palette);
                                        ui.add_space(10.0);
                                        self.details(ui, product, palette);
                                    }
                                    Tab::Infrastructure => self.ecosystem(
                                        ui,
                                        product,
                                        play,
                                        apple,
                                        palette,
                                        &mut command,
                                    ),
                                    _ => self.unavailable_tab(ui, palette),
                                }
                            } else {
                                ui.heading("Overview");
                                ui.add_space(12.0);
                                ui.label(if apple_status.loading || play_status.loading {
                                    "Loading your apps…"
                                } else if !self.search.is_empty() {
                                    "No apps match your search."
                                } else {
                                    "Your apps will appear here when a store is connected."
                                });
                                ui.add_space(12.0);
                                if ui
                                    .add_sized([160.0, 44.0], egui::Button::new("Connect a store"))
                                    .clicked()
                                {
                                    command = Some(Command::Settings);
                                }
                            }
                        });
                }
            });
        command
    }

    fn matches(&self, product: &Product) -> bool {
        let query = self.search.trim().to_lowercase();
        (product.name.to_lowercase().contains(&query)
            || product.identifier.to_lowercase().contains(&query))
            && match self.filter {
                Filter::All => true,
                Filter::Apple => product.listings.iter().any(|l| l.store == Store::Apple),
                Filter::Android => product.listings.iter().any(|l| l.store == Store::Play),
            }
    }

    fn sidebar(&mut self, ui: &mut Ui, count: usize, p: Palette) -> Option<Command> {
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
        let entries = [
            ("Overview", Icon::Home),
            ("My Apps", Icon::Apps),
            ("Websites", Icon::Globe),
            ("Domains", Icon::Domain),
            ("Hosting", Icon::Server),
            ("Certificates", Icon::Shield),
            ("Analytics", Icon::Chart),
            ("Deployments", Icon::Deploy),
            ("Monetization", Icon::Dollar),
            ("Team", Icon::Users),
            ("Settings", Icon::Settings),
        ];
        let available_height = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("navigation_items")
            .max_height((available_height - 230.0).max(160.0))
            .show(ui, |ui| {
                for (index, (label, icon)) in entries.iter().enumerate() {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
                    let response = control(ui, rect, ("navigation", index), label);
                    if self.navigation == index || response.hovered() || response.has_focus() {
                        ui.painter().rect_filled(
                            rect,
                            6.0,
                            if self.navigation == index {
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
                    if index == 1 {
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
                        if index == 10 {
                            command = Some(Command::Settings);
                        } else {
                            self.navigation = index;
                            self.tab = match index {
                                6 => Tab::Analytics,
                                8 => Tab::Monetization,
                                _ => Tab::Overview,
                            };
                        }
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

    #[allow(clippy::too_many_arguments)]
    fn app_list(
        &mut self,
        ui: &mut Ui,
        all: &[Product],
        filtered: &[&Product],
        apple_status: &StoreStatus,
        play_status: &StoreStatus,
        play: &mut PlayStore,
        apple: &mut AppleStore,
        p: Palette,
    ) -> bool {
        let mut settings = false;
        ui.horizontal(|ui| {
            ui.heading("My Apps");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                let response =
                    control(ui, rect, "refresh_apps", "Refresh apps").on_hover_text("Refresh apps");
                Icon::Refresh.paint(
                    ui,
                    rect.min + vec2(8.0, 8.0),
                    16.0,
                    if response.hovered() || response.has_focus() {
                        p.blue
                    } else {
                        p.muted
                    },
                );
                if response.clicked() {
                    play.refresh_dashboard(ui.ctx());
                    apple.refresh_dashboard(ui.ctx());
                }
            });
        });
        ui.add_space(8.0);
        ui.add_sized(
            [ui.available_width(), 36.0],
            egui::TextEdit::singleline(&mut self.search).hint_text("Search apps or identifiers…"),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (filter, label, count) in [
                (Filter::All, "All", all.len()),
                (
                    Filter::Apple,
                    "iOS",
                    all.iter()
                        .filter(|p| p.listings.iter().any(|l| l.store == Store::Apple))
                        .count(),
                ),
                (
                    Filter::Android,
                    "Android",
                    all.iter()
                        .filter(|p| p.listings.iter().any(|l| l.store == Store::Play))
                        .count(),
                ),
            ] {
                if ui
                    .add(
                        egui::Button::selectable(
                            self.filter == filter,
                            format!("{label} ({count})"),
                        )
                        .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    self.filter = filter;
                }
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .id_salt("app_rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for product in filtered {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 82.0), Sense::hover());
                    let response = control(ui, rect, ("app", &product.identifier), &product.name)
                        .on_hover_text(&product.identifier);
                    let selected = self.selected.as_deref() == Some(&product.identifier);
                    if selected || response.hovered() || response.has_focus() {
                        ui.painter().rect(
                            rect,
                            8.0,
                            if selected { p.selected } else { p.surface },
                            Stroke::new(
                                1.0,
                                if selected || response.has_focus() {
                                    p.blue
                                } else {
                                    p.border
                                },
                            ),
                            StrokeKind::Inside,
                        );
                    } else {
                        ui.painter().line_segment(
                            [rect.left_bottom(), rect.right_bottom()],
                            Stroke::new(1.0, p.border),
                        );
                    }
                    paint_product(
                        ui,
                        Rect::from_min_size(rect.min + vec2(10.0, 14.0), vec2(48.0, 48.0)),
                        product,
                        play,
                        apple,
                    );
                    let left = rect.left() + 72.0;
                    text(
                        ui,
                        pos2(left, rect.top() + 12.0),
                        &product.name,
                        14.0,
                        true,
                        p.text,
                        rect.width() - 98.0,
                    );
                    text(
                        ui,
                        pos2(left, rect.top() + 34.0),
                        &product.identifier,
                        11.0,
                        false,
                        p.muted,
                        rect.width() - 88.0,
                    );
                    let mut x = left;
                    for listing in &product.listings {
                        let (icon, label) = match listing.store {
                            Store::Apple => (Icon::Apple, "iOS"),
                            Store::Play => (Icon::Play, "Android"),
                        };
                        icon.paint(ui, pos2(x, rect.top() + 56.0), 12.0, p.muted);
                        text(
                            ui,
                            pos2(x + 16.0, rect.top() + 56.0),
                            label,
                            10.0,
                            false,
                            p.muted,
                            46.0,
                        );
                        x += if listing.store == Store::Apple {
                            48.0
                        } else {
                            70.0
                        };
                    }
                    Icon::Chevron.paint(
                        ui,
                        pos2(rect.right() - 22.0, rect.center().y - 8.0),
                        16.0,
                        p.muted,
                    );
                    if response.clicked() {
                        self.selected = Some(product.identifier.clone());
                        self.mobile_detail = true;
                        self.navigation = 0;
                        self.tab = Tab::Overview;
                    }
                }
                if filtered.is_empty() {
                    if apple_status.loading || play_status.loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("Loading apps…");
                        });
                    } else if !self.search.is_empty() || self.filter != Filter::All {
                        ui.weak("No apps match your filters.");
                        if ui.button("Clear filters").clicked() {
                            self.search.clear();
                            self.filter = Filter::All;
                        }
                    } else {
                        ui.weak("You haven't connected any apps yet.");
                    }
                }
                for (label, status) in [("Apple", apple_status), ("Google Play", play_status)] {
                    if let Some(error) = &status.error {
                        ui.add_space(8.0);
                        ui.colored_label(ui.visuals().error_fg_color, format!("{label}: {error}"));
                        settings |= ui.button(format!("{label} settings…")).clicked();
                    } else if status.loading && !filtered.is_empty() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak(format!("Loading {label}…"));
                        });
                    }
                }
                if !apple_status.connected || !play_status.connected {
                    ui.add_space(12.0);
                    settings |= ui
                        .add_sized(
                            [ui.available_width(), 40.0],
                            egui::Button::new("Connect a store"),
                        )
                        .clicked();
                }
            });
        settings
    }

    fn product_header(
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

    fn tabs(&mut self, ui: &mut Ui, p: Palette, command: &mut Option<Command>) {
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

    fn ecosystem(
        &mut self,
        ui: &mut Ui,
        product: &Product,
        play: &mut PlayStore,
        apple: &mut AppleStore,
        p: Palette,
        command: &mut Option<Command>,
    ) {
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
                let services = services(product, p);
                if self.list_map || ui.available_width() < 560.0 {
                    for (i, service) in services.iter().enumerate() {
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 92.0), Sense::hover());
                        if service_card(ui, rect, service, p, i) {
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
                        ui.painter().circle_filled(b, 3.5, color);
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
                        if service_card(ui, rect, &services[i], p, i) {
                            service_action(&services[i], product, ui, command);
                        }
                    }
                }
            });
    }

    fn details(&self, ui: &mut Ui, product: &Product, p: Palette) {
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

    fn unavailable_tab(&self, ui: &mut Ui, p: Palette) {
        let (title, message) = match self.tab {
            Tab::Releases => (
                "Releases",
                "Release history isn't available from the connected app discovery services.",
            ),
            Tab::Analytics => (
                "Analytics",
                "No analytics service is connected to this app.",
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

    fn service_page(&self, ui: &mut Ui, p: Palette, command: &mut Option<Command>) {
        let (title, message) = match self.navigation {
            2 => ("Websites", "No websites are connected yet."),
            3 => ("Domains", "No domains are connected yet."),
            4 => ("Hosting", "No hosting providers are connected yet."),
            5 => ("Certificates", "No certificates are connected yet."),
            7 => ("Deployments", "No deployment sources are connected yet."),
            _ => ("Team", "Team management isn't available yet."),
        };
        ui.heading(title);
        ui.add_space(8.0);
        ui.label(egui::RichText::new(message).color(p.muted));
        if self.navigation != 9 {
            ui.add_space(12.0);
            if ui.button("View connections").clicked() {
                *command = Some(Command::Settings);
            }
        }
    }
}

struct Service {
    title: &'static str,
    icon: Icon,
    color: Color32,
    subtitle: String,
    detail: String,
    connected: bool,
    store: Option<Store>,
}

fn services(product: &Product, p: Palette) -> Vec<Service> {
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
        ("Certificates", Icon::Lock, Color32::from_rgb(50, 191, 187)),
        (
            "Analytics & Monetization",
            Icon::Chart,
            Color32::from_rgb(174, 123, 238),
        ),
    ] {
        services.push(Service {
            title,
            icon,
            color,
            subtitle: "No service linked".into(),
            detail: String::new(),
            connected: false,
            store: None,
        });
    }
    services
}

fn service_card(ui: &Ui, rect: Rect, service: &Service, p: Palette, index: usize) -> bool {
    let response = control(ui, rect, ("ecosystem_service", index), service.title);
    // Only configured store connections can currently be edited.
    let actionable = service.store.is_some();
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
            .on_hover_text(if service.connected {
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

fn metrics(ui: &mut Ui, p: Palette) {
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

fn paint_product(
    ui: &Ui,
    rect: Rect,
    product: &Product,
    play: &mut PlayStore,
    apple: &mut AppleStore,
) {
    if let Some(listing) = product.listings.first() {
        match listing.store {
            Store::Apple => {
                apple.paint_dashboard_icon(ui, rect, &listing.identifier, &product.name)
            }
            Store::Play => play.paint_dashboard_icon(ui, rect, &listing.identifier, &product.name),
        }
    }
}

fn control(ui: &Ui, rect: Rect, id: impl egui::AsIdSalt, label: &str) -> Response {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response
}

fn text(ui: &Ui, pos: Pos2, value: &str, size: f32, strong: bool, color: Color32, width: f32) {
    if width <= 0.0 {
        return;
    }
    let font = FontId::new(
        size,
        if strong {
            FontFamily::Name("semibold".into())
        } else {
            FontFamily::Proportional
        },
    );
    let mut job = egui::text::LayoutJob::simple(value.to_owned(), font, color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(pos, galley, color);
}

#[derive(Clone, Copy)]
enum Icon {
    Home,
    Apps,
    Globe,
    Domain,
    Server,
    Shield,
    Chart,
    Deploy,
    Dollar,
    Users,
    Settings,
    Link,
    Box,
    Refresh,
    External,
    Chevron,
    Phone,
    Apple,
    Play,
    Lock,
    Activity,
}

impl Icon {
    fn paint(self, ui: &Ui, origin: Pos2, size: f32, color: Color32) {
        let p = ui.painter();
        let point = |x: f32, y: f32| origin + vec2(x, y) * (size / 24.0);
        let stroke = Stroke::new((size / 14.0).max(1.1), color);
        let line = |coords: &[(f32, f32)]| {
            p.add(Shape::line(
                coords.iter().map(|&(x, y)| point(x, y)).collect(),
                stroke,
            ));
        };
        let rect = |x: f32, y: f32, w: f32, h: f32, radius: f32| {
            p.rect_stroke(
                Rect::from_min_size(point(x, y), vec2(w, h) * size / 24.0),
                radius,
                stroke,
                StrokeKind::Middle,
            );
        };
        match self {
            Self::Home => {
                line(&[(2., 11.), (12., 3.), (22., 11.)]);
                line(&[
                    (5., 9.),
                    (5., 21.),
                    (10., 21.),
                    (10., 14.),
                    (14., 14.),
                    (14., 21.),
                    (19., 21.),
                    (19., 9.),
                ]);
            }
            Self::Apps => {
                for (x, y) in [(3., 3.), (14., 3.), (3., 14.), (14., 14.)] {
                    rect(x, y, 7., 7., 1.5);
                }
            }
            Self::Globe | Self::Domain => {
                p.circle_stroke(point(12., 12.), size * 0.4, stroke);
                line(&[(3., 12.), (21., 12.)]);
                p.add(Shape::ellipse_stroke(
                    point(12., 12.),
                    vec2(size * 0.18, size * 0.4),
                    stroke,
                ));
            }
            Self::Server => {
                rect(3., 3., 18., 7., 1.5);
                rect(3., 14., 18., 7., 1.5);
                for y in [6.5, 17.5] {
                    p.circle_filled(point(7., y), size / 24., color);
                    line(&[(12., y), (17., y)]);
                }
            }
            Self::Shield => {
                line(&[
                    (12., 2.),
                    (21., 6.),
                    (20., 15.),
                    (17., 19.),
                    (12., 22.),
                    (7., 19.),
                    (4., 15.),
                    (3., 6.),
                    (12., 2.),
                ]);
                line(&[(8., 12.), (11., 15.), (16., 9.)]);
            }
            Self::Chart => {
                line(&[(3., 3.), (3., 21.), (22., 21.)]);
                line(&[(7., 15.), (11., 10.), (15., 13.), (21., 5.)]);
            }
            Self::Deploy => {
                rect(3., 5., 7., 6., 1.);
                rect(14., 14., 7., 6., 1.);
                line(&[(10., 8.), (17., 8.), (17., 14.)]);
                line(&[(14., 11.), (17., 14.), (20., 11.)]);
            }
            Self::Dollar => {
                line(&[
                    (17., 5.),
                    (8., 5.),
                    (5., 8.),
                    (8., 11.),
                    (16., 13.),
                    (19., 16.),
                    (16., 19.),
                    (6., 19.),
                ]);
                line(&[(12., 2.), (12., 22.)]);
            }
            Self::Users => {
                p.circle_stroke(point(10., 7.), size * 0.16, stroke);
                line(&[
                    (3., 21.),
                    (3., 17.),
                    (6., 14.),
                    (14., 14.),
                    (17., 17.),
                    (17., 21.),
                ]);
                line(&[(17., 3.), (20., 5.), (20., 9.), (18., 11.)]);
                line(&[(20., 15.), (23., 18.), (23., 21.)]);
            }
            Self::Settings => {
                p.circle_stroke(point(12., 12.), size * 0.17, stroke);
                let pts = (0..24)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 24.;
                        let r = if i % 4 < 2 { 10. } else { 8. };
                        point(12. + angle.cos() * r, 12. + angle.sin() * r)
                    })
                    .collect();
                p.add(Shape::closed_line(pts, stroke));
            }
            Self::Link => {
                line(&[
                    (10., 7.),
                    (14., 3.),
                    (18., 3.),
                    (21., 6.),
                    (21., 10.),
                    (17., 14.),
                ]);
                line(&[
                    (14., 17.),
                    (10., 21.),
                    (6., 21.),
                    (3., 18.),
                    (3., 14.),
                    (7., 10.),
                ]);
                line(&[(8., 16.), (16., 8.)]);
            }
            Self::Box => {
                line(&[
                    (3., 7.),
                    (12., 2.),
                    (21., 7.),
                    (21., 17.),
                    (12., 22.),
                    (3., 17.),
                    (3., 7.),
                    (12., 12.),
                    (21., 7.),
                ]);
                line(&[(12., 12.), (12., 22.)]);
            }
            Self::Refresh => {
                p.circle_stroke(point(12., 12.), size * 0.35, stroke);
                line(&[(20., 3.), (20., 9.), (14., 9.)]);
            }
            Self::External => {
                line(&[(13., 3.), (21., 3.), (21., 11.)]);
                line(&[(21., 3.), (11., 13.)]);
                line(&[(9., 4.), (4., 4.), (4., 20.), (20., 20.), (20., 15.)]);
            }
            Self::Chevron => line(&[(9., 5.), (15., 12.), (9., 19.)]),
            Self::Phone => {
                rect(5., 2., 14., 20., 2.);
                line(&[(10., 18.), (14., 18.)]);
            }
            Self::Apple => {
                line(&[(5., 19.), (18., 19.)]);
                line(&[(5., 16.), (13., 3.)]);
                line(&[(11., 3.), (19., 16.)]);
                line(&[(4., 14.), (20., 14.)]);
            }
            Self::Play => {
                p.add(Shape::convex_polygon(
                    vec![point(5., 2.), point(22., 12.), point(5., 22.)],
                    color,
                    Stroke::NONE,
                ));
            }
            Self::Lock => {
                rect(4., 10., 16., 12., 2.);
                line(&[
                    (8., 10.),
                    (8., 5.),
                    (10., 2.),
                    (14., 2.),
                    (16., 5.),
                    (16., 10.),
                ]);
            }
            Self::Activity => line(&[
                (1., 12.),
                (7., 12.),
                (10., 3.),
                (14., 21.),
                (17., 12.),
                (23., 12.),
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
