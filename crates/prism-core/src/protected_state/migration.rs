use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use prism_memory::{MemoryEvent, MemoryScope};
use prism_projections::{ConceptEvent, ConceptRelationEvent, ContractEvent};
use serde::{de::DeserializeOwned, Serialize};

use crate::protected_state::envelope::{
    ProtectedEventEnvelope, ProtectedSignatureAlgorithm, PROTECTED_EVENT_ENVELOPE_VERSION,
};
use crate::protected_state::repo_streams::{implicit_principal_identity, inspect_protected_stream};
use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};
use crate::protected_state::trust::load_active_runtime_signing_key;
use crate::util::{
    repo_concept_events_path, repo_concept_relations_path, repo_contract_events_path,
};
use crate::PrismPaths;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtectedStateMigrationReport {
    pub migrated_stream_count: usize,
    pub migrated_event_count: usize,
    pub migrated_paths: Vec<PathBuf>,
}

pub fn migrate_legacy_protected_repo_state(
    root: impl AsRef<Path>,
) -> Result<ProtectedStateMigrationReport> {
    let root = root.as_ref();
    let mut report = ProtectedStateMigrationReport::default();

    migrate_stream(
        root,
        &ProtectedRepoStream::concept_events(),
        &repo_concept_events_path(root),
        load_legacy_jsonl::<ConceptEvent>,
        |event| implicit_principal_identity(event.actor.as_ref(), event.execution_context.as_ref()),
        |event| event.id.as_str(),
        &mut report,
    )?;
    migrate_stream(
        root,
        &ProtectedRepoStream::concept_relations(),
        &repo_concept_relations_path(root),
        load_legacy_jsonl::<ConceptRelationEvent>,
        |event| implicit_principal_identity(event.actor.as_ref(), event.execution_context.as_ref()),
        |event| event.id.as_str(),
        &mut report,
    )?;
    migrate_stream(
        root,
        &ProtectedRepoStream::contract_events(),
        &repo_contract_events_path(root),
        load_legacy_jsonl::<ContractEvent>,
        |event| implicit_principal_identity(event.actor.as_ref(), event.execution_context.as_ref()),
        |event| event.id.as_str(),
        &mut report,
    )?;

    let memory_dir = root.join(".prism").join("memory");
    if memory_dir.exists() {
        for entry in fs::read_dir(&memory_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(stream) = ProtectedRepoStream::memory_stream(file_name) else {
                continue;
            };
            migrate_stream(
                root,
                &stream,
                &path,
                load_legacy_jsonl::<MemoryEvent>,
                |event| {
                    implicit_principal_identity(
                        event.actor.as_ref(),
                        event.execution_context.as_ref(),
                    )
                },
                |event| event.id.as_str(),
                &mut report,
            )?;
        }
    }

    Ok(report)
}

fn migrate_stream<T, L, P, E>(
    root: &Path,
    stream: &ProtectedRepoStream,
    path: &Path,
    load_legacy: L,
    principal_for: P,
    event_id_for: E,
    report: &mut ProtectedStateMigrationReport,
) -> Result<()>
where
    T: Serialize + DeserializeOwned + Clone,
    L: Fn(&Path) -> Result<Vec<T>>,
    P: Fn(&T) -> crate::protected_state::repo_streams::ProtectedPrincipalIdentity,
    E: Fn(&T) -> &str,
{
    if !path.exists() {
        return Ok(());
    }

    let inspection = inspect_protected_stream::<T>(root, stream)?;
    match inspection.verification.verification_status {
        ProtectedVerificationStatus::Verified => return Ok(()),
        ProtectedVerificationStatus::LegacyUnsigned => {}
        status => bail!(
            "cannot migrate {} because current verification status is {:?}: {}",
            path.display(),
            status,
            inspection
                .verification
                .diagnostic_summary
                .as_deref()
                .unwrap_or("verification failed"),
        ),
    }

    let payloads = load_legacy(path)?;
    if payloads.is_empty() {
        return Ok(());
    }

    let paths = PrismPaths::for_workspace_root(root)?;
    let active_key = load_active_runtime_signing_key(&paths)?;
    let temp_path = path.with_extension("jsonl.migrating");
    let mut file = File::create(&temp_path)
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    let mut prev_event_id = None;
    let mut prev_entry_hash = None;
    let mut sequence = 1_u64;

    for payload in &payloads {
        if let Some(event) = any_as_memory_event(payload) {
            if event.scope != MemoryScope::Repo {
                bail!(
                    "cannot migrate non-repo memory event `{}` from {}",
                    event.id,
                    path.display()
                );
            }
        }

        let mut envelope = ProtectedEventEnvelope {
            envelope_version: PROTECTED_EVENT_ENVELOPE_VERSION,
            stream: stream.stream().to_string(),
            stream_id: stream.stream_id().to_string(),
            event_id: event_id_for(payload).to_string(),
            prev_event_id: prev_event_id.clone(),
            prev_entry_hash: prev_entry_hash.clone(),
            sequence,
            runtime_authority_id: active_key.state.runtime_authority_id.clone(),
            runtime_key_id: active_key.runtime_key.runtime_key_id.clone(),
            trust_bundle_id: active_key.bundle.bundle_id.clone(),
            principal_authority_id: principal_for(payload).principal_authority_id,
            principal_id: principal_for(payload).principal_id,
            credential_id: principal_for(payload).credential_id,
            algorithm: ProtectedSignatureAlgorithm::Ed25519,
            payload_hash: String::new(),
            signature: String::new(),
            payload: payload.clone(),
        };
        envelope.sign_with(&active_key.signing_key)?;
        let entry_hash = envelope.computed_entry_hash()?;
        file.write_all(&envelope.canonical_entry_bytes()?)?;
        file.write_all(b"\n")?;
        prev_event_id = Some(envelope.event_id);
        prev_entry_hash = Some(entry_hash);
        sequence += 1;
    }
    file.sync_all()?;

    let backup_path = path.with_extension("jsonl.legacy-unsigned.bak");
    if backup_path.exists() {
        fs::remove_file(&backup_path)?;
    }
    fs::rename(path, &backup_path).with_context(|| {
        format!(
            "failed to move legacy protected stream {} to {}",
            path.display(),
            backup_path.display()
        )
    })?;
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to install migrated protected stream {} from {}",
            path.display(),
            temp_path.display()
        )
    })?;

    report.migrated_stream_count += 1;
    report.migrated_event_count += payloads.len();
    report.migrated_paths.push(path.to_path_buf());
    Ok(())
}

fn load_legacy_jsonl<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut values = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str::<T>(&line).with_context(|| {
            format!(
                "failed to parse legacy protected record on line {} in {}",
                index + 1,
                path.display()
            )
        })?;
        values.push(value);
    }
    Ok(values)
}

fn any_as_memory_event<T>(_value: &T) -> Option<&MemoryEvent> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use prism_projections::{
        ConceptEvent, ConceptEventAction, ConceptPacket, ConceptPublication,
        ConceptPublicationStatus,
    };

    use super::migrate_legacy_protected_repo_state;
    use crate::prism_paths::set_test_prism_home_override;

    #[test]
    fn migrates_legacy_unsigned_repo_concept_log_to_signed_stream() {
        let home = std::env::temp_dir().join(format!(
            "prism-protected-migration-home-{}",
            prism_ir::new_sortable_token()
        ));
        let _guard = set_test_prism_home_override(&home);
        let root = std::env::temp_dir().join(format!(
            "prism-protected-migration-workspace-{}",
            prism_ir::new_sortable_token()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        fs::create_dir_all(root.join(".prism").join("concepts")).unwrap();
        fs::write(
            root.join(".prism").join("concepts").join("events.jsonl"),
            format!(
                "{}\n",
                serde_json::to_string(&ConceptEvent {
                    id: "concept-event:test".to_string(),
                    recorded_at: 1,
                    task_id: None,
                    actor: None,
                    execution_context: None,
                    action: ConceptEventAction::Promote,
                    patch: None,
                    concept: ConceptPacket {
                        handle: "concept://demo".to_string(),
                        canonical_name: "demo".to_string(),
                        summary: "Signed concept summary for migration coverage.".to_string(),
                        aliases: vec!["demo".to_string()],
                        confidence: 0.91,
                        core_members: Vec::new(),
                        core_member_lineages: Vec::new(),
                        supporting_members: Vec::new(),
                        supporting_member_lineages: Vec::new(),
                        likely_tests: Vec::new(),
                        likely_test_lineages: Vec::new(),
                        evidence: vec!["migration".to_string()],
                        risk_hint: None,
                        decode_lenses: Vec::new(),
                        scope: prism_projections::ConceptScope::Repo,
                        provenance: prism_projections::ConceptProvenance {
                            origin: "test".to_string(),
                            kind: "migration".to_string(),
                            task_id: None,
                        },
                        publication: Some(ConceptPublication {
                            published_at: 1,
                            last_reviewed_at: Some(1),
                            status: ConceptPublicationStatus::Active,
                            supersedes: Vec::new(),
                            retired_at: None,
                            retirement_reason: None,
                        }),
                    },
                })
                .unwrap()
            ),
        )
        .unwrap();

        let report = migrate_legacy_protected_repo_state(&root).unwrap();
        assert_eq!(report.migrated_stream_count, 1);
        assert!(root
            .join(".prism")
            .join("concepts")
            .join("events.jsonl.legacy-unsigned.bak")
            .exists());
    }
}
