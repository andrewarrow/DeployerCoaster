use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

use reqwest::Method;
use serde_json::{Value, json};

use super::{Event, StepState};
use crate::apple::api::{Transport, checked_url, id, validate_id};

const CONTACT: &[&str] = &[
    "contactFirstName",
    "contactLastName",
    "contactPhone",
    "contactEmail",
];
const LOCALIZATION: &[&str] = &["description", "keywords", "marketingUrl", "supportUrl"];
// Writable fields from Apple's OpenAPI specification. Never PATCH computed ratings.
const AGE: &[&str] = &[
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

#[derive(Clone)]
pub(super) struct Snapshot {
    pub app: Value,
    info: Value,
    info_localizations: Vec<Value>,
    age: Option<Value>,
    versions: Vec<Version>,
}

#[derive(Clone)]
struct Version {
    resource: Value,
    localizations: Vec<Value>,
    review: Option<Value>,
}

pub(super) struct Operation {
    pub label: String,
    pub method: Method,
    pub path: String,
    pub body: Value,
}

pub(super) enum Planned {
    Write(Operation),
    Skip(String),
}

pub(super) struct Session<'a, T> {
    pub transport: T,
    pub cancelled: &'a AtomicBool,
    pub emit: &'a mut dyn FnMut(Event),
    next_step: usize,
}

impl<'a, T: Transport> Session<'a, T> {
    pub fn new(transport: T, cancelled: &'a AtomicBool, emit: &'a mut dyn FnMut(Event)) -> Self {
        Self {
            transport,
            cancelled,
            emit,
            next_step: 0,
        }
    }

    fn check_cancelled(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err("Cancelled. Completed changes have been kept.".into())
        } else {
            Ok(())
        }
    }

    fn add_step(&mut self, label: &str, request: &str, state: StepState) -> usize {
        let index = self.next_step;
        self.next_step += 1;
        (self.emit)(Event::Step {
            label: label.into(),
            request: request.into(),
            state,
        });
        index
    }

    fn request(
        &mut self,
        label: &str,
        method: Method,
        path: &str,
        body: Option<&Value>,
        optional: bool,
        index: Option<usize>,
    ) -> Result<Value, String> {
        self.check_cancelled()?;
        let index = index.unwrap_or_else(|| {
            self.add_step(label, &format!("{method} {path}"), StepState::Pending)
        });
        (self.emit)(Event::Status(index, StepState::Running));
        let result = self
            .transport
            .send(method, path, body)
            .and_then(|(status, value)| {
                if (200..300).contains(&status) {
                    Ok((status, value))
                } else if optional && status == 404 {
                    Ok((status, json!({"data": null})))
                } else {
                    Err(api_error(status, &value))
                }
            });
        match result {
            Ok((status, value)) => {
                (self.emit)(Event::Status(
                    index,
                    StepState::Success(format!("HTTP {status}")),
                ));
                Ok(value)
            }
            Err(error) => {
                (self.emit)(Event::Status(index, StepState::Failed(error.clone())));
                Err(format!("{label}: {error}"))
            }
        }
    }

    fn one(&mut self, label: &str, path: &str, optional: bool) -> Result<Option<Value>, String> {
        let value = self.request(label, Method::GET, path, None, optional, None)?;
        match value.get("data") {
            Some(Value::Null) if optional => Ok(None),
            Some(data) if data.is_object() => {
                id(data)?;
                Ok(Some(data.clone()))
            }
            _ => Err(format!("{label}: Apple returned invalid resource data.")),
        }
    }

    fn list(&mut self, label: &str, path: &str) -> Result<Vec<Value>, String> {
        let original = checked_url(path)?;
        let mut next = Some(original.clone());
        let mut seen = HashSet::new();
        let mut resources = Vec::new();
        while let Some(url) = next {
            if url.path() != original.path() || !seen.insert(url.to_string()) {
                return Err("Apple returned an unexpected or repeated page.".into());
            }
            let value = self.request(label, Method::GET, url.as_str(), None, false, None)?;
            let data = value["data"]
                .as_array()
                .ok_or_else(|| format!("{label}: Apple returned an invalid list."))?;
            for resource in data {
                id(resource)?;
            }
            resources.extend(data.iter().cloned());
            next = value
                .pointer("/links/next")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(checked_url)
                .transpose()?;
        }
        Ok(resources)
    }

    pub fn load(&mut self, app_id: &str, destination: bool) -> Result<Snapshot, String> {
        // IDs are used only as path segments, never as URLs supplied by the clipboard.
        validate_id(app_id)?;
        let app = self
            .one("Load app settings", &format!("/v1/apps/{app_id}"), false)?
            .unwrap();
        let infos = self.list(
            "Load app information",
            &format!("/v1/apps/{app_id}/appInfos?include=primaryCategory&limit=200"),
        )?;
        let info = select_info(&infos, destination)?.clone();
        let info_id = id(&info)?;
        let versions = self.list(
            "Load app versions",
            &format!("/v1/apps/{app_id}/appStoreVersions?limit=200"),
        )?;
        let selected = select_versions(&versions, destination)?;
        let info_localizations = self.list(
            "Load privacy policy localizations",
            &format!("/v1/appInfos/{info_id}/appInfoLocalizations?limit=200"),
        )?;
        let age = self.one(
            "Load age rating declaration",
            &format!("/v1/appInfos/{info_id}/ageRatingDeclaration"),
            true,
        )?;
        let mut loaded_versions = Vec::new();
        for resource in selected {
            let version_id = id(resource)?;
            let platform = attr(resource, "platform");
            let localizations = self.list(
                &format!("Load {platform} listing localizations"),
                &format!(
                    "/v1/appStoreVersions/{version_id}/appStoreVersionLocalizations?limit=200"
                ),
            )?;
            let review = self.one(
                &format!("Load {platform} review contact"),
                &format!("/v1/appStoreVersions/{version_id}/appStoreReviewDetail"),
                true,
            )?;
            loaded_versions.push(Version {
                resource: resource.clone(),
                localizations,
                review,
            });
        }
        self.check_cancelled()?;
        Ok(Snapshot {
            app,
            info,
            info_localizations,
            age,
            versions: loaded_versions,
        })
    }

    pub fn apply(&mut self, plan: Vec<Planned>) -> Result<(), String> {
        let steps: Vec<_> = plan
            .into_iter()
            .map(|entry| match entry {
                Planned::Write(op) => {
                    let index = self.add_step(
                        &op.label,
                        &format!("{} {}", op.method, op.path),
                        StepState::Pending,
                    );
                    Some((index, op))
                }
                Planned::Skip(reason) => {
                    self.add_step(&reason, "", StepState::Skipped);
                    None
                }
            })
            .flatten()
            .collect();
        for (position, (index, op)) in steps.iter().enumerate() {
            if let Err(error) = self.request(
                &op.label,
                op.method.clone(),
                &op.path,
                Some(&op.body),
                false,
                Some(*index),
            ) {
                for (remaining, _) in &steps[position..] {
                    if remaining != index || self.cancelled.load(Ordering::Relaxed) {
                        (self.emit)(Event::Status(*remaining, StepState::Skipped));
                    }
                }
                return Err(format!(
                    "{error} No further requests were sent. Completed changes have been kept."
                ));
            }
        }
        Ok(())
    }
}

fn attr<'a>(resource: &'a Value, field: &str) -> &'a str {
    resource["attributes"][field].as_str().unwrap_or_default()
}

fn state(resource: &Value) -> &str {
    ["state", "appVersionState", "appStoreState"]
        .into_iter()
        .map(|field| attr(resource, field))
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

fn source_priority(resource: &Value) -> u8 {
    match state(resource) {
        "PREPARE_FOR_SUBMISSION" => 0,
        "READY_FOR_DISTRIBUTION" | "READY_FOR_SALE" => 1,
        "REPLACED_WITH_NEW_VERSION" | "REPLACED_WITH_NEW_INFO" => 3,
        _ => 2,
    }
}

fn select_info(infos: &[Value], destination: bool) -> Result<&Value, String> {
    if destination {
        let candidates: Vec<_> = infos
            .iter()
            .filter(|v| state(v) == "PREPARE_FOR_SUBMISSION")
            .collect();
        if candidates.len() != 1 {
            return Err("The destination must have one app information record in Prepare for Submission. No settings were changed.".into());
        }
        Ok(candidates[0])
    } else {
        infos
            .iter()
            .min_by_key(|v| source_priority(v))
            .ok_or_else(|| "The source has no app information to copy.".into())
    }
}

fn select_versions(versions: &[Value], destination: bool) -> Result<Vec<&Value>, String> {
    let mut platforms = HashMap::new();
    for version in versions {
        if destination && state(version) != "PREPARE_FOR_SUBMISSION" {
            continue;
        }
        let platform = attr(version, "platform");
        if platform.is_empty() {
            return Err("Apple returned a version without a platform.".into());
        }
        if let Some(existing) = platforms.get(platform).copied() {
            if destination {
                return Err(format!(
                    "The destination has multiple editable {platform} versions. No settings were changed."
                ));
            }
            if (
                source_priority(version),
                std::cmp::Reverse(attr(version, "createdDate")),
            ) >= (
                source_priority(existing),
                std::cmp::Reverse(attr(existing, "createdDate")),
            ) {
                continue;
            }
        }
        platforms.insert(platform, version);
    }
    if platforms.is_empty() {
        return Err(if destination {
            "The destination needs a version in Prepare for Submission. No settings were changed."
        } else {
            "The source has no App Store version to copy."
        }
        .into());
    }
    let mut result: Vec<_> = platforms.into_values().collect();
    result.sort_by_key(|v| attr(v, "platform"));
    Ok(result)
}

fn attributes(resource: &Value, fields: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for &field in fields {
        if let Some(value) = resource["attributes"]
            .get(field)
            .filter(|v| !v.is_null() && !v.as_str().is_some_and(str::is_empty))
        {
            result.insert(field.into(), value.clone());
        }
    }
    Value::Object(result)
}

fn patch(label: &str, kind: &str, resource: &Value, attrs: Value) -> Result<Planned, String> {
    if attrs.as_object().is_none_or(|a| a.is_empty()) {
        return Ok(Planned::Skip(format!("{label}: no source values")));
    }
    let resource_id = id(resource)?;
    Ok(Planned::Write(Operation {
        label: label.into(),
        method: Method::PATCH,
        path: format!("/v1/{kind}/{resource_id}"),
        body: json!({"data": {"type": kind, "id": resource_id, "attributes": attrs}}),
    }))
}

fn upsert(
    label: &str,
    kind: &str,
    destination: Option<&Value>,
    mut attrs: Value,
    parent: (&str, &str, &Value),
    locale: Option<&str>,
) -> Result<Planned, String> {
    if attrs.as_object().is_none_or(|a| a.is_empty()) {
        return Ok(Planned::Skip(format!("{label}: no source values")));
    }
    if let Some(destination) = destination {
        return patch(label, kind, destination, attrs);
    }
    if let Some(locale) = locale {
        attrs["locale"] = json!(locale);
    }
    Ok(Planned::Write(Operation {
        label: label.into(),
        method: Method::POST,
        path: format!("/v1/{kind}"),
        body: json!({"data": {"type": kind, "attributes": attrs, "relationships": {
            parent.0: {"data": {"type": parent.1, "id": id(parent.2)?}}
        }}}),
    }))
}

fn locale(resource: &Value) -> Result<&str, String> {
    let locale = attr(resource, "locale");
    if locale.is_empty() {
        Err("Apple returned a localization without a locale. No settings were changed.".into())
    } else {
        Ok(locale)
    }
}

pub(super) fn plan(source: &Snapshot, target: &Snapshot) -> Result<Vec<Planned>, String> {
    if id(&source.app)? == id(&target.app)? {
        return Err("Choose a different destination app.".into());
    }
    // Preflight all platform and localization mappings before the first write.
    if !source.versions.iter().any(|s| {
        target
            .versions
            .iter()
            .any(|t| attr(&s.resource, "platform") == attr(&t.resource, "platform"))
    }) {
        return Err("The destination has no editable version matching the source platforms. No settings were changed.".into());
    }
    let mut result = vec![patch(
        "Copy content rights",
        "apps",
        &target.app,
        attributes(&source.app, &["contentRightsDeclaration"]),
    )?];
    if let Some(category) = source
        .info
        .pointer("/relationships/primaryCategory/data")
        .filter(|v| !v.is_null())
    {
        let target_id = id(&target.info)?;
        result.push(Planned::Write(Operation {
            label: "Copy primary category".into(), method: Method::PATCH, path: format!("/v1/appInfos/{target_id}"),
            body: json!({"data": {"type": "appInfos", "id": target_id, "relationships": {"primaryCategory": {"data": {"type": "appCategories", "id": id(category)?}}}}}),
        }));
    } else {
        result.push(Planned::Skip("Primary category: no source value".into()));
    }
    for source_loc in &source.info_localizations {
        let locale = locale(source_loc)?;
        let target_loc = target
            .info_localizations
            .iter()
            .find(|v| attr(v, "locale") == locale);
        let mut attrs = attributes(source_loc, &["privacyPolicyUrl"]);
        if target_loc.is_none() && attrs.get("privacyPolicyUrl").is_some() {
            let name = attr(&target.app, "name");
            if name.is_empty() {
                return Err(
                    "The destination needs a name to create a privacy policy localization.".into(),
                );
            }
            attrs["name"] = json!(name);
        }
        result.push(upsert(
            &format!("Copy privacy URL ({locale})"),
            "appInfoLocalizations",
            target_loc,
            attrs,
            ("appInfo", "appInfos", &target.info),
            Some(locale),
        )?);
    }
    match (&source.age, &target.age) {
        (Some(source), Some(target)) => result.push(patch(
            "Copy age rating declaration",
            "ageRatingDeclarations",
            target,
            attributes(source, AGE),
        )?),
        (Some(_), None) => {
            return Err(
                "The destination has no age rating declaration. No settings were changed.".into(),
            );
        }
        _ => result.push(Planned::Skip("Age rating: no source values".into())),
    }
    for source in &source.versions {
        let platform = attr(&source.resource, "platform");
        let Some(target) = target
            .versions
            .iter()
            .find(|v| attr(&v.resource, "platform") == platform)
        else {
            result.push(Planned::Skip(format!(
                "{platform}: no destination version in Prepare for Submission"
            )));
            continue;
        };
        result.push(patch(
            &format!("Copy {platform} copyright"),
            "appStoreVersions",
            &target.resource,
            attributes(&source.resource, &["copyright"]),
        )?);
        if let Some(review) = &source.review {
            result.push(upsert(
                &format!("Copy {platform} review contact"),
                "appStoreReviewDetails",
                target.review.as_ref(),
                attributes(review, CONTACT),
                ("appStoreVersion", "appStoreVersions", &target.resource),
                None,
            )?);
        } else {
            result.push(Planned::Skip(format!(
                "{platform} review contact: no source values"
            )));
        }
        for source_loc in &source.localizations {
            let locale = locale(source_loc)?;
            let target_loc = target
                .localizations
                .iter()
                .find(|v| attr(v, "locale") == locale);
            result.push(upsert(
                &format!("Copy {platform} listing ({locale})"),
                "appStoreVersionLocalizations",
                target_loc,
                attributes(source_loc, LOCALIZATION),
                ("appStoreVersion", "appStoreVersions", &target.resource),
                Some(locale),
            )?);
        }
    }
    Ok(result)
}

fn api_error(status: u16, value: &Value) -> String {
    let detail = value["errors"]
        .as_array()
        .and_then(|errors| errors.first())
        .and_then(|error| error["detail"].as_str().or(error["title"].as_str()))
        .unwrap_or(match status {
            401 => "Apple could not verify your API key. Check Apple settings.",
            403 => "This API key does not have permission to change these settings.",
            429 => "Apple's request limit was reached. Wait before trying again.",
            _ => "App Store Connect rejected the request.",
        });
    format!(
        "HTTP {status}: {}",
        detail.chars().take(600).collect::<String>()
    )
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
