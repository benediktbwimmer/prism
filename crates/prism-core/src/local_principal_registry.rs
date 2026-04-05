use std::path::Path;

use anyhow::Result;
use prism_ir::{
    CredentialCapability, CredentialId, CredentialRecord, CredentialStatus,
    HumanPrincipalProfile, PrincipalAuthorityId, PrincipalId, PrincipalKind, PrincipalProfile,
    PrincipalRegistrySnapshot, PrincipalStatus,
};
use prism_store::MaterializationStore;
use serde_json::Value;

use crate::local_credentials::{
    CredentialProfile, CredentialProfileCredentialMetadata, CredentialProfilePrincipalMetadata,
    CredentialsFile, HumanSessionFile, HumanSessionRecord,
};
use crate::principal_registry::credential_token_verifier;
use crate::util::current_timestamp;
use crate::PrismPaths;

pub fn ensure_local_principal_registry_snapshot<S: MaterializationStore>(
    root: &Path,
    store: &mut S,
) -> Result<Option<PrincipalRegistrySnapshot>> {
    if let Some(snapshot) = store.load_principal_registry_snapshot()? {
        if !snapshot.principals.is_empty() || !snapshot.credentials.is_empty() {
            return Ok(Some(snapshot));
        }
    }

    let paths = PrismPaths::for_workspace_root(root)?;
    let credentials = CredentialsFile::load(&paths.credentials_path()?)?;
    let mut sessions = HumanSessionFile::load(&paths.human_session_path()?)?;
    let active_human_session = sessions.active_session(current_timestamp(), false);
    let Some(snapshot) =
        rebuild_registry_snapshot_from_local_credentials(&credentials, active_human_session.as_ref())
    else {
        return Ok(None);
    };
    store.save_principal_registry_snapshot(&snapshot)?;
    Ok(Some(snapshot))
}

pub fn ensure_local_principal_registry_snapshot_with_unlocked_profile<S: MaterializationStore>(
    root: &Path,
    store: &mut S,
    unlocked_profile: &CredentialProfile,
    principal_token: &str,
) -> Result<Option<PrincipalRegistrySnapshot>> {
    if let Some(snapshot) = store.load_principal_registry_snapshot()? {
        if !snapshot.principals.is_empty() || !snapshot.credentials.is_empty() {
            return Ok(Some(snapshot));
        }
    }

    let paths = PrismPaths::for_workspace_root(root)?;
    let credentials = CredentialsFile::load(&paths.credentials_path()?)?;
    let mut sessions = HumanSessionFile::load(&paths.human_session_path()?)?;
    let active_human_session = sessions.active_session(current_timestamp(), false);
    let Some(snapshot) = rebuild_registry_snapshot_from_local_credentials_with_unlocked_profile(
        &credentials,
        active_human_session.as_ref(),
        Some((unlocked_profile, principal_token)),
    ) else {
        return Ok(None);
    };
    store.save_principal_registry_snapshot(&snapshot)?;
    Ok(Some(snapshot))
}

pub(crate) fn rebuild_registry_snapshot_from_local_credentials(
    credentials: &CredentialsFile,
    active_human_session: Option<&HumanSessionRecord>,
) -> Option<PrincipalRegistrySnapshot> {
    rebuild_registry_snapshot_from_local_credentials_with_unlocked_profile(
        credentials,
        active_human_session,
        None,
    )
}

fn rebuild_registry_snapshot_from_local_credentials_with_unlocked_profile(
    credentials: &CredentialsFile,
    active_human_session: Option<&HumanSessionRecord>,
    unlocked_profile: Option<(&CredentialProfile, &str)>,
) -> Option<PrincipalRegistrySnapshot> {
    let now = current_timestamp();
    let mut snapshot = PrincipalRegistrySnapshot::default();

    for profile in &credentials.profiles {
        let Some(principal) =
            build_principal_profile(profile, active_human_session, unlocked_profile, now)
        else {
            continue;
        };
        let Some(credential) =
            build_credential_record(profile, active_human_session, unlocked_profile, now)
        else {
            continue;
        };
        if !snapshot
            .principals
            .iter()
            .any(|candidate| candidate.principal_id == principal.principal_id)
        {
            snapshot.principals.push(principal);
        }
        if !snapshot
            .credentials
            .iter()
            .any(|candidate| candidate.credential_id == credential.credential_id)
        {
            snapshot.credentials.push(credential);
        }
    }

    if snapshot.principals.is_empty() || snapshot.credentials.is_empty() {
        None
    } else {
        Some(snapshot)
    }
}

fn build_principal_profile(
    profile: &CredentialProfile,
    active_human_session: Option<&HumanSessionRecord>,
    unlocked_profile: Option<(&CredentialProfile, &str)>,
    now: u64,
) -> Option<PrincipalProfile> {
    if let Some(metadata) = profile.principal_metadata.as_ref() {
        return Some(metadata_to_principal_profile(profile, metadata));
    }

    if matches_active_human_session(profile, active_human_session)
        || matches_unlocked_profile(profile, unlocked_profile)
    {
        return Some(PrincipalProfile {
            authority_id: PrincipalAuthorityId::new(profile.authority_id.clone()),
            principal_id: PrincipalId::new(profile.principal_id.clone()),
            kind: PrincipalKind::Human,
            name: infer_human_name(profile),
            role: None,
            status: PrincipalStatus::Active,
            created_at: now,
            updated_at: now,
            parent_principal_id: None,
            profile: serde_json::to_value(HumanPrincipalProfile { attestation: None })
                .unwrap_or(Value::Null),
        });
    }

    build_credential_record(profile, active_human_session, unlocked_profile, now).map(|_| {
        PrincipalProfile {
            authority_id: PrincipalAuthorityId::new(profile.authority_id.clone()),
            principal_id: PrincipalId::new(profile.principal_id.clone()),
            kind: PrincipalKind::Agent,
            name: profile.profile.clone(),
            role: None,
            status: PrincipalStatus::Active,
            created_at: now,
            updated_at: now,
            parent_principal_id: None,
            profile: Value::Null,
        }
    })
}

fn build_credential_record(
    profile: &CredentialProfile,
    active_human_session: Option<&HumanSessionRecord>,
    unlocked_profile: Option<(&CredentialProfile, &str)>,
    now: u64,
) -> Option<CredentialRecord> {
    if let Some(metadata) = profile.credential_metadata.as_ref() {
        return Some(metadata_to_credential_record(profile, metadata));
    }

    let token_verifier = profile
        .has_inline_principal_token()
        .then(|| credential_token_verifier(&profile.principal_token))
        .or_else(|| {
            active_human_session
                .filter(|session| matches_active_human_session(profile, Some(session)))
                .map(|session| credential_token_verifier(&session.principal_token))
        })
        .or_else(|| {
            unlocked_profile
                .filter(|(unlocked, _)| same_profile_identity(profile, unlocked))
                .map(|(_, principal_token)| credential_token_verifier(principal_token))
        })?;

    Some(CredentialRecord {
        credential_id: CredentialId::new(profile.credential_id.clone()),
        authority_id: PrincipalAuthorityId::new(profile.authority_id.clone()),
        principal_id: PrincipalId::new(profile.principal_id.clone()),
        token_verifier,
        capabilities: default_capabilities_for_profile(profile, active_human_session),
        status: CredentialStatus::Active,
        created_at: now,
        last_used_at: None,
        revoked_at: None,
    })
}

fn matches_active_human_session(
    profile: &CredentialProfile,
    active_human_session: Option<&HumanSessionRecord>,
) -> bool {
    let Some(session) = active_human_session else {
        return false;
    };
    session.profile == profile.profile
        && session.principal_id == profile.principal_id
        && session.credential_id == profile.credential_id
}

fn matches_unlocked_profile(
    profile: &CredentialProfile,
    unlocked_profile: Option<(&CredentialProfile, &str)>,
) -> bool {
    unlocked_profile.is_some_and(|(unlocked, _)| same_profile_identity(profile, unlocked))
}

fn same_profile_identity(left: &CredentialProfile, right: &CredentialProfile) -> bool {
    left.profile == right.profile
        && left.principal_id == right.principal_id
        && left.credential_id == right.credential_id
}

fn metadata_to_principal_profile(
    profile: &CredentialProfile,
    metadata: &CredentialProfilePrincipalMetadata,
) -> PrincipalProfile {
    PrincipalProfile {
        authority_id: PrincipalAuthorityId::new(profile.authority_id.clone()),
        principal_id: PrincipalId::new(profile.principal_id.clone()),
        kind: metadata.kind,
        name: metadata.name.clone(),
        role: metadata.role.clone(),
        status: metadata.status,
        created_at: metadata.created_at,
        updated_at: metadata.updated_at,
        parent_principal_id: metadata
            .parent_principal_id
            .as_ref()
            .map(|value| PrincipalId::new(value.clone())),
        profile: metadata.profile.clone(),
    }
}

fn metadata_to_credential_record(
    profile: &CredentialProfile,
    metadata: &CredentialProfileCredentialMetadata,
) -> CredentialRecord {
    CredentialRecord {
        credential_id: CredentialId::new(profile.credential_id.clone()),
        authority_id: PrincipalAuthorityId::new(profile.authority_id.clone()),
        principal_id: PrincipalId::new(profile.principal_id.clone()),
        token_verifier: metadata.token_verifier.clone(),
        capabilities: metadata.capabilities.clone(),
        status: metadata.status,
        created_at: metadata.created_at,
        last_used_at: metadata.last_used_at,
        revoked_at: metadata.revoked_at,
    }
}

fn default_capabilities_for_profile(
    _profile: &CredentialProfile,
    _active_human_session: Option<&HumanSessionRecord>,
) -> Vec<CredentialCapability> {
    vec![CredentialCapability::All]
}

fn infer_human_name(profile: &CredentialProfile) -> String {
    let raw = profile
        .principal_id
        .strip_prefix("principal:")
        .and_then(|value| value.rsplit_once(':').map(|(name, _)| name))
        .or_else(|| {
            profile
                .profile
                .strip_prefix("principal:")
                .and_then(|value| value.rsplit_once(':').map(|(name, _)| name))
        })
        .unwrap_or(profile.profile.as_str());
    raw.replace('-', " ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::rebuild_registry_snapshot_from_local_credentials;
    use crate::local_credentials::{
        CredentialProfile, CredentialProfileCredentialMetadata,
        CredentialProfilePrincipalMetadata, CredentialsFile, HumanSessionRecord,
    };
    use prism_ir::{
        CredentialCapability, CredentialStatus, PrincipalKind, PrincipalStatus,
    };

    #[test]
    fn rebuild_registry_snapshot_uses_stored_metadata_and_human_session_fallbacks() {
        let credentials = CredentialsFile {
            version: 3,
            active_profile: Some("owner".to_string()),
            profiles: vec![
                CredentialProfile {
                    profile: "owner".to_string(),
                    authority_id: "local-daemon".to_string(),
                    principal_id: "principal:owner".to_string(),
                    credential_id: "credential:owner".to_string(),
                    principal_token: String::new(),
                    encrypted_secret: None,
                    principal_metadata: Some(CredentialProfilePrincipalMetadata {
                        kind: PrincipalKind::Human,
                        name: "Owner".to_string(),
                        role: Some("repo_owner".to_string()),
                        status: PrincipalStatus::Active,
                        created_at: 11,
                        updated_at: 12,
                        parent_principal_id: None,
                        profile: json!({ "attestation": { "issuer": "github" } }),
                    }),
                    credential_metadata: Some(CredentialProfileCredentialMetadata {
                        token_verifier: "verifier:owner".to_string(),
                        capabilities: vec![CredentialCapability::All],
                        status: CredentialStatus::Active,
                        created_at: 13,
                        last_used_at: Some(14),
                        revoked_at: None,
                    }),
                },
                CredentialProfile {
                    profile: "codex-d".to_string(),
                    authority_id: "local-daemon".to_string(),
                    principal_id: "principal:codex-d:01legacy".to_string(),
                    credential_id: "credential:codex-d".to_string(),
                    principal_token: "token:codex-d".to_string(),
                    encrypted_secret: None,
                    principal_metadata: None,
                    credential_metadata: None,
                },
            ],
        };
        let session = HumanSessionRecord {
            profile: "owner".to_string(),
            authority_id: "local-daemon".to_string(),
            principal_id: "principal:owner".to_string(),
            credential_id: "credential:owner".to_string(),
            principal_token: "token:owner".to_string(),
            unlocked_at: 1,
            last_used_at: 1,
            idle_timeout_secs: 900,
            absolute_expires_at: 999,
            fresh_until: 300,
        };

        let snapshot =
            rebuild_registry_snapshot_from_local_credentials(&credentials, Some(&session)).unwrap();

        assert_eq!(snapshot.principals.len(), 2);
        assert_eq!(snapshot.credentials.len(), 2);
        assert!(snapshot
            .principals
            .iter()
            .any(|principal| principal.kind == PrincipalKind::Human && principal.name == "Owner"));
        assert!(snapshot
            .principals
            .iter()
            .any(|principal| principal.kind == PrincipalKind::Agent && principal.name == "codex-d"));
    }
}
