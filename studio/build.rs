use std::{env, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    if let Some(manifest_dir) = env::var_os("CARGO_MANIFEST_DIR") {
        command.current_dir(manifest_dir);
    }
    let output = command.args(args).output().ok()?;
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
    if let Some(reference) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    println!("cargo:rerun-if-env-changed=DEPLOYERCOASTER_GIT_SHA");
    let sha = env::var("DEPLOYERCOASTER_GIT_SHA")
        .ok()
        .and_then(|sha| normalize_sha(&sha))
        .or_else(|| git(&["rev-parse", "HEAD"]).and_then(|sha| normalize_sha(&sha)))
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    println!("cargo:rustc-env=DEPLOYERCOASTER_GIT_SHA={sha}");
}

fn normalize_sha(sha: &str) -> Option<String> {
    let sha = sha.trim();
    (sha.len() >= 8 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| sha[..8].to_owned())
}
