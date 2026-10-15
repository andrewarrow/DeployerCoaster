pub(crate) mod api;
pub(crate) mod catalog;
pub(crate) mod details;
mod settings;
pub use settings::AppleSettings;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

// Credentials deliberately have no Debug implementation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppleCredentials {
    connection_id: String,
    base_url: String,
    api_key: String,
    #[serde(default)]
    account_id: String,
}

impl AppleCredentials {
    pub(crate) fn sales_cache_account(&self) -> String {
        if !self.account_id.is_empty() {
            return self.account_id.clone();
        }
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(self.connection_id.as_bytes()))
    }

    #[cfg(test)]
    pub(crate) fn test_credentials(account: &str) -> Self {
        Self {
            connection_id: account.into(),
            base_url: format!("https://parsequerade.mainbrella.com/v1/connections/{account}"),
            api_key: "pq_test-key".into(),
            account_id: account.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AppleApp {
    pub(crate) id: String,
    #[serde(flatten)]
    pub(crate) attributes: AppAttributes,
}

impl AppleApp {
    pub(crate) fn title(&self) -> &str {
        if self.attributes.name.is_empty() {
            &self.id
        } else {
            &self.attributes.name
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppAttributes {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) bundle_id: String,
    #[serde(default)]
    sku: String,
}

struct AppJob {
    result: Receiver<Result<catalog::AppList, String>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for AppJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct AppleStore {
    credentials: Option<AppleCredentials>,
    apps: Vec<AppleApp>,
    loaded: bool,
    fetched_at: u64,
    job: Option<AppJob>,
    error: Option<String>,
    cancelled: bool,
    icons: crate::app_icons::AppIcons,
    sales: crate::apple_sales::SalesReports,
    app_copy: crate::apple_copy::AppCopy,
    details: crate::ios_view::IosView,
}

impl AppleStore {
    #[cfg(test)]
    pub(crate) fn test_apps(apps: Vec<AppleApp>) -> Self {
        Self {
            apps,
            credentials: Some(AppleCredentials::test_credentials("picker-tests")),
            loaded: true,
            fetched_at: catalog::now(),
            ..Default::default()
        }
    }

    pub(crate) fn selected_app_ui(&mut self, ui: &mut egui::Ui, app: &AppleApp) {
        self.details
            .ui(ui, Some((&app.id, app.title())), self.credentials.as_ref());
    }

    pub(crate) fn selection_account(&self) -> Option<String> {
        self.credentials.as_ref().map(catalog::account_key)
    }

    pub(crate) fn picker_apps(&self) -> &[AppleApp] {
        &self.apps
    }

    pub(crate) fn prepare_app_list(&mut self, ctx: &egui::Context) {
        self.poll();
        if self.credentials.is_some()
            && (!self.loaded || !catalog::is_fresh(self.fetched_at, catalog::now()))
            && !self.cancelled
            && self.job.is_none()
            && self.error.is_none()
        {
            self.start(ctx, false);
        }
    }

    pub(crate) fn clear_app_details_selection(&mut self) {
        self.details.clear_selection();
    }

    pub(crate) fn app_context_menu(
        &mut self,
        response: &egui::Response,
        app: Option<&crate::dashboard::StoreApp>,
    ) {
        self.app_copy
            .context_menu(response, app, self.credentials.as_ref());
    }

    pub(crate) fn app_copy_ui(&mut self, ctx: &egui::Context) {
        self.app_copy.ui(ctx);
    }

    pub(crate) fn dashboard_apps(&self) -> Vec<crate::dashboard::StoreApp> {
        self.apps
            .iter()
            .map(|app| crate::dashboard::StoreApp {
                identifier: if app.attributes.bundle_id.is_empty() {
                    format!("apple:{}", app.id)
                } else {
                    app.attributes.bundle_id.clone()
                },
                name: if app.attributes.name.is_empty() {
                    app.id.clone()
                } else {
                    app.attributes.name.clone()
                },
                store_id: app.id.clone(),
                store: crate::app_icons::Store::Apple,
            })
            .collect()
    }

    pub(crate) fn dashboard_status(&self) -> crate::dashboard::StoreStatus {
        crate::dashboard::StoreStatus {
            connected: self.credentials.is_some(),
            loading: self.job.is_some(),
            error: self.error.clone(),
        }
    }

    pub(crate) fn prepare_dashboard(&mut self, ctx: &egui::Context) {
        self.poll();
        if self.credentials.is_some()
            && !self.loaded
            && !self.cancelled
            && self.job.is_none()
            && self.error.is_none()
        {
            self.start(ctx, false);
        }
        if self.loaded && self.job.is_none() && self.icons.needs_start() {
            self.icons.ensure_started(
                crate::app_icons::Store::Apple,
                self.apps
                    .iter()
                    .filter(|app| !app.attributes.bundle_id.is_empty())
                    .map(|app| crate::app_icons::IconRequest {
                        key: app.attributes.bundle_id.clone(),
                        artwork_url: None,
                        store_id: Some(app.id.clone()),
                    })
                    .collect(),
                ctx,
            );
        }
    }

    pub(crate) fn refresh_dashboard(&mut self, ctx: &egui::Context) {
        if self.credentials.is_some() {
            self.icons.refresh();
            self.start(ctx, true);
        }
    }

    pub(crate) fn has_dashboard_icon(&mut self, key: &str) -> bool {
        self.icons.has_icon(key)
    }

    pub(crate) fn paint_dashboard_icon(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        key: &str,
        title: &str,
    ) {
        self.icons.paint_icon(ui, rect, key, title);
    }

    pub(crate) fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.result.try_recv() {
            Ok(Ok(list)) => {
                self.apps = list.apps;
                self.fetched_at = list.fetched_at;
                self.loaded = true;
                self.job = None;
            }
            Ok(Err(error)) => {
                self.error = Some(error);
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.error = Some("The Apple connection stopped. Please refresh again.".into());
                self.job = None;
            }
        }
    }

    // Only saved credentials are used. Replacing a key invalidates the old account's list/job.
    pub(crate) fn set_credentials(&mut self, credentials: Option<AppleCredentials>) {
        if self.credentials != credentials {
            *self = Self {
                credentials,
                ..Self::default()
            };
        }
    }

    pub(crate) fn set_reporting_vendor(&mut self, vendor: &str) {
        self.sales.set_account(self.credentials.clone(), vendor);
    }

    /// Returns true when sales setup needs the Apple settings window.
    pub(crate) fn sales_ui(&mut self, ui: &mut egui::Ui, app_id: Option<&str>) -> bool {
        let listings = self.dashboard_apps();
        let selected = app_id.map(|id| {
            let sku = self
                .apps
                .iter()
                .find(|app| app.id == id)
                .map(|app| app.attributes.sku.as_str())
                .unwrap_or_default();
            (id, sku)
        });
        self.sales.ui(ui, selected, &listings, &mut self.icons)
    }

    /// Returns true when the user asks to edit Apple credentials in Settings.
    pub fn apps_ui(&mut self, ui: &mut egui::Ui) -> bool {
        self.poll();
        ui.heading("App Store Connect apps");
        ui.add_space(8.0);
        if self.credentials.is_none() {
            ui.add(
                egui::Label::new(
                    "Connect your Apple key through Parsequerade in Apple settings to load apps.",
                )
                .wrap(),
            );
            return ui
                .add(egui::Button::new("Apple settings…").min_size(egui::vec2(140.0, 44.0)))
                .clicked();
        }
        if !self.loaded && !self.cancelled && self.job.is_none() && self.error.is_none() {
            self.start(ui.ctx(), false);
        }
        let mut settings = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::new("Refresh").min_size(egui::vec2(80.0, 44.0)),
                )
                .clicked()
            {
                self.icons.refresh();
                self.start(ui.ctx(), true);
            }
            settings = ui
                .add(egui::Button::new("Apple settings…").min_size(egui::vec2(140.0, 44.0)))
                .clicked();
        });
        if self.job.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label("Loading apps…");
                if ui
                    .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 44.0)))
                    .clicked()
                {
                    self.job = None;
                    // Keep automatic loading from restarting a cancelled request.
                    self.cancelled = true;
                }
            });
        }
        if self.loaded
            && self.job.is_none()
            && !self.cancelled
            && self.error.is_none()
            && self.icons.needs_start()
        {
            self.icons.ensure_started(
                crate::app_icons::Store::Apple,
                self.apps
                    .iter()
                    .filter(|app| !app.attributes.bundle_id.is_empty())
                    .map(|app| crate::app_icons::IconRequest {
                        key: app.attributes.bundle_id.clone(),
                        artwork_url: None,
                        store_id: Some(app.id.clone()),
                    })
                    .collect(),
                ui.ctx(),
            );
        }
        if let Some(error) = &self.error {
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        }
        ui.add_space(8.0);
        if self.loaded && self.job.is_none() && self.error.is_none() && self.apps.is_empty() {
            ui.label("No apps are accessible to this Parsequerade connection.");
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let icons = &mut self.icons;
                    let app_copy = &mut self.app_copy;
                    for app in &self.apps {
                        let title = if app.attributes.name.is_empty() {
                            &app.id
                        } else {
                            &app.attributes.name
                        };
                        let row = ui.horizontal_top(|ui| {
                            icons.ui_icon(ui, &app.attributes.bundle_id, title);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(title).strong()).wrap(),
                                );
                                if !app.attributes.bundle_id.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&app.attributes.bundle_id)
                                                .monospace(),
                                        )
                                        .wrap(),
                                    );
                                }
                                ui.label(format!("App ID: {}", app.id));
                            });
                        });
                        let response = ui.interact(
                            row.response.rect,
                            ui.id().with(("apple_app", &app.id)),
                            egui::Sense::click(),
                        );
                        app_copy.context_menu(
                            &response,
                            Some(&crate::dashboard::StoreApp {
                                identifier: app.attributes.bundle_id.clone(),
                                name: title.clone(),
                                store_id: app.id.clone(),
                                store: crate::app_icons::Store::Apple,
                            }),
                            self.credentials.as_ref(),
                        );
                        ui.add_space(8.0);
                    }
                });
        }
        self.app_copy_ui(ui.ctx());
        settings
    }

    fn start(&mut self, ctx: &egui::Context, refresh: bool) {
        let Some(credentials) = self.credentials.clone() else {
            return;
        };
        if self.job.is_some() {
            return;
        }
        self.error = None;
        self.cancelled = false;
        let ctx = ctx.clone();
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        self.job = Some(AppJob { result, cancelled });
        thread::spawn(move || {
            let path = catalog::cache_path(&credentials);
            let result = catalog::load_or_fetch(path.as_deref(), catalog::now(), refresh, || {
                api::ParsequeradeApi::new(credentials)
                    .and_then(|mut api| list_apps(&mut api, &cancellation))
            });
            if !cancellation.load(Ordering::Relaxed) {
                let _ = sender.send(result);
                ctx.request_repaint();
            }
        });
    }
}

fn list_apps<T: api::Transport>(
    transport: &mut T,
    cancelled: &AtomicBool,
) -> Result<Vec<AppleApp>, String> {
    let values = api::list(transport, "/apps?limit=200", cancelled)?;
    let mut apps: Vec<AppleApp> = values
        .into_iter()
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|_| "Parsequerade returned an invalid app list.".to_owned())
        })
        .collect::<Result<_, _>>()?;
    let mut ids = HashSet::new();
    apps.retain(|app| ids.insert(app.id.clone()));
    apps.sort_by_cached_key(|app| {
        (
            app.attributes.name.to_lowercase(),
            app.attributes.bundle_id.clone(),
            app.id.clone(),
        )
    });
    Ok(apps)
}
