use std::{fs, path::PathBuf};

/// Keep installable extension files beside Studio's local settings, including in packaged builds.
pub(crate) fn install() -> Result<PathBuf, String> {
    let root = crate::storage::credential_path("console-bridge")?;
    for (browser, manifest) in [
        ("chrome", include_str!("../browser-extension/manifest.json")),
        (
            "firefox",
            include_str!("../browser-extension/manifest.firefox.json"),
        ),
    ] {
        let directory = root.join(browser);
        fs::create_dir_all(&directory)
            .map_err(|_| "Could not prepare the Console browser extension.".to_owned())?;
        for (name, content) in [
            ("manifest.json", manifest),
            (
                "background.js",
                include_str!("../browser-extension/background.js"),
            ),
            (
                "observer.js",
                include_str!("../browser-extension/observer.js"),
            ),
            (
                "content.js",
                include_str!("../browser-extension/content.js"),
            ),
            (
                "connect.js",
                include_str!("../browser-extension/connect.js"),
            ),
        ] {
            crate::storage::atomic_write(&directory.join(name), content.as_bytes())
                .map_err(|_| "Could not prepare the Console browser extension.".to_owned())?;
        }
    }
    Ok(root)
}
