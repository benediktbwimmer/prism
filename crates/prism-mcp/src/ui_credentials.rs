use std::path::Path;

use anyhow::{Result, anyhow};
use prism_core::{CredentialProfile, CredentialsFile, PrismPaths, WorkspaceSession};
use prism_ir::{CredentialStatus, PrincipalRegistrySnapshot};

pub(crate) fn load_ui_credentials(root: &Path) -> Result<CredentialsFile> {
    let credentials_path = PrismPaths::for_workspace_root(root)?.credentials_path()?;
    CredentialsFile::load(&credentials_path)
}

pub(crate) fn resolve_ui_credential_profile(
    credentials: &CredentialsFile,
    workspace: Option<&WorkspaceSession>,
) -> Result<CredentialProfile> {
    select_ui_credential_profile(
        credentials,
        workspace
            .and_then(WorkspaceSession::bound_worktree_principal)
            .as_ref()
            .map(|bound| bound.principal_id.as_str()),
        workspace
            .and_then(|session| session.load_principal_registry().ok().flatten())
            .as_ref(),
    )
    .cloned()
}

fn select_ui_credential_profile<'a>(
    credentials: &'a CredentialsFile,
    bound_principal_id: Option<&str>,
    registry: Option<&PrincipalRegistrySnapshot>,
) -> Result<&'a CredentialProfile> {
    let profiles = credentials.profiles.iter().enumerate().collect::<Vec<_>>();
    if profiles.is_empty() {
        return Err(anyhow!(
            "no stored credential matched the requested selector"
        ));
    }

    let valid_credentials = registry.map(|snapshot| {
        snapshot
            .credentials
            .iter()
            .filter(|credential| credential.status == CredentialStatus::Active)
            .map(|credential| credential.credential_id.0.clone())
            .collect::<std::collections::HashSet<_>>()
    });

    let valid_profiles = valid_credentials.as_ref().map(|valid_ids| {
        profiles
            .iter()
            .copied()
            .filter(|(_, profile)| valid_ids.contains(profile.credential_id.as_str()))
            .collect::<Vec<_>>()
    });

    let bound_candidates = |items: &Vec<(usize, &'a CredentialProfile)>| {
        if let Some(bound_principal_id) = bound_principal_id {
            let matching = items
                .iter()
                .copied()
                .filter(|(_, profile)| profile.principal_id == bound_principal_id)
                .collect::<Vec<_>>();
            if !matching.is_empty() {
                return matching;
            }
        }
        items.clone()
    };

    let candidates = if let Some(valid_profiles) = valid_profiles.as_ref() {
        if !valid_profiles.is_empty() {
            bound_candidates(valid_profiles)
        } else {
            bound_candidates(&profiles)
        }
    } else {
        bound_candidates(&profiles)
    };

    let active_profile = credentials.active_profile.as_deref();
    candidates
        .iter()
        .copied()
        .max_by_key(|(index, profile)| (active_profile == Some(profile.profile.as_str()), *index))
        .map(|(_, profile)| profile)
        .ok_or_else(|| anyhow!("no stored credential matched the requested selector"))
}

#[cfg(test)]
mod tests {
    use prism_core::{CredentialProfile, CredentialsFile};
    use prism_ir::{
        CredentialCapability, CredentialId, CredentialRecord, CredentialStatus,
        PrincipalAuthorityId, PrincipalId, PrincipalRegistrySnapshot,
    };

    use super::select_ui_credential_profile;

    fn profile(profile: &str, principal_id: &str, credential_id: &str) -> CredentialProfile {
        CredentialProfile {
            profile: profile.to_string(),
            authority_id: "local-daemon".to_string(),
            principal_id: principal_id.to_string(),
            credential_id: credential_id.to_string(),
            principal_token: format!("token:{credential_id}"),
            encrypted_secret: None,
            principal_metadata: None,
            credential_metadata: None,
        }
    }

    fn active_registry(credential_id: &str, principal_id: &str) -> PrincipalRegistrySnapshot {
        PrincipalRegistrySnapshot {
            principals: Vec::new(),
            credentials: vec![CredentialRecord {
                credential_id: CredentialId::new(credential_id),
                authority_id: PrincipalAuthorityId::new("local-daemon"),
                principal_id: PrincipalId::new(principal_id),
                token_verifier: "sha256:test".to_string(),
                capabilities: vec![CredentialCapability::MutateCoordination],
                status: CredentialStatus::Active,
                created_at: 1,
                last_used_at: None,
                revoked_at: None,
            }],
        }
    }

    #[test]
    fn prefers_valid_registry_profile_over_stale_active_profile() {
        let credentials = CredentialsFile {
            version: 1,
            active_profile: Some("stale".to_string()),
            profiles: vec![
                profile("stale", "principal:stale", "credential:stale"),
                profile("fresh", "principal:fresh", "credential:fresh"),
            ],
        };

        let selected = select_ui_credential_profile(
            &credentials,
            None,
            Some(&active_registry("credential:fresh", "principal:fresh")),
        )
        .unwrap();

        assert_eq!(selected.profile, "fresh");
    }

    #[test]
    fn prefers_bound_principal_when_matching_valid_profile_exists() {
        let credentials = CredentialsFile {
            version: 1,
            active_profile: Some("other".to_string()),
            profiles: vec![
                profile("other", "principal:other", "credential:other"),
                profile("bound", "principal:bound", "credential:bound"),
            ],
        };

        let selected = select_ui_credential_profile(
            &credentials,
            Some("principal:bound"),
            Some(&active_registry("credential:bound", "principal:bound")),
        )
        .unwrap();

        assert_eq!(selected.profile, "bound");
    }
}
