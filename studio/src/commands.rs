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
