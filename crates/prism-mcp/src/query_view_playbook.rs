use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_js::{
    QueryEvidenceView, RepoPlaybookGotchaView, RepoPlaybookSectionView, RepoPlaybookView,
};
use serde_json::Value;

use crate::QueryExecution;

const BUILD_PATTERNS: &[&str] = &[
    "cargo build --release",
    "cargo build",
    "npm run build",
    "pnpm build",
    "yarn build",
    "just build",
    "make build",
];
const TEST_PATTERNS: &[&str] = &[
    "cargo test",
    "pytest",
    "npm test",
    "pnpm test",
    "yarn test",
    "just test",
    "make test",
];
const LINT_PATTERNS: &[&str] = &[
    "cargo clippy",
    "cargo check",
    "ruff check",
    "eslint",
    "npm run lint",
    "pnpm lint",
    "yarn lint",
    "just lint",
    "make lint",
];
const FORMAT_PATTERNS: &[&str] = &[
    "cargo fmt --check",
    "cargo fmt",
    "ruff format",
    "prettier",
    "rustfmt",
    "npm run format",
    "pnpm format",
    "yarn format",
    "just format",
    "make format",
];
const WORKFLOW_PATTERNS: &[&str] = &[
    "cargo build --release -p prism-cli -p prism-mcp",
    "./target/release/prism-cli mcp restart --internal-developer",
    "./target/release/prism-cli mcp status",
    "./target/release/prism-cli mcp health",
];

#[derive(Debug, Clone)]
struct CommandSignal {
    command: String,
    evidence: QueryEvidenceView,
}

#[derive(Debug, Clone)]
struct ScannedFile {
    logical_path: String,
    content: String,
}

pub(crate) fn repo_playbook_view(execution: &QueryExecution) -> Result<Value> {
    Ok(serde_json::to_value(collect_repo_playbook(
        execution.workspace_root(),
    ))?)
}

pub(crate) fn collect_repo_playbook(workspace_root: Option<&Path>) -> RepoPlaybookView {
    let files = candidate_files(workspace_root);
    let rust_workspace = files.iter().any(|file| {
        file.logical_path == "Cargo.toml" || file.logical_path.ends_with("/Cargo.toml")
    });
    let root = workspace_root
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    let build = section_from_signals(
        "build",
        find_command_signals(&files, BUILD_PATTERNS, 4),
        rust_workspace.then_some((
            "cargo build".to_string(),
            "Inferred a Rust build command from the workspace Cargo.toml.".to_string(),
        )),
        "No repo-specific build command surfaced in docs, scripts, or build files.",
    );
    let test = section_from_signals(
        "test",
        find_command_signals(&files, TEST_PATTERNS, 4),
        rust_workspace.then_some((
            "cargo test".to_string(),
            "Inferred a Rust test command from the workspace Cargo.toml.".to_string(),
        )),
        "No repo-specific test command surfaced in docs, scripts, or build files.",
    );
    let lint = section_from_signals(
        "lint",
        find_command_signals(&files, LINT_PATTERNS, 4),
        rust_workspace.then_some((
            "cargo check".to_string(),
            "No dedicated lint command surfaced; using `cargo check` as the lightest repo-wide static validation fallback for this Rust workspace.".to_string(),
        )),
        "No repo-specific lint command surfaced in docs, scripts, or build files.",
    );
    let format = section_from_signals(
        "format",
        find_command_signals(&files, FORMAT_PATTERNS, 4),
        rust_workspace.then_some((
            "cargo fmt --all".to_string(),
            "Inferred a Rust formatting command from the workspace Cargo.toml.".to_string(),
        )),
        "No repo-specific format command surfaced in docs, scripts, or build files.",
    );
    let workflow = section_from_signals(
        "workflow",
        find_command_signals(&files, WORKFLOW_PATTERNS, 4),
        None,
        "No explicit repo workflow command sequence surfaced in docs or scripts.",
    );

    let mut gotchas = gotcha_signals(&files);
    if gotchas.is_empty() && rust_workspace && !workflow.commands.is_empty() {
        gotchas.push(RepoPlaybookGotchaView {
            summary: "Keep the live MCP daemon in sync with PRISM MCP changes.".to_string(),
            why: "The repo already exposes a release-binary workflow for rebuilding and restarting the daemon; use it before dogfooding new query or runtime behavior.".to_string(),
            provenance: workflow.provenance.clone(),
        });
    }

    RepoPlaybookView {
        root,
        build,
        test,
        lint,
        format,
        workflow,
        gotchas,
    }
}

fn section_from_signals(
    label: &str,
    signals: Vec<CommandSignal>,
    fallback: Option<(String, String)>,
    missing_summary: &str,
) -> RepoPlaybookSectionView {
    if !signals.is_empty() {
        let provenance = signals
            .iter()
            .map(|signal| signal.evidence.clone())
            .collect::<Vec<_>>();
        let commands = signals
            .iter()
            .map(|signal| signal.command.clone())
            .collect::<Vec<_>>();
        let first_source = provenance
            .first()
            .and_then(|item| item.path.as_deref())
            .unwrap_or("repo files");
        return RepoPlaybookSectionView {
            status: "explicit".to_string(),
            summary: format!("Found repo-specific {label} guidance."),
            commands,
            why: format!("Matched explicit {label} workflow guidance in {first_source}."),
            provenance,
        };
    }

    if let Some((command, detail)) = fallback {
        return RepoPlaybookSectionView {
            status: "inferred".to_string(),
            summary: format!("No explicit {label} command surfaced; using a repo-type fallback."),
            commands: vec![command],
            why: detail.clone(),
            provenance: vec![QueryEvidenceView {
                kind: "inference".to_string(),
                detail,
                path: Some("Cargo.toml".to_string()),
                line: None,
                target: None,
            }],
        };
    }

    RepoPlaybookSectionView {
        status: "missing".to_string(),
        summary: missing_summary.to_string(),
        commands: Vec::new(),
        why: missing_summary.to_string(),
        provenance: Vec::new(),
    }
}

fn gotcha_signals(files: &[ScannedFile]) -> Vec<RepoPlaybookGotchaView> {
    let mut gotchas = Vec::new();
    let restart = find_line_signals(
        files,
        &[
            "restart the mcp daemon",
            "mcp restart --internal-developer",
            "prefer the release binaries",
        ],
        4,
    );
    if !restart.is_empty() {
        gotchas.push(RepoPlaybookGotchaView {
            summary: "Rebuild and restart the live MCP daemon after meaningful PRISM MCP or query/runtime changes.".to_string(),
            why: "The repo guidance explicitly calls for release builds plus daemon restart, status, and health checks before relying on live PRISM behavior.".to_string(),
            provenance: restart,
        });
    }
    gotchas
}

fn candidate_files(workspace_root: Option<&Path>) -> Vec<ScannedFile> {
    let Some(root) = workspace_root else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for relative in [
        "AGENTS.md",
        "README.md",
        "README",
        "justfile",
        "Justfile",
        "Makefile",
        "Cargo.toml",
        "package.json",
        "crates/prism-mcp/src/compact_tools/task_brief.rs",
    ] {
        push_file(root, relative, &mut files);
    }
    push_dir(root, ".github/workflows", &mut files);
    push_dir(root, "scripts", &mut files);
    files
}

fn push_file(root: &Path, relative: &str, files: &mut Vec<ScannedFile>) {
    let path = root.join(relative);
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    files.push(ScannedFile {
        logical_path: relative.replace('\\', "/"),
        content,
    });
}

fn push_dir(root: &Path, relative: &str, files: &mut Vec<ScannedFile>) {
    let path = root.join(relative);
    let Ok(entries) = fs::read_dir(&path) else {
        return;
    };
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .collect::<Vec<PathBuf>>();
    paths.sort();
    for path in paths {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(relative_path) = path.strip_prefix(root) else {
            continue;
        };
        files.push(ScannedFile {
            logical_path: relative_path.to_string_lossy().replace('\\', "/"),
            content,
        });
    }
}

fn find_command_signals(
    files: &[ScannedFile],
    patterns: &[&str],
    limit: usize,
) -> Vec<CommandSignal> {
    let mut results = Vec::new();
    let mut seen = HashSet::<String>::new();
    for file in files {
        for (line_number, line) in file.content.lines().enumerate() {
            let lowercase = line.to_ascii_lowercase();
            for pattern in patterns {
                if !lowercase.contains(&pattern.to_ascii_lowercase()) {
                    continue;
                }
                let command = extract_command(line, pattern);
                if command.is_empty() || !seen.insert(command.clone()) {
                    continue;
                }
                results.push(CommandSignal {
                    command,
                    evidence: QueryEvidenceView {
                        kind: "file_match".to_string(),
                        detail: line.trim().to_string(),
                        path: Some(file.logical_path.clone()),
                        line: Some(line_number + 1),
                        target: None,
                    },
                });
                if results.len() >= limit {
                    return results;
                }
            }
        }
    }
    results
}

fn find_line_signals(
    files: &[ScannedFile],
    patterns: &[&str],
    limit: usize,
) -> Vec<QueryEvidenceView> {
    let mut results = Vec::new();
    let mut seen = HashSet::<(String, usize)>::new();
    for file in files {
        for (line_number, line) in file.content.lines().enumerate() {
            let lowercase = line.to_ascii_lowercase();
            if !patterns
                .iter()
                .any(|pattern| lowercase.contains(&pattern.to_ascii_lowercase()))
            {
                continue;
            }
            if !seen.insert((file.logical_path.clone(), line_number + 1)) {
                continue;
            }
            results.push(QueryEvidenceView {
                kind: "file_match".to_string(),
                detail: line.trim().to_string(),
                path: Some(file.logical_path.clone()),
                line: Some(line_number + 1),
                target: None,
            });
            if results.len() >= limit {
                return results;
            }
        }
    }
    results
}

fn extract_command(line: &str, pattern: &str) -> String {
    for delimiter in ['`', '"'] {
        for fragment in line.split(delimiter) {
            if fragment
                .to_ascii_lowercase()
                .contains(&pattern.to_ascii_lowercase())
            {
                return fragment.trim().to_string();
            }
        }
    }
    line.trim()
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim()
        .to_string()
}
