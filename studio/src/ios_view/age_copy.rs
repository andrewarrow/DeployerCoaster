use reqwest::Method;
use serde_json::{Map, Value};

use super::*;
use crate::apple::api::{self, Transport};

// Keep this restricted to the writable Age Rating section. IDs and computed ratings
// never enter the clipboard or the POST body.
const FIELDS: &[&str] = &[
    "advertising",
    "alcoholTobaccoOrDrugUseOrReferences",
    "contests",
    "gambling",
    "gamblingSimulated",
    "gunsOrOtherWeapons",
    "healthOrWellnessTopics",
    "kidsAgeBand",
    "lootBox",
    "medicalOrTreatmentInformation",
    "messagingAndChat",
    "parentalControls",
    "profanityOrCrudeHumor",
    "ageAssurance",
    "sexualContentGraphicAndNudity",
    "sexualContentOrNudity",
    "socialMedia",
    "socialMediaAgeRestricted",
    "horrorOrFearThemes",
    "matureOrSuggestiveThemes",
    "unrestrictedWebAccess",
    "userGeneratedContent",
    "violenceCartoonOrFantasy",
    "violenceRealisticProlongedGraphicOrSadistic",
    "violenceRealistic",
    "ageRatingOverrideV2",
    "koreaAgeRatingOverride",
    "gracRatingClassificationNumber",
    "developerAgeRatingInfoUrl",
];
const OPTIONAL: &[&str] = &[
    "kidsAgeBand",
    "ageRatingOverrideV2",
    "koreaAgeRatingOverride",
    "gracRatingClassificationNumber",
    "developerAgeRatingInfoUrl",
];

fn snapshot(details: &AppDetails) -> Option<Value> {
    let rating = details.age_rating.as_ref()?.as_object()?;
    let fields: Map<String, Value> = FIELDS
        .iter()
        .filter_map(|&field| {
            let value = rating.get(field)?;
            // Unanswered questions cannot be written. Explicit null overrides clear
            // the destination's optional settings instead of retaining stale values.
            if value.is_null() && !OPTIONAL.contains(&field) {
                return None;
            }
            Some((field.into(), value.clone()))
        })
        .collect();
    (!fields.is_empty()).then_some(Value::Object(fields))
}

struct Paste {
    app_id: String,
    info_id: String,
    result: Receiver<Result<Value, String>>,
}

#[derive(Default)]
pub(super) struct AgeCopy {
    clipboard: AgeRatingClipboard,
    paste: Option<Paste>,
    feedback: HashMap<String, Result<String, String>>,
}

impl AgeCopy {
    pub(super) fn with_clipboard(clipboard: AgeRatingClipboard) -> Self {
        Self {
            clipboard,
            ..Default::default()
        }
    }

    pub(super) fn pasting(&self, app_id: &str) -> bool {
        self.paste
            .as_ref()
            .is_some_and(|paste| paste.app_id == app_id)
    }

    pub(super) fn poll(&mut self, cache: &mut HashMap<String, Entry>) {
        let Some(paste) = &self.paste else { return };
        let result = match paste.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(
                "The paste result could not be confirmed. Refresh Age Rating before trying again."
                    .into(),
            ),
        };
        let paste = self.paste.take().unwrap();
        let feedback = result.map(|rating| {
            if let Some(details) = cache
                .get_mut(&paste.app_id)
                .and_then(|entry| entry.data.as_mut())
                && details
                    .info
                    .as_ref()
                    .is_some_and(|info| attr(info, "id") == paste.info_id)
            {
                // POST accepts a subset, and Apple may omit unchanged fields in
                // its update response. Keep the destination's other answers visible.
                if let Some(current) = details.age_rating.as_mut().and_then(Value::as_object_mut) {
                    current.extend(rating.as_object().unwrap().clone());
                } else {
                    details.age_rating = Some(rating);
                }
                details
                    .warnings
                    .retain(|warning| !warning.starts_with("Age rating:"));
            }
            "Age rating pasted.".to_owned()
        });
        self.feedback.insert(paste.app_id, feedback);
    }

    pub(super) fn feedback(&self, ui: &mut Ui, app_id: &str, palette: Palette) {
        if self.pasting(app_id) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Pasting age rating…");
            });
            ui.add_space(8.0);
        } else if let Some(result) = self.feedback.get(app_id) {
            let (message, color) = match result {
                Ok(message) => (message, palette.muted),
                Err(message) => (message, palette.error),
            };
            ui.add(egui::Label::new(RichText::new(message).color(color)).wrap());
            ui.add_space(8.0);
        }
    }

    pub(super) fn context_menu(
        &mut self,
        response: &egui::Response,
        app_id: &str,
        details: Option<&AppDetails>,
        loading: bool,
        credentials: &AppleCredentials,
    ) {
        response.context_menu(|ui| {
            let values = details.and_then(snapshot);
            if ui
                .add_enabled(!loading && values.is_some(), egui::Button::new("Copy"))
                .on_disabled_hover_text("Wait for an age rating declaration to load.")
                .clicked()
            {
                *self.clipboard.borrow_mut() = values;
                self.feedback
                    .insert(app_id.into(), Ok("Age rating copied.".into()));
                ui.close();
            }
            let target = details.and_then(|details| {
                Some((
                    api::id(details.info.as_ref()?).ok()?,
                    api::id(details.age_rating.as_ref()?).ok()?,
                ))
            });
            let enabled = !loading
                && self.paste.is_none()
                && self.clipboard.borrow().is_some()
                && target.is_some();
            let reason = if self.paste.is_some() {
                "Wait for the current paste to finish."
            } else if self.clipboard.borrow().is_none() {
                "Copy Age Rating from an app first."
            } else {
                "Wait for the destination’s age rating declaration to load."
            };
            if ui
                .add_enabled(enabled, egui::Button::new("Paste"))
                .on_hover_text(
                    "Replace this app’s age-rating answers and overrides with the copied section.",
                )
                .on_disabled_hover_text(reason)
                .clicked()
            {
                let (info_id, rating_id) = target.unwrap();
                self.start(app_id, info_id, rating_id, credentials.clone(), ui.ctx());
                ui.close();
            }
        });
    }

    fn start(
        &mut self,
        app_id: &str,
        info_id: &str,
        rating_id: &str,
        credentials: AppleCredentials,
        ctx: &egui::Context,
    ) {
        let Some(values) = self.clipboard.borrow().clone() else {
            return;
        };
        if self.paste.is_some() {
            return;
        }
        let (sender, result) = mpsc::channel();
        self.paste = Some(Paste {
            app_id: app_id.into(),
            info_id: info_id.into(),
            result,
        });
        self.feedback.remove(app_id);
        let (app_id, info_id, rating_id) =
            (app_id.to_owned(), info_id.to_owned(), rating_id.to_owned());
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ParsequeradeApi::new(credentials)
                .and_then(|mut api| save(&mut api, &app_id, &info_id, &rating_id, &values));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
}

fn save<T: Transport>(
    transport: &mut T,
    app_id: &str,
    info_id: &str,
    rating_id: &str,
    values: &Value,
) -> Result<Value, String> {
    for id in [app_id, info_id, rating_id] {
        api::validate_id(id)?;
    }
    let (status, rating) = transport.send(
        Method::POST,
        &format!("/apps/{app_id}/app-infos/{info_id}/age-rating"),
        Some(values),
    )?;
    if status != 200 {
        return Err(match status {
            409 => "Apple could not apply these age-rating settings to this app. Refresh and check the app’s editable state.".into(),
            422 => "Apple rejected these age-rating settings. Check the source answers and the destination app’s requirements.".into(),
            _ => api::api_error(status, &rating),
        });
    }
    if !rating.is_object()
        || api::id(&rating).ok() != Some(rating_id)
        || rating["appId"].as_str() != Some(app_id)
        || rating["appInfoId"].as_str() != Some(info_id)
    {
        return Err(
            "The paste result could not be confirmed. Refresh Age Rating before trying again."
                .into(),
        );
    }
    Ok(rating)
}

#[cfg(test)]
mod tests;
