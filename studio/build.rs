use std::{env, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    // This crate lives inside the repository, and may also be built in a worktree.
    for file in ["HEAD", "logs/HEAD", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", file]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    println!("cargo:rerun-if-env-changed=DEPLOYERCOASTER_GIT_SHA");
    let sha = env::var("DEPLOYERCOASTER_GIT_SHA")
        .ok()
        .or_else(|| git(&["rev-parse", "--short=8", "HEAD"]))
        .filter(|sha| sha.len() == 8 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    println!("cargo:rustc-env=DEPLOYERCOASTER_GIT_SHA={sha}");
}
