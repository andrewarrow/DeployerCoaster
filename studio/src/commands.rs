use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Command {
    OpenWorkspace,
    OpenPath(PathBuf),
    CreateWebsite { name: String, description: String },
    RewriteDescription { name: String, description: String },
    Save,
    SaveAs,
    CloseWorkspace,
    Quit,
    Settings,
    DynadotSettings,
    AppleSettings,
    Android,
    Apple,
    About,
    ToggleSidebar,
    ToggleInspector,
    ToggleActivity,
    ConfirmDiscard,
    CancelPending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingAction {
    Open,
    OpenPath(PathBuf),
    CloseWorkspace,
    Quit,
}
