use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::prism_paths::PrismPaths;
use crate::protected_state::envelope::{
    ProtectedEventEnvelope, ProtectedSignatureAlgorithm, PROTECTED_EVENT_ENVELOPE_VERSION,
};
use crate::protected_state::repo_streams::inspect_protected_stream;
use crate::protected_state::streams::{classify_protected_repo_relative_path, ProtectedRepoStream};
use crate::protected_state::trust::{
    export_trust_bundle, import_trust_bundle, load_active_runtime_signing_key, load_trusted_root,
    resolve_trusted_runtime_key, TrustedAuthorityRoot,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedStateStreamReport {
    pub stream: String,
    pub stream_id: String,
    pub protected_path: String,
    pub verification_status: String,
    pub last_verified_event_id: Option<String>,
    pub last_verified_entry_hash: Option<String>,
    pub trust_bundle_id: Option<String>,
    pub diagnostic_code: Option<String>,
    pub diagnostic_summary: Option<String>,
    pub repair_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedStateVerifyReport {
    pub streams: Vec<ProtectedStateStreamReport>,
    pub all_verified: bool,
    pub non_verified_stream_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedStateTrustExport {
    pub bundle_id: String,
    pub authority_root_id: String,
    pub bundle_json: String,
    pub root_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedStateTrustImportReport {
    pub bundle_id: String,
    pub authority_root_id: String,
    pub pinned_root_supplied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedStateQuarantineReport {
    pub stream_id: String,
    pub protected_path: String,
    pub quarantined_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedStateRepairReport {
    pub stream_id: String,
    pub protected_path: String,
    pub quarantined_path: String,
    pub restored_event_id: Option<String>,
    pub restored_entry_hash: Option<String>,
    pub restored_record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedStateReconcileReport {
    pub stream_id: String,
    pub protected_path: String,
    pub quarantined_path: String,
    pub accepted_head_event_id: String,
    pub restored_record_count: usize,
}

#[derive(Debug, Clone)]
struct ParsedProtectedEntry {
    line: String,
    envelope: ProtectedEventEnvelope<Value>,
    entry_hash: String,
}

pub fn verify_protected_state(root: &Path) -> Result<ProtectedStateVerifyReport> {
    let streams = collect_protected_state_reports(root)?;
    let non_verified_stream_count = streams
        .iter()
        .filter(|stream| stream.verification_status != "Verified")
        .count();
    Ok(ProtectedStateVerifyReport {
        all_verified: non_verified_stream_count == 0,
        non_verified_stream_count,
        streams,
    })
}

pub fn diagnose_protected_state(
    root: &Path,
    selector: Option<&str>,
) -> Result<Vec<ProtectedStateStreamReport>> {
    match selector {
        Some(selector) => Ok(vec![inspect_stream_report(
            root,
            &resolve_stream(root, selector)?,
        )?]),
        None => collect_protected_state_reports(root),
    }
}

pub fn export_protected_state_trust_material(
    root: &Path,
    bundle_id: Option<&str>,
) -> Result<ProtectedStateTrustExport> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let active_key = load_active_runtime_signing_key(&paths)?;
    let bundle_id = bundle_id.unwrap_or(&active_key.state.active_trust_bundle_id);
    let bundle = export_trust_bundle(&paths, bundle_id)?;
    let trusted_root = load_trusted_root(&paths, &bundle.authority_root_id)?
        .ok_or_else(|| anyhow!("trusted root `{}` was not found", bundle.authority_root_id))?;
    Ok(ProtectedStateTrustExport {
        bundle_id: bundle.bundle_id.clone(),
        authority_root_id: bundle.authority_root_id.clone(),
        bundle_json: serde_json::to_string_pretty(&bundle)?,
        root_json: serde_json::to_string_pretty(&trusted_root)?,
    })
}

pub fn import_protected_state_trust_material(
    root: &Path,
    bundle_json: &str,
    root_json: Option<&str>,
) -> Result<ProtectedStateTrustImportReport> {
    let bundle = serde_json::from_str(bundle_json).context("failed to parse trust bundle JSON")?;
    let pinned_root = match root_json {
        Some(root_json) => Some(
            serde_json::from_str::<TrustedAuthorityRoot>(root_json)
                .context("failed to parse trusted root JSON")?,
        ),
        None => None,
    };
    let paths = PrismPaths::for_workspace_root(root)?;
    import_trust_bundle(&paths, &bundle, pinned_root.as_ref())?;
    Ok(ProtectedStateTrustImportReport {
        bundle_id: bundle.bundle_id.clone(),
        authority_root_id: bundle.authority_root_id.clone(),
        pinned_root_supplied: pinned_root.is_some(),
    })
}

pub fn quarantine_protected_state_stream(
    root: &Path,
    selector: &str,
) -> Result<ProtectedStateQuarantineReport> {
    let stream = resolve_stream(root, selector)?;
    let report = inspect_stream_report(root, &stream)?;
    ensure!(
        report.verification_status != "Verified",
        "refusing to quarantine verified protected stream `{}`",
        report.stream_id
    );
    let (active_path, quarantine_path) = quarantine_stream_file(root, &stream)?;
    ensure!(
        active_path.exists() || quarantine_path.exists(),
        "protected stream `{}` does not exist on disk",
        report.stream_id
    );
    Ok(ProtectedStateQuarantineReport {
        stream_id: report.stream_id,
        protected_path: report.protected_path,
        quarantined_path: quarantine_path.display().to_string(),
    })
}

pub fn repair_protected_state_stream_to_last_valid(
    root: &Path,
    selector: &str,
) -> Result<ProtectedStateRepairReport> {
    let stream = resolve_stream(root, selector)?;
    let report = inspect_stream_report(root, &stream)?;
    ensure!(
        report.verification_status != "Verified",
        "protected stream `{}` is already verified",
        report.stream_id
    );
    ensure!(
        report.verification_status != "Conflict",
        "conflicted protected stream `{}` requires explicit reconcile-stream instead of repair",
        report.stream_id
    );

    let restored_entries = if let Some(last_event_id) = report.last_verified_event_id.as_deref() {
        prefix_entries_through_event(root, &stream, last_event_id)?
    } else if report.verification_status == "Truncated" {
        prefix_entries_until_invalid(root, &stream)?
    } else {
        Vec::new()
    };

    let (_, quarantine_path) = quarantine_stream_file(root, &stream)?;
    let active_path = root.join(stream.relative_path());
    rewrite_stream_file(&active_path, &restored_entries)?;

    Ok(ProtectedStateRepairReport {
        stream_id: report.stream_id,
        protected_path: report.protected_path,
        quarantined_path: quarantine_path.display().to_string(),
        restored_event_id: report.last_verified_event_id,
        restored_entry_hash: report.last_verified_entry_hash,
        restored_record_count: restored_entries.len(),
    })
}

pub fn reconcile_protected_state_stream(
    root: &Path,
    selector: &str,
    accepted_head_event_id: &str,
) -> Result<ProtectedStateReconcileReport> {
    let stream = resolve_stream(root, selector)?;
    let report = inspect_stream_report(root, &stream)?;
    ensure!(
        report.verification_status == "Conflict",
        "protected stream `{}` is not in conflict",
        report.stream_id
    );

    let entries = parse_verified_entries(root, &stream)?;
    let selected = reconcile_entries(&stream, &entries, accepted_head_event_id)?;
    let (_, quarantine_path) = quarantine_stream_file(root, &stream)?;
    let active_path = root.join(stream.relative_path());
    rewrite_stream_file(&active_path, &selected)?;

    Ok(ProtectedStateReconcileReport {
        stream_id: report.stream_id,
        protected_path: report.protected_path,
        quarantined_path: quarantine_path.display().to_string(),
        accepted_head_event_id: accepted_head_event_id.to_string(),
        restored_record_count: selected.len(),
    })
}

pub fn collect_protected_state_reports(root: &Path) -> Result<Vec<ProtectedStateStreamReport>> {
    let mut reports = Vec::new();
    for stream in discover_protected_streams(root)? {
        reports.push(inspect_stream_report(root, &stream)?);
    }
    reports.sort_by(|left, right| left.protected_path.cmp(&right.protected_path));
    Ok(reports)
}

fn inspect_stream_report(
    root: &Path,
    stream: &ProtectedRepoStream,
) -> Result<ProtectedStateStreamReport> {
    let inspection = inspect_protected_stream::<Value>(root, stream)?;
    Ok(ProtectedStateStreamReport {
        stream: stream.stream().to_string(),
        stream_id: inspection.verification.stream_id,
        protected_path: inspection.verification.protected_path,
        verification_status: serde_json::to_value(inspection.verification.verification_status)?
            .as_str()
            .unwrap_or_default()
            .to_string(),
        last_verified_event_id: inspection.verification.last_verified_event_id,
        last_verified_entry_hash: inspection.verification.last_verified_entry_hash,
        trust_bundle_id: inspection.verification.trust_bundle_id,
        diagnostic_code: inspection.verification.diagnostic_code,
        diagnostic_summary: inspection.verification.diagnostic_summary,
        repair_hint: inspection.verification.repair_hint,
    })
}

fn discover_protected_streams(root: &Path) -> Result<Vec<ProtectedRepoStream>> {
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();

    for stream in [
        ProtectedRepoStream::concept_events(),
        ProtectedRepoStream::concept_relations(),
        ProtectedRepoStream::contract_events(),
        ProtectedRepoStream::patch_events(),
    ] {
        insert_stream(&mut ordered, &mut seen, stream);
    }

    discover_streams_in_dir(root, Path::new(".prism/memory"), &mut ordered, &mut seen)?;
    Ok(ordered)
}

fn discover_streams_in_dir(
    root: &Path,
    relative_dir: &Path,
    ordered: &mut Vec<ProtectedRepoStream>,
    seen: &mut BTreeSet<String>,
) -> Result<()> {
    let dir = root.join(relative_dir);
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if let Some(stream) = classify_protected_repo_relative_path(relative) {
            insert_stream(ordered, seen, stream);
        }
    }
    Ok(())
}

fn insert_stream(
    ordered: &mut Vec<ProtectedRepoStream>,
    seen: &mut BTreeSet<String>,
    stream: ProtectedRepoStream,
) {
    if seen.insert(stream.stream_id().to_string()) {
        ordered.push(stream);
    }
}

fn resolve_stream(root: &Path, selector: &str) -> Result<ProtectedRepoStream> {
    for stream in discover_protected_streams(root)? {
        if stream.stream_id() == selector
            || stream.relative_path().display().to_string() == selector
        {
            return Ok(stream);
        }
    }

    if let Some(stream) = classify_protected_repo_relative_path(Path::new(selector)) {
        return Ok(stream);
    }

    bail!("no protected stream matched `{selector}`")
}

fn quarantine_stream_file(root: &Path, stream: &ProtectedRepoStream) -> Result<(PathBuf, PathBuf)> {
    let active_path = root.join(stream.relative_path());
    let paths = PrismPaths::for_workspace_root(root)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let quarantine_path = paths
        .repo_home_dir()
        .join("quarantine")
        .join("protected-state")
        .join(timestamp.to_string())
        .join(stream.relative_path());
    if let Some(parent) = quarantine_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if active_path.exists() {
        fs::rename(&active_path, &quarantine_path).with_context(|| {
            format!(
                "failed to move protected stream {} to {}",
                active_path.display(),
                quarantine_path.display()
            )
        })?;
        sync_parent_dir(&active_path)?;
        sync_parent_dir(&quarantine_path)?;
    }
    Ok((active_path, quarantine_path))
}

fn rewrite_stream_file(path: &Path, entries: &[ParsedProtectedEntry]) -> Result<()> {
    if entries.is_empty() {
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            sync_parent_dir(path)?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    for entry in entries {
        file.write_all(entry.line.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    file.sync_all()
        .with_context(|| format!("failed to fsync {}", path.display()))?;
    sync_parent_dir(path)?;
    Ok(())
}

fn prefix_entries_through_event(
    root: &Path,
    stream: &ProtectedRepoStream,
    last_event_id: &str,
) -> Result<Vec<ParsedProtectedEntry>> {
    let path = root.join(stream.relative_path());
    let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line
            .with_context(|| format!("failed to read line {} in {}", index + 1, path.display()))?;
        if line.trim().is_empty() {
            break;
        }
        let envelope: ProtectedEventEnvelope<Value> =
            serde_json::from_str(&line).with_context(|| {
                format!(
                    "failed to parse protected stream line {} in {}",
                    index + 1,
                    path.display()
                )
            })?;
        let event_id = envelope.event_id.clone();
        let entry_hash = envelope.computed_entry_hash()?;
        entries.push(ParsedProtectedEntry {
            line,
            envelope,
            entry_hash,
        });
        if event_id == last_event_id {
            return Ok(entries);
        }
    }
    bail!(
        "failed to locate last verified event `{last_event_id}` in protected stream {}",
        path.display()
    )
}

fn prefix_entries_until_invalid(
    root: &Path,
    stream: &ProtectedRepoStream,
) -> Result<Vec<ParsedProtectedEntry>> {
    let path = root.join(stream.relative_path());
    let raw = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let contents =
        String::from_utf8(raw).with_context(|| format!("{} is not valid UTF-8", path.display()))?;
    let mut entries = Vec::new();
    for line in contents.lines() {
        if line.trim().is_empty() {
            break;
        }
        let Ok(envelope) = serde_json::from_str::<ProtectedEventEnvelope<Value>>(line) else {
            break;
        };
        let entry_hash = envelope.computed_entry_hash()?;
        entries.push(ParsedProtectedEntry {
            line: line.to_string(),
            envelope,
            entry_hash,
        });
    }
    Ok(entries)
}

fn parse_verified_entries(
    root: &Path,
    stream: &ProtectedRepoStream,
) -> Result<Vec<ParsedProtectedEntry>> {
    let path = root.join(stream.relative_path());
    let raw = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    ensure!(
        !raw.is_empty(),
        "protected stream {} is empty",
        path.display()
    );
    ensure!(
        raw.ends_with(b"\n"),
        "protected stream {} is truncated and cannot be reconciled",
        path.display()
    );
    let contents =
        String::from_utf8(raw).with_context(|| format!("{} is not valid UTF-8", path.display()))?;
    let paths = PrismPaths::for_workspace_root(root)?;
    let mut entries = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        ensure!(
            !line.trim().is_empty(),
            "protected stream {} contains a blank line at record {}",
            path.display(),
            index + 1
        );
        let envelope: ProtectedEventEnvelope<Value> =
            serde_json::from_str(line).with_context(|| {
                format!(
                    "failed to parse protected stream line {} in {}",
                    index + 1,
                    path.display()
                )
            })?;
        ensure!(
            envelope.envelope_version == PROTECTED_EVENT_ENVELOPE_VERSION,
            "protected stream {} used unsupported envelope version {}",
            path.display(),
            envelope.envelope_version
        );
        ensure!(
            envelope.stream == stream.stream() && envelope.stream_id == stream.stream_id(),
            "protected stream {} contained envelope for {} / {}",
            path.display(),
            envelope.stream,
            envelope.stream_id
        );
        ensure!(
            envelope.algorithm == ProtectedSignatureAlgorithm::Ed25519,
            "protected stream {} used unsupported signature algorithm",
            path.display()
        );
        envelope.verify_hashes()?;
        let trusted_key = resolve_trusted_runtime_key(
            &paths,
            &envelope.trust_bundle_id,
            &envelope.runtime_authority_id,
            &envelope.runtime_key_id,
        )?;
        envelope.verify_signature(&trusted_key.verifying_key)?;
        let entry_hash = envelope.computed_entry_hash()?;
        entries.push(ParsedProtectedEntry {
            line: line.to_string(),
            envelope,
            entry_hash,
        });
    }
    Ok(entries)
}

fn reconcile_entries(
    stream: &ProtectedRepoStream,
    entries: &[ParsedProtectedEntry],
    accepted_head_event_id: &str,
) -> Result<Vec<ParsedProtectedEntry>> {
    let mut by_event = BTreeMap::new();
    for entry in entries {
        ensure!(
            by_event
                .insert(entry.envelope.event_id.clone(), entry.clone())
                .is_none(),
            "protected stream `{}` has duplicate event ids; automatic reconcile is not supported",
            stream.stream_id()
        );
    }
    let accepted = by_event.get(accepted_head_event_id).ok_or_else(|| {
        anyhow!(
            "accepted head `{accepted_head_event_id}` was not found in protected stream `{}`",
            stream.stream_id()
        )
    })?;

    let mut chain = Vec::new();
    let mut cursor = accepted.clone();
    loop {
        chain.push(cursor.clone());
        let Some(parent_event_id) = cursor.envelope.prev_event_id.as_ref() else {
            break;
        };
        cursor = by_event.get(parent_event_id).cloned().ok_or_else(|| {
            anyhow!(
                "accepted head `{accepted_head_event_id}` references missing predecessor `{parent_event_id}`"
            )
        })?;
    }
    chain.reverse();

    for window in chain.windows(2) {
        let previous = &window[0];
        let current = &window[1];
        ensure!(
            current.envelope.prev_event_id.as_deref() == Some(previous.envelope.event_id.as_str()),
            "accepted head `{accepted_head_event_id}` does not produce a linear chain"
        );
        ensure!(
            current.envelope.prev_entry_hash.as_deref() == Some(previous.entry_hash.as_str()),
            "accepted head `{accepted_head_event_id}` had a predecessor hash mismatch"
        );
    }

    Ok(chain)
}

fn sync_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path {} has no parent directory", path.display()))?;
    File::open(parent)
        .with_context(|| format!("failed to open parent directory {}", parent.display()))?
        .sync_all()
        .with_context(|| format!("failed to fsync parent directory {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        collect_protected_state_reports, diagnose_protected_state,
        export_protected_state_trust_material, import_protected_state_trust_material,
        quarantine_protected_state_stream, reconcile_protected_state_stream,
        repair_protected_state_stream_to_last_valid, verify_protected_state,
    };
    use crate::prism_paths::set_test_prism_home_override;
    use crate::protected_state::repo_streams::{
        append_protected_stream_event, implicit_principal_identity,
    };
    use crate::protected_state::streams::ProtectedRepoStream;
    use std::cell::RefCell;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    thread_local! {
        static TEMP_TEST_DIRS: RefCell<TempTestDirState> = RefCell::new(TempTestDirState {
            paths: Vec::new(),
        });
    }

    struct TempTestDirState {
        paths: Vec<PathBuf>,
    }

    impl Drop for TempTestDirState {
        fn drop(&mut self) {
            for path in self.paths.drain(..).rev() {
                let _ = fs::remove_dir_all(path);
            }
        }
    }

    fn track_temp_dir(path: &Path) {
        TEMP_TEST_DIRS.with(|state| state.borrow_mut().paths.push(path.to_path_buf()));
    }

    fn tempdir(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "prism-protected-state-operators-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        track_temp_dir(&path);
        path
    }

    #[test]
    fn verify_reports_non_verified_streams() {
        let home = tempdir("verify-home");
        let root = tempdir("verify-root");
        let _guard = set_test_prism_home_override(&home);
        let stream_path = root.join(".prism/concepts/events.jsonl");
        fs::create_dir_all(stream_path.parent().unwrap()).unwrap();
        fs::write(&stream_path, "{\"legacy\":true}\n").unwrap();

        let report = verify_protected_state(&root).unwrap();
        assert!(!report.all_verified);
        assert!(report.streams.iter().any(|stream| stream.protected_path
            == ".prism/concepts/events.jsonl"
            && stream.verification_status == "LegacyUnsigned"));
    }

    #[test]
    fn trust_material_round_trips_with_explicit_root_pin() {
        let home_a = tempdir("trust-home-a");
        let root_a = tempdir("trust-root-a");
        let _guard_a = set_test_prism_home_override(&home_a);
        let exported = export_protected_state_trust_material(&root_a, None).unwrap();

        let home_b = tempdir("trust-home-b");
        let root_b = tempdir("trust-root-b");
        let _guard_b = set_test_prism_home_override(&home_b);
        let imported = import_protected_state_trust_material(
            &root_b,
            &exported.bundle_json,
            Some(&exported.root_json),
        )
        .unwrap();

        assert_eq!(imported.bundle_id, exported.bundle_id);
        assert!(imported.pinned_root_supplied);
    }

    #[test]
    fn quarantine_moves_invalid_stream_out_of_repo_plane() {
        let home = tempdir("quarantine-home");
        let root = tempdir("quarantine-root");
        let _guard = set_test_prism_home_override(&home);
        let stream_path = root.join(".prism/concepts/events.jsonl");
        fs::create_dir_all(stream_path.parent().unwrap()).unwrap();
        fs::write(&stream_path, "{\"legacy\":true}\n").unwrap();

        let report = quarantine_protected_state_stream(&root, "concepts:events").unwrap();
        assert!(!stream_path.exists());
        assert!(Path::new(&report.quarantined_path).exists());
    }

    #[test]
    fn repair_truncated_stream_restores_last_valid_boundary() {
        let home = tempdir("repair-home");
        let root = tempdir("repair-root");
        let _guard = set_test_prism_home_override(&home);
        let principal = implicit_principal_identity(None, None);
        append_protected_stream_event(
            &root,
            &ProtectedRepoStream::concept_events(),
            "event:root",
            &serde_json::json!({"kind":"root"}),
            &principal,
        )
        .unwrap();
        let path = root.join(".prism/concepts/events.jsonl");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"broken\"");
        fs::write(&path, bytes).unwrap();

        let report = repair_protected_state_stream_to_last_valid(&root, "concepts:events").unwrap();
        assert_eq!(report.restored_record_count, 1);
        let diagnose = diagnose_protected_state(&root, Some("concepts:events")).unwrap();
        assert_eq!(diagnose[0].verification_status, "Verified");
    }

    #[test]
    fn reconcile_conflicting_heads_restores_selected_chain() {
        let home = tempdir("reconcile-home");
        let root = tempdir("reconcile-root");
        let _guard = set_test_prism_home_override(&home);
        let stream = ProtectedRepoStream::concept_events();
        let principal = implicit_principal_identity(None, None);

        append_protected_stream_event(
            &root,
            &stream,
            "event:root",
            &serde_json::json!({"kind":"root"}),
            &principal,
        )
        .unwrap();
        append_protected_stream_event(
            &root,
            &stream,
            "event:left",
            &serde_json::json!({"kind":"left"}),
            &principal,
        )
        .unwrap();
        let path = root.join(".prism/concepts/events.jsonl");
        let lines = fs::read_to_string(&path).unwrap();
        let entries: Vec<_> = lines.lines().collect();
        let root_envelope: serde_json::Value = serde_json::from_str(entries[0]).unwrap();
        let root_event_id = root_envelope["event_id"].as_str().unwrap().to_string();
        let root_hash = {
            let parsed: crate::protected_state::envelope::ProtectedEventEnvelope<
                serde_json::Value,
            > = serde_json::from_str(entries[0]).unwrap();
            parsed.computed_entry_hash().unwrap()
        };
        let first: crate::protected_state::envelope::ProtectedEventEnvelope<serde_json::Value> =
            serde_json::from_str(entries[1]).unwrap();
        let mut branch = first.clone();
        branch.event_id = "published:concepts:events:branch".to_string();
        branch.prev_event_id = Some(root_event_id);
        branch.prev_entry_hash = Some(root_hash);
        branch.payload = serde_json::json!({"kind":"right"});
        branch.refresh_payload_hash().unwrap();
        let active_key = crate::protected_state::trust::load_active_runtime_signing_key(
            &crate::PrismPaths::for_workspace_root(&root).unwrap(),
        )
        .unwrap();
        branch.sign_with(&active_key.signing_key).unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", serde_json::to_string(&branch).unwrap()).unwrap();

        let report = diagnose_protected_state(&root, Some("concepts:events")).unwrap();
        assert_eq!(report[0].verification_status, "Conflict");

        let reconcile =
            reconcile_protected_state_stream(&root, "concepts:events", &first.event_id).unwrap();
        assert_eq!(reconcile.accepted_head_event_id, first.event_id);
        let after = diagnose_protected_state(&root, Some("concepts:events")).unwrap();
        assert_eq!(after[0].verification_status, "Verified");
    }

    #[test]
    fn collect_reports_lists_fixed_and_dynamic_streams() {
        let home = tempdir("collect-home");
        let root = tempdir("collect-root");
        let _guard = set_test_prism_home_override(&home);
        let reports = collect_protected_state_reports(&root).unwrap();
        assert!(reports
            .iter()
            .any(|stream| stream.stream_id == "concepts:events"));
    }
}
