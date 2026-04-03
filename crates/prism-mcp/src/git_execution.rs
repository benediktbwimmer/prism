use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use prism_coordination::{GitExecutionPolicy, GitPreflightReport, GitPublishReport};
use prism_ir::CoordinationTaskId;

fn run_git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    // Preserve leading spaces for porcelain formats like `git status --porcelain`.
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

fn is_prism_managed_path(path: &str) -> bool {
    path.starts_with(".prism/")
        || path == "PRISM.md"
        || path == "docs/prism"
        || path.starts_with("docs/prism/")
}

pub(crate) fn user_dirty_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| !is_prism_managed_path(path))
        .cloned()
        .collect()
}

pub(crate) fn prism_managed_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| is_prism_managed_path(path))
        .cloned()
        .collect()
}

fn parse_porcelain_paths(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            Some(line[3..].trim().to_string())
        })
        .collect()
}

pub(crate) fn worktree_dirty_paths(root: &Path) -> Result<Vec<String>> {
    Ok(parse_porcelain_paths(&run_git(
        root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?))
}

#[derive(Debug, Clone)]
pub(crate) struct GitPreflightOutcome {
    pub(crate) report: GitPreflightReport,
    pub(crate) current_branch: String,
}

pub(crate) fn run_preflight(
    root: &Path,
    policy: &GitExecutionPolicy,
    now: u64,
    require_clean_worktree: bool,
) -> Result<GitPreflightOutcome> {
    run_git(root, &["fetch", "origin", policy.target_branch.as_str()])?;

    let current_branch = run_git(root, &["branch", "--show-current"])?;
    let head_commit = run_git(root, &["rev-parse", "HEAD"])?;
    let target_ref = format!("origin/{}", policy.target_branch);
    let target_commit = run_git(root, &["rev-parse", target_ref.as_str()])?;
    let merge_base_commit = run_git(root, &["merge-base", "HEAD", target_ref.as_str()])?;
    let behind_target_commits = run_git(root, &["rev-list", "--count", "HEAD..FETCH_HEAD"])?
        .parse::<u32>()
        .unwrap_or(0);
    let dirty_paths = worktree_dirty_paths(root)?;
    let protected_dirty_paths = dirty_paths
        .iter()
        .filter(|path| path.starts_with(".prism/plans/"))
        .cloned()
        .collect::<Vec<_>>();
    let blocking_dirty_paths = user_dirty_paths(&dirty_paths);
    let worktree_dirty = !dirty_paths.is_empty();
    let mut failure = None;
    if policy.require_task_branch && current_branch == policy.target_branch {
        failure = Some(format!(
            "current branch `{}` must differ from target branch `{}`",
            current_branch, policy.target_branch
        ));
    } else if behind_target_commits > 0 {
        failure = Some(format!(
            "branch is {} commit(s) behind `{}`",
            behind_target_commits, target_ref
        ));
    } else if require_clean_worktree && !blocking_dirty_paths.is_empty() {
        failure = Some(format!(
            "worktree must be clean before starting task execution; dirty user paths: {}",
            blocking_dirty_paths.join(", ")
        ));
    }
    let report = GitPreflightReport {
        source_ref: Some(current_branch.clone()),
        target_ref: Some(policy.target_branch.clone()),
        publish_ref: Some(current_branch.clone()),
        checked_at: now,
        target_branch: policy.target_branch.clone(),
        current_branch: Some(current_branch.clone()),
        head_commit: Some(head_commit),
        target_commit: Some(target_commit),
        merge_base_commit: Some(merge_base_commit),
        behind_target_commits,
        worktree_dirty,
        dirty_paths,
        protected_dirty_paths,
        failure: failure.clone(),
    };
    Ok(GitPreflightOutcome {
        report,
        current_branch,
    })
}

pub(crate) fn commit_all(root: &Path, message: &str, now: u64) -> Result<GitPublishReport> {
    run_git(root, &["add", "-A"])?;
    let staged_paths = run_git(root, &["diff", "--cached", "--name-only"])?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect::<Vec<_>>();
    if staged_paths.is_empty() {
        return Err(anyhow!("no changes are staged for commit"));
    }
    run_git(root, &["commit", "-m", message])?;
    let code_commit = run_git(root, &["rev-parse", "HEAD"])?;
    let protected_paths = staged_paths
        .iter()
        .filter(|path| path.starts_with(".prism/"))
        .cloned()
        .collect::<Vec<_>>();
    Ok(GitPublishReport {
        attempted_at: now,
        publish_ref: None,
        code_commit: Some(code_commit),
        coordination_commit: None,
        pushed_ref: None,
        staged_paths,
        protected_paths,
        failure: None,
    })
}

pub(crate) fn commit_paths(
    root: &Path,
    message: &str,
    now: u64,
    paths: &[String],
) -> Result<GitPublishReport> {
    if paths.is_empty() {
        return Err(anyhow!("no paths were selected for commit"));
    }

    let mut add_args = vec!["add", "-A", "--"];
    add_args.extend(paths.iter().map(String::as_str));
    run_git(root, &add_args)?;

    let staged_paths = run_git(root, &["diff", "--cached", "--name-only"])?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect::<Vec<_>>();
    if staged_paths.is_empty() {
        return Err(anyhow!("no selected paths are staged for commit"));
    }
    run_git(root, &["commit", "-m", message])?;
    let code_commit = run_git(root, &["rev-parse", "HEAD"])?;
    let protected_paths = staged_paths
        .iter()
        .filter(|path| path.starts_with(".prism/"))
        .cloned()
        .collect::<Vec<_>>();
    Ok(GitPublishReport {
        attempted_at: now,
        publish_ref: None,
        code_commit: Some(code_commit),
        coordination_commit: None,
        pushed_ref: None,
        staged_paths,
        protected_paths,
        failure: None,
    })
}

pub(crate) fn head_commit(root: &Path) -> Result<String> {
    run_git(root, &["rev-parse", "HEAD"])
}

fn slugify_branch_component(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else {
            None
        };
        match normalized {
            Some(ch) => {
                slug.push(ch);
                last_was_dash = false;
            }
            None if !slug.is_empty() && !last_was_dash => {
                slug.push('-');
                last_was_dash = true;
            }
            None => {}
        }
    }
    slug.trim_matches('-').to_string()
}

fn suggested_task_branch(task_title: &str, task_id: &CoordinationTaskId) -> String {
    let title_slug = slugify_branch_component(task_title);
    let id_suffix = task_id
        .0
        .rsplit(':')
        .next()
        .unwrap_or(task_id.0.as_str())
        .chars()
        .take(8)
        .collect::<String>();
    let title_part = if title_slug.is_empty() {
        "coord-task".to_string()
    } else {
        title_slug
    };
    format!("task/{title_part}-{id_suffix}")
}

pub(crate) fn ensure_task_branch(
    root: &Path,
    task_title: &str,
    task_id: &CoordinationTaskId,
) -> Result<String> {
    let branch = suggested_task_branch(task_title, task_id);
    if run_git(root, &["rev-parse", "--verify", branch.as_str()]).is_ok() {
        run_git(root, &["checkout", branch.as_str()])?;
    } else {
        run_git(root, &["checkout", "-b", branch.as_str()])?;
    }
    Ok(branch)
}

pub(crate) fn push_current_branch(
    root: &Path,
    branch: &str,
    report: &mut GitPublishReport,
) -> Result<()> {
    run_git(root, &["push", "origin", branch])?;
    report.pushed_ref = Some(format!("refs/heads/{branch}"));
    Ok(())
}
