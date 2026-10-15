use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Command {
    OpenWorkspace,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    CloseWorkspace,
    Quit,
    Settings,
    DynadotSettings,
    AppleSettings,
    Wizard,
    Android,
    Apple,
    Sales,
    About,
    ToggleSidebar,
    ToggleInspector,
    ToggleActivity,
    ConfirmDiscard,
    CancelPending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingAction {
    OpenPath(PathBuf),
    Quit,
}
