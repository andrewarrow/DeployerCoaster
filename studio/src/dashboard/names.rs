use std::{fs, io::ErrorKind, path::PathBuf};

#[derive(Default)]
pub(super) struct Names {
    entries: Vec<String>,
    path: Option<PathBuf>,
    draft: Option<String>,
    focus_input: bool,
    error: Option<String>,
}

impl Names {
    pub(super) fn load() -> Self {
        match crate::storage::credential_path("names.json") {
            Ok(path) => Self::from_path(path),
            Err(error) => {
                log::warn!("Could not locate names storage: {error}");
                Self {
                    error: Some("Could not locate local storage for names.".into()),
                    ..Default::default()
                }
            }
        }
    }

    pub(super) fn from_path(path: PathBuf) -> Self {
        let result = match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<Vec<String>>(&bytes).map_err(|error| error.to_string())
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        };
        match result {
            Ok(mut entries) => {
                entries.sort_by_cached_key(|name| name.to_lowercase());
                Self {
                    entries,
                    path: Some(path),
                    ..Default::default()
                }
            }
            Err(error) => {
                log::warn!("Could not load {}: {error}", path.display());
                Self {
                    error: Some("Could not load saved names. Check the local names file and restart the app.".into()),
                    ..Default::default()
                }
            }
        }
    }

    fn add(&mut self) -> Result<(), String> {
        let name = self.draft.as_deref().unwrap_or_default().trim();
        if name.is_empty() {
            return Err("Enter a name.".into());
        }
        let key = name.to_lowercase();
        if self.entries.iter().any(|entry| entry.to_lowercase() == key) {
            return Err("This name is already in the list.".into());
        }
        let path = self
            .path
            .as_deref()
            .ok_or("Local storage is unavailable.")?;
        let mut entries = self.entries.clone();
        entries.push(name.to_owned());
        entries.sort_by_cached_key(|name| name.to_lowercase());
        let result = (|| {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let bytes = serde_json::to_vec_pretty(&entries).map_err(|error| error.to_string())?;
            crate::storage::atomic_write(path, &bytes)
        })();
        if let Err(error) = result {
            log::warn!("Could not save names: {error}");
            return Err(
                "Could not save the name locally. Check storage permissions and try again.".into(),
            );
        }
        self.entries = entries;
        self.draft = None;
        self.error = None;
        Ok(())
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut create_website = None;
        ui.horizontal_wrapped(|ui| {
            ui.heading("Names");
            if ui
                .add_enabled(
                    self.path.is_some() && self.draft.is_none(),
                    egui::Button::new("Add new name").min_size(egui::vec2(140.0, 44.0)),
                )
                .clicked()
            {
                self.draft = Some(String::new());
                self.focus_input = true;
                self.error = None;
            }
        });

        if let Some(draft) = &mut self.draft {
            ui.add_space(8.0);
            let label = ui.label("Name");
            let input = ui
                .add(
                    egui::TextEdit::singleline(draft)
                        .desired_width(ui.available_width().min(400.0)),
                )
                .labelled_by(label.id);
            if self.focus_input {
                input.request_focus();
                self.focus_input = false;
            }
            let mut submit =
                input.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let mut cancel =
                input.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape));
            ui.horizontal(|ui| {
                submit |= ui
                    .add_enabled(
                        !draft.trim().is_empty(),
                        egui::Button::new("Add").min_size(egui::vec2(72.0, 44.0)),
                    )
                    .clicked();
                cancel |= ui
                    .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 44.0)))
                    .clicked();
            });
            if cancel {
                self.draft = None;
                self.error = None;
            } else if submit && let Err(error) = self.add() {
                self.error = Some(error);
            }
        }

        if let Some(error) = &self.error {
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        }
        ui.add_space(8.0);
        ui.label(format!(
            "{} {}",
            self.entries.len(),
            if self.entries.len() == 1 {
                "name"
            } else {
                "names"
            }
        ));
        if self.entries.is_empty() {
            ui.label("You haven't added any names.");
            return None;
        }
        egui::ScrollArea::vertical()
            .id_salt("local_names")
            .show(ui, |ui| {
                let action_width = 132.0;
                let width = (ui.available_width() - action_width - ui.spacing().item_spacing.x)
                    .clamp(0.0, 600.0);
                egui::Grid::new("names_list")
                    .striped(true)
                    .min_row_height(44.0)
                    .show(ui, |ui| {
                        for name in &self.entries {
                            ui.add_sized([width, 44.0], egui::Label::new(name).truncate())
                                .on_hover_text(name);
                            if ui
                                .add_sized(
                                    [action_width, 44.0],
                                    egui::Button::new("Create website"),
                                )
                                .clicked()
                            {
                                create_website = Some(name.clone());
                            }
                            ui.end_row();
                        }
                    });
            });
        create_website
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty_and_persists_only_user_added_names() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings/names.json");
        let mut names = Names::from_path(path.clone());
        assert!(names.entries.is_empty());
        assert!(!path.exists());
        for name in ["  Zebra  ", "alpha"] {
            names.draft = Some(name.into());
            names.add().unwrap();
            assert!(names.draft.is_none());
        }
        let reopened = Names::from_path(path.clone());
        assert_eq!(reopened.entries, ["alpha", "Zebra"]);
        let original = fs::read(&path).unwrap();
        for invalid in ["  ", "ALPHA"] {
            names.draft = Some(invalid.into());
            assert!(names.add().is_err());
            assert_eq!(names.draft.as_deref(), Some(invalid));
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn storage_failures_preserve_input_and_existing_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        fs::write(&path, b"invalid json").unwrap();
        let mut names = Names::from_path(path.clone());
        assert!(names.error.is_some());
        names.draft = Some("Example".into());
        assert!(names.add().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid json");

        // Replace the storage parent with a file to simulate an unavailable disk.
        let parent = directory.path().join("blocked");
        let mut names = Names::from_path(parent.join("names.json"));
        fs::write(&parent, b"not a directory").unwrap();
        names.draft = Some("Example".into());
        assert!(names.add().is_err());
        assert!(names.entries.is_empty());
        assert_eq!(names.draft.as_deref(), Some("Example"));
    }
}
