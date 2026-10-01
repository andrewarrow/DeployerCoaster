pub const APP_NAME: &str = "DeployerCoaster";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_SHA: &str = env!("DEPLOYERCOASTER_GIT_SHA");
pub const LOGO_BYTES: &[u8] = include_bytes!("../assets/logo.png");

pub fn version_label() -> String {
    format!("{VERSION} ({GIT_SHA})")
}
