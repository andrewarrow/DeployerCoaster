use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::storage::atomic_write;

pub const WORKSPACE_EXTENSION: &str = "dcstudio";
const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceDocument {
    pub format_version: u32,
    pub name: String,
    /// Reserved for the new app's domain model; no game schema is carried over.
    #[serde(default)]
    pub data: Map<String, Value>,
    // Preserve additions made by other versions instead of silently deleting them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl WorkspaceDocument {
    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != FORMAT_VERSION {
            return Err(format!(
                "Workspace format {} is unsupported. This app supports format {FORMAT_VERSION}.",
                self.format_version
            ));
        }
        if self.name.trim().is_empty() {
            return Err("Give the workspace a name before saving.".to_owned());
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        atomic_write(path, &bytes)
    }
}

#[derive(Debug)]
pub struct Workspace {
    pub document: WorkspaceDocument,
    pub path: Option<PathBuf>,
    saved: Option<WorkspaceDocument>,
}

impl Workspace {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            document: WorkspaceDocument {
                format_version: FORMAT_VERSION,
                name: "Untitled".to_owned(),
                data: Map::new(),
                extra: Map::new(),
            },
            path: None,
            saved: None,
        }
    }

    pub fn open(path: PathBuf) -> Result<Self, String> {
        let bytes = fs::read(&path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let document: WorkspaceDocument = serde_json::from_slice(&bytes)
            .map_err(|error| format!("This file is not a valid workspace: {error}"))?;
        document.validate()?;
        Ok(Self {
            saved: Some(document.clone()),
            document,
            path: Some(path),
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.document)
    }

    pub fn mark_saved(&mut self, path: PathBuf, document: WorkspaceDocument) {
        self.path = Some(path);
        self.saved = Some(document);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_domain_data_and_unknown_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.dcstudio");
        let mut workspace = Workspace::new();
        workspace.document.data.insert(
            "nested".into(),
            serde_json::json!({"values": [1, "hello", null]}),
        );
        workspace
            .document
            .extra
            .insert("future_metadata".into(), serde_json::json!(true));
        workspace.document.save(&path).unwrap();
        let reopened = Workspace::open(path).unwrap();
        assert_eq!(reopened.document, workspace.document);
        assert!(!reopened.is_dirty());
    }

    #[test]
    fn dirty_state_tracks_the_saved_snapshot_including_edits_during_save() {
        let mut workspace = Workspace::new();
        assert!(workspace.is_dirty());
        let saved = workspace.document.clone();
        workspace.document.name = "Changed during save".into();
        workspace.mark_saved("test.dcstudio".into(), saved.clone());
        assert!(workspace.is_dirty());
        workspace.document = saved;
        assert!(!workspace.is_dirty());
    }

    #[test]
    fn invalid_document_does_not_replace_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.dcstudio");
        let mut workspace = Workspace::new();
        workspace.document.save(&path).unwrap();
        let original = fs::read(&path).unwrap();
        workspace.document.name = "  ".into();
        assert!(workspace.document.save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        workspace.document.name = "Valid".into();
        workspace.document.format_version = FORMAT_VERSION + 1;
        assert!(workspace.document.save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn repeated_saves_replace_the_destination() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.dcstudio");
        let mut workspace = Workspace::new();
        workspace.document.save(&path).unwrap();
        workspace.document.name = "Renamed".into();
        workspace.document.save(&path).unwrap();
        assert_eq!(Workspace::open(path).unwrap().document.name, "Renamed");
    }
}
