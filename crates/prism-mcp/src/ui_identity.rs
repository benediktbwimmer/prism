use std::path::Path;

use prism_core::{HumanSessionFile, PrismPaths, WorkspaceSession};

use crate::resource_schemas::BridgeIdentityView;
use crate::ui_credentials::{load_ui_credentials, resolve_ui_credential_profile};

pub(crate) fn ui_operator_identity_view(
    root: &Path,
    workspace: Option<&WorkspaceSession>,
) -> BridgeIdentityView {
    let (credentials_path, human_session_path) = match PrismPaths::for_workspace_root(root)
        .and_then(|paths| Ok((paths.credentials_path()?, paths.human_session_path()?)))
    {
        Ok(paths) => paths,
        Err(error) => {
            return BridgeIdentityView {
                status: "unavailable".to_string(),
                profile: None,
                principal_id: None,
                credential_id: None,
                worktree_id: None,
                agent_label: None,
                worktree_mode: None,
                error: Some(format!(
                    "failed to resolve local PRISM credentials path: {error}"
                )),
                next_action:
                    "Bootstrap a local PRISM owner profile before using the operator console."
                        .to_string(),
            };
        }
    };
    let credentials = match load_ui_credentials(root) {
        Ok(credentials) => credentials,
        Err(error) => {
            return BridgeIdentityView {
                status: "unavailable".to_string(),
                profile: None,
                principal_id: None,
                credential_id: None,
                worktree_id: None,
                agent_label: None,
                worktree_mode: None,
                error: Some(format!(
                    "failed to load local PRISM credentials from {}: {error}",
                    credentials_path.display()
                )),
                next_action:
                    "Run `prism auth login` or bootstrap the local owner principal before using the operator console."
                        .to_string(),
            };
        }
    };
    let profile = match resolve_ui_credential_profile(&credentials, workspace) {
        Ok(profile) => profile,
        Err(error) => {
            return BridgeIdentityView {
                status: "unavailable".to_string(),
                profile: None,
                principal_id: None,
                credential_id: None,
                worktree_id: None,
                agent_label: None,
                worktree_mode: None,
                error: Some(error.to_string()),
                next_action:
                    "Run `prism auth login` or bootstrap the local owner principal before using the operator console."
                        .to_string(),
            };
        }
    };
    let bound = workspace.and_then(WorkspaceSession::bound_worktree_principal);
    let mut sessions = HumanSessionFile::load(&human_session_path).ok();
    let active_session = sessions
        .as_mut()
        .and_then(HumanSessionFile::active_session_now);
    if let Some(bound) = bound.as_ref() {
        if bound.authority_id != profile.authority_id || bound.principal_id != profile.principal_id
        {
            return BridgeIdentityView {
                status: "conflict".to_string(),
                profile: Some(profile.profile.clone()),
                principal_id: Some(profile.principal_id.clone()),
                credential_id: Some(profile.credential_id.clone()),
                worktree_id: None,
                agent_label: None,
                worktree_mode: None,
                error: Some(format!(
                    "worktree is bound to `{}` while the active local profile resolves to `{}`",
                    bound.principal_id, profile.principal_id
                )),
                next_action:
                    "Switch the active local PRISM profile to the bound principal before using the operator console mutate endpoint."
                        .to_string(),
            };
        }
    }
    BridgeIdentityView {
        status: if active_session.is_some() {
            "unlocked_human_session".to_string()
        } else if bound.is_some() {
            "bound".to_string()
        } else {
            "locked_local_profile".to_string()
        },
        profile: Some(profile.profile.clone()),
        principal_id: Some(profile.principal_id.clone()),
        credential_id: Some(profile.credential_id.clone()),
        worktree_id: None,
        agent_label: None,
        worktree_mode: None,
        error: None,
        next_action: if active_session.is_some() {
            "The active local human session is unlocked for direct operator work in this worktree."
                .to_string()
        } else {
            "Run `prism auth login` to unlock a short-lived local human session before using direct operator mutation flows."
                .to_string()
        },
    }
}
