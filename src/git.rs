//! Thin wrapper around the `git` executable.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// Run `git -C <repo> <args>` and return trimmed stdout.
pub fn run(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .context("failed to run git (is it installed and on PATH?)")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True when the index has staged changes relative to HEAD (or any staged
/// file in a repository without commits).
pub fn has_staged_changes(repo: &Path) -> Result<bool> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("failed to run git")?;
    match status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => bail!("git diff --cached failed"),
    }
}
