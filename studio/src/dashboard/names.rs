use std::{collections::HashMap, fs, io::ErrorKind, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct NameEntry {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    rewritten_description: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NamesFile {
    Entries(Vec<NameEntry>),
    Legacy(Vec<String>),
}

impl NamesFile {
    fn into_entries(self) -> Vec<NameEntry> {
        match self {
            Self::Entries(entries) => entries,
            Self::Legacy(names) => names
                .into_iter()
                .map(|name| NameEntry {
                    name,
                    description: String::new(),
                    rewritten_description: String::new(),
                })
                .collect(),
        }
    }
}

#[derive(Default)]
pub(super) struct Names {
    entries: Vec<NameEntry>,
    path: Option<PathBuf>,
    draft: Option<String>,
    description_drafts: HashMap<String, String>,
    description_messages: HashMap<String, String>,
    rewritten_drafts: HashMap<String, (String, String)>,
    rewritten_messages: HashMap<String, String>,
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
            Ok(bytes) => serde_json::from_slice::<NamesFile>(&bytes)
                .map(NamesFile::into_entries)
                .map_err(|error| error.to_string()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        };
        match result {
            Ok(mut entries) => {
                entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
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

    fn persist(&self, entries: &[NameEntry]) -> Result<(), String> {
        let path = self
            .path
            .as_deref()
            .ok_or("Local storage is unavailable.")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(entries).map_err(|error| error.to_string())?;
        crate::storage::atomic_write(path, &bytes)
    }

    fn add(&mut self) -> Result<(), String> {
        let name = self.draft.as_deref().unwrap_or_default().trim();
        if name.is_empty() {
            return Err("Enter a name.".into());
        }
        let key = name.to_lowercase();
        if self
            .entries
            .iter()
            .any(|entry| entry.name.to_lowercase() == key)
        {
            return Err("This name is already in the list.".into());
        }
        let mut entries = self.entries.clone();
        entries.push(NameEntry {
            name: name.to_owned(),
            description: String::new(),
            rewritten_description: String::new(),
        });
        entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
        if let Err(error) = self.persist(&entries) {
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

    fn save_description(&mut self, name: &str) -> Result<(), String> {
        let description = self
            .description_drafts
            .get(name)
            .cloned()
            .unwrap_or_else(|| {
                self.entries
                    .iter()
                    .find(|entry| entry.name == name)
                    .map(|entry| entry.description.clone())
                    .unwrap_or_default()
            });
        let mut entries = self.entries.clone();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.name == name)
            .ok_or("This name is no longer available.")?;
        entry.description = description;
        if let Err(error) = self.persist(&entries) {
            log::warn!("Could not save description for {name}: {error}");
            return Err(
                "Could not save the description. Check storage permissions and try again.".into(),
            );
        }
        self.entries = entries;
        self.description_drafts.remove(name);
        self.description_messages
            .insert(name.to_owned(), "Description saved.".into());
        Ok(())
    }

    pub(super) fn receive_rewritten_description(
        &mut self,
        name: &str,
        original: &str,
        rewritten: &str,
    ) -> Result<(), String> {
        crate::description::validate_rewritten_description(name, rewritten)?;
        let Some(current) = self.entries.iter().find(|entry| entry.name == name) else {
            let error =
                "This name is no longer available; the rewritten description was not saved.";
            self.rewritten_messages
                .insert(name.to_owned(), error.into());
            return Err(error.into());
        };
        if current.description != original {
            let error = "The original description changed while rewriting. Save the current original and rewrite it again.";
            self.rewritten_messages
                .insert(name.to_owned(), error.into());
            return Err(error.into());
        }

        self.rewritten_drafts
            .insert(name.to_owned(), (original.to_owned(), rewritten.to_owned()));
        self.save_rewritten_description(name)
    }

    fn save_rewritten_description(&mut self, name: &str) -> Result<(), String> {
        let (original, rewritten) = self
            .rewritten_drafts
            .get(name)
            .cloned()
            .ok_or("There is no rewritten description to save.")?;
        let mut entries = self.entries.clone();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.name == name)
            .ok_or("This name is no longer available.")?;
        if entry.description != original {
            let error = "The original description changed while rewriting. Save the current original and rewrite it again.";
            self.rewritten_messages
                .insert(name.to_owned(), error.into());
            return Err(error.into());
        }
        entry.rewritten_description = rewritten;
        if let Err(error) = self.persist(&entries) {
            log::warn!("Could not save rewritten description for {name}: {error}");
            let message = "Could not save the rewritten description. Check storage permissions and try again.";
            self.rewritten_messages
                .insert(name.to_owned(), message.into());
            return Err(message.into());
        }
        self.entries = entries;
        self.rewritten_drafts.remove(name);
        self.rewritten_messages.remove(name);
        Ok(())
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) -> Option<crate::commands::Command> {
        use crate::commands::Command;

        let mut command = None;
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
        let mut save_description = None;
        let mut retry_rewritten = None;
        let content_width = ui.available_width().min(740.0);
        egui::ScrollArea::vertical()
            .id_salt("local_names")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(content_width);
                for entry in &self.entries {
                    let name = &entry.name;
                    let description = &entry.description;
                    let rewritten_description = &entry.rewritten_description;
                    let slug = name.to_lowercase().replace(' ', "-");
                    let url = format!("https://andrewarrow.dev/{slug}/");
                    let privacy_url = format!("{url}privacy/");
                    let dirty = self
                        .description_drafts
                        .get(name)
                        .is_some_and(|draft| draft != description);

                    let row_width =
                        (ui.available_width().min(content_width) - 140.0).clamp(0.0, 600.0);
                    egui::Grid::new(("names_list_row", name))
                        .min_row_height(60.0)
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(row_width, 60.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                    ui.add(egui::Label::new(name).truncate())
                                        .on_hover_text(name);
                                },
                            );
                            if ui
                                .add_enabled(
                                    !dirty && !rewritten_description.trim().is_empty(),
                                    egui::Button::new("Create website")
                                        .min_size([132.0, 60.0].into()),
                                )
                                .on_disabled_hover_text(
                                    if dirty {
                                        "Save the description before creating the website."
                                    } else {
                                        "Rewrite the description before creating the website."
                                    },
                                )
                                .clicked()
                            {
                                command = Some(Command::CreateWebsite {
                                    name: name.clone(),
                                    description: rewritten_description.clone(),
                                });
                            }
                            ui.end_row();
                        });
                    for (label, url) in [("website", &url), ("privacy", &privacy_url)] {
                        ui.push_id((name, label), |ui| {
                            ui.horizontal(|ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                let link_width =
                                    (ui.available_width() - 72.0 - ui.spacing().item_spacing.x)
                                        .max(0.0);
                                ui.add_sized(
                                    [link_width, 44.0],
                                    egui::Hyperlink::from_label_and_url(url, url),
                                )
                                .on_hover_text(url);
                                if ui
                                    .add(egui::Button::new("Copy").min_size([72.0, 44.0].into()))
                                    .on_hover_text(format!("Copy {label} URL"))
                                    .clicked()
                                {
                                    ui.ctx().copy_text(url.clone());
                                }
                            });
                        });
                    }
                    egui::CollapsingHeader::new("Description")
                        .id_salt(("name_description", name))
                        .show(ui, |ui| {
                            let draft = self
                                .description_drafts
                                .entry(name.clone())
                                .or_insert_with(|| description.clone());
                            let label = ui.label("Original description");
                            let response = egui::ScrollArea::vertical()
                                .id_salt(("description_editor", name))
                                .max_height(160.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(draft)
                                            .id_salt(("description_text", name))
                                            .desired_width(ui.available_width())
                                            .desired_rows(5)
                                            .hint_text("Describe the product, audience, tone, and useful details."),
                                    )
                                    .labelled_by(label.id)
                                })
                                .inner;
                            let changed = response.changed();
                            let dirty = draft != description;
                            if changed {
                                self.description_messages.remove(name);
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add_enabled(
                                        dirty && self.path.is_some(),
                                        egui::Button::new("Save description")
                                            .min_size([150.0, 44.0].into()),
                                    )
                                    .clicked()
                                {
                                    save_description = Some(name.clone());
                                }
                                if dirty {
                                    ui.label("Unsaved changes");
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add_enabled(
                                        !description.trim().is_empty()
                                            && !dirty
                                            && self.path.is_some(),
                                        egui::Button::new("Rewrite description")
                                            .min_size([176.0, 44.0].into()),
                                    )
                                    .on_disabled_hover_text(if dirty {
                                        "Save the original description before rewriting it."
                                    } else {
                                        "Save a non-empty original description before rewriting it."
                                    })
                                    .clicked()
                                {
                                    command = Some(Command::RewriteDescription {
                                        name: name.clone(),
                                        description: description.clone(),
                                    });
                                }
                                if dirty {
                                    ui.label("Save changes before rewriting.");
                                }
                            });
                            if let Some(message) = self.description_messages.get(name) {
                                let color = if message.starts_with("Could not") {
                                    ui.visuals().error_fg_color
                                } else {
                                    ui.visuals().weak_text_color()
                                };
                                ui.add(
                                    egui::Label::new(egui::RichText::new(message).color(color))
                                        .wrap(),
                                );
                            }
                            ui.add_space(8.0);
                            ui.label("Rewritten description");
                            let pending_rewrite = self.rewritten_drafts.get(name).cloned();
                            let shown_rewrite = pending_rewrite
                                .as_ref()
                                .map(|(_, text)| text.as_str())
                                .unwrap_or(rewritten_description);
                            let pending_matches_source = pending_rewrite
                                .as_ref()
                                .is_some_and(|(pending_original, _)| pending_original == description);
                            if shown_rewrite.is_empty() {
                                ui.label("No rewritten description yet.");
                            } else {
                                ui.label(format!(
                                    "{} / 3999 characters",
                                    shown_rewrite.chars().count()
                                ));
                                egui::ScrollArea::vertical()
                                    .id_salt(("rewritten_description", name))
                                    .max_height(160.0)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.add(
                                            egui::Label::new(shown_rewrite)
                                                .selectable(true)
                                                .wrap(),
                                        );
                                    });
                                if pending_matches_source {
                                    if ui
                                        .add_enabled(
                                            self.path.is_some(),
                                            egui::Button::new("Retry save")
                                                .min_size([112.0, 44.0].into()),
                                        )
                                        .clicked()
                                    {
                                        retry_rewritten = Some(name.clone());
                                    }
                                } else if pending_rewrite.is_some() {
                                    ui.label(
                                        "The original changed. Rewrite it again to save a matching copy.",
                                    );
                                }
                            }
                            if let Some(message) = self.rewritten_messages.get(name) {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(message)
                                            .color(ui.visuals().error_fg_color),
                                    )
                                    .wrap(),
                                );
                            }
                        });
                    ui.add_space(4.0);
                }
            });
        if let Some(name) = save_description {
            if let Err(error) = self.save_description(&name) {
                self.description_messages.insert(name, error);
            }
        }
        if let Some(name) = retry_rewritten
            && let Err(error) = self.save_rewritten_description(&name)
        {
            self.rewritten_messages.insert(name, error);
        }
        command
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
        assert_eq!(
            reopened
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "Zebra"]
        );
        let original = fs::read(&path).unwrap();
        for invalid in ["  ", "ALPHA"] {
            names.draft = Some(invalid.into());
            assert!(names.add().is_err());
            assert_eq!(names.draft.as_deref(), Some(invalid));
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn reads_legacy_names_and_saves_large_multiline_unicode_descriptions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        fs::write(&path, br#"["Example"]"#).unwrap();
        let mut names = Names::from_path(path.clone());
        assert_eq!(names.entries[0].name, "Example");
        assert_eq!(names.entries[0].description, "");
        assert_eq!(names.entries[0].rewritten_description, "");

        let description = format!(
            "Product brief — café 🪴\n{}",
            "Long pasted detail. ".repeat(500)
        );
        names
            .description_drafts
            .insert("Example".into(), description.clone());
        names.save_description("Example").unwrap();
        let reopened = Names::from_path(path);
        assert_eq!(reopened.entries[0].description, description);
        assert_eq!(reopened.entries[0].rewritten_description, "");
    }

    #[test]
    fn loads_old_object_records_and_round_trips_rewritten_descriptions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        fs::write(
            &path,
            br#"[{"name":"Example","description":"Original brief"}]"#,
        )
        .unwrap();
        let mut names = Names::from_path(path.clone());
        assert_eq!(names.entries[0].rewritten_description, "");
        names
            .receive_rewritten_description(
                "Example",
                "Original brief",
                "A clear rewritten brief for Example.",
            )
            .unwrap();
        let reopened = Names::from_path(path);
        assert_eq!(reopened.entries[0].description, "Original brief");
        assert_eq!(
            reopened.entries[0].rewritten_description,
            "A clear rewritten brief for Example."
        );
    }

    #[test]
    fn changing_or_clearing_original_keeps_rewritten_copy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        let mut names = Names::from_path(path.clone());
        names.draft = Some("Example".into());
        names.add().unwrap();
        names
            .description_drafts
            .insert("Example".into(), "Original for Example".into());
        names.save_description("Example").unwrap();
        names
            .receive_rewritten_description(
                "Example",
                "Original for Example",
                "Rewritten for Example.",
            )
            .unwrap();
        names
            .description_drafts
            .insert("Example".into(), "Updated original".into());
        names.save_description("Example").unwrap();
        assert_eq!(
            names.entries[0].rewritten_description,
            "Rewritten for Example."
        );
        names
            .description_drafts
            .insert("Example".into(), String::new());
        names.save_description("Example").unwrap();
        let reopened = Names::from_path(path);
        assert_eq!(reopened.entries[0].description, "");
        assert_eq!(
            reopened.entries[0].rewritten_description,
            "Rewritten for Example."
        );
    }

    #[test]
    fn failed_rewrite_save_keeps_result_for_retry_and_preserves_saved_copy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        let mut names = Names::from_path(path.clone());
        names.draft = Some("Example".into());
        names.add().unwrap();
        names
            .description_drafts
            .insert("Example".into(), "Original for Example".into());
        names.save_description("Example").unwrap();
        names
            .receive_rewritten_description(
                "Example",
                "Original for Example",
                "Previous copy for Example.",
            )
            .unwrap();
        let saved = fs::read(&path).unwrap();
        names.path = Some(directory.path().join("blocked/names.json"));
        fs::write(directory.path().join("blocked"), b"not a directory").unwrap();
        assert!(
            names
                .receive_rewritten_description(
                    "Example",
                    "Original for Example",
                    "New returned copy for Example.",
                )
                .is_err()
        );
        assert_eq!(
            names.rewritten_drafts["Example"].1,
            "New returned copy for Example."
        );
        assert_eq!(names.entries[0].description, "Original for Example");
        assert_eq!(
            names.entries[0].rewritten_description,
            "Previous copy for Example."
        );
        assert_eq!(fs::read(path).unwrap(), saved);
    }

    #[test]
    fn stale_rewrite_is_rejected_and_rewrites_stay_with_each_name() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        let mut names = Names::from_path(path.clone());
        for name in ["Alpha", "Beta"] {
            names.draft = Some(name.into());
            names.add().unwrap();
            names
                .description_drafts
                .insert(name.into(), format!("Original for {name}"));
            names.save_description(name).unwrap();
        }
        assert!(
            names
                .receive_rewritten_description(
                    "Alpha",
                    "Outdated original",
                    "Stale rewritten text for Alpha.",
                )
                .is_err()
        );
        assert!(!names.rewritten_drafts.contains_key("Alpha"));
        names
            .receive_rewritten_description(
                "Alpha",
                "Original for Alpha",
                "Rewritten text for Alpha.",
            )
            .unwrap();
        names
            .receive_rewritten_description("Beta", "Original for Beta", "Rewritten text for Beta.")
            .unwrap();
        let reopened = Names::from_path(path);
        let copies: HashMap<_, _> = reopened
            .entries
            .into_iter()
            .map(|entry| (entry.name, entry.rewritten_description))
            .collect();
        assert_eq!(copies["Alpha"], "Rewritten text for Alpha.");
        assert_eq!(copies["Beta"], "Rewritten text for Beta.");
    }

    #[test]
    fn rewritten_description_validation_counts_unicode_characters() {
        let below = format!("Example{}", "🌿".repeat(3992));
        assert!(crate::description::validate_rewritten_description("Example", &below).is_ok());
        let at_limit = format!("Example{}", "🌿".repeat(3993));
        assert!(crate::description::validate_rewritten_description("Example", &at_limit).is_err());
    }

    #[test]
    fn failed_description_save_preserves_draft_and_existing_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        let mut names = Names::from_path(path.clone());
        names.draft = Some("Example".into());
        names.add().unwrap();
        let original = fs::read(&path).unwrap();
        names
            .description_drafts
            .insert("Example".into(), "Keep this draft".into());
        names.path = Some(directory.path().join("blocked/names.json"));
        fs::write(directory.path().join("blocked"), b"not a directory").unwrap();
        assert!(names.save_description("Example").is_err());
        assert_eq!(
            names.description_drafts.get("Example").unwrap(),
            "Keep this draft"
        );
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(names.entries[0].description, "");
    }

    #[test]
    fn descriptions_stay_with_their_names_and_can_be_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("names.json");
        let mut names = Names::from_path(path.clone());
        for name in ["Alpha", "Beta"] {
            names.draft = Some(name.into());
            names.add().unwrap();
        }
        names
            .description_drafts
            .insert("Alpha".into(), "Alpha brief".into());
        names.save_description("Alpha").unwrap();
        names
            .description_drafts
            .insert("Beta".into(), "Beta brief".into());
        names.save_description("Beta").unwrap();
        names
            .description_drafts
            .insert("Alpha".into(), String::new());
        names.save_description("Alpha").unwrap();

        let reopened = Names::from_path(path);
        let descriptions: HashMap<_, _> = reopened
            .entries
            .into_iter()
            .map(|entry| (entry.name, entry.description))
            .collect();
        assert_eq!(descriptions["Alpha"], "");
        assert_eq!(descriptions["Beta"], "Beta brief");
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
