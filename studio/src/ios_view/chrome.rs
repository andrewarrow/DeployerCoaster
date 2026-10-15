use egui::{Align, Button, Color32, Label, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};

use super::documents::{GROUPS, locale_name, locales, readable};
use super::{Action, AppDetails, Document, Palette, Workspace, attr};
use crate::style::semibold;
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
        if crate::native_titlebar_active(ui.ctx()) {
            ui.add_space(80.0);
        }
        if compact {
            let response = ui
                .menu_button("    ", |ui| {
                    ui.set_width(260.0);
                    navigation_rows(ui, workspace, details, palette, true);
                })
                .response
                .on_hover_text("Resource navigator");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Resource navigator")
            });
            paint_pane_icon(ui, response.rect.center(), true, true, palette);
        } else if pane_button(ui, true, workspace.navigator, palette).clicked() {
            workspace.navigator = !workspace.navigator;
        }
        let reserved = if compact { 136.0 } else { 432.0 };
        let title_width = (ui.available_width() - reserved).clamp(24.0, 300.0);
        ui.allocate_ui_with_layout(
            vec2(title_width, 32.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                if title_width > 110.0 {
                    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                    Document::Information.paint_icon(ui, rect.min, 20.0, palette.accent);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(Label::new(semibold(name).size(13.0)).truncate())
                        .on_hover_text(name);
                    if !compact {
                        let status = if loading {
                            "Refreshing…".into()
                        } else {
                            details
                                .and_then(|d| d.version.as_ref())
                                .map(|version| readable(state(version)))
                                .unwrap_or_else(|| "No iOS version".into())
                        };
                        ui.add(
                            Label::new(RichText::new(status).size(11.0).color(palette.muted))
                                .truncate(),
                        );
                    }
                });
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
            let drag = ui.interact(
                ui.available_rect_before_wrap(),
                ui.id().with("toolbar_drag"),
                Sense::click_and_drag(),
            );
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
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
        ui.spacing_mut().button_padding = vec2(12.0, 5.0);
        let widgets = &mut ui.visuals_mut().widgets;
        for widget in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            widget.corner_radius = egui::CornerRadius::same(14);
        }
        egui::ComboBox::from_id_salt("version_selector")
            .selected_text(text)
            .width(170.0)
            .icon(|ui, rect, visuals, _| {
                let c = rect.center();
                let stroke = Stroke::new(1.2, visuals.fg_stroke.color);
                ui.painter().add(egui::Shape::line(
                    vec![
                        c + vec2(-3.0, -2.0),
                        c + vec2(0.0, -5.0),
                        c + vec2(3.0, -2.0),
                    ],
                    stroke,
                ));
                ui.painter().add(egui::Shape::line(
                    vec![c + vec2(-3.0, 2.0), c + vec2(0.0, 5.0), c + vec2(3.0, 2.0)],
                    stroke,
                ));
            })
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

pub(super) fn navigator(
    ui: &mut Ui,
    workspace: &mut Workspace,
    details: Option<&AppDetails>,
    name: &str,
    palette: Palette,
) {
    egui::Panel::bottom("navigator_filter")
        .exact_size(34.0)
        .frame(palette.frame(palette.sidebar, Margin::symmetric(8, 6)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                paint_filter_icon(ui, rect.center(), palette);
                ui.add(
                    egui::TextEdit::singleline(&mut workspace.filter)
                        .hint_text("Filter")
                        .margin(vec2(5.0, 3.0))
                        .desired_width(ui.available_width()),
                )
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Filter resources")
                });
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
                        Document::Information.paint_icon(ui, rect.min, 18.0, palette.accent);
                        ui.add(Label::new(semibold(name).size(13.0)).truncate());
                    });
                    ui.add_space(6.0);
                    navigation_rows(ui, workspace, details, palette, false);
                });
        });
}

fn navigation_rows(
    ui: &mut Ui,
    workspace: &mut Workspace,
    details: Option<&AppDetails>,
    palette: Palette,
    menu: bool,
) {
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
                if document_row(ui, workspace, *document, details, palette) && menu {
                    ui.close();
                }
            }
        };
        if !filter.is_empty() {
            ui.label(RichText::new(*group).size(11.0).color(palette.muted));
            rows(ui);
        } else {
            egui::CollapsingHeader::new(semibold(*group).size(11.0).color(palette.muted))
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
    details: Option<&AppDetails>,
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
    let missing = details.and_then(|details| document.missing_data(details));
    // Reserve a stable gutter, outside the selection, so late results don't move rows.
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (gutter, marker) = ui.allocate_exact_size(vec2(14.0, 25.0), Sense::hover());
            if let Some(reason) = missing {
                marker.on_hover_text(reason);
                let red = if ui.visuals().dark_mode {
                    Color32::from_rgb(255, 69, 58)
                } else {
                    Color32::from_rgb(255, 59, 48)
                };
                ui.painter().circle_filled(gutter.center(), 5.0, red);
            }
            ui.add_sized(
                [ui.available_width(), 25.0],
                Button::selectable(selected, "").fill(if selected {
                    palette.selection
                } else {
                    Color32::TRANSPARENT
                }),
            )
        })
        .inner;
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            missing.map_or_else(
                || document.title().to_owned(),
                |reason| format!("{} · {reason}", document.title()),
            ),
        )
    });
    let rect = response.rect;
    let icon_color = if selected {
        Color32::WHITE
    } else {
        document.icon_color(ui.visuals().dark_mode)
    };
    document.paint_icon(ui, rect.min + vec2(5.0, 4.5), 16.0, icon_color);
    let galley = egui::WidgetText::from(RichText::new(document.title()).size(13.0).color(color))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.width() - 35.0).max(0.0),
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
    response.on_hover_text(if let Some(reason) = missing {
        format!("{} · {reason}", document.title())
    } else if document.connected() {
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
        .exact_size(29.0)
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
                            let fill = if active { palette.tab } else { palette.toolbar };
                            let title = format!(
                                "{}{}",
                                if workspace.dirty(document) {
                                    "• "
                                } else {
                                    ""
                                },
                                document.title()
                            );
                            let title_galley = ui.painter().layout_no_wrap(
                                title.clone(),
                                egui::FontId::proportional(12.0),
                                palette.text,
                            );
                            let width = (title_galley.size().x + 38.0).clamp(118.0, 190.0);
                            let response = ui.add_sized(
                                [width, 28.0],
                                Button::selectable(active, "")
                                    .fill(fill)
                                    .corner_radius(0)
                                    .stroke(Stroke::NONE),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    ui.is_enabled(),
                                    active,
                                    document.title(),
                                )
                            });
                            let rect = response.rect;
                            document.paint_icon(
                                ui,
                                rect.min + vec2(9.0, 7.0),
                                13.0,
                                document.icon_color(ui.visuals().dark_mode),
                            );
                            let color = if active { palette.text } else { palette.muted };
                            let galley = egui::WidgetText::from(
                                RichText::new(title).size(12.0).color(color),
                            )
                            .into_galley(
                                ui,
                                Some(egui::TextWrapMode::Truncate),
                                width - 32.0,
                                egui::TextStyle::Button,
                            );
                            ui.painter().galley(
                                egui::pos2(
                                    rect.left() + 29.0,
                                    rect.center().y - galley.size().y * 0.5,
                                ),
                                galley,
                                color,
                            );
                            if response.clicked() {
                                workspace.open(document);
                            }
                            let close = ui.add_enabled(
                                workspace.tabs.len() > 1,
                                Button::new("")
                                    .fill(fill)
                                    .stroke(Stroke::NONE)
                                    .corner_radius(0)
                                    .min_size(vec2(20.0, 28.0)),
                            );
                            let center = close.rect.center();
                            let close_stroke = Stroke::new(1.0, palette.muted);
                            for sign in [-1.0, 1.0] {
                                ui.painter().line_segment(
                                    [
                                        center + vec2(-2.5, -2.5 * sign),
                                        center + vec2(2.5, 2.5 * sign),
                                    ],
                                    close_stroke,
                                );
                            }
                            close.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    workspace.tabs.len() > 1,
                                    format!("Close {}", document.title()),
                                )
                            });
                            if close
                                .on_hover_text(format!("Close {}", document.title()))
                                .clicked()
                            {
                                workspace.close(document);
                            }
                            ui.painter().line_segment(
                                [
                                    rect.right_top() + vec2(20.0, 0.0),
                                    rect.right_bottom() + vec2(20.0, 0.0),
                                ],
                                Stroke::new(1.0, palette.border),
                            );
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
        .exact_size(28.0)
        .frame(palette.frame(palette.editor, Margin::symmetric(10, 2)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let localized = matches!(workspace.active, Document::Listing | Document::Privacy);
                let available = ui.available_width();
                let breadcrumb_width = if localized {
                    (available - 188.0).max(40.0)
                } else {
                    (available - 24.0).max(0.0)
                };
                let (icon_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                workspace.active.paint_icon(
                    ui,
                    icon_rect.min,
                    14.0,
                    workspace.active.icon_color(ui.visuals().dark_mode),
                );
                let text = if available < 460.0 {
                    workspace.active.group().into()
                } else {
                    format!(
                        "{}  ›  {}",
                        workspace.active.group(),
                        workspace.active.title()
                    )
                };
                ui.allocate_ui_with_layout(
                    vec2(breadcrumb_width, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_width(breadcrumb_width);
                        ui.add(
                            Label::new(RichText::new(text).size(12.0).color(palette.text))
                                .truncate(),
                        );
                    },
                );
                if localized {
                    let choices = details.map(locales).unwrap_or_default();
                    let before = workspace.locale.clone();
                    egui::ComboBox::from_id_salt("localization_selector")
                        .width(156.0)
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
        .exact_size(29.0)
        .frame(palette.frame(palette.toolbar, Margin::symmetric(10, 6)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(13.0, 15.0), Sense::hover());
                Document::Agreements.paint_icon(ui, rect.min, 13.0, palette.muted);
                ui.label(semibold("Identity Inspector").size(11.0));
            });
        });
    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(10)
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
            Document::AgeRating => "Age rating declaration",
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
    inspector_value(
        ui,
        "Access",
        if workspace.active == Document::AgeRating {
            "Age rating paste"
        } else {
            "Read only"
        },
        palette,
    );
}

fn inspector_section(ui: &mut Ui, label: &str, palette: Palette) {
    ui.label(semibold(label).size(12.0).color(palette.muted));
    ui.add_space(4.0);
}

fn inspector_value(ui: &mut Ui, label: &str, value: &str, palette: Palette) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(72.0, 0.0), Layout::right_to_left(Align::Min), |ui| {
            ui.set_width(72.0);
            ui.add(Label::new(RichText::new(label).size(12.0).color(palette.muted)).wrap());
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
                RichText::new(if loading {
                    "Syncing…"
                } else if workspace.active == Document::AgeRating {
                    "Age rating paste"
                } else {
                    "Read only"
                })
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
    let response = ui.add_sized(
        [36.0, 30.0],
        Button::new("")
            .fill(if selected {
                palette.control
            } else {
                Color32::TRANSPARENT
            })
            .corner_radius(15)
            .stroke(Stroke::new(
                1.0,
                if selected {
                    palette.hover
                } else {
                    Color32::TRANSPARENT
                },
            )),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    paint_pane_icon(ui, response.rect.center(), left, selected, palette);
    response.on_hover_text(label)
}

fn paint_pane_icon(ui: &Ui, center: egui::Pos2, left: bool, selected: bool, palette: Palette) {
    let rect = egui::Rect::from_center_size(center, vec2(18.0, 14.0));
    let color = if selected {
        palette.text
    } else {
        palette.muted
    };
    #[cfg(target_os = "macos")]
    if crate::macos::paint_symbol(
        ui,
        if left {
            "sidebar.left"
        } else {
            "sidebar.right"
        },
        center - vec2(10.0, 10.0),
        20.0,
        color,
    ) {
        return;
    }
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

fn paint_filter_icon(ui: &Ui, center: egui::Pos2, palette: Palette) {
    #[cfg(target_os = "macos")]
    if crate::macos::paint_symbol(
        ui,
        "line.3.horizontal.decrease.circle",
        center - vec2(7.0, 7.0),
        14.0,
        palette.muted,
    ) {
        return;
    }
    ui.painter()
        .circle_stroke(center, 6.0, Stroke::new(1.0, palette.muted));
    for (offset, width) in [(-2.0, 7.0), (0.0, 5.0), (2.0, 3.0)] {
        ui.painter().line_segment(
            [
                center + vec2(-width * 0.5, offset),
                center + vec2(width * 0.5, offset),
            ],
            Stroke::new(1.0, palette.muted),
        );
    }
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
