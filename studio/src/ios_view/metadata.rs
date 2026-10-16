use std::collections::BTreeMap;

use reqwest::Method;
use serde_json::{Map, Value};

use super::documents::{Edit, app_route, info_route, version_route};
use super::*;
use crate::apple::api::{self, Transport};

type Edits = BTreeMap<(String, String), Edit>;
type Outcome = (String, Result<Value, String>);

const UNCONFIRMED: &str =
    "Saving could not be confirmed. Your edits are kept; retry to check and save them.";

struct Save {
    app_id: String,
    edits: Edits,
    result: Receiver<Vec<Outcome>>,
}

#[derive(Default)]
pub(super) struct Metadata {
    save: Option<Save>,
    feedback: HashMap<String, Result<String, String>>,
}

impl Metadata {
    pub(super) fn saving(&self, app_id: &str) -> bool {
        self.save.as_ref().is_some_and(|save| save.app_id == app_id)
    }

    pub(super) fn controls(
        &self,
        ui: &mut Ui,
        workspace: &Workspace,
        app_id: &str,
        loading: bool,
        palette: Palette,
    ) -> bool {
        if !matches!(
            workspace.active,
            Document::Information | Document::Listing | Document::Privacy | Document::Changes
        ) {
            return false;
        }
        let mut clicked = false;
        Frame::new()
            .inner_margin(Margin::symmetric(16, 8))
            .show(ui, |ui| {
                clicked = ui
                    .add_enabled(
                        !loading && self.save.is_none() && !workspace.edits.is_empty(),
                        egui::Button::new(if self.saving(app_id) {
                            "Saving…"
                        } else {
                            "Save"
                        })
                        .min_size(egui::vec2(72.0, 44.0)),
                    )
                    .on_hover_text("Save all local changes for this app to App Store Connect.")
                    .clicked();
                if !self.saving(app_id)
                    && let Some(result) = self.feedback.get(app_id)
                    && (result.is_err() || workspace.edits.is_empty())
                {
                    let (message, color) = match result {
                        Ok(message) => (message, palette.muted),
                        Err(message) => (message, palette.error),
                    };
                    ui.add(egui::Label::new(RichText::new(message).color(color)).wrap());
                }
            });
        clicked
    }

    pub(super) fn start(
        &mut self,
        app_id: &str,
        workspace: &Workspace,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
        if self.save.is_some() || workspace.edits.is_empty() {
            return;
        }
        let edits = workspace.edits.clone();
        let pending = edits.clone();
        let (sender, result) = mpsc::channel();
        self.save = Some(Save {
            app_id: app_id.into(),
            edits,
            result,
        });
        self.feedback.remove(app_id);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut api = ParsequeradeApi::new(credentials);
            let outcomes = save_edits(&pending, |route, edits| match &mut api {
                Ok(api) => save_resource(api, route, edits),
                Err(error) => Err(error.clone()),
            });
            let _ = sender.send(outcomes);
            ctx.request_repaint();
        });
    }

    pub(super) fn poll(
        &mut self,
        cache: &mut HashMap<String, Entry>,
        workspaces: &mut HashMap<String, Workspace>,
    ) {
        let Some(save) = &self.save else { return };
        let outcomes = match save.result.try_recv() {
            Ok(outcomes) => outcomes,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                save_edits(&save.edits, |_, _| Err(UNCONFIRMED.into()))
            }
        };
        let save = self.save.take().unwrap();
        let mut errors = Vec::new();
        let mut saved = false;
        for (route, outcome) in outcomes {
            match outcome {
                Ok(resource) => {
                    saved = true;
                    if let Some(details) = cache.get_mut(&save.app_id).and_then(|e| e.data.as_mut())
                    {
                        merge_resource(details, &route, &resource);
                    }
                    if let Some(workspace) = workspaces.get_mut(&save.app_id) {
                        for (key, submitted) in save.edits.iter().filter(|(key, _)| key.0 == route)
                        {
                            if workspace
                                .edits
                                .get(key)
                                .is_some_and(|edit| edit.value == submitted.value)
                            {
                                workspace.edits.remove(key);
                            } else if let Some(edit) = workspace.edits.get_mut(key) {
                                edit.original = resource.get(&key.1).cloned();
                            }
                        }
                    }
                }
                Err(error) => errors.push(error),
            }
        }
        self.feedback.insert(
            save.app_id,
            if errors.is_empty() {
                Ok("Changes saved.".into())
            } else {
                Err(format!(
                    "{} {}",
                    if saved {
                        "Some changes could not be saved."
                    } else {
                        "Changes could not be saved."
                    },
                    errors[0]
                ))
            },
        );
    }
}

fn save_edits(
    edits: &Edits,
    mut save: impl FnMut(&str, &BTreeMap<String, Edit>) -> Result<Value, String>,
) -> Vec<Outcome> {
    let mut groups: BTreeMap<String, BTreeMap<String, Edit>> = BTreeMap::new();
    for ((route, field), edit) in edits {
        groups
            .entry(route.clone())
            .or_default()
            .insert(field.clone(), edit.clone());
    }
    groups
        .into_iter()
        .map(|(route, edits)| {
            let result = save(&route, &edits);
            (route, result)
        })
        .collect()
}

fn save_resource<T: Transport>(
    api: &mut T,
    route: &str,
    edits: &BTreeMap<String, Edit>,
) -> Result<Value, String> {
    // Re-read at click time to avoid overwriting changes made outside Studio.
    // Already-applied values also make retrying an uncertain result safe.
    let (status, current) = api.send(Method::GET, route, None)?;
    if status != 200 {
        return Err(api::api_error(status, &current));
    }
    let id = api::id(&current)?;
    let mut attributes = Map::new();
    for (field, edit) in edits {
        let remote = attr(&current, field);
        if remote == edit.value {
            continue;
        }
        if remote
            != edit
                .original
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or_default()
        {
            return Err(format!(
                "{} changed in App Store Connect. Refresh and review your local edit before saving again.",
                edit.label
            ));
        }
        attributes.insert(field.clone(), Value::String(edit.value.clone()));
    }
    if attributes.is_empty() {
        return Ok(current);
    }
    let (status, resource) = api.send(Method::PATCH, route, Some(&Value::Object(attributes)))?;
    if status != 200 {
        return Err(match resource.pointer("/error/code").and_then(Value::as_str) {
            Some("write_outcome_unknown") => UNCONFIRMED.into(),
            _ if status == 409 => "Apple could not apply these changes. Check that the selected version is editable, then retry.".into(),
            _ if status == 400 || status == 422 => "Apple rejected these changes. Check text lengths, URLs, and the selected version’s requirements.".into(),
            _ => api::api_error(status, &resource),
        });
    }
    if api::id(&resource).ok() != Some(id)
        || edits
            .iter()
            .any(|(field, edit)| attr(&resource, field) != edit.value)
    {
        return Err(UNCONFIRMED.into());
    }
    Ok(resource)
}

fn merge_resource(details: &mut AppDetails, route: &str, resource: &Value) {
    let merge = |current: &mut Value| {
        if attr(current, "id") == attr(resource, "id")
            && let (Some(current), Some(updated)) = (current.as_object_mut(), resource.as_object())
        {
            current.extend(updated.clone());
        }
    };
    if route == app_route(details) {
        merge(&mut details.app);
    } else if route == version_route(details) {
        if let Some(current) = &mut details.version {
            merge(current);
        }
        for current in &mut details.versions {
            merge(current);
        }
    } else {
        let info = info_route(details);
        let version = version_route(details);
        for (base, resources) in [
            (info, &mut details.info_localizations),
            (version, &mut details.version_localizations),
        ] {
            for current in resources {
                if route == format!("{base}/localizations/{}", attr(current, "locale")) {
                    merge(current);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
