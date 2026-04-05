use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(crate) fn resolve(root: Option<&Path>) -> Result<PathBuf> {
    match root {
        Some(root) => Ok(root.to_path_buf()),
        None => auto_discover(),
    }
}

fn auto_discover() -> Result<PathBuf> {
    let cwd = env::current_dir().context("failed to read current directory for PRISM root")?;
    Ok(discover_git_root(&cwd).unwrap_or(cwd))
}

fn discover_git_root(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if is_git_workspace_root(candidate) {
            return Some(candidate.to_path_buf());
        }
    }
    None
}

fn is_git_workspace_root(path: &Path) -> bool {
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return true;
    }
    match fs::read_to_string(&dot_git) {
        Ok(contents) => contents.starts_with("gitdir: "),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!(
            "prism-cli-workspace-root-{label}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn explicit_root_wins() {
        let root = unique_temp_dir("explicit-root");
        assert_eq!(resolve(Some(&root)).unwrap(), root);
    }

    #[test]
    fn discovers_nearest_git_root() {
        let root = unique_temp_dir("git-root");
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();

        assert_eq!(discover_git_root(&nested).unwrap(), root);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recognizes_gitdir_file_worktrees() {
        let root = unique_temp_dir("git-file-root");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join(".git"), "gitdir: ../prism-linked-worktree\n").unwrap();

        assert_eq!(discover_git_root(&nested).unwrap(), root);

        let _ = fs::remove_dir_all(root);
    }
}
