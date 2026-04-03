use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use tracing::warn;

pub(crate) fn ensure_repo_git_support_for_runtime(root: &Path) {
    if let Err(error) = try_install_repo_git_support(root) {
        warn!(
            error = %error,
            root = %root.display(),
            "failed to auto-install PRISM git support for runtime attach"
        );
    }
}

fn try_install_repo_git_support(root: &Path) -> Result<()> {
    let cli = resolve_prism_cli_binary().context("failed to resolve prism-cli binary")?;
    let output = Command::new(&cli)
        .arg("--root")
        .arg(root)
        .arg("protected-state")
        .arg("install-git-support")
        .output()
        .with_context(|| format!("failed to launch {}", cli.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("prism-cli install-git-support failed: {}", stderr.trim());
}

fn resolve_prism_cli_binary() -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to resolve current executable")?;
    if let Some(dir) = current.parent() {
        let sibling = dir.join("prism-cli");
        if sibling.exists() {
            return Ok(sibling);
        }
    }
    Ok(PathBuf::from("prism-cli"))
}
