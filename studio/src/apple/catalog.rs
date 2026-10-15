use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{AppleApp, AppleCredentials};

const CACHE_SECONDS: u64 = 24 * 60 * 60;
const MAX_RECENTS: usize = 10;

#[derive(Serialize, Deserialize)]
pub(super) struct AppList {
    pub fetched_at: u64,
    pub apps: Vec<AppleApp>,
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn is_fresh(fetched_at: u64, now: u64) -> bool {
    now.checked_sub(fetched_at)
        .is_some_and(|age| age < CACHE_SECONDS)
}

pub(super) fn account_key(credentials: &AppleCredentials) -> String {
    format!(
        "{:x}",
        Sha256::digest(credentials.sales_cache_account().as_bytes())
    )
}

pub(super) fn cache_path(credentials: &AppleCredentials) -> Option<PathBuf> {
    dirs::cache_dir().map(|root| {
        root.join("DeployerCoaster")
            .join("ios-apps")
            .join(format!("{}.json", account_key(credentials)))
    })
}

fn read_apps(path: &Path) -> Option<Vec<AppleApp>> {
    let apps: Vec<AppleApp> = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    apps.iter()
        .all(|app| super::api::validate_id(&app.id).is_ok())
        .then_some(apps)
}

fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("Invalid app list path")?)
        .map_err(|_| "Could not create the app list directory.")?;
    let bytes = serde_json::to_vec(value).map_err(|_| "Could not encode the app list.")?;
    crate::storage::atomic_write(path, &bytes).map_err(|_| "Could not save the app list.".into())
}

pub(super) fn load_or_fetch(
    path: Option<&Path>,
    now: u64,
    refresh: bool,
    fetch: impl FnOnce() -> Result<Vec<AppleApp>, String>,
) -> Result<AppList, String> {
    if !refresh
        && let Some(path) = path
        && let Ok(bytes) = fs::read(path)
        && let Ok(list) = serde_json::from_slice::<AppList>(&bytes)
        && is_fresh(list.fetched_at, now)
        && list
            .apps
            .iter()
            .all(|app| super::api::validate_id(&app.id).is_ok())
    {
        return Ok(list);
    }
    let list = AppList {
        fetched_at: now,
        apps: fetch()?,
    };
    if let Some(path) = path
        && let Err(error) = save(path, &list)
    {
        log::warn!("iOS app cache: {error}");
    }
    Ok(list)
}

pub(crate) fn recents_path(account: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|root| {
        root.join("DeployerCoaster")
            .join("recent-ios-apps")
            .join(format!("{account}.json"))
    })
}

pub(crate) fn load_recents(path: Option<&Path>) -> Vec<AppleApp> {
    let mut apps = path.and_then(read_apps).unwrap_or_default();
    let mut ids = std::collections::HashSet::new();
    apps.retain(|app| ids.insert(app.id.clone()));
    apps.truncate(MAX_RECENTS);
    apps
}

pub(crate) fn remember(
    recents: &mut Vec<AppleApp>,
    app: AppleApp,
    path: Option<&Path>,
) -> Result<(), String> {
    recents.retain(|recent| recent.id != app.id);
    recents.insert(0, app);
    recents.truncate(MAX_RECENTS);
    match path {
        Some(path) => save(path, recents),
        None => Err("Could not locate the settings directory to save recent apps.".into()),
    }
}

#[cfg(test)]
mod tests;
