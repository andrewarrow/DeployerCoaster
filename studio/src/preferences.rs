use crate::storage::atomic_write;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn theme(self) -> egui::ThemePreference {
        match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub appearance: Appearance,
    pub show_sidebar: bool,
    pub show_inspector: bool,
    pub show_activity: bool,
    pub recent_workspaces: Vec<PathBuf>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            show_sidebar: true,
            show_inspector: true,
            show_activity: false,
            recent_workspaces: Vec::new(),
        }
    }
}

impl Preferences {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|root| root.join("DeployerCoaster").join("preferences.json"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("Could not load preferences: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("Could not read preferences: {error}")),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        atomic_write(path, &bytes)
    }

    pub fn remember(&mut self, path: PathBuf) {
        self.recent_workspaces.retain(|entry| entry != &path);
        self.recent_workspaces.insert(0, path);
        self.recent_workspaces.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_workspaces_are_unique_ordered_and_bounded() {
        let mut preferences = Preferences::default();
        for index in 0..15 {
            preferences.remember(format!("{index}.dcstudio").into());
        }
        preferences.remember("9.dcstudio".into());
        assert_eq!(preferences.recent_workspaces.len(), 10);
        assert_eq!(
            preferences.recent_workspaces[0],
            PathBuf::from("9.dcstudio")
        );
        assert_eq!(
            preferences
                .recent_workspaces
                .iter()
                .filter(|path| **path == PathBuf::from("9.dcstudio"))
                .count(),
            1
        );
    }
}
