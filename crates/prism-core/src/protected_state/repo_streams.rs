use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use prism_ir::{
    EventActor, EventExecutionContext, PrincipalActor, PrincipalAuthorityId, PrincipalId,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use crate::protected_state::envelope::{
    ProtectedEventEnvelope, ProtectedSignatureAlgorithm, PROTECTED_EVENT_ENVELOPE_VERSION,
};
use crate::protected_state::streams::{
    ProtectedRepoStream, ProtectedStreamVerification, ProtectedVerificationStatus,
};
use crate::protected_state::trust::{load_active_runtime_signing_key, resolve_trusted_runtime_key};
use crate::PrismPaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProtectedPrincipalIdentity {
    pub(crate) principal_authority_id: String,
    pub(crate) principal_id: String,
    pub(crate) credential_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ProtectedStreamInspection<T> {
    pub(crate) verification: ProtectedStreamVerification,
    pub(crate) payloads: Vec<T>,
    pub(crate) next_sequence: u64,
    pub(crate) last_event_id: Option<String>,
    pub(crate) last_entry_hash: Option<String>,
}

pub(crate) fn implicit_principal_identity(
    actor: Option<&EventActor>,
    execution_context: Option<&EventExecutionContext>,
) -> ProtectedPrincipalIdentity {
    let principal = principal_actor_for_signature(actor);
    let scoped_actor_id = principal.scoped_id();
    let credential_id = execution_context
        .and_then(|context| {
            context
                .credential_id
                .as_ref()
                .map(|value| value.0.to_string())
        })
        .unwrap_or_else(|| format!("credential:implicit:{scoped_actor_id}"));
    ProtectedPrincipalIdentity {
        principal_authority_id: principal.authority_id.0.to_string(),
        principal_id: principal.principal_id.0.to_string(),
        credential_id,
    }
}

pub(crate) fn inspect_protected_stream<T>(
    root: &Path,
    stream: &ProtectedRepoStream,
) -> Result<ProtectedStreamInspection<T>>
where
    T: Serialize + DeserializeOwned + Clone,
{
    let path = root.join(stream.relative_path());
    if !path.exists() {
        return Ok(ProtectedStreamInspection {
            verification: verified_inspection(stream, None, None, None),
            payloads: Vec::new(),
            next_sequence: 1,
            last_event_id: None,
            last_entry_hash: None,
        });
    }

    let raw = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    if raw.is_empty() {
        return Ok(ProtectedStreamInspection {
            verification: verified_inspection(stream, None, None, None),
            payloads: Vec::new(),
            next_sequence: 1,
            last_event_id: None,
            last_entry_hash: None,
        });
    }
    if raw.last() != Some(&b'\n') {
        return Ok(classified_failure(
            stream,
            ProtectedVerificationStatus::Truncated,
            None,
            None,
            None,
            "protected_stream_truncated",
            format!(
                "protected stream {} ended with an unterminated final record",
                path.display()
            ),
            Some("repair or restore the stream before authoritative hydration".to_string()),
        ));
    }

    let paths = PrismPaths::for_workspace_root(root)?;
    let contents = std::str::from_utf8(&raw)
        .with_context(|| format!("protected stream {} is not valid UTF-8", path.display()))?;
    let mut payloads = Vec::new();
    let mut seen_entries = HashMap::new();
    let mut previous_envelope: Option<ProtectedEventEnvelope<Value>> = None;
    let mut previous_entry_hash: Option<String> = None;

    for (index, line) in contents.lines().enumerate() {
        if line.is_empty() {
            return Ok(classified_failure(
                stream,
                ProtectedVerificationStatus::Corrupt,
                previous_envelope
                    .as_ref()
                    .map(|envelope| envelope.event_id.clone()),
                previous_entry_hash.clone(),
                previous_envelope
                    .as_ref()
                    .map(|envelope| envelope.trust_bundle_id.clone()),
                "protected_stream_blank_line",
                format!(
                    "protected stream {} contained a blank line at record {}",
                    path.display(),
                    index + 1
                ),
                Some(
                    "remove blank lines and regenerate the stream from verified events".to_string(),
                ),
            ));
        }

        let envelope = match serde_json::from_str::<ProtectedEventEnvelope<Value>>(line) {
            Ok(envelope) => envelope,
            Err(envelope_error) => {
                if serde_json::from_str::<T>(line).is_ok() {
                    return Ok(classified_failure(
                        stream,
                        ProtectedVerificationStatus::LegacyUnsigned,
                        None,
                        None,
                        None,
                        "protected_stream_legacy_unsigned",
                        format!(
                            "protected stream {} still uses unsigned legacy records at line {}",
                            path.display(),
                            index + 1
                        ),
                        Some(
                            "run an explicit migrate-sign flow before using this stream authoritatively"
                                .to_string(),
                        ),
                    ));
                }
                return Ok(classified_failure(
                    stream,
                    ProtectedVerificationStatus::Corrupt,
                    previous_envelope
                        .as_ref()
                        .map(|candidate| candidate.event_id.clone()),
                    previous_entry_hash.clone(),
                    previous_envelope
                        .as_ref()
                        .map(|candidate| candidate.trust_bundle_id.clone()),
                    "protected_stream_parse_error",
                    format!(
                        "failed to parse protected stream {} line {}: {envelope_error}",
                        path.display(),
                        index + 1
                    ),
                    Some("repair or restore the stream from a verified source".to_string()),
                ));
            }
        };

        if envelope.envelope_version != PROTECTED_EVENT_ENVELOPE_VERSION {
            return Ok(classified_failure(
                stream,
                ProtectedVerificationStatus::Corrupt,
                previous_envelope
                    .as_ref()
                    .map(|candidate| candidate.event_id.clone()),
                previous_entry_hash.clone(),
                Some(envelope.trust_bundle_id.clone()),
                "protected_stream_unknown_envelope_version",
                format!(
                    "protected stream {} used unsupported envelope version {}",
                    path.display(),
                    envelope.envelope_version
                ),
                Some("upgrade the verifier or migrate the stream".to_string()),
            ));
        }
        if envelope.stream != stream.stream() || envelope.stream_id != stream.stream_id() {
            return Ok(classified_failure(
                stream,
                ProtectedVerificationStatus::Corrupt,
                previous_envelope
                    .as_ref()
                    .map(|candidate| candidate.event_id.clone()),
                previous_entry_hash.clone(),
                Some(envelope.trust_bundle_id.clone()),
                "protected_stream_identity_mismatch",
                format!(
                    "protected stream {} contained envelope for stream {} / {}",
                    path.display(),
                    envelope.stream,
                    envelope.stream_id
                ),
                Some(
                    "move the record back to its original stream or restore from backup"
                        .to_string(),
                ),
            ));
        }
        if envelope.algorithm != ProtectedSignatureAlgorithm::Ed25519 {
            return Ok(classified_failure(
                stream,
                ProtectedVerificationStatus::Corrupt,
                previous_envelope
                    .as_ref()
                    .map(|candidate| candidate.event_id.clone()),
                previous_entry_hash.clone(),
                Some(envelope.trust_bundle_id.clone()),
                "protected_stream_algorithm_mismatch",
                format!(
                    "protected stream {} used unsupported signature algorithm",
                    path.display()
                ),
                Some("migrate the stream to a supported signature algorithm".to_string()),
            ));
        }

        let trusted_key = match resolve_trusted_runtime_key(
            &paths,
            &envelope.trust_bundle_id,
            &envelope.runtime_authority_id,
            &envelope.runtime_key_id,
        ) {
            Ok(key) => key,
            Err(error) => {
                let error_text = error.to_string();
                let status = if error_text.contains("not imported locally")
                    || error_text.contains("unknown authority root")
                {
                    ProtectedVerificationStatus::UnknownTrust
                } else {
                    ProtectedVerificationStatus::Corrupt
                };
                return Ok(classified_failure(
                    stream,
                    status,
                    previous_envelope
                        .as_ref()
                        .map(|candidate| candidate.event_id.clone()),
                    previous_entry_hash.clone(),
                    Some(envelope.trust_bundle_id.clone()),
                    "protected_stream_trust_resolution_failed",
                    format!(
                        "failed to resolve trust for {} line {}: {error}",
                        path.display(),
                        index + 1
                    ),
                    Some(
                        "import the required trust bundle before hydrating this stream".to_string(),
                    ),
                ));
            }
        };

        if let Err(error) = envelope.verify_signature(&trusted_key.verifying_key) {
            return Ok(classified_failure(
                stream,
                ProtectedVerificationStatus::Tampered,
                previous_envelope
                    .as_ref()
                    .map(|candidate| candidate.event_id.clone()),
                previous_entry_hash.clone(),
                Some(envelope.trust_bundle_id.clone()),
                "protected_stream_signature_verification_failed",
                format!(
                    "protected stream {} failed signature verification on line {}: {error}",
                    path.display(),
                    index + 1
                ),
                Some("restore the tampered stream from a verified copy".to_string()),
            ));
        }

        let computed_entry_hash = envelope.computed_entry_hash()?;
        let typed_payload = match serde_json::from_value::<T>(envelope.payload.clone()) {
            Ok(payload) => payload,
            Err(error) => {
                return Ok(classified_failure(
                    stream,
                    ProtectedVerificationStatus::Corrupt,
                    Some(envelope.event_id.clone()),
                    Some(computed_entry_hash.clone()),
                    Some(envelope.trust_bundle_id.clone()),
                    "protected_stream_payload_deserialize_failed",
                    format!(
                        "protected stream {} contained an incompatible payload at line {}: {error}",
                        path.display(),
                        index + 1
                    ),
                    Some(
                        "migrate or repair the protected stream payload before typed hydration"
                            .to_string(),
                    ),
                ));
            }
        };
        if seen_entries
            .insert(envelope.event_id.clone(), computed_entry_hash.clone())
            .is_some()
        {
            return Ok(classified_failure(
                stream,
                ProtectedVerificationStatus::Conflict,
                previous_envelope
                    .as_ref()
                    .map(|candidate| candidate.event_id.clone()),
                previous_entry_hash.clone(),
                Some(envelope.trust_bundle_id.clone()),
                "protected_stream_duplicate_event_id",
                format!(
                    "protected stream {} repeated event id {}",
                    path.display(),
                    envelope.event_id
                ),
                Some("reconcile conflicting branches before hydrating this stream".to_string()),
            ));
        }

        match (
            &previous_envelope,
            &envelope.prev_event_id,
            &envelope.prev_entry_hash,
        ) {
            (None, None, None) => {}
            (None, _, _) => {
                return Ok(classified_failure(
                    stream,
                    ProtectedVerificationStatus::Tampered,
                    None,
                    None,
                    Some(envelope.trust_bundle_id.clone()),
                    "protected_stream_invalid_root",
                    format!(
                        "protected stream {} used a non-root predecessor on its first record",
                        path.display()
                    ),
                    Some("restore the root record or regenerate the stream".to_string()),
                ));
            }
            (Some(previous), Some(prev_event_id), Some(prev_entry_hash))
                if prev_event_id == &previous.event_id
                    && Some(prev_entry_hash.clone()) == previous_entry_hash => {}
            (Some(_), Some(prev_event_id), Some(prev_entry_hash))
                if seen_entries.get(prev_event_id) == Some(prev_entry_hash) =>
            {
                return Ok(classified_failure(
                    stream,
                    ProtectedVerificationStatus::Conflict,
                    previous_envelope
                        .as_ref()
                        .map(|candidate| candidate.event_id.clone()),
                    previous_entry_hash.clone(),
                    Some(envelope.trust_bundle_id.clone()),
                    "protected_stream_multiple_heads",
                    format!(
                        "protected stream {} contains multiple valid heads without reconciliation",
                        path.display()
                    ),
                    Some("reconcile the conflicting branch before hydration".to_string()),
                ));
            }
            (Some(_), _, _) => {
                return Ok(classified_failure(
                    stream,
                    ProtectedVerificationStatus::Tampered,
                    previous_envelope
                        .as_ref()
                        .map(|candidate| candidate.event_id.clone()),
                    previous_entry_hash.clone(),
                    Some(envelope.trust_bundle_id.clone()),
                    "protected_stream_predecessor_mismatch",
                    format!(
                        "protected stream {} had a predecessor hash mismatch at line {}",
                        path.display(),
                        index + 1
                    ),
                    Some("restore the stream to a linear verified history".to_string()),
                ));
            }
        }

        payloads.push(typed_payload);
        previous_entry_hash = Some(computed_entry_hash);
        previous_envelope = Some(envelope);
    }

    Ok(ProtectedStreamInspection {
        verification: verified_inspection(
            stream,
            previous_envelope
                .as_ref()
                .map(|envelope| envelope.event_id.clone()),
            previous_entry_hash.clone(),
            previous_envelope
                .as_ref()
                .map(|envelope| envelope.trust_bundle_id.clone()),
        ),
        payloads,
        next_sequence: previous_envelope
            .as_ref()
            .map_or(1, |envelope| envelope.sequence.saturating_add(1)),
        last_event_id: previous_envelope
            .as_ref()
            .map(|envelope| envelope.event_id.clone()),
        last_entry_hash: previous_entry_hash,
    })
}

pub(crate) fn append_protected_stream_event<T>(
    root: &Path,
    stream: &ProtectedRepoStream,
    event_id: &str,
    payload: &T,
    principal: &ProtectedPrincipalIdentity,
) -> Result<()>
where
    T: Serialize + DeserializeOwned + Clone,
{
    let inspection = inspect_protected_stream::<T>(root, stream)?;
    if inspection.verification.verification_status != ProtectedVerificationStatus::Verified {
        bail!(
            "refused to append to protected stream {} because its status is {:?}: {}",
            inspection.verification.protected_path,
            inspection.verification.verification_status,
            inspection
                .verification
                .diagnostic_summary
                .as_deref()
                .unwrap_or("verification failed")
        );
    }

    let paths = PrismPaths::for_workspace_root(root)?;
    let active_key = load_active_runtime_signing_key(&paths)?;
    let path = root.join(stream.relative_path());
    let created_new_file = !path.exists();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut envelope = ProtectedEventEnvelope {
        envelope_version: PROTECTED_EVENT_ENVELOPE_VERSION,
        stream: stream.stream().to_string(),
        stream_id: stream.stream_id().to_string(),
        event_id: event_id.to_string(),
        prev_event_id: inspection.last_event_id.clone(),
        prev_entry_hash: inspection.last_entry_hash.clone(),
        sequence: inspection.next_sequence,
        runtime_authority_id: active_key.state.runtime_authority_id.clone(),
        runtime_key_id: active_key.runtime_key.runtime_key_id.clone(),
        trust_bundle_id: active_key.bundle.bundle_id.clone(),
        principal_authority_id: principal.principal_authority_id.clone(),
        principal_id: principal.principal_id.clone(),
        credential_id: principal.credential_id.clone(),
        algorithm: ProtectedSignatureAlgorithm::Ed25519,
        payload_hash: String::new(),
        signature: String::new(),
        payload: payload.clone(),
    };
    envelope.sign_with(&active_key.signing_key)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.write_all(&envelope.canonical_entry_bytes()?)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    if created_new_file {
        sync_parent_dir(&path)?;
    }
    Ok(())
}

pub(crate) fn rewrite_protected_stream_events<T, I>(
    root: &Path,
    stream: &ProtectedRepoStream,
    events: I,
    principal: &ProtectedPrincipalIdentity,
) -> Result<()>
where
    T: Serialize + DeserializeOwned + Clone,
    I: IntoIterator<Item = (String, T)>,
{
    let paths = PrismPaths::for_workspace_root(root)?;
    let active_key = load_active_runtime_signing_key(&paths)?;
    let path = root.join(stream.relative_path());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("jsonl.tmp");
    let mut file = File::create(&tmp_path)
        .with_context(|| format!("failed to create {}", tmp_path.display()))?;
    let mut prev_event_id = None;
    let mut prev_entry_hash = None;
    let mut sequence = 1u64;
    let mut wrote_any = false;

    for (event_id, payload) in events {
        let mut envelope = ProtectedEventEnvelope {
            envelope_version: PROTECTED_EVENT_ENVELOPE_VERSION,
            stream: stream.stream().to_string(),
            stream_id: stream.stream_id().to_string(),
            event_id,
            prev_event_id: prev_event_id.clone(),
            prev_entry_hash: prev_entry_hash.clone(),
            sequence,
            runtime_authority_id: active_key.state.runtime_authority_id.clone(),
            runtime_key_id: active_key.runtime_key.runtime_key_id.clone(),
            trust_bundle_id: active_key.bundle.bundle_id.clone(),
            principal_authority_id: principal.principal_authority_id.clone(),
            principal_id: principal.principal_id.clone(),
            credential_id: principal.credential_id.clone(),
            algorithm: ProtectedSignatureAlgorithm::Ed25519,
            payload_hash: String::new(),
            signature: String::new(),
            payload,
        };
        envelope.sign_with(&active_key.signing_key)?;
        let entry_bytes = envelope.canonical_entry_bytes()?;
        prev_event_id = Some(envelope.event_id.clone());
        prev_entry_hash = Some(envelope.computed_entry_hash()?);
        file.write_all(&entry_bytes)?;
        file.write_all(b"\n")?;
        sequence = sequence.saturating_add(1);
        wrote_any = true;
    }

    file.sync_all()?;
    drop(file);

    if wrote_any {
        fs::rename(&tmp_path, &path)
            .with_context(|| format!("failed to replace {}", path.display()))?;
        sync_parent_dir(&path)?;
    } else {
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        if tmp_path.exists() {
            fs::remove_file(&tmp_path)
                .with_context(|| format!("failed to remove {}", tmp_path.display()))?;
        }
        sync_parent_dir(&path)?;
    }
    Ok(())
}

fn principal_actor_for_signature(actor: Option<&EventActor>) -> PrincipalActor {
    match actor
        .cloned()
        .unwrap_or(EventActor::Agent)
        .canonical_identity_actor()
    {
        EventActor::Principal(actor) => actor,
        EventActor::System => PrincipalActor {
            authority_id: PrincipalAuthorityId::new("legacy"),
            principal_id: PrincipalId::new("legacy_system_fallback"),
            kind: None,
            name: Some("legacy_system_fallback".to_string()),
        },
        EventActor::CI => PrincipalActor {
            authority_id: PrincipalAuthorityId::new("legacy"),
            principal_id: PrincipalId::new("legacy_ci_fallback"),
            kind: None,
            name: Some("legacy_ci_fallback".to_string()),
        },
        EventActor::GitAuthor { name, email } => PrincipalActor {
            authority_id: PrincipalAuthorityId::new("legacy"),
            principal_id: PrincipalId::new(format!(
                "legacy_git_author:{}:{}",
                name,
                email.unwrap_or_default()
            )),
            kind: None,
            name: Some(name.to_string()),
        },
        EventActor::User | EventActor::Agent => {
            unreachable!("canonical identity actor normalizes user and agent")
        }
    }
}

fn sync_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    File::open(parent)
        .with_context(|| format!("failed to open parent directory {}", parent.display()))?
        .sync_all()
        .with_context(|| format!("failed to fsync parent directory {}", parent.display()))
}

fn verified_inspection(
    stream: &ProtectedRepoStream,
    last_verified_event_id: Option<String>,
    last_verified_entry_hash: Option<String>,
    trust_bundle_id: Option<String>,
) -> ProtectedStreamVerification {
    ProtectedStreamVerification {
        verification_status: ProtectedVerificationStatus::Verified,
        stream_id: stream.stream_id().to_string(),
        protected_path: stream.relative_path().display().to_string(),
        last_verified_event_id,
        last_verified_entry_hash,
        trust_bundle_id,
        diagnostic_code: None,
        diagnostic_summary: None,
        repair_hint: None,
    }
}

fn classified_failure<T>(
    stream: &ProtectedRepoStream,
    status: ProtectedVerificationStatus,
    last_verified_event_id: Option<String>,
    last_verified_entry_hash: Option<String>,
    trust_bundle_id: Option<String>,
    diagnostic_code: &str,
    diagnostic_summary: String,
    repair_hint: Option<String>,
) -> ProtectedStreamInspection<T> {
    ProtectedStreamInspection {
        verification: ProtectedStreamVerification {
            verification_status: status,
            stream_id: stream.stream_id().to_string(),
            protected_path: stream.relative_path().display().to_string(),
            last_verified_event_id,
            last_verified_entry_hash,
            trust_bundle_id,
            diagnostic_code: Some(diagnostic_code.to_string()),
            diagnostic_summary: Some(diagnostic_summary),
            repair_hint,
        },
        payloads: Vec::new(),
        next_sequence: 0,
        last_event_id: None,
        last_entry_hash: None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::{
        append_protected_stream_event, implicit_principal_identity, inspect_protected_stream,
    };
    use crate::prism_paths::set_test_prism_home_override;
    use crate::protected_state::streams::{ProtectedRepoStream, ProtectedVerificationStatus};

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

    fn track_temp_dir(path: &std::path::Path) {
        TEMP_TEST_DIRS.with(|state| state.borrow_mut().paths.push(path.to_path_buf()));
    }

    fn temp_workspace(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "prism-protected-repo-streams-{label}-{}",
            prism_ir::new_sortable_token()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        track_temp_dir(&root);
        root
    }

    #[test]
    fn appending_and_loading_signed_stream_round_trips_payloads() {
        let home = temp_workspace("home-round-trip");
        let _guard = set_test_prism_home_override(&home);
        let workspace = temp_workspace("workspace-round-trip");
        let stream = ProtectedRepoStream::concept_events();
        let principal = implicit_principal_identity(None, None);

        append_protected_stream_event(
            &workspace,
            &stream,
            "event:test-1",
            &json!({"kind":"concept","value":"alpha"}),
            &principal,
        )
        .unwrap();
        append_protected_stream_event(
            &workspace,
            &stream,
            "event:test-2",
            &json!({"kind":"concept","value":"beta"}),
            &principal,
        )
        .unwrap();

        let inspection =
            inspect_protected_stream::<serde_json::Value>(&workspace, &stream).unwrap();
        assert_eq!(
            inspection.verification.verification_status,
            ProtectedVerificationStatus::Verified
        );
        assert_eq!(inspection.payloads.len(), 2);
        assert_eq!(inspection.next_sequence, 3);
    }

    #[test]
    fn legacy_unsigned_streams_are_classified_without_hydrating() {
        let home = temp_workspace("home-legacy");
        let _guard = set_test_prism_home_override(&home);
        let workspace = temp_workspace("workspace-legacy");
        let path = workspace.join(".prism/concepts/events.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\"id\":\"legacy\"}\n").unwrap();

        let inspection = inspect_protected_stream::<serde_json::Value>(
            &workspace,
            &ProtectedRepoStream::concept_events(),
        )
        .unwrap();
        assert_eq!(
            inspection.verification.verification_status,
            ProtectedVerificationStatus::LegacyUnsigned
        );
        assert!(inspection.payloads.is_empty());
    }

    #[test]
    fn missing_imported_bundle_yields_unknown_trust() {
        let home_a = temp_workspace("home-trust-a");
        let _guard_a = set_test_prism_home_override(&home_a);
        let workspace = temp_workspace("workspace-trust");
        let stream = ProtectedRepoStream::contract_events();
        let principal = implicit_principal_identity(None, None);
        append_protected_stream_event(
            &workspace,
            &stream,
            "event:test-contract",
            &json!({"kind":"contract"}),
            &principal,
        )
        .unwrap();

        let home_b = temp_workspace("home-trust-b");
        let _guard_b = set_test_prism_home_override(&home_b);
        let inspection =
            inspect_protected_stream::<serde_json::Value>(&workspace, &stream).unwrap();
        assert_eq!(
            inspection.verification.verification_status,
            ProtectedVerificationStatus::UnknownTrust
        );
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct CompatibilityPayload {
        required: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        optional: Option<String>,
    }

    #[test]
    fn typed_hydration_uses_raw_verified_payload_bytes() {
        let home = temp_workspace("home-compat");
        let _guard = set_test_prism_home_override(&home);
        let workspace = temp_workspace("workspace-compat");
        let stream = ProtectedRepoStream::concept_events();
        let principal = implicit_principal_identity(None, None);

        append_protected_stream_event(
            &workspace,
            &stream,
            "event:test-compat",
            &json!({
                "required": "alpha",
                "optional": null
            }),
            &principal,
        )
        .unwrap();

        let inspection = inspect_protected_stream::<CompatibilityPayload>(&workspace, &stream)
            .expect("typed hydration should accept raw verified payloads");
        assert_eq!(
            inspection.verification.verification_status,
            ProtectedVerificationStatus::Verified
        );
        assert_eq!(
            inspection.payloads,
            vec![CompatibilityPayload {
                required: "alpha".to_string(),
                optional: None,
            }]
        );
    }

    #[test]
    fn unterminated_final_record_is_truncated() {
        let home = temp_workspace("home-truncated");
        let _guard = set_test_prism_home_override(&home);
        let workspace = temp_workspace("workspace-truncated");
        let path = workspace.join(".prism/contracts/events.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\"bad\":true}").unwrap();

        let inspection = inspect_protected_stream::<serde_json::Value>(
            &workspace,
            &ProtectedRepoStream::contract_events(),
        )
        .unwrap();
        assert_eq!(
            inspection.verification.verification_status,
            ProtectedVerificationStatus::Truncated
        );
    }
}
