use std::collections::BTreeMap;

use serde_json::Value;

use super::{AppDetails, attr};
use crate::dashboard::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Document {
    Information,
    Listing,
    Media,
    Versions,
    Review,
    Builds,
    TestFlight,
    AppClips,
    BackgroundAssets,
    Pricing,
    Purchases,
    Subscriptions,
    ProductPages,
    Experiments,
    Events,
    Featuring,
    CustomerReviews,
    Privacy,
    AgeRating,
    Accessibility,
    Agreements,
    GameCenter,
    Changes,
}

pub(super) const GROUPS: &[(&str, &[Document])] = &[
    (
        "App Store",
        &[
            Document::Information,
            Document::Listing,
            Document::Media,
            Document::Versions,
            Document::Review,
        ],
    ),
    (
        "Distribution",
        &[
            Document::Builds,
            Document::TestFlight,
            Document::AppClips,
            Document::BackgroundAssets,
        ],
    ),
    (
        "Monetization",
        &[
            Document::Pricing,
            Document::Purchases,
            Document::Subscriptions,
        ],
    ),
    (
        "Growth",
        &[
            Document::ProductPages,
            Document::Experiments,
            Document::Events,
            Document::Featuring,
        ],
    ),
    ("Customers", &[Document::CustomerReviews]),
    (
        "Trust & capabilities",
        &[
            Document::Privacy,
            Document::AgeRating,
            Document::Accessibility,
            Document::Agreements,
            Document::GameCenter,
        ],
    ),
];

impl Document {
    pub(super) fn paint_icon(
        self,
        ui: &egui::Ui,
        origin: egui::Pos2,
        size: f32,
        color: egui::Color32,
    ) {
        #[cfg(target_os = "macos")]
        {
            let symbol = match self {
                Self::Information => "info.circle.fill",
                Self::Listing => "globe",
                Self::Media => "photo.on.rectangle.angled",
                Self::Versions => "square.stack.3d.up.fill",
                Self::Review => "checkmark.seal.fill",
                Self::Builds => "shippingbox.fill",
                Self::TestFlight => "airplane",
                Self::AppClips => "appclip",
                Self::BackgroundAssets => "square.3.layers.3d",
                Self::Pricing => "tag.fill",
                Self::Purchases => "cart.fill",
                Self::Subscriptions => "repeat",
                Self::ProductPages => "rectangle.stack.fill",
                Self::Experiments => "testtube.2",
                Self::Events => "calendar",
                Self::Featuring => "star.fill",
                Self::CustomerReviews => "star.bubble.fill",
                Self::Privacy => "hand.raised.fill",
                Self::AgeRating => "person.crop.square.fill",
                Self::Accessibility => "accessibility",
                Self::Agreements => "doc.text",
                Self::GameCenter => "gamecontroller.fill",
                Self::Changes => "arrow.triangle.branch",
            };
            if crate::macos::paint_symbol(ui, symbol, origin, size, color) {
                return;
            }
        }
        self.icon().paint(ui, origin, size, color);
    }

    pub(super) fn icon_color(self, dark: bool) -> egui::Color32 {
        // Keep navigator symbols vivid at small sizes, like Xcode's file icons.
        // Dark appearances use the brighter macOS accent variants.
        let (light, dark_color) = match self {
            Self::Information | Self::Versions | Self::Accessibility => {
                ([0, 122, 255], [10, 132, 255])
            }
            Self::Listing | Self::ProductPages | Self::TestFlight | Self::AppClips => {
                ([0, 145, 200], [90, 200, 250])
            }
            Self::Privacy => ([0, 151, 167], [64, 200, 224]),
            Self::Media | Self::Experiments | Self::GameCenter => ([175, 82, 222], [191, 90, 242]),
            Self::Pricing | Self::Purchases | Self::Subscriptions | Self::Review => {
                ([40, 160, 70], [48, 209, 88])
            }
            Self::Builds
            | Self::Events
            | Self::CustomerReviews
            | Self::Featuring
            | Self::Changes => ([220, 135, 0], [255, 179, 64]),
            Self::BackgroundAssets | Self::AgeRating | Self::Agreements => {
                ([110, 110, 115], [174, 174, 178])
            }
        };
        let [r, g, b] = if dark { dark_color } else { light };
        egui::Color32::from_rgb(r, g, b)
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Information => "App Information",
            Self::Listing => "Store Listing",
            Self::Media => "Screenshots & Previews",
            Self::Versions => "Versions & Releases",
            Self::Review => "App Review",
            Self::Builds => "Builds",
            Self::TestFlight => "TestFlight",
            Self::AppClips => "App Clips",
            Self::BackgroundAssets => "Background Assets",
            Self::Pricing => "Pricing & Availability",
            Self::Purchases => "In-App Purchases",
            Self::Subscriptions => "Subscriptions",
            Self::ProductPages => "Custom Product Pages",
            Self::Experiments => "Product Page Optimization",
            Self::Events => "In-App Events",
            Self::Featuring => "Featuring",
            Self::CustomerReviews => "Ratings & Reviews",
            Self::Privacy => "Privacy & Content Rights",
            Self::AgeRating => "Age Rating",
            Self::Accessibility => "Accessibility",
            Self::Agreements => "License Agreement",
            Self::GameCenter => "Game Center",
            Self::Changes => "Local Changes",
        }
    }

    pub(super) fn group(self) -> &'static str {
        GROUPS
            .iter()
            .find(|(_, docs)| docs.contains(&self))
            .map_or("Workspace", |(name, _)| name)
    }

    pub(super) fn connected(self) -> bool {
        matches!(
            self,
            Self::Information
                | Self::Listing
                | Self::Media
                | Self::Versions
                | Self::Privacy
                | Self::AgeRating
                | Self::Review
                | Self::Changes
        )
    }

    pub(super) fn icon(self) -> Icon {
        match self {
            Self::Information => Icon::Apps,
            Self::Listing | Self::ProductPages | Self::Experiments => Icon::Globe,
            Self::Privacy | Self::AgeRating | Self::Agreements => Icon::Shield,
            Self::Pricing | Self::Purchases | Self::Subscriptions => Icon::Dollar,
            Self::CustomerReviews | Self::TestFlight | Self::Review => Icon::Users,
            Self::Versions | Self::Builds | Self::BackgroundAssets => Icon::Box,
            Self::Changes => Icon::Deploy,
            _ => Icon::Phone,
        }
    }

    pub(super) fn topics(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Media => &[
                ("Screenshot sets", "Images by localization and device size"),
                ("App previews", "Preview videos and processing status"),
                ("Asset library", "Reusable images, videos, and placements"),
            ],
            Self::Review => &[
                ("Review contact", "Name, email, and phone number"),
                (
                    "Review details",
                    "Review notes, attachments, and sign-in information",
                ),
                ("Submissions", "Submitted items and review status"),
            ],
            Self::Builds => &[
                ("Builds", "Build numbers, processing status, and expiration"),
                ("Build bundles", "Device support, icons, and capabilities"),
                ("Encryption", "Export compliance declarations"),
                ("Uploads", "Upload history and delivery status"),
            ],
            Self::TestFlight => &[
                ("Beta builds", "Builds available for testing"),
                ("Test groups", "Internal and external testing groups"),
                ("Beta review", "Test information and beta review status"),
            ],
            Self::AppClips => &[
                ("App Clips", "Clip configuration and associated builds"),
                (
                    "Default experience",
                    "Localized headers and invocation details",
                ),
                (
                    "Advanced experiences",
                    "Locations, imagery, and experience status",
                ),
            ],
            Self::BackgroundAssets => &[
                ("Asset packs", "Versions and delivery configuration"),
                ("Uploads", "Asset validation and processing status"),
            ],
            Self::Pricing => &[
                ("Availability", "Territories and distribution status"),
                ("Pricing", "Price schedules and territory pricing"),
            ],
            Self::Purchases => &[
                ("Products", "Product IDs, types, and review status"),
                ("Localizations", "Display names, descriptions, and images"),
                (
                    "Pricing & offers",
                    "Price schedules, offer codes, and promotions",
                ),
                ("Review", "Screenshots and submission information"),
            ],
            Self::Subscriptions => &[
                (
                    "Subscription groups",
                    "Group order and localized display names",
                ),
                ("Subscriptions", "Duration, availability, and review status"),
                (
                    "Prices & offers",
                    "Price points, promotional offers, and win-back offers",
                ),
                ("Billing", "Grace period and renewal configuration"),
            ],
            Self::ProductPages => &[
                ("Product pages", "Page names, URLs, and review status"),
                (
                    "Localizations",
                    "Localized metadata, screenshots, and previews",
                ),
                ("Versions", "Draft and published page versions"),
            ],
            Self::Experiments => &[
                (
                    "Experiments",
                    "Test duration, traffic, and experiment status",
                ),
                ("Treatments", "Alternate icons, screenshots, and previews"),
                ("Localizations", "Localized treatment assets"),
            ],
            Self::Events => &[
                ("Events", "Event type, schedule, and territory availability"),
                ("Localizations", "Names, descriptions, and event media"),
            ],
            Self::Featuring => &[("Nominations", "Nomination details and related app content")],
            Self::CustomerReviews => &[
                (
                    "Customer reviews",
                    "Ratings, territories, and written feedback",
                ),
                (
                    "Developer responses",
                    "Response text and publication status",
                ),
            ],
            Self::AgeRating => &[("Age rating", "Content questionnaire and rating overrides")],
            Self::Accessibility => &[(
                "Declarations",
                "Accessibility features by platform and device",
            )],
            Self::Agreements => &[("Custom EULA", "License text and applicable territories")],
            Self::GameCenter => &[(
                "Enabled versions",
                "Game Center availability by app version",
            )],
            _ => &[],
        }
    }

    pub(super) fn reference(self) -> &'static str {
        match self {
            Self::Review => "app-store-review-details",
            Self::AgeRating => "age-ratings",
            Self::Builds => "builds",
            Self::Media => "app-screenshot-sets",
            Self::Purchases => "in-app-purchases",
            Self::Subscriptions => "subscriptions",
            Self::ProductPages => "app-custom-product-pages",
            Self::Experiments => "app-store-version-experiments",
            Self::Events => "app-events",
            Self::Featuring => "featuring-nominations",
            Self::CustomerReviews => "customer-reviews",
            Self::AppClips => "app-clips",
            Self::BackgroundAssets => "background-assets",
            Self::Pricing => "app-availability",
            Self::Accessibility => "accessibility-declarations",
            Self::Agreements => "end-user-license-agreements-eula",
            Self::GameCenter => "game-center-enabled-versions",
            _ => "apps",
        }
    }
}

pub(super) struct Workspace {
    pub active: Document,
    pub tabs: Vec<Document>,
    pub locale: String,
    pub filter: String,
    pub navigator: bool,
    pub inspector: bool,
    pub edits: BTreeMap<(String, String), Edit>,
    pub focused: Option<FieldContext>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            active: Document::Information,
            tabs: vec![Document::Information, Document::Listing],
            locale: String::new(),
            filter: String::new(),
            navigator: true,
            inspector: true,
            edits: BTreeMap::new(),
            focused: None,
        }
    }
}

pub(super) struct FieldContext {
    pub label: String,
    pub scope: &'static str,
    pub characters: usize,
    pub modified: bool,
}

pub(super) struct Edit {
    pub document: Document,
    pub label: String,
    pub locale: String,
    pub scope: String,
    pub original: Option<Value>,
    pub value: String,
}

impl Workspace {
    pub(super) fn open(&mut self, document: Document) {
        if !self.tabs.contains(&document) {
            self.tabs.push(document);
        }
        if self.active != document {
            self.focused = None;
        }
        self.active = document;
    }

    pub(super) fn close(&mut self, document: Document) {
        if self.tabs.len() <= 1 {
            return;
        }
        if let Some(index) = self.tabs.iter().position(|tab| *tab == document) {
            self.tabs.remove(index);
            if self.active == document {
                self.active = self.tabs[index.saturating_sub(1)];
                self.focused = None;
            }
        }
    }

    pub(super) fn reconcile(&mut self, details: &AppDetails) {
        let locales = locales(details);
        if !locales.contains(&self.locale.as_str()) {
            let primary = attr(&details.app, "primaryLocale");
            self.locale = if locales.contains(&primary) {
                primary
            } else {
                locales.first().copied().unwrap_or_default()
            }
            .into();
        }
    }

    pub(super) fn dirty(&self, document: Document) -> bool {
        self.edits.values().any(|edit| edit.document == document)
    }

    pub(super) fn edit(
        &mut self,
        route: &str,
        field: &str,
        source: Option<&Value>,
        value: String,
        label: &str,
        scope: &str,
    ) {
        let key = (route.into(), field.into());
        let original = self
            .edits
            .get(&key)
            .map_or(source, |edit| edit.original.as_ref());
        if original.and_then(Value::as_str).unwrap_or_default() == value {
            self.edits.remove(&key);
        } else {
            let original = original.cloned();
            let locale = if route.contains("/localizations/") {
                self.locale.clone()
            } else {
                String::new()
            };
            self.edits.insert(
                key,
                Edit {
                    document: self.active,
                    label: label.into(),
                    locale,
                    scope: scope.into(),
                    original,
                    value,
                },
            );
        }
    }
}

pub(super) fn locales(details: &AppDetails) -> Vec<&str> {
    let mut locales: Vec<_> = details
        .info_localizations
        .iter()
        .chain(&details.version_localizations)
        .map(|resource| attr(resource, "locale"))
        .filter(|locale| !locale.is_empty())
        .collect();
    locales.sort();
    locales.dedup();
    let primary = attr(&details.app, "primaryLocale");
    locales.sort_by_key(|locale| (*locale != primary, *locale));
    locales
}

pub(super) fn locale_name(locale: &str) -> String {
    let name = match locale {
        "en-US" => "English (U.S.)",
        "en-GB" => "English (U.K.)",
        "en-AU" => "English (Australia)",
        "en-CA" => "English (Canada)",
        "fr-FR" => "French",
        "fr-CA" => "French (Canada)",
        "de-DE" => "German",
        "es-ES" => "Spanish",
        "es-MX" => "Spanish (Mexico)",
        "it" => "Italian",
        "ja" => "Japanese",
        "ko" => "Korean",
        "pt-BR" => "Portuguese (Brazil)",
        "zh-Hans" => "Chinese (Simplified)",
        "zh-Hant" => "Chinese (Traditional)",
        "ar-SA" => "Arabic",
        "nl-NL" => "Dutch",
        "sv" => "Swedish",
        _ => locale,
    };
    name.into()
}

pub(super) fn readable(value: &str) -> String {
    value
        .split('_')
        .map(|word| {
            let word = word.to_lowercase();
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn app_route(details: &AppDetails) -> String {
    format!("/apps/{}", attr(&details.app, "id"))
}
pub(super) fn version_route(details: &AppDetails) -> String {
    format!(
        "{}/versions/{}",
        app_route(details),
        details.version.as_ref().map_or("", |v| attr(v, "id"))
    )
}
pub(super) fn info_route(details: &AppDetails) -> String {
    format!(
        "{}/app-infos/{}",
        app_route(details),
        details.info.as_ref().map_or("", |v| attr(v, "id"))
    )
}

pub(super) fn source_resource<'a>(details: &'a AppDetails, route: &str) -> Option<&'a Value> {
    if route == app_route(details) {
        return Some(&details.app);
    }
    if route == version_route(details) {
        return details.version.as_ref();
    }
    if route == info_route(details) {
        return details.info.as_ref();
    }
    for (base, resources) in [
        (info_route(details), &details.info_localizations),
        (version_route(details), &details.version_localizations),
    ] {
        for resource in resources {
            if route == format!("{base}/localizations/{}", attr(resource, "locale")) {
                return Some(resource);
            }
        }
    }
    None
}
