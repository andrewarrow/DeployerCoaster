use super::*;

impl Dashboard {
    pub(super) fn matches(&self, product: &Product) -> bool {
        let query = self.search.trim().to_lowercase();
        (product.name.to_lowercase().contains(&query)
            || product.identifier.to_lowercase().contains(&query))
            && match self.filter {
                Filter::All => true,
                Filter::Apple => product.listings.iter().any(|l| l.store == Store::Apple),
                Filter::Android => product.listings.iter().any(|l| l.store == Store::Play),
            }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn app_list(
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
                let loading = apple_status.loading || play_status.loading;
                let response = ui.add_enabled_ui(!loading, refresh_control).inner;
                let rect = response.rect;
                if loading {
                    egui::Spinner::new()
                        .size(16.0)
                        .paint_at(ui, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)));
                } else {
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
                }
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
                        self.filter,
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
                        self.navigation = Section::Overview;
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
}
