use serde_json::Value;

use super::{AppDetails, Document, attr};

fn blank(resource: &Value, field: &str) -> bool {
    match resource.get(field) {
        None | Some(Value::Null) => true,
        Some(Value::String(value)) => value.trim().is_empty(),
        _ => false,
    }
}

impl Document {
    // Inspect fetched values only. Unknown/unavailable sections must not appear incomplete.
    pub(super) fn missing_data(self, details: &AppDetails) -> Option<&'static str> {
        let failed = |prefix| details.warnings.iter().any(|w| w.starts_with(prefix));
        let info_known = !failed("App information:");
        let info_locales_known = info_known && !failed("App localizations:");
        let version_known = !failed("iOS version:");
        let version_locales_known = version_known && !failed("Version localizations:");
        let locales = || {
            std::iter::once(attr(&details.app, "primaryLocale"))
                .chain(details.info_localizations.iter().map(|v| attr(v, "locale")))
                .chain(
                    details
                        .version_localizations
                        .iter()
                        .map(|v| attr(v, "locale")),
                )
                .filter(|locale| !locale.is_empty())
        };
        match self {
            Self::Information => {
                if ["name", "bundleId", "sku", "primaryLocale"]
                    .iter()
                    .any(|f| blank(&details.app, f))
                {
                    return Some("Missing app identity information");
                }
                if info_known
                    && details
                        .info
                        .as_ref()
                        .is_none_or(|v| blank(v, "primaryCategoryId"))
                {
                    return Some("Missing primary category");
                }
            }
            Self::Listing => {
                if info_locales_known
                    && (details.info_localizations.is_empty()
                        || locales().any(|locale| {
                            details
                                .info_localizations
                                .iter()
                                .find(|v| attr(v, "locale") == locale)
                                .is_none_or(|v| blank(v, "name"))
                        }))
                {
                    return Some("Missing app name in one or more languages");
                }
                if version_locales_known
                    && (details.version_localizations.is_empty()
                        || locales().any(|locale| {
                            details
                                .version_localizations
                                .iter()
                                .find(|v| attr(v, "locale") == locale)
                                .is_none_or(|v| {
                                    ["description", "keywords", "supportUrl"]
                                        .iter()
                                        .any(|f| blank(v, f))
                                })
                        }))
                {
                    return Some(
                        "Missing description, keywords, or support URL in one or more languages",
                    );
                }
            }
            Self::Versions => {
                if version_known
                    && details
                        .version
                        .as_ref()
                        .is_none_or(|v| ["versionString", "copyright"].iter().any(|f| blank(v, f)))
                {
                    return Some("Missing iOS version or copyright");
                }
            }
            Self::Privacy => {
                if blank(&details.app, "contentRightsDeclaration") {
                    return Some("Missing content rights declaration");
                }
                if info_locales_known
                    && (details.info_localizations.is_empty()
                        || locales().any(|locale| {
                            details
                                .info_localizations
                                .iter()
                                .find(|v| attr(v, "locale") == locale)
                                .is_none_or(|v| blank(v, "privacyPolicyUrl"))
                        }))
                {
                    return Some("Missing privacy policy URL in one or more languages");
                }
            }
            Self::AgeRating if info_known && !failed("Age rating:") => {
                let Some(rating) = &details.age_rating else {
                    return Some("Missing age rating declaration");
                };
                // Overrides, kids age bands, and regional/extra information are optional.
                if [
                    "advertising",
                    "alcoholTobaccoOrDrugUseOrReferences",
                    "contests",
                    "gambling",
                    "gamblingSimulated",
                    "gunsOrOtherWeapons",
                    "healthOrWellnessTopics",
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
                ]
                .iter()
                .any(|field| blank(rating, field))
                {
                    return Some("Missing age rating questionnaire answers");
                }
            }
            Self::Review if version_known => match &details.review_contact {
                Some(Ok(None)) => return Some("Missing review contact"),
                Some(Ok(Some(contact)))
                    if [
                        "contactFirstName",
                        "contactLastName",
                        "contactPhone",
                        "contactEmail",
                    ]
                    .iter()
                    .any(|field| blank(contact, field)) =>
                {
                    return Some("Missing review contact information");
                }
                _ => {}
            },
            _ => {}
        }
        None
    }
}
