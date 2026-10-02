use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ConsoleApp {
    pub display_name: String,
    pub icon_url: Option<String>,
    pub package_name: String,
}

/// Parses public metadata already supplied by the caller; this module does not perform requests.
pub(crate) fn parse_console_apps_json(input: &str) -> Result<Vec<ConsoleApp>, String> {
    let value: Value = serde_json::from_str(input)
        .map_err(|_| "The Play Console app metadata is not valid JSON.".to_owned())?;
    parse_console_apps(&value)
}

pub(crate) fn parse_console_apps(value: &Value) -> Result<Vec<ConsoleApp>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "The Play Console app metadata has an unsupported format.".to_owned())?;

    if object.contains_key("apps") || object.contains_key("developer_id") {
        return parse_bridge_apps(object);
    }

    let apps = object
        .get("1")
        .and_then(Value::as_array)
        .ok_or_else(|| "The Play Console app metadata has an unsupported format.".to_owned())?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for app in apps {
        let app = app
            .as_object()
            .ok_or_else(|| "The Play Console app metadata contains an invalid app.".to_owned())?;
        let package_name = required_string(app.get("5"))?;
        if !seen.insert(package_name.clone()) {
            continue;
        }
        result.push(ConsoleApp {
            display_name: optional_string(app.get("2")).unwrap_or_default(),
            icon_url: valid_icon_url(optional_string(app.get("3"))),
            package_name,
        });
    }
    Ok(result)
}

fn parse_bridge_apps(object: &serde_json::Map<String, Value>) -> Result<Vec<ConsoleApp>, String> {
    let apps = object
        .get("apps")
        .and_then(Value::as_array)
        .ok_or_else(|| "The Play Console app metadata has an unsupported format.".to_owned())?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for app in apps {
        let app = app
            .as_object()
            .ok_or_else(|| "The Play Console app metadata contains an invalid app.".to_owned())?;
        let package_name = required_string(app.get("package_name"))?;
        if !seen.insert(package_name.clone()) {
            continue;
        }
        result.push(ConsoleApp {
            display_name: optional_string(app.get("display_name")).unwrap_or_default(),
            icon_url: valid_icon_url(optional_string(app.get("icon_url"))),
            package_name,
        });
    }
    Ok(result)
}

fn required_string(value: Option<&Value>) -> Result<String, String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "The Play Console app metadata contains an invalid package name.".to_owned())
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn valid_icon_url(value: Option<String>) -> Option<String> {
    value.filter(|url| crate::app_icons::validate_google_artwork_url(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numeric_console_fields_and_deduplicates_packages() {
        let apps = parse_console_apps_json(
            r#"{"1":[
                {"2":"TrixWTF","3":"https://lh3.googleusercontent.com/icon","5":"wtf.trix.app"},
                {"2":"Duplicate","5":"wtf.trix.app"},
                {"5":"other.app"}
            ]}"#,
        )
        .unwrap();

        assert_eq!(
            apps,
            vec![
                ConsoleApp {
                    display_name: "TrixWTF".into(),
                    icon_url: Some("https://lh3.googleusercontent.com/icon".into()),
                    package_name: "wtf.trix.app".into(),
                },
                ConsoleApp {
                    display_name: String::new(),
                    icon_url: None,
                    package_name: "other.app".into(),
                },
            ]
        );
    }

    #[test]
    fn parses_bridge_snapshots_and_ignores_invalid_icon_urls() {
        let apps = parse_console_apps_json(
            r#"{"developer_id":"123","apps":[
                {"display_name":"Bridge app","package_name":"example.app","icon_url":"http://lh3.googleusercontent.com/icon"},
                {"package_name":"second.app","icon_url":"https://attacker.test/icon"}
            ]}"#,
        )
        .unwrap();

        assert_eq!(apps[0].display_name, "Bridge app");
        assert_eq!(apps[0].icon_url, None);
        assert_eq!(apps[1].package_name, "second.app");
        assert_eq!(apps[1].display_name, "");
        assert_eq!(apps[1].icon_url, None);
    }

    #[test]
    fn rejects_malformed_top_level_and_required_package_fields_safely() {
        assert!(parse_console_apps_json(r#"{"unexpected":[]}"#).is_err());
        let error = parse_console_apps_json(r#"{"1":[{"2":"No package"}]}"#).unwrap_err();
        assert_eq!(
            error,
            "The Play Console app metadata contains an invalid package name."
        );
        assert!(!error.contains("No package"));
    }
}
