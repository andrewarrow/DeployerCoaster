use egui::{Align, Button, Label, Layout, RichText, TextEdit, Ui, vec2};
use serde_json::Value;

use super::documents::{
    FieldContext, app_route, info_route, locale_name, readable, source_resource, version_route,
};
use super::{Action, AppDetails, Document, Palette, Workspace, attr};
use crate::apple::details::state;
use crate::style::semibold;

pub(super) fn document(
    ui: &mut Ui,
    workspace: &mut Workspace,
    details: &AppDetails,
    palette: Palette,
    loading: bool,
    action: &mut Option<Action>,
) {
    // Xcode's inspectors and forms use a smaller type size than its navigator.
    for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
        ui.style_mut()
            .text_styles
            .insert(text_style, egui::FontId::proportional(12.0));
    }
    if ui.ctx().content_rect().width() < 700.0 {
        super::chrome::version_picker(ui, Some(details), loading, action);
        ui.add_space(12.0);
    }
    match workspace.active {
        Document::Information => information(ui, workspace, details, palette),
        Document::Listing => listing(ui, workspace, details, palette),
        Document::Privacy => privacy(ui, workspace, details, palette),
        Document::AgeRating => age_rating(ui, details, palette),
        Document::Versions => versions(ui, details, palette, loading, action),
        Document::Changes => changes(ui, workspace, details, palette),
        document => planned(ui, document, palette),
    }
}

fn age_rating(ui: &mut Ui, details: &AppDetails, palette: Palette) {
    let Some(rating) = details.age_rating.as_ref() else {
        if details
            .warnings
            .iter()
            .any(|warning| warning.starts_with("App information:"))
        {
            return;
        }
        unavailable(
            ui,
            details,
            "Age rating:",
            "No age rating declaration is available.",
            palette,
        );
        return;
    };

    section(ui, "Rating Overrides", palette);
    for (label, field) in [
        ("Age rating override", "ageRatingOverrideV2"),
        ("Korea age rating override", "koreaAgeRatingOverride"),
        ("Kids age band", "kidsAgeBand"),
        (
            "GRAC classification number",
            "gracRatingClassificationNumber",
        ),
        ("Developer age rating info URL", "developerAgeRatingInfoUrl"),
    ] {
        read_age_row(ui, rating, label, field, palette);
    }

    section_break(ui);
    section(ui, "Features", palette);
    for (label, field) in [
        ("Advertising", "advertising"),
        ("Contests", "contests"),
        ("Gambling", "gambling"),
        ("Simulated gambling", "gamblingSimulated"),
        ("Loot boxes", "lootBox"),
        ("Messaging and chat", "messagingAndChat"),
        ("Parental controls", "parentalControls"),
        ("Social media", "socialMedia"),
        ("Social media age restricted", "socialMediaAgeRestricted"),
        ("Unrestricted web access", "unrestrictedWebAccess"),
        ("User generated content", "userGeneratedContent"),
        ("Age assurance", "ageAssurance"),
    ] {
        read_age_row(ui, rating, label, field, palette);
    }

    section_break(ui);
    section(ui, "Content", palette);
    for (label, field) in [
        (
            "Alcohol, tobacco, or drug use or references",
            "alcoholTobaccoOrDrugUseOrReferences",
        ),
        ("Weapons", "gunsOrOtherWeapons"),
        ("Health or wellness topics", "healthOrWellnessTopics"),
        (
            "Medical or treatment information",
            "medicalOrTreatmentInformation",
        ),
        ("Profanity or crude humor", "profanityOrCrudeHumor"),
        (
            "Graphic sexual content or nudity",
            "sexualContentGraphicAndNudity",
        ),
        ("Sexual content or nudity", "sexualContentOrNudity"),
        ("Horror or fear themes", "horrorOrFearThemes"),
        ("Mature or suggestive themes", "matureOrSuggestiveThemes"),
        ("Cartoon or fantasy violence", "violenceCartoonOrFantasy"),
        (
            "Realistic, prolonged, graphic, or sadistic violence",
            "violenceRealisticProlongedGraphicOrSadistic",
        ),
        ("Realistic violence", "violenceRealistic"),
    ] {
        read_age_row(ui, rating, label, field, palette);
    }
}

fn read_age_row(ui: &mut Ui, rating: &Value, label: &str, field: &str, palette: Palette) {
    let value = match rating.get(field) {
        None | Some(Value::Null) => String::new(),
        Some(Value::Bool(value)) => if *value { "Yes" } else { "No" }.into(),
        Some(Value::String(value))
            if !matches!(
                field,
                "gracRatingClassificationNumber" | "developerAgeRatingInfoUrl"
            ) =>
        {
            age_rating_value(value)
        }
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
    };
    read_row(ui, label, &value, palette);
}

fn age_rating_value(value: &str) -> String {
    match value {
        "NONE" => "None".into(),
        "ALL" => "All ages".into(),
        "NINE_PLUS" => "9+".into(),
        "THIRTEEN_PLUS" => "13+".into(),
        "SIXTEEN_PLUS" => "16+".into(),
        "EIGHTEEN_PLUS" => "18+".into(),
        "TWELVE_PLUS" => "12+".into(),
        "FIFTEEN_PLUS" => "15+".into(),
        "NINETEEN_PLUS" => "19+".into(),
        "FIVE_AND_UNDER" => "5 and under".into(),
        "SIX_TO_EIGHT" => "6–8".into(),
        "NINE_TO_ELEVEN" => "9–11".into(),
        "INFREQUENT_OR_MILD" => "Infrequent or mild".into(),
        "FREQUENT_OR_INTENSE" => "Frequent or intense".into(),
        "INFREQUENT" => "Infrequent".into(),
        "FREQUENT" => "Frequent".into(),
        "UNRATED" => "Unrated".into(),
        _ => value.into(),
    }
}

fn information(ui: &mut Ui, workspace: &mut Workspace, details: &AppDetails, palette: Palette) {
    section(ui, "Identity", palette);
    read_row(ui, "Name", attr(&details.app, "name"), palette);
    read_row(ui, "Bundle ID", attr(&details.app, "bundleId"), palette);
    read_row(ui, "SKU", attr(&details.app, "sku"), palette);
    read_row(ui, "Apple ID", attr(&details.app, "id"), palette);
    read_row(
        ui,
        "Primary language",
        &locale_name(attr(&details.app, "primaryLocale")),
        palette,
    );
    section_break(ui);
    section(ui, "App Store Categories", palette);
    if let Some(info) = &details.info {
        read_row(
            ui,
            "Primary category",
            &readable(attr(info, "primaryCategoryId")),
            palette,
        );
        read_row(
            ui,
            "Secondary category",
            &readable(attr(info, "secondaryCategoryId")),
            palette,
        );
        read_row(ui, "Metadata status", &readable(state(info)), palette);
    } else {
        unavailable(
            ui,
            details,
            "App information:",
            "No app information is available.",
            palette,
        );
    }
    section_break(ui);
    section(ui, "Selected Version", palette);
    if let Some(version) = &details.version {
        read_row(ui, "Status", &readable(state(version)), palette);
        let route = version_route(details);
        let scope = format!("iOS {}", attr(version, "versionString"));
        edit_field(
            ui,
            workspace,
            version,
            &route,
            Field::line("Version", "versionString"),
            &scope,
            palette,
        );
        edit_field(
            ui,
            workspace,
            version,
            &route,
            Field::line("Copyright", "copyright"),
            &scope,
            palette,
        );
        read_row(ui, "Created", attr(version, "createdDate"), palette);
        field_note(ui, palette);
    } else {
        unavailable(
            ui,
            details,
            "iOS version:",
            "No iOS App Store version is available.",
            palette,
        );
    }
}

fn listing(ui: &mut Ui, workspace: &mut Workspace, details: &AppDetails, palette: Palette) {
    let info = details
        .info_localizations
        .iter()
        .find(|value| attr(value, "locale") == workspace.locale);
    let version = details
        .version_localizations
        .iter()
        .find(|value| attr(value, "locale") == workspace.locale);
    section(ui, "App Details", palette);
    if let Some(info) = info {
        read_row(ui, "App name", attr(info, "name"), palette);
        read_row(ui, "Subtitle", attr(info, "subtitle"), palette);
    } else {
        unavailable(
            ui,
            details,
            "App localizations:",
            "No app details exist for this localization.",
            palette,
        );
    }
    section_break(ui);
    section(ui, "Version Details", palette);
    if let Some(version) = version {
        let route = format!(
            "{}/localizations/{}",
            version_route(details),
            workspace.locale
        );
        let scope = format!(
            "iOS {}",
            details
                .version
                .as_ref()
                .map_or("", |version| attr(version, "versionString"))
        );
        for field in [
            Field::prose("Promotional text", "promotionalText", 2),
            Field::prose("Description", "description", 9),
            Field::prose("What's new", "whatsNew", 4),
            Field::line("Keywords", "keywords"),
            Field::line("Support URL", "supportUrl"),
            Field::line("Marketing URL", "marketingUrl"),
        ] {
            edit_field(ui, workspace, version, &route, field, &scope, palette);
        }
        field_note(ui, palette);
    } else if details.version.is_none() {
        unavailable(
            ui,
            details,
            "iOS version:",
            "No iOS App Store version is available.",
            palette,
        );
    } else {
        unavailable(
            ui,
            details,
            "Version localizations:",
            "No version details exist for this localization.",
            palette,
        );
    }
}

fn privacy(ui: &mut Ui, workspace: &mut Workspace, details: &AppDetails, palette: Palette) {
    section(ui, "Privacy Policy", palette);
    if let Some(info) = details
        .info_localizations
        .iter()
        .find(|value| attr(value, "locale") == workspace.locale)
    {
        let route = format!("{}/localizations/{}", info_route(details), workspace.locale);
        edit_field(
            ui,
            workspace,
            info,
            &route,
            Field::line("Privacy policy URL", "privacyPolicyUrl"),
            "App information",
            palette,
        );
        read_row(
            ui,
            "Privacy choices URL",
            attr(info, "privacyChoicesUrl"),
            palette,
        );
    } else {
        unavailable(
            ui,
            details,
            "App localizations:",
            "No privacy policy exists for this localization.",
            palette,
        );
    }
    section_break(ui);
    section(ui, "Content Rights", palette);
    let route = app_route(details);
    let field = "contentRightsDeclaration";
    let key = (route.clone(), field.into());
    let mut value = workspace.edits.get(&key).map_or_else(
        || attr(&details.app, field).to_owned(),
        |edit| edit.value.clone(),
    );
    let before = value.clone();
    form_row(ui, "Third-party content", palette, 3.0, |ui, label_id| {
        egui::ComboBox::from_id_salt("content_rights")
            .width(ui.available_width().min(300.0))
            .selected_text(if value.is_empty() {
                "Not set".into()
            } else {
                readable(&value)
            })
            .show_ui(ui, |ui| {
                for (id, label) in [
                    (
                        "DOES_NOT_USE_THIRD_PARTY_CONTENT",
                        "Does not use third-party content",
                    ),
                    ("USES_THIRD_PARTY_CONTENT", "Uses third-party content"),
                ] {
                    ui.selectable_value(&mut value, id.into(), label);
                }
            })
            .response
            .labelled_by(label_id)
    });
    if before != value {
        workspace.edit(
            &route,
            field,
            details.app.get(field),
            value,
            "Content rights",
            "App",
        );
    }
    field_note(ui, palette);
}

fn versions(
    ui: &mut Ui,
    details: &AppDetails,
    palette: Palette,
    loading: bool,
    action: &mut Option<Action>,
) {
    section(ui, "iOS Releases", palette);
    if details.versions.is_empty() {
        unavailable(
            ui,
            details,
            "iOS version:",
            "No iOS App Store version is available.",
            palette,
        );
    }
    for version in &details.versions {
        let current = details
            .version
            .as_ref()
            .is_some_and(|selected| attr(selected, "id") == attr(version, "id"));
        ui.push_id(attr(version, "id"), |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(0, 12))
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let width = (ui.available_width() - 100.0).max(20.0);
                        ui.allocate_ui_with_layout(
                            vec2(width, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.add(
                                    Label::new(semibold(format!(
                                        "iOS {}",
                                        attr(version, "versionString")
                                    )))
                                    .wrap(),
                                );
                                ui.add(
                                    Label::new(
                                        RichText::new(readable(state(version)))
                                            .color(palette.muted),
                                    )
                                    .wrap(),
                                );
                                let created = attr(version, "createdDate");
                                if !created.is_empty() {
                                    ui.label(
                                        RichText::new(created).size(12.0).color(palette.muted),
                                    );
                                }
                            },
                        );
                        if ui
                            .add_enabled(
                                !current && !loading,
                                Button::new(if current { "Selected" } else { "Inspect" }),
                            )
                            .clicked()
                        {
                            *action = Some(Action::Version(attr(version, "id").into()));
                        }
                    });
                });
            ui.separator();
        });
    }
}

fn planned(ui: &mut Ui, document: Document, palette: Palette) {
    section(ui, document.title(), palette);
    ui.label(RichText::new("Planned").size(12.0).color(palette.muted));
    let message = if document == Document::Review {
        "This resource is not connected to the workspace yet."
    } else {
        "This resource is not available through the connection yet."
    };
    ui.add(Label::new(message).wrap());
    ui.add_space(20.0);
    for (title, description) in document.topics() {
        ui.label(semibold(*title));
        ui.add(Label::new(RichText::new(*description).color(palette.muted)).wrap());
        ui.add_space(12.0);
    }
    ui.hyperlink_to(
        "API reference ↗",
        format!(
            "https://parsequerade.mainbrella.com/docs/reference/topics/{}/",
            document.reference()
        ),
    );
}

fn changes(ui: &mut Ui, workspace: &mut Workspace, details: &AppDetails, palette: Palette) {
    section(ui, "Local Changes", palette);
    if workspace.edits.is_empty() {
        ui.label("You have no local changes.");
        return;
    }
    draft_note(ui, palette);
    let mut revert = None;
    for (key, edit) in &workspace.edits {
        ui.push_id(key, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let title = ui.label(semibold(edit.label.clone()));
                if ui.button("Revert").labelled_by(title.id).clicked() {
                    revert = Some(key.clone());
                }
            });
            let target = if edit.locale.is_empty() || edit.scope == "App" || edit.scope == "Version"
            {
                edit.scope.clone()
            } else {
                format!("{} · {}", edit.scope, edit.locale)
            };
            ui.add(Label::new(RichText::new(target).size(12.0).color(palette.muted)).wrap());
            if let Some(resource) = source_resource(details, &key.0)
                && resource.get(&key.1) != edit.original.as_ref()
            {
                ui.add(
                    Label::new(
                        RichText::new(
                            "The source changed since this edit. Your local value is preserved.",
                        )
                        .color(palette.error),
                    )
                    .wrap(),
                );
                change_value(
                    ui,
                    "Current source",
                    resource
                        .get(&key.1)
                        .and_then(Value::as_str)
                        .unwrap_or("Not set"),
                    palette,
                );
            }
            change_value(
                ui,
                "Before",
                edit.original
                    .as_ref()
                    .and_then(Value::as_str)
                    .unwrap_or("Not set"),
                palette,
            );
            change_value(ui, "After", &edit.value, palette);
            ui.add_space(12.0);
            ui.separator();
        });
    }
    if let Some(key) = revert {
        workspace.edits.remove(&key);
        workspace.focused = None;
    }
}

fn change_value(ui: &mut Ui, label: &str, value: &str, palette: Palette) {
    ui.label(RichText::new(label).size(11.0).color(palette.muted));
    ui.add(
        Label::new(if value.is_empty() { "Empty" } else { value })
            .wrap()
            .selectable(true),
    );
}

struct Field {
    label: &'static str,
    key: &'static str,
    rows: usize,
}

impl Field {
    fn line(label: &'static str, key: &'static str) -> Self {
        Self {
            label,
            key,
            rows: 1,
        }
    }
    fn prose(label: &'static str, key: &'static str, rows: usize) -> Self {
        Self { label, key, rows }
    }
}

fn edit_field(
    ui: &mut Ui,
    workspace: &mut Workspace,
    resource: &Value,
    route: &str,
    field: Field,
    scope: &str,
    palette: Palette,
) {
    let key = (route.to_owned(), field.key.to_owned());
    let mut value = workspace.edits.get(&key).map_or_else(
        || attr(resource, field.key).to_owned(),
        |edit| edit.value.clone(),
    );
    let response = form_row(ui, field.label, palette, 3.0, |ui, label_id| {
        let edit = if field.rows > 1 {
            TextEdit::multiline(&mut value).desired_rows(field.rows)
        } else {
            TextEdit::singleline(&mut value)
        };
        let response = ui
            .add(
                edit.id_salt(&key)
                    .desired_width(if field.rows == 1 {
                        ui.available_width().min(300.0)
                    } else {
                        f32::INFINITY
                    })
                    .margin(vec2(6.0, 3.0))
                    .hint_text("Not set"),
            )
            .labelled_by(label_id);
        if field.rows > 1 {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.label(
                    RichText::new(format!("{} characters", value.chars().count()))
                        .size(11.0)
                        .color(palette.muted),
                );
            });
        }
        response
    });
    if response.changed() {
        workspace.edit(
            route,
            field.key,
            resource.get(field.key),
            value.clone(),
            field.label,
            scope,
        );
    }
    if response.has_focus() {
        workspace.focused = Some(FieldContext {
            label: field.label.into(),
            scope: if route.contains("/localizations/") {
                "Localization"
            } else {
                "Version"
            },
            characters: value.chars().count(),
            modified: workspace.edits.contains_key(&key),
        });
    }
    if field.rows > 1 {
        ui.add_space(4.0);
    }
}

fn section(ui: &mut Ui, title: &str, palette: Palette) {
    ui.add(Label::new(semibold(title).size(12.0).color(palette.text)).wrap());
    ui.add_space(8.0);
}

fn section_break(ui: &mut Ui) {
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(10.0);
}

fn read_row(ui: &mut Ui, label: &str, value: &str, palette: Palette) {
    form_row(ui, label, palette, 0.0, |ui, _| {
        ui.add(
            Label::new(if value.is_empty() { "—" } else { value })
                .wrap()
                .selectable(true),
        );
    });
}

fn form_row<R>(
    ui: &mut Ui,
    label: &str,
    palette: Palette,
    label_top: f32,
    contents: impl FnOnce(&mut Ui, egui::Id) -> R,
) -> R {
    if ui.available_width() < 460.0 {
        ui.vertical(|ui| {
            let label_id = if label.is_empty() {
                ui.id()
            } else {
                ui.add(Label::new(RichText::new(label).size(12.0).color(palette.muted)).wrap())
                    .id
            };
            contents(ui, label_id)
        })
        .inner
    } else {
        ui.horizontal_top(|ui| {
            ui.set_min_height(18.0);
            let label_id = ui
                .allocate_ui_with_layout(vec2(160.0, 0.0), Layout::top_down(Align::Max), |ui| {
                    ui.set_width(160.0);
                    if label_top > 0.0 {
                        ui.add_space(label_top);
                    }
                    ui.add(
                        Label::new(RichText::new(label).size(12.0).color(palette.muted))
                            .halign(Align::Max)
                            .wrap(),
                    )
                    .id
                })
                .inner;
            let width = ui.available_width();
            ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                contents(ui, label_id)
            })
            .inner
        })
        .inner
    }
}

fn field_note(ui: &mut Ui, palette: Palette) {
    ui.add_space(4.0);
    form_row(ui, "", palette, 0.0, |ui, _| {
        draft_note(ui, palette);
    });
}

fn draft_note(ui: &mut Ui, palette: Palette) {
    ui.add(
        Label::new(
            RichText::new("Edits stay in this session. Publishing is not available yet.")
                .size(11.0)
                .color(palette.muted),
        )
        .wrap(),
    );
}

fn unavailable(ui: &mut Ui, details: &AppDetails, prefix: &str, message: &str, palette: Palette) {
    if !details
        .warnings
        .iter()
        .any(|warning| warning.starts_with(prefix))
    {
        ui.add(Label::new(RichText::new(message).color(palette.muted)).wrap());
    }
}
