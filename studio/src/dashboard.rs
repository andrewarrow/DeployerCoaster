mod library;
mod navigation;
mod product;
#[cfg(test)]
mod tests;
mod widgets;

use navigation::{Section, Tab};
use product::metrics;
#[cfg(test)]
use product::services;
use widgets::*;

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

#[derive(Default)]
pub(crate) struct Dashboard {
    search: String,
    filter: Filter,
    selected: Option<String>,
    tab: Tab,
    navigation: Section,
    list_map: bool,
    mobile_detail: bool,
    logo: Option<egui::TextureHandle>,
    github: crate::github::GitHub,
    google_oauth: crate::google_oauth::GoogleOAuth,
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
        dynadot: &mut crate::dynadot::Dynadot,
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
                        if compact && self.mobile_detail && self.navigation.is_product_section() {
                            if ui.button("‹  My Apps").clicked() {
                                self.mobile_detail = false;
                            }
                        } else {
                            egui::ComboBox::from_id_salt("compact_section")
                                .height(480.0)
                                .selected_text(self.navigation.label())
                                .show_ui(ui, |ui| {
                                    for &(section, label, _) in Section::ALL {
                                        if ui
                                            .selectable_label(self.navigation == section, label)
                                            .clicked()
                                        {
                                            command = self.select_section(section);
                                        }
                                    }
                                });
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

        if !compact && !self.navigation.has_dedicated_page() {
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
            .show(ui, |ui| match self.navigation {
                Section::Domains => {
                    if dynadot.domains_ui(ui) {
                        command = Some(Command::DynadotSettings);
                    }
                }
                Section::Hosting => {
                    if dynadot.hosting_ui(ui) {
                        command = Some(Command::DynadotSettings);
                    }
                }
                Section::GitHub => self.github.ui(ui),
                Section::GoogleOAuth => self.google_oauth.ui(ui),
                Section::Analytics => {
                    egui::ScrollArea::vertical()
                        .id_salt("sales_reports")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if apple.sales_ui(ui, None) {
                                command = Some(Command::AppleSettings);
                            }
                        });
                }
                _ if compact && !self.mobile_detail && self.navigation.is_product_section() => {
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
                }
                _ => {
                    egui::ScrollArea::vertical()
                        .id_salt("product_detail")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if !self.navigation.is_product_section() {
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
                                    Tab::Analytics => {
                                        if let Some(listing) = product.listings.iter().find(|listing| listing.store == Store::Apple) {
                                            if apple.sales_ui(ui, Some(&listing.store_id)) {
                                                command = Some(Command::AppleSettings);
                                            }
                                        } else {
                                            ui.heading("Sales reports");
                                            ui.add(egui::Label::new("Sales reports are available for App Store listings. This app has no connected App Store listing.").wrap());
                                        }
                                    }
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
}
