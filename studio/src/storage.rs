use std::{fs, io::Write, path::Path};

/// Replace the destination only after a complete write, on the same filesystem.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(bytes)
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    if let Ok(metadata) = fs::metadata(path) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(|error| error.to_string())?;
    }
    temporary
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

/// Write credentials with owner-only permissions on Unix.
pub fn save_credentials(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid credential path")?;
    fs::create_dir_all(parent).map_err(|_| "Could not create credential directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Could not create credential file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| "Could not protect credential file")?;
    }
    temporary
        .write_all(bytes)
        .map_err(|_| "Could not write credentials")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Could not save credentials")?;
    temporary
        .persist(path)
        .map_err(|_| "Could not replace credentials")?;
    Ok(())
}

pub fn credential_path(name: &str) -> Result<std::path::PathBuf, String> {
    dirs::config_dir()
        .map(|root| root.join("DeployerCoaster").join(name))
        .ok_or_else(|| "Could not locate the settings directory".to_owned())
}
