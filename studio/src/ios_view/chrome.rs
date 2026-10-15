use egui::{Align, Button, Color32, Label, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};

use super::documents::{GROUPS, locale_name, locales, readable};
use super::{Action, AppDetails, Document, Palette, Workspace, attr};
use crate::{apple::details::state, dashboard::Icon};

pub(super) fn toolbar(
    ui: &mut Ui,
    workspace: &mut Workspace,
    details: Option<&AppDetails>,
    name: &str,
    loading: bool,
    palette: Palette,
) -> Option<Action> {
    let mut action = None;
    let compact = ui.available_width() < 680.0;
    let inline_inspector = ui.available_width() >= 1100.0;
    ui.horizontal_centered(|ui| {
        if compact {
            let response = ui
                .menu_button("☰", |ui| {
                    ui.set_width(260.0);
                    navigation_rows(ui, workspace, palette, true);
                })
                .response
                .on_hover_text("Resource navigator");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Resource navigator")
            });
        } else if pane_button(ui, true, workspace.navigator, palette).clicked() {
            workspace.navigator = !workspace.navigator;
        }
        let reserved = if compact { 126.0 } else { 420.0 };
        let title_width = (ui.available_width() - reserved).clamp(24.0, 300.0);
        ui.allocate_ui_with_layout(
            vec2(title_width, 32.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                if title_width > 110.0 {
                    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                    ui.painter().rect_filled(rect, 5.0, palette.selection);
                    Icon::Apps.paint(ui, rect.min + vec2(5.0, 5.0), 14.0, Color32::WHITE);
                }
                ui.add(Label::new(RichText::new(name).strong()).truncate())
                    .on_hover_text(name);
            },
        );
        if !compact {
            version_picker(ui, details, loading, &mut action);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.available_width() > 32.0 {
                if !inline_inspector {
                    let response = ui
                        .menu_button("  ", |ui| {
                            ui.set_width(252.0);
                            inspector_contents(ui, workspace, details, palette);
                        })
                        .response
                        .on_hover_text("Resource inspector");
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "Resource inspector",
                        )
                    });
                    paint_pane_icon(ui, response.rect.center(), false, false, palette);
                } else if pane_button(ui, false, workspace.inspector, palette).clicked() {
                    workspace.inspector = !workspace.inspector;
                }
            }
            if icon_button(ui, Icon::Refresh, "Refresh app details", !loading, palette).clicked() {
                action = Some(Action::Refresh);
            }
            let count = workspace.edits.len();
            let label = if count == 0 {
                "Changes".into()
            } else {
                format!("Changes ({count})")
            };
            if ui
                .add(Button::new(label).frame(false))
                .on_hover_text("Review local changes")
                .clicked()
            {
                workspace.open(Document::Changes);
            }
            if loading {
                ui.spinner().on_hover_text("Refreshing app details…");
            }
        });
    });
    action
}

pub(super) fn version_picker(
    ui: &mut Ui,
    details: Option<&AppDetails>,
    loading: bool,
    action: &mut Option<Action>,
) {
    let selected = details.and_then(|d| d.version.as_ref());
    let version = selected.map_or("", |v| attr(v, "versionString"));
    let text = if version.is_empty() {
        "iOS · No version".into()
    } else {
        format!("iOS  {version}")
    };
    ui.add_enabled_ui(!loading, |ui| {
        egui::ComboBox::from_id_salt("version_selector")
            .selected_text(text)
            .width(152.0)
            .show_ui(ui, |ui| {
                if let Some(details) = details {
                    for version in &details.versions {
                        let current =
                            selected.is_some_and(|v| attr(v, "id") == attr(version, "id"));
                        let label = format!(
                            "{} — {}",
                            attr(version, "versionString"),
                            readable(state(version))
                        );
                        if ui.selectable_label(current, label).clicked() && !current {
                            *action = Some(Action::Version(attr(version, "id").into()));
                        }
                    }
                    if details.versions.is_empty() {
                        ui.label("No iOS versions are available.");
                    }
                }
            })
            .response
            .on_hover_text("Selected iOS version");
    });
}

pub(super) fn navigator(ui: &mut Ui, workspace: &mut Workspace, name: &str, palette: Palette) {
    egui::Panel::bottom("navigator_filter")
        .exact_size(44.0)
        .frame(palette.frame(palette.sidebar, Margin::symmetric(10, 7)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let label = ui.label(RichText::new("Filter").size(11.0).color(palette.muted));
                ui.add(
                    egui::TextEdit::singleline(&mut workspace.filter)
                        .hint_text("Resources")
                        .desired_width(ui.available_width()),
                )
                .labelled_by(label.id);
            });
        });
    egui::ScrollArea::vertical()
        .id_salt("navigator_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin::symmetric(8, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                        Icon::Apps.paint(ui, rect.min, 18.0, palette.accent);
                        ui.add(Label::new(RichText::new(name).strong()).truncate());
                    });
                    ui.add_space(8.0);
                    navigation_rows(ui, workspace, palette, false);
                });
        });
}

fn navigation_rows(ui: &mut Ui, workspace: &mut Workspace, palette: Palette, menu: bool) {
    let filter = workspace.filter.trim().to_lowercase();
    ui.spacing_mut().item_spacing.y = 2.0;
    for (group, documents) in GROUPS {
        let visible: Vec<_> = documents
            .iter()
            .copied()
            .filter(|doc| {
                filter.is_empty()
                    || doc.title().to_lowercase().contains(&filter)
                    || group.to_lowercase().contains(&filter)
            })
            .collect();
        if visible.is_empty() {
            continue;
        }
        let mut rows = |ui: &mut Ui| {
            for document in &visible {
                if document_row(ui, workspace, *document, palette) && menu {
                    ui.close();
                }
            }
        };
        if !filter.is_empty() {
            ui.label(RichText::new(*group).size(11.0).color(palette.muted));
            rows(ui);
        } else {
            egui::CollapsingHeader::new(RichText::new(*group).size(12.0).strong())
                .id_salt(("resource_group", group, menu))
                .default_open(matches!(
                    *group,
                    "App Store" | "Distribution" | "Monetization"
                ))
                .show(ui, rows);
        }
    }
    if !filter.is_empty()
        && !GROUPS.iter().any(|(group, docs)| {
            group.to_lowercase().contains(&filter)
                || docs
                    .iter()
                    .any(|doc| doc.title().to_lowercase().contains(&filter))
        })
    {
        ui.label(RichText::new("No matching resources.").color(palette.muted));
    }
}

fn document_row(
    ui: &mut Ui,
    workspace: &mut Workspace,
    document: Document,
    palette: Palette,
) -> bool {
    let selected = workspace.active == document;
    let color = if selected {
        Color32::WHITE
    } else if document.connected() {
        palette.text
    } else {
        palette.muted
    };
    let response = ui.add_sized(
        [ui.available_width(), 28.0],
        Button::selectable(selected, "").fill(if selected {
            palette.selection
        } else {
            Color32::TRANSPARENT
        }),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            document.title(),
        )
    });
    let rect = response.rect;
    document
        .icon()
        .paint(ui, rect.min + vec2(7.0, 6.0), 15.0, color);
    let galley = egui::WidgetText::from(RichText::new(document.title()).size(12.0).color(color))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.width() - 37.0).max(0.0),
            egui::TextStyle::Body,
        );
    ui.painter().galley(
        egui::pos2(rect.left() + 29.0, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
    if workspace.dirty(document) {
        ui.painter()
            .circle_filled(egui::pos2(rect.right() - 8.0, rect.center().y), 2.5, color);
    }
    let clicked = response.clicked();
    response.on_hover_text(if document.connected() {
        document.title().into()
    } else {
        format!("{} · Planned section", document.title())
    });
    if clicked {
        workspace.open(document);
    }
    clicked
}

pub(super) fn tabs(ui: &mut Ui, workspace: &mut Workspace, palette: Palette) {
    egui::Panel::top("document_tabs")
        .exact_size(34.0)
        .frame(palette.frame(palette.toolbar, Margin::ZERO))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("open_document_tabs")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for document in workspace.tabs.clone() {
                            let active = workspace.active == document;
                            let fill = if active {
                                palette.editor
                            } else {
                                palette.toolbar
                            };
                            let title = format!(
                                "{}{}",
                                if workspace.dirty(document) {
                                    "• "
                                } else {
                                    ""
                                },
                                document.title()
                            );
                            let width =
                                (document.title().len() as f32 * 6.2 + 24.0).clamp(108.0, 180.0);
                            let response = ui.add_sized(
                                [width, 33.0],
                                Button::selectable(active, title)
                                    .fill(fill)
                                    .corner_radius(0)
                                    .stroke(Stroke::NONE)
                                    .truncate(),
                            );
                            if response.clicked() {
                                workspace.open(document);
                            }
                            let close = ui.add_enabled(
                                workspace.tabs.len() > 1,
                                Button::new("×")
                                    .fill(fill)
                                    .stroke(Stroke::NONE)
                                    .corner_radius(0)
                                    .min_size(vec2(22.0, 33.0)),
                            );
                            if close
                                .on_hover_text(format!("Close {}", document.title()))
                                .clicked()
                            {
                                workspace.close(document);
                            }
                            let rect = response.rect;
                            if active {
                                ui.painter().line_segment(
                                    [rect.left_bottom(), rect.right_bottom() + vec2(22.0, 0.0)],
                                    Stroke::new(2.0, palette.accent),
                                );
                            }
                        }
                    });
                });
        });
}

pub(super) fn jump_bar(
    ui: &mut Ui,
    workspace: &mut Workspace,
    details: Option<&AppDetails>,
    palette: Palette,
) {
    egui::Panel::top("document_jump_bar")
        .exact_size(36.0)
        .frame(palette.frame(palette.editor, Margin::symmetric(12, 4)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let localized = matches!(workspace.active, Document::Listing | Document::Privacy);
                let available = ui.available_width();
                let breadcrumb_width = if localized {
                    (available - 190.0).max(40.0)
                } else {
                    available
                };
                let text = if available < 460.0 {
                    workspace.active.group().into()
                } else {
                    format!(
                        "{}  ›  {}",
                        workspace.active.group(),
                        workspace.active.title()
                    )
                };
                ui.add_sized(
                    [breadcrumb_width, 26.0],
                    Label::new(RichText::new(text).size(12.0).color(palette.muted))
                        .halign(Align::Min)
                        .truncate(),
                );
                if localized {
                    let choices = details.map(locales).unwrap_or_default();
                    let before = workspace.locale.clone();
                    egui::ComboBox::from_id_salt("localization_selector")
                        .width(168.0)
                        .selected_text(if before.is_empty() {
                            "No localizations".into()
                        } else {
                            locale_name(&before)
                        })
                        .show_ui(ui, |ui| {
                            for locale in choices {
                                ui.selectable_value(
                                    &mut workspace.locale,
                                    locale.into(),
                                    format!("{} · {locale}", locale_name(locale)),
                                );
                            }
                        })
                        .response
                        .on_hover_text(format!("Localization: {}", workspace.locale));
                    if before != workspace.locale {
                        workspace.focused = None;
                    }
                }
            });
        });
}

pub(super) fn inspector(
    ui: &mut Ui,
    workspace: &Workspace,
    details: Option<&AppDetails>,
    palette: Palette,
) {
    egui::Panel::top("inspector_heading")
        .exact_size(34.0)
        .frame(palette.frame(palette.toolbar, Margin::symmetric(12, 8)))
        .show(ui, |ui| {
            ui.label(RichText::new("Identity Inspector").size(12.0).strong());
        });
    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(16)
                .show(ui, |ui| inspector_contents(ui, workspace, details, palette));
        });
}

fn inspector_contents(
    ui: &mut Ui,
    workspace: &Workspace,
    details: Option<&AppDetails>,
    palette: Palette,
) {
    inspector_section(ui, "Identity", palette);
    inspector_value(ui, "Name", workspace.active.title(), palette);
    inspector_value(
        ui,
        "Type",
        match workspace.active {
            Document::Information => "App",
            Document::Listing => "Version localization",
            Document::Privacy => "App metadata",
            Document::Versions => "Version collection",
            Document::Changes => "Session draft",
            _ => "Planned resource",
        },
        palette,
    );
    if let Some(details) = details {
        inspector_value(ui, "App ID", attr(&details.app, "id"), palette);
        inspector_value(ui, "Bundle ID", attr(&details.app, "bundleId"), palette);
        ui.add_space(14.0);
        inspector_section(ui, "Selected Release", palette);
        if let Some(version) = &details.version {
            inspector_value(ui, "Platform", "iOS", palette);
            inspector_value(ui, "Version", attr(version, "versionString"), palette);
            inspector_value(ui, "Status", &readable(state(version)), palette);
        } else {
            ui.label(RichText::new("No iOS version available.").color(palette.muted));
        }
        if !workspace.locale.is_empty()
            && matches!(workspace.active, Document::Listing | Document::Privacy)
        {
            inspector_value(ui, "Locale", &workspace.locale, palette);
            let primary = attr(&details.app, "primaryLocale");
            inspector_value(ui, "Primary", primary, palette);
        }
    }
    if let Some(field) = &workspace.focused {
        ui.add_space(14.0);
        inspector_section(ui, "Selected Field", palette);
        inspector_value(ui, "Field", &field.label, palette);
        inspector_value(ui, "Scope", field.scope, palette);
        inspector_value(
            ui,
            "Length",
            &format!("{} characters", field.characters),
            palette,
        );
        inspector_value(
            ui,
            "State",
            if field.modified {
                "Modified locally"
            } else {
                "Unmodified"
            },
            palette,
        );
    }
    ui.add_space(14.0);
    inspector_section(ui, "Connection", palette);
    inspector_value(ui, "Access", "Read only", palette);
}

fn inspector_section(ui: &mut Ui, label: &str, palette: Palette) {
    ui.label(RichText::new(label).size(12.0).strong().color(palette.text));
    ui.add_space(4.0);
}

fn inspector_value(ui: &mut Ui, label: &str, value: &str, palette: Palette) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(58.0, 0.0), Layout::right_to_left(Align::Min), |ui| {
            ui.add(Label::new(RichText::new(label).size(11.0).color(palette.muted)).wrap());
        });
        ui.add(
            Label::new(RichText::new(if value.is_empty() { "—" } else { value }).size(12.0))
                .wrap()
                .selectable(true),
        );
    });
}

pub(super) fn status(
    ui: &mut Ui,
    workspace: &mut Workspace,
    details: Option<&AppDetails>,
    loading: bool,
    palette: Palette,
) {
    ui.horizontal(|ui| {
        let count = workspace.edits.len();
        if ui
            .add(
                Button::new(
                    RichText::new(if count == 0 {
                        "No local changes".into()
                    } else {
                        format!("{count} local changes · Session only")
                    })
                    .size(11.0)
                    .color(palette.muted),
                )
                .frame(false)
                .min_size(vec2(0.0, 16.0)),
            )
            .clicked()
        {
            workspace.open(Document::Changes);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(if loading { "Syncing…" } else { "Read only" })
                    .size(11.0)
                    .color(palette.muted),
            );
            if ui.available_width() > 150.0
                && let Some(details) = details
            {
                ui.label(
                    RichText::new(format!("{} localizations", locales(details).len()))
                        .size(11.0)
                        .color(palette.muted),
                );
            }
        });
    });
}

fn pane_button(ui: &mut Ui, left: bool, selected: bool, palette: Palette) -> egui::Response {
    let label = if left {
        "Toggle navigator"
    } else {
        "Toggle inspector"
    };
    let response = ui.add_sized([28.0, 28.0], Button::new("").frame(false));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    paint_pane_icon(ui, response.rect.center(), left, selected, palette);
    response.on_hover_text(label)
}

fn paint_pane_icon(ui: &Ui, center: egui::Pos2, left: bool, selected: bool, palette: Palette) {
    let rect = egui::Rect::from_center_size(center, vec2(18.0, 14.0));
    let color = if selected {
        palette.accent
    } else {
        palette.muted
    };
    ui.painter()
        .rect_stroke(rect, 2.0, Stroke::new(1.2, color), egui::StrokeKind::Inside);
    let x = if left {
        rect.left() + 5.5
    } else {
        rect.right() - 5.5
    };
    ui.painter().line_segment(
        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
        Stroke::new(1.2, color),
    );
}

fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    palette: Palette,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        Button::new("").frame(false).min_size(vec2(28.0, 28.0)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    icon.paint(
        ui,
        response.rect.center() - vec2(7.5, 7.5),
        15.0,
        if enabled {
            palette.muted
        } else {
            palette.muted.gamma_multiply(0.5)
        },
    );
    response.on_hover_text(label)
}
