use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crate::codex::{ChatGptAccount, CodexClient, CodexEvent, CodexWorkStatus};

const MODELS: &[&str] = &[
    "gpt-6-astra",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
];
const EFFORTS: &[&str] = &["medium", "high", "xhigh", "max"];

struct ChatMessage {
    user: bool,
    text: String,
}

pub(crate) struct Codex {
    client: Option<CodexClient>,
    root: PathBuf,
    account: Option<ChatGptAccount>,
    checking: bool,
    login_pending: bool,
    available: bool,
    ready: bool,
    opening: bool,
    busy: bool,
    cancelling: bool,
    activity: String,
    error: Option<String>,
    messages: Vec<ChatMessage>,
    streaming_message: Option<usize>,
    draft: String,
    sent_draft: Option<String>,
    model: &'static str,
    effort: &'static str,
    pub(crate) open: bool,
}

impl Codex {
    pub(crate) fn new(root: PathBuf) -> Self {
        let mut codex = Self::disconnected(root);
        codex.restart();
        codex
    }

    fn disconnected(root: PathBuf) -> Self {
        Self {
            client: None,
            root,
            account: None,
            checking: false,
            login_pending: false,
            available: true,
            ready: false,
            opening: false,
            busy: false,
            cancelling: false,
            activity: String::new(),
            error: None,
            messages: Vec::new(),
            streaming_message: None,
            draft: String::new(),
            sent_draft: None,
            model: "gpt-5.6-luna",
            effort: "medium",
            open: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_disconnected(root: PathBuf) -> Self {
        let mut codex = Self::disconnected(root);
        codex.available = false;
        codex
    }

    fn restart(&mut self) {
        // Stop the old process before opening a new conversation. Late responses
        // from a previous workspace must never appear in the current workspace.
        self.client = None;
        self.checking = true;
        self.login_pending = false;
        self.ready = false;
        self.opening = false;
        self.busy = false;
        self.cancelling = false;
        self.streaming_message = None;
        self.sent_draft = None;
        self.activity.clear();
        self.error = None;
        match CodexClient::new(&self.root) {
            Ok(client) => {
                self.client = Some(client);
                self.available = true;
            }
            Err(error) => self.unavailable(error),
        }
    }

    pub(crate) fn set_root(&mut self, root: &Path) {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
        if self.root != root {
            if let Some(draft) = self.sent_draft.take()
                && self.draft.is_empty()
            {
                self.draft = draft;
            }
            self.root = root;
            self.messages.clear();
            self.restart();
        }
    }

    /// A completed or interrupted turn may have written workspace files.
    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut changed = false;
        for _ in 0..24 {
            let Some(event) = self.client.as_ref().and_then(CodexClient::try_recv) else {
                break;
            };
            match event {
                CodexEvent::AccountStatus(account) => {
                    self.checking = false;
                    if !self.login_pending {
                        self.account = account;
                    }
                }
                CodexEvent::BrowserOpened => {
                    self.activity = "Finish signing in with ChatGPT in your browser.".into()
                }
                CodexEvent::LoginCompleted(account) => {
                    self.account = Some(account);
                    self.login_pending = false;
                    self.checking = false;
                    self.error = None;
                    self.activity.clear();
                }
                CodexEvent::ChatReady => {
                    self.ready = true;
                    self.opening = false;
                }
                CodexEvent::WorkStatus(status) => {
                    self.activity = match status {
                        CodexWorkStatus::Thinking => "Thinking…",
                        CodexWorkStatus::Editing => "Editing files…",
                        CodexWorkStatus::Checking => "Checking…",
                        CodexWorkStatus::Working => "Working…",
                    }
                    .into()
                }
                CodexEvent::ActivityDelta(delta) => {
                    self.activity.push_str(&delta);
                    // Keep the activity line bounded while preserving Unicode.
                    if self.activity.chars().count() > 300 {
                        self.activity = self
                            .activity
                            .chars()
                            .rev()
                            .take(300)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                    }
                }
                CodexEvent::AssistantDelta(delta) => {
                    let index = *self.streaming_message.get_or_insert_with(|| {
                        self.messages.push(ChatMessage {
                            user: false,
                            text: String::new(),
                        });
                        self.messages.len() - 1
                    });
                    self.messages[index].text.push_str(&delta);
                }
                CodexEvent::AssistantMessage(text) => {
                    if let Some(index) = self.streaming_message.take() {
                        self.messages[index].text = text;
                    } else if !text.is_empty() {
                        self.messages.push(ChatMessage { user: false, text });
                    }
                }
                CodexEvent::ChatTurnCompleted => {
                    self.finish_turn();
                    changed = true;
                }
                CodexEvent::ChatTurnCancelled => {
                    self.finish_turn();
                    self.activity = "Request cancelled.".into();
                    changed = true;
                }
                CodexEvent::CancelError(error) => {
                    self.cancelling = false;
                    self.error = Some(error);
                }
                CodexEvent::ChatError(error) => {
                    if let Some(draft) = self.sent_draft.take()
                        && self.draft.is_empty()
                    {
                        self.draft = draft;
                    }
                    self.finish_turn();
                    self.opening = false;
                    self.error = Some(error);
                    changed = true;
                }
                CodexEvent::Error(error) if self.busy => {
                    self.unavailable(error);
                    changed = true;
                }
                CodexEvent::Error(error) => {
                    self.login_pending = false;
                    self.checking = false;
                    self.error = Some(error);
                }
                CodexEvent::Unavailable(error) => self.unavailable(error),
            }
        }
        if self.open
            && self.account.is_some()
            && !self.ready
            && !self.opening
            && self.available
            && !self.checking
            && self.error.is_none()
        {
            match self
                .client
                .as_ref()
                .expect("available Codex has a client")
                .open_chat()
            {
                Ok(()) => self.opening = true,
                Err(error) => self.unavailable(error),
            }
        }
        if self.checking || self.login_pending || self.opening || self.busy {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        changed
    }

    fn finish_turn(&mut self) {
        self.busy = false;
        self.cancelling = false;
        self.streaming_message = None;
        self.sent_draft = None;
        self.activity.clear();
    }

    fn unavailable(&mut self, error: String) {
        if let Some(draft) = self.sent_draft.take()
            && self.draft.is_empty()
        {
            self.draft = draft;
        }
        self.finish_turn();
        self.available = false;
        self.account = None;
        self.checking = false;
        self.login_pending = false;
        self.opening = false;
        self.ready = false;
        self.error = Some(error);
    }

    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if let Some(account) = &self.account {
                if ui
                    .button(if self.open {
                        "Close Codex chat"
                    } else {
                        "Codex chat"
                    })
                    .clicked()
                {
                    self.open = !self.open;
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            account.email.as_deref().unwrap_or("ChatGPT connected"),
                        )
                        .weak(),
                    )
                    .truncate(),
                )
                .on_hover_text(account.plan_type.as_deref().unwrap_or("ChatGPT"));
            } else if self.checking || self.login_pending {
                ui.spinner();
                ui.label(if self.login_pending {
                    "Signing in with ChatGPT…"
                } else {
                    "Checking ChatGPT…"
                });
            } else if !self.available {
                if ui
                    .button("Retry Codex connection")
                    .on_hover_text(self.error.as_deref().unwrap_or("Codex is unavailable"))
                    .clicked()
                {
                    self.restart();
                }
            } else if ui.button("Connect ChatGPT").clicked() {
                self.begin_login();
            }
            if self.account.is_none() && !self.checking {
                if let Some(error) = &self.error {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(error).color(ui.visuals().error_fg_color),
                        )
                        .truncate(),
                    )
                    .on_hover_text(error);
                } else if self.login_pending {
                    ui.weak("Continue in your browser.");
                }
            }
        });
    }

    fn begin_login(&mut self) {
        if let Some(client) = &self.client {
            match client.begin_chatgpt_login() {
                Ok(()) => {
                    self.login_pending = true;
                    self.error = None;
                }
                Err(error) => self.unavailable(error),
            }
        }
    }

    pub(crate) fn settings_ui(&mut self, ui: &mut egui::Ui) {
        self.toolbar(ui);
        ui.add_space(8.0);
        ui.add(egui::Label::new("Codex manages your ChatGPT sign-in on this device.").wrap());
        if self.account.is_some()
            && !self.busy
            && ui.button("Sign in with another ChatGPT account").clicked()
        {
            self.messages.clear();
            self.restart();
            self.begin_login();
        }
    }

    pub(crate) fn chat_ui(&mut self, ui: &mut egui::Ui, workspace_path: Option<&Path>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Codex chat").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.open = false;
                }
                if ui
                    .add_enabled(
                        !self.busy && !self.checking && !self.login_pending,
                        egui::Button::new("New chat"),
                    )
                    .clicked()
                {
                    self.messages.clear();
                    self.restart();
                }
            });
        });
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("Working in {}", self.root.display()))
                    .small()
                    .weak(),
            )
            .truncate(),
        )
        .on_hover_text(self.root.display().to_string());
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Model")
                    .selected_text(self.model)
                    .width(136.0)
                    .show_ui(ui, |ui| {
                        for &model in MODELS {
                            ui.selectable_value(&mut self.model, model, model);
                        }
                    });
            });
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Reasoning")
                    .selected_text(self.effort)
                    .width(72.0)
                    .show_ui(ui, |ui| {
                        for &effort in EFFORTS {
                            ui.selectable_value(&mut self.effort, effort, effort);
                        }
                    });
            });
        });
        ui.add_space(8.0);
        // The composer stays visible as the conversation grows.
        let height = (ui.available_height() - 172.0).max(40.0);
        egui::ScrollArea::vertical()
            .id_salt("codex_messages")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .max_height(height)
            .show(ui, |ui| {
                if self.messages.is_empty() {
                    ui.label("Describe what you want to do in this workspace.");
                }
                for message in &self.messages {
                    ui.label(
                        egui::RichText::new(if message.user { "You" } else { "Codex" })
                            .small()
                            .strong(),
                    );
                    ui.add(egui::Label::new(&message.text).wrap().selectable(true));
                    ui.add_space(12.0);
                }
            });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if self.busy {
                ui.spinner();
                ui.label(if self.cancelling {
                    "Stopping Codex…"
                } else {
                    "Codex is working…"
                });
                if ui
                    .add_enabled(!self.cancelling, egui::Button::new("Cancel"))
                    .clicked()
                {
                    match self
                        .client
                        .as_ref()
                        .expect("active Codex has a client")
                        .cancel_chat_message()
                    {
                        Ok(()) => self.cancelling = true,
                        Err(error) => self.error = Some(error),
                    }
                }
            } else if self.opening || self.checking {
                ui.spinner();
                ui.label("Starting Codex…");
            } else if !self.ready && self.available && ui.button("Retry chat").clicked() {
                self.error = None;
            }
        });
        if !self.activity.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(&self.activity).small().weak()).truncate())
                .on_hover_text(&self.activity);
        }
        if let Some(error) = &self.error {
            ui.add(
                egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                    .truncate(),
            )
            .on_hover_text(error);
            if !self.available && ui.button("Retry connection").clicked() {
                self.restart();
            }
        }
        ui.label("Message");
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.draft)
                .id_salt("codex_draft")
                .desired_width(f32::INFINITY)
                .desired_rows(2)
                .hint_text("Ask Codex…"),
        );
        let shortcut = response.has_focus()
            && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Enter));
        let can_send =
            self.ready && self.account.is_some() && !self.busy && !self.draft.trim().is_empty();
        let send = ui
            .add_enabled(
                can_send,
                egui::Button::new("Send").min_size(egui::vec2(64.0, 36.0)),
            )
            .on_hover_text("Send (⌘/Ctrl+Enter)")
            .clicked();
        if can_send && (send || shortcut) {
            self.send(workspace_path);
        }
    }

    fn send(&mut self, workspace_path: Option<&Path>) {
        let text = self.draft.trim().to_owned();
        let message = contextual_message(&text, workspace_path);
        match self
            .client
            .as_ref()
            .expect("ready Codex has a client")
            .send_chat_message(message, self.model.into(), self.effort.into())
        {
            Ok(()) => {
                self.messages.push(ChatMessage { user: true, text });
                self.sent_draft = Some(std::mem::take(&mut self.draft));
                self.busy = true;
                self.error = None;
                self.activity.clear();
            }
            Err(error) => self.unavailable(error),
        }
    }
}

fn contextual_message(text: &str, workspace_path: Option<&Path>) -> String {
    let context = workspace_path.map(|path| format!("The open workspace document is {}. It is a JSON .dcstudio file. Preserve its format and unknown fields.", path.display())).unwrap_or_else(|| "No workspace document is open.".into());
    format!(
        "You are assisting inside DeployerCoaster Studio, an app for managing apps, websites, domains, and deployments. {context}\n\nUser request:\n{text}"
    )
}

#[cfg(test)]
#[path = "codex_ui/tests.rs"]
mod tests;
