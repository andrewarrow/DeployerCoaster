mod api;
mod data;
mod period;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

use crate::{app_icons::AppIcons, apple::AppleCredentials, dashboard::StoreApp};
use api::LoadedReport;
use data::AppSales;
use period::Frequency;
use time::{Date, OffsetDateTime};

struct ReportJob {
    receiver: Receiver<Result<LoadedReport, String>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for ReportJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(crate) struct SalesReports {
    credentials: Option<AppleCredentials>,
    vendor: String,
    frequency: Frequency,
    date: Date,
    report: Option<LoadedReport>,
    job: Option<ReportJob>,
    error: Option<String>,
    attempted: bool,
}

impl Default for SalesReports {
    fn default() -> Self {
        Self {
            credentials: None,
            vendor: String::new(),
            frequency: Frequency::Monthly,
            date: Frequency::Monthly.latest(today()),
            report: None,
            job: None,
            error: None,
            attempted: false,
        }
    }
}

impl SalesReports {
    pub(crate) fn set_account(&mut self, credentials: Option<AppleCredentials>, vendor: &str) {
        let vendor = vendor.trim();
        if self.credentials != credentials || self.vendor != vendor {
            self.credentials = credentials;
            self.vendor = vendor.to_owned();
            self.select_period(self.frequency, self.frequency.latest(today()));
        }
    }

    fn select_period(&mut self, frequency: Frequency, date: Date) {
        // Dropping the receiver prevents an older period/account from replacing this result.
        self.job = None;
        self.frequency = frequency;
        self.date = date;
        self.report = None;
        self.error = None;
        self.attempted = false;
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.receiver.try_recv() {
            Ok(Ok(report)) => {
                self.report = Some(report);
                self.error = None;
                self.job = None;
            }
            Ok(Err(error)) => {
                self.error = Some(error);
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.error = Some("Sales report loading stopped. Please retry.".into());
                self.job = None;
            }
        }
    }

    fn start(&mut self, ctx: &egui::Context, refresh: bool) {
        let Some(credentials) = self.credentials.clone() else {
            return;
        };
        if self.vendor.is_empty() || self.job.is_some() {
            return;
        }
        self.error = None;
        self.attempted = true;
        let vendor = self.vendor.clone();
        let frequency = self.frequency;
        let date = self.date;
        let context = ctx.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        self.job = Some(ReportJob {
            receiver,
            cancelled,
        });
        thread::spawn(move || {
            let result = api::load_report(
                &credentials,
                &vendor,
                frequency,
                date,
                refresh,
                &cancellation,
            );
            if !cancellation.load(Ordering::Relaxed) {
                let _ = sender.send(result);
                context.request_repaint();
            }
        });
    }

    /// Renders inside the caller's scroll area. Returns true to open Apple settings.
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        selected: Option<(&str, &str)>,
        listings: &[StoreApp],
        icons: &mut AppIcons,
    ) -> bool {
        self.poll();
        ui.set_max_width(ui.available_width().min(960.0));
        ui.heading("App Store sales");
        ui.add_space(8.0);
        if self.credentials.is_none() || self.vendor.is_empty() {
            ui.add(egui::Label::new(if self.credentials.is_none() {
                "Add your App Store Connect API key and vendor number in Apple settings to load sales reports."
            } else {
                "Add your vendor number in Apple settings to load sales reports."
            }).wrap());
            ui.add_space(8.0);
            return ui
                .add(egui::Button::new("Apple settings…").min_size(egui::vec2(140.0, 44.0)))
                .clicked();
        }

        let mut settings = false;
        let compact = ui.available_width() < 420.0;
        ui.horizontal_wrapped(|ui| {
            for frequency in [Frequency::Monthly, Frequency::Daily] {
                if ui
                    .add(
                        egui::Button::selectable(self.frequency == frequency, frequency.label())
                            .min_size(egui::vec2(72.0, 44.0)),
                    )
                    .clicked()
                    && self.frequency != frequency
                {
                    self.select_period(frequency, frequency.latest(today()));
                }
            }
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::new(if self.error.is_some() {
                        "Retry"
                    } else {
                        "Refresh"
                    })
                    .min_size(egui::vec2(72.0, 44.0)),
                )
                .clicked()
            {
                self.start(ui.ctx(), true);
            }
            settings = ui
                .add(
                    egui::Button::new(if compact {
                        "Settings…"
                    } else {
                        "Apple settings…"
                    })
                    .min_size(egui::vec2(if compact { 84.0 } else { 120.0 }, 44.0)),
                )
                .on_hover_text("Apple settings")
                .clicked();
        });
        ui.horizontal_wrapped(|ui| {
            let previous = self.frequency.previous(self.date);
            if ui
                .add_enabled(
                    previous.is_some(),
                    egui::Button::new("‹").min_size(egui::vec2(44.0, 44.0)),
                )
                .on_hover_text("Previous report period")
                .clicked()
                && let Some(date) = previous
            {
                self.select_period(self.frequency, date);
            }
            ui.label(egui::RichText::new(self.frequency.display_date(self.date)).strong());
            let next = self.frequency.next(self.date, today());
            if ui
                .add_enabled(
                    next.is_some(),
                    egui::Button::new("›").min_size(egui::vec2(44.0, 44.0)),
                )
                .on_hover_text("Next report period")
                .clicked()
                && let Some(date) = next
            {
                self.select_period(self.frequency, date);
            }
        });
        if !self.attempted {
            self.start(ui.ctx(), false);
        }
        ui.add_space(8.0);
        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if self.report.is_some() {
                    "Refreshing sales report…"
                } else {
                    "Loading sales report…"
                });
            });
        }
        if let Some(error) = &self.error {
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        }
        let Some(report) = &self.report else {
            return settings;
        };
        if let Some(warning) = &report.cache_warning {
            ui.add(egui::Label::new(warning).wrap());
        }
        if report.cached {
            ui.label(
                egui::RichText::new("Saved report · Refresh to check for revisions")
                    .small()
                    .weak(),
            );
        }
        let apps: Vec<_> = report
            .apps
            .iter()
            .filter(|app| {
                selected.is_none_or(|(id, sku)| {
                    app.apple_identifier == id || (!sku.is_empty() && app.parent_identifier == sku)
                })
            })
            .collect();
        if apps.is_empty() {
            ui.label(if selected.is_some() {
                "No sales reported for this app in this period."
            } else {
                "No sales reported for this period."
            });
            return settings;
        }

        let units: i128 = apps.iter().map(|app| i128::from(app.total_units)).sum();
        ui.label(
            egui::RichText::new(format!(
                "{} {} · {units} units",
                apps.len(),
                if apps.len() == 1 {
                    "product"
                } else {
                    "products"
                }
            ))
            .strong(),
        );
        let mut proceeds = BTreeMap::<String, i128>::new();
        let mut proceeds_overflow = false;
        for app in &apps {
            for (currency, amount) in &app.proceeds {
                let total = proceeds.entry(currency.clone()).or_default();
                if let Some(sum) = total.checked_add(*amount) {
                    *total = sum;
                } else {
                    proceeds_overflow = true;
                }
            }
        }
        if proceeds_overflow {
            ui.add(egui::Label::new("Combined proceeds exceed the supported range. Expand a product to see its proceeds.").wrap());
        } else if !proceeds.is_empty() {
            ui.add(
                egui::Label::new(format!(
                    "Developer proceeds: {}",
                    format_proceeds(&proceeds)
                ))
                .wrap(),
            );
        }
        ui.add_space(8.0);
        for app in apps {
            sales_row(ui, app, &self.vendor, listings, icons);
        }
        settings
    }
}

fn today() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

fn format_proceeds(proceeds: &BTreeMap<String, i128>) -> String {
    proceeds
        .iter()
        .map(|(currency, cents)| {
            let amount = cents.unsigned_abs();
            format!(
                "{currency} {}{}.{:02}",
                if *cents < 0 { "−" } else { "" },
                amount / 100,
                amount % 100
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn sales_row(
    ui: &mut egui::Ui,
    app: &AppSales,
    vendor: &str,
    listings: &[StoreApp],
    icons: &mut AppIcons,
) {
    let id = ui.make_persistent_id(("sales_product", vendor, &app.sku));
    let state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let mut clicked = false;
    let mut header = state.show_header(ui, |ui| {
        let width = ui.available_width();
        let response = ui
            .allocate_ui_with_layout(
                egui::vec2(width, 48.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // Header text belongs to the disclosure button; selection would consume its clicks.
                    ui.style_mut().interaction.selectable_labels = false;
                    let key = listings
                        .iter()
                        .find(|listing| listing.store_id == app.apple_identifier)
                        .map(|listing| listing.identifier.as_str())
                        .unwrap_or(&app.sku);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                    icons.paint_icon(ui, rect, key, &app.title);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let units = format!("{} units", app.total_units);
                            let units_width = ui
                                .painter()
                                .layout_no_wrap(
                                    units.clone(),
                                    egui::TextStyle::Body.resolve(ui.style()),
                                    ui.visuals().text_color(),
                                )
                                .size()
                                .x;
                            let name_width =
                                (ui.available_width() - units_width - ui.spacing().item_spacing.x)
                                    .max(0.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(name_width, 20.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_min_width(name_width);
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(&app.title).strong())
                                            .truncate(),
                                    )
                                    .on_hover_text(&app.title);
                                },
                            );
                            ui.label(units);
                        });
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("SKU: {}", app.sku))
                                    .small()
                                    .weak(),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&app.sku);
                    });
                },
            )
            .response;
        let response = ui.interact(response.rect, id.with("button"), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{}: {} units, toggle breakdown", app.title, app.total_units),
            )
        });
        clicked = response.clicked();
        if clicked {
            response.request_focus();
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                response.rect,
                4.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
    });
    if clicked {
        header.toggle();
    }
    header.body(|ui| {
        if !app.category.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(&app.category).small().weak()).wrap());
        }
        if !app.proceeds.is_empty() {
            ui.add(
                egui::Label::new(format!(
                    "Developer proceeds: {}",
                    format_proceeds(&app.proceeds)
                ))
                .wrap(),
            );
        }
        for breakdown in &app.breakdowns {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let device = if breakdown.device.is_empty() {
                    "Unknown device"
                } else {
                    &breakdown.device
                };
                ui.label(
                    egui::RichText::new(if breakdown.version.is_empty() {
                        device.to_owned()
                    } else {
                        format!("{device} · v{}", breakdown.version)
                    })
                    .strong(),
                );
                ui.label(format!("{} units", breakdown.units));
            });
            let mut metrics = vec![
                format!("{} downloads", breakdown.downloads),
                format!("{} updates", breakdown.updates),
                format!("{} redownloads", breakdown.redownloads),
            ];
            if breakdown.other != 0 {
                metrics.push(format!("{} other", breakdown.other));
            }
            ui.add(egui::Label::new(egui::RichText::new(metrics.join(" · ")).small()).wrap());
            if !breakdown.countries.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!(
                            "Countries: {}",
                            breakdown
                                .countries
                                .iter()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                        .small()
                        .weak(),
                    )
                    .wrap(),
                );
            }
        }
        ui.add_space(8.0);
    });
    ui.add_space(4.0);
}
