use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use prism_core::{
    regenerate_repo_published_plan_artifacts, regenerate_repo_snapshot_derived_artifacts,
};
use serde::Deserialize;

const MANAGED_BLOCK_START: &str = "# BEGIN PRISM MANAGED";
const MANAGED_BLOCK_END: &str = "# END PRISM MANAGED";
const STREAM_DRIVER_NAME: &str = "prism-protected-stream";
const DERIVED_DRIVER_NAME: &str = "prism-derived-prism";
const SNAPSHOT_DERIVED_DRIVER_NAME: &str = "prism-snapshot-derived";

const MANAGED_GITATTRIBUTES_BLOCK: &str = "\
# BEGIN PRISM MANAGED
.prism/concepts/events.jsonl merge=prism-protected-stream
.prism/concepts/relations.jsonl merge=prism-protected-stream
.prism/contracts/events.jsonl merge=prism-protected-stream
.prism/changes/events.jsonl merge=prism-protected-stream
.prism/memory/*.jsonl merge=prism-protected-stream
.prism/plans/index.jsonl merge=prism-derived-prism
.prism/plans/active/*.jsonl merge=prism-derived-prism
.prism/plans/archived/*.jsonl merge=prism-derived-prism
.prism/state/manifest.json merge=prism-snapshot-derived
.prism/state/indexes/*.json merge=prism-snapshot-derived
# END PRISM MANAGED
";

#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamMergeRecord {
    event_id: String,
    line: String,
}

#[derive(Debug, Deserialize)]
struct MergeEnvelopeHeader {
    event_id: String,
}

pub(crate) fn ensure_repo_git_support(root: &Path) -> Result<()> {
    if !is_git_workspace_root(root) {
        return Ok(());
    }
    ensure_managed_gitattributes(root)?;
    ensure_merge_driver_config(root)?;
    let _ = regenerate_repo_published_plan_artifacts(root);
    Ok(())
}

pub(crate) fn install_repo_git_support(root: &Path) -> Result<()> {
    ensure_repo_git_support(root)?;
    println!("configured PRISM git merge support for {}", root.display());
    Ok(())
}

pub(crate) fn run_stream_merge_driver(
    _root: &Path,
    ancestor: &Path,
    current: &Path,
    other: &Path,
    path: &str,
) -> Result<()> {
    ensure!(
        is_authoritative_protected_stream_path(Path::new(path)),
        "path `{path}` is not an authoritative protected stream"
    );
    let ancestor_text = read_optional_text(ancestor)?;
    let current_text = read_optional_text(current)?;
    let other_text = read_optional_text(other)?;
    let merged = merge_stream_texts(&ancestor_text, &current_text, &other_text)?;
    write_merge_result(current, &merged)
}

pub(crate) fn run_derived_merge_driver(
    _root: &Path,
    ancestor: &Path,
    current: &Path,
    other: &Path,
    path: &str,
) -> Result<()> {
    ensure!(
        is_derived_prism_path(Path::new(path)),
        "path `{path}` is not a derived PRISM artifact"
    );
    let ancestor_text = read_optional_text(ancestor)?;
    let current_text = read_optional_text(current)?;
    let other_text = read_optional_text(other)?;
    let merged = merge_text_lines(&ancestor_text, &current_text, &other_text)?;
    write_merge_result(current, &merged)
}

pub(crate) fn run_snapshot_derived_merge_driver(
    root: &Path,
    _ancestor: &Path,
    current: &Path,
    _other: &Path,
    path: &str,
) -> Result<()> {
    ensure!(
        is_snapshot_derived_prism_path(Path::new(path)),
        "path `{path}` is not a snapshot-derived PRISM artifact"
    );
    regenerate_repo_snapshot_derived_artifacts(root)?;
    let regenerated = fs::read_to_string(root.join(path))
        .with_context(|| format!("failed to read regenerated snapshot-derived artifact {path}"))?;
    write_merge_result(current, &regenerated)
}

fn ensure_managed_gitattributes(root: &Path) -> Result<()> {
    let path = root.join(".gitattributes");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let updated = upsert_managed_block(&existing, MANAGED_GITATTRIBUTES_BLOCK);
    if updated != existing {
        fs::write(&path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn ensure_merge_driver_config(root: &Path) -> Result<()> {
    let common_dir = git_common_dir(root)?;
    let config_path = common_dir.join("config");
    let executable = std::env::current_exe().context("failed to resolve current prism-cli path")?;
    let quoted_executable = shell_single_quote(&executable.display().to_string());
    let stream_driver = format!(
        "sh -c 'root=\"$(git rev-parse --show-toplevel)\" || exit 1; exec {quoted_executable} --root \"$root\" protected-state merge-driver-stream --ancestor \"$1\" --current \"$2\" --other \"$3\" --path \"$4\"' sh"
    );
    let derived_driver = format!(
        "sh -c 'root=\"$(git rev-parse --show-toplevel)\" || exit 1; exec {quoted_executable} --root \"$root\" protected-state merge-driver-derived --ancestor \"$1\" --current \"$2\" --other \"$3\" --path \"$4\"' sh"
    );
    let snapshot_derived_driver = format!(
        "sh -c 'root=\"$(git rev-parse --show-toplevel)\" || exit 1; exec {quoted_executable} --root \"$root\" protected-state merge-driver-snapshot-derived --ancestor \"$1\" --current \"$2\" --other \"$3\" --path \"$4\"' sh"
    );
    set_git_config(
        &config_path,
        &format!("merge.{STREAM_DRIVER_NAME}.name"),
        "PRISM protected stream merge driver",
    )?;
    set_git_config(
        &config_path,
        &format!("merge.{STREAM_DRIVER_NAME}.driver"),
        &format!("{stream_driver} %O %A %B %P"),
    )?;
    set_git_config(
        &config_path,
        &format!("merge.{DERIVED_DRIVER_NAME}.name"),
        "PRISM derived artifact merge driver",
    )?;
    set_git_config(
        &config_path,
        &format!("merge.{DERIVED_DRIVER_NAME}.driver"),
        &format!("{derived_driver} %O %A %B %P"),
    )?;
    set_git_config(
        &config_path,
        &format!("merge.{SNAPSHOT_DERIVED_DRIVER_NAME}.name"),
        "PRISM snapshot-derived artifact merge driver",
    )?;
    set_git_config(
        &config_path,
        &format!("merge.{SNAPSHOT_DERIVED_DRIVER_NAME}.driver"),
        &format!("{snapshot_derived_driver} %O %A %B %P"),
    )?;
    Ok(())
}

fn set_git_config(config_path: &Path, key: &str, value: &str) -> Result<()> {
    if git_config_value(config_path, key)?.as_deref() == Some(value) {
        return Ok(());
    }

    for attempt in 0..5 {
        let output = Command::new("git")
            .arg("config")
            .arg("--file")
            .arg(config_path)
            .arg(key)
            .arg(value)
            .output()
            .with_context(|| {
                format!("failed to launch git config for {}", config_path.display())
            })?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("could not lock config file") {
            if git_config_value(config_path, key)?.as_deref() == Some(value) {
                return Ok(());
            }
            if attempt < 4 {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        }
        bail!(
            "failed to update git config {} {}: {}",
            config_path.display(),
            key,
            stderr
        );
    }

    Ok(())
}

fn git_config_value(config_path: &Path, key: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("config")
        .arg("--file")
        .arg(config_path)
        .arg("--get")
        .arg(key)
        .output()
        .with_context(|| format!("failed to read git config for {}", config_path.display()))?;
    if output.status.success() {
        let value =
            String::from_utf8(output.stdout).context("git returned non-utf8 config value")?;
        return Ok(Some(value.trim().to_string()));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("key does not contain a section")
        || stderr.contains("invalid key")
        || stderr.contains("no such section")
    {
        bail!(
            "failed to read git config {} {}: {}",
            config_path.display(),
            key,
            stderr.trim()
        );
    }
    Ok(None)
}

fn git_common_dir(root: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg("--path-format=absolute")
        .arg("--git-common-dir")
        .output()
        .with_context(|| format!("failed to query git common dir for {}", root.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "failed to resolve git common dir for {}: {}",
            root.display(),
            stderr.trim()
        );
    }
    let path = String::from_utf8(output.stdout).context("git returned non-utf8 common dir path")?;
    Ok(PathBuf::from(path.trim()))
}

fn is_git_workspace_root(root: &Path) -> bool {
    let dot_git = root.join(".git");
    dot_git.is_dir()
        || fs::read_to_string(&dot_git)
            .map(|contents| contents.starts_with("gitdir: "))
            .unwrap_or(false)
}

fn read_optional_text(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn write_merge_result(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))
}

fn merge_stream_texts(ancestor: &str, current: &str, other: &str) -> Result<String> {
    let ancestor_records = parse_stream_records(ancestor)?;
    let current_records = parse_stream_records(current)?;
    let other_records = parse_stream_records(other)?;
    let mut by_event_id = BTreeMap::<String, String>::new();
    for records in [&ancestor_records, &current_records, &other_records] {
        for record in records {
            match by_event_id.get(&record.event_id) {
                Some(existing) if existing != &record.line => {
                    bail!(
                        "protected stream merge encountered divergent payloads for event {}",
                        record.event_id
                    );
                }
                Some(_) => {}
                None => {
                    by_event_id.insert(record.event_id.clone(), record.line.clone());
                }
            }
        }
    }
    merge_records(
        ancestor_records
            .into_iter()
            .map(|record| record.line)
            .collect(),
        current_records
            .into_iter()
            .map(|record| record.line)
            .collect(),
        other_records
            .into_iter()
            .map(|record| record.line)
            .collect(),
    )
}

fn parse_stream_records(text: &str) -> Result<Vec<StreamMergeRecord>> {
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let envelope = serde_json::from_str::<MergeEnvelopeHeader>(line).with_context(|| {
            format!(
                "failed to parse protected stream merge record {}",
                index + 1
            )
        })?;
        records.push(StreamMergeRecord {
            event_id: envelope.event_id,
            line: line.to_string(),
        });
    }
    Ok(records)
}

fn merge_text_lines(ancestor: &str, current: &str, other: &str) -> Result<String> {
    merge_records(
        parse_lines(ancestor),
        parse_lines(current),
        parse_lines(other),
    )
}

fn merge_records(
    ancestor: Vec<String>,
    current: Vec<String>,
    other: Vec<String>,
) -> Result<String> {
    let mut merged = Vec::new();
    let mut seen = BTreeMap::<String, ()>::new();
    for line in ancestor.into_iter().chain(current).chain(other) {
        if seen.insert(line.clone(), ()).is_none() {
            merged.push(line);
        }
    }
    if merged.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("{}\n", merged.join("\n")))
}

fn parse_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect()
}

fn upsert_managed_block(existing: &str, block: &str) -> String {
    let normalized_block = if block.ends_with('\n') {
        block.to_string()
    } else {
        format!("{block}\n")
    };
    let start = existing.find(MANAGED_BLOCK_START);
    let end = existing.find(MANAGED_BLOCK_END);
    match (start, end) {
        (Some(start), Some(end)) if end >= start => {
            let suffix_start = existing[end..]
                .find('\n')
                .map(|offset| end + offset + 1)
                .unwrap_or_else(|| existing.len());
            let prefix = existing[..start].trim_end_matches('\n');
            let suffix = existing[suffix_start..].trim_start_matches('\n');
            join_sections(prefix, &normalized_block, suffix)
        }
        _ if existing.trim().is_empty() => normalized_block,
        _ => join_sections(existing.trim_end_matches('\n'), &normalized_block, ""),
    }
}

fn join_sections(prefix: &str, block: &str, suffix: &str) -> String {
    let mut sections = Vec::new();
    if !prefix.trim().is_empty() {
        sections.push(prefix.trim_end_matches('\n').to_string());
    }
    sections.push(block.trim_end_matches('\n').to_string());
    if !suffix.trim().is_empty() {
        sections.push(
            suffix
                .trim_start_matches('\n')
                .trim_end_matches('\n')
                .to_string(),
        );
    }
    format!("{}\n", sections.join("\n\n"))
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn is_authoritative_protected_stream_path(path: &Path) -> bool {
    let segments = path
        .iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [prism, concepts, file]
            if prism == ".prism" && concepts == "concepts" && file == "events.jsonl" =>
        {
            true
        }
        [prism, concepts, file]
            if prism == ".prism" && concepts == "concepts" && file == "relations.jsonl" =>
        {
            true
        }
        [prism, contracts, file]
            if prism == ".prism" && contracts == "contracts" && file == "events.jsonl" =>
        {
            true
        }
        [prism, changes, file]
            if prism == ".prism" && changes == "changes" && file == "events.jsonl" =>
        {
            true
        }
        [prism, memory, file]
            if prism == ".prism" && memory == "memory" && file.ends_with(".jsonl") =>
        {
            true
        }
        [prism, plans, streams, file]
            if prism == ".prism"
                && plans == "plans"
                && streams == "streams"
                && file.ends_with(".jsonl") =>
        {
            true
        }
        _ => false,
    }
}

fn is_derived_prism_path(path: &Path) -> bool {
    let segments = path
        .iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [prism, plans, file] if prism == ".prism" && plans == "plans" && file == "index.jsonl" => {
            true
        }
        [prism, plans, active, file]
            if prism == ".prism"
                && plans == "plans"
                && (active == "active" || active == "archived")
                && file.ends_with(".jsonl") =>
        {
            true
        }
        _ => false,
    }
}

fn is_snapshot_derived_prism_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized == ".prism/state/manifest.json" || normalized.starts_with(".prism/state/indexes/")
}

#[cfg(test)]
mod tests {
    use super::{
        is_authoritative_protected_stream_path, is_derived_prism_path,
        is_snapshot_derived_prism_path, merge_stream_texts, merge_text_lines, upsert_managed_block,
        MANAGED_BLOCK_END, MANAGED_BLOCK_START, MANAGED_GITATTRIBUTES_BLOCK,
    };
    use std::path::Path;

    #[test]
    fn upsert_managed_block_inserts_and_replaces_owned_section() {
        let initial = "*.rs text eol=lf\n";
        let block = format!("{MANAGED_BLOCK_START}\nfoo\n{MANAGED_BLOCK_END}\n");
        let inserted = upsert_managed_block(initial, &block);
        assert!(inserted.contains("*.rs text eol=lf"));
        assert!(inserted.contains("foo"));

        let replaced = upsert_managed_block(
            &inserted,
            &format!("{MANAGED_BLOCK_START}\nbar\n{MANAGED_BLOCK_END}\n"),
        );
        assert!(replaced.contains("bar"));
        assert!(!replaced.contains("foo"));
        assert!(replaced.contains("*.rs text eol=lf"));
    }

    #[test]
    fn authoritative_and_derived_path_matchers_cover_expected_prism_files() {
        assert!(is_authoritative_protected_stream_path(Path::new(
            ".prism/concepts/events.jsonl"
        )));
        assert!(is_authoritative_protected_stream_path(Path::new(
            ".prism/memory/events.jsonl"
        )));
        assert!(!is_authoritative_protected_stream_path(Path::new(
            ".prism/plans/index.jsonl"
        )));

        assert!(is_derived_prism_path(Path::new(".prism/plans/index.jsonl")));
        assert!(is_derived_prism_path(Path::new(
            ".prism/plans/active/plan:1.jsonl"
        )));
        assert!(is_derived_prism_path(Path::new(
            ".prism/plans/archived/plan:1.jsonl"
        )));
        assert!(!is_derived_prism_path(Path::new(
            ".prism/concepts/events.jsonl"
        )));

        assert!(is_snapshot_derived_prism_path(Path::new(
            ".prism/state/manifest.json"
        )));
        assert!(is_snapshot_derived_prism_path(Path::new(
            ".prism/state/indexes/plans.json"
        )));
        assert!(!is_snapshot_derived_prism_path(Path::new(
            ".prism/state/plans/plan-1.json"
        )));
        assert!(!is_snapshot_derived_prism_path(Path::new("PRISM.md")));
        assert!(!is_snapshot_derived_prism_path(Path::new(
            "docs/prism/plans/index.md"
        )));
    }

    #[test]
    fn managed_gitattributes_block_covers_snapshot_outputs() {
        assert!(MANAGED_GITATTRIBUTES_BLOCK
            .contains(".prism/state/manifest.json merge=prism-snapshot-derived"));
        assert!(MANAGED_GITATTRIBUTES_BLOCK
            .contains(".prism/state/indexes/*.json merge=prism-snapshot-derived"));
        assert!(!MANAGED_GITATTRIBUTES_BLOCK
            .contains(".prism/state/plans/*.json merge=prism-snapshot-derived"));
        assert!(!MANAGED_GITATTRIBUTES_BLOCK.contains("PRISM.md merge=prism-snapshot-derived"));
        assert!(!MANAGED_GITATTRIBUTES_BLOCK.contains("docs/prism/** merge=prism-snapshot-derived"));
    }

    #[test]
    fn stream_merge_unions_divergent_heads_without_conflict_markers() {
        let ancestor = "{\"event_id\":\"event:1\"}\n";
        let current = "{\"event_id\":\"event:1\"}\n{\"event_id\":\"event:2\"}\n";
        let other = "{\"event_id\":\"event:1\"}\n{\"event_id\":\"event:3\"}\n";
        let merged = merge_stream_texts(ancestor, current, other).unwrap();
        assert_eq!(
            merged,
            "{\"event_id\":\"event:1\"}\n{\"event_id\":\"event:2\"}\n{\"event_id\":\"event:3\"}\n"
        );
        assert!(!merged.contains("<<<<<<<"));
    }

    #[test]
    fn stream_merge_rejects_divergent_payloads_for_same_event_id() {
        let current = "{\"event_id\":\"event:1\",\"sequence\":1}\n";
        let other = "{\"event_id\":\"event:1\",\"sequence\":2}\n";
        let error = merge_stream_texts("", current, other)
            .unwrap_err()
            .to_string();
        assert!(error.contains("divergent payloads"));
    }

    #[test]
    fn derived_merge_unions_text_lines_losslessly() {
        let merged = merge_text_lines("a\n", "a\nb\n", "a\nc\n").unwrap();
        assert_eq!(merged, "a\nb\nc\n");
    }
}
