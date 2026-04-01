use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_core::{
    hydrate_workspace_session_with_options, BootstrapOwnerInput, MintPrincipalRequest, PrismPaths,
    SharedRuntimeBackend, WorkspaceSession, WorkspaceSessionOptions,
};
use prism_ir::{CredentialId, PrincipalAuthorityId, PrincipalId, PrincipalKind};
use serde_json::Value;

use crate::auth_storage::{CredentialProfile, CredentialsFile};
use crate::cli::{AuthCommand, PrincipalCommand};
use crate::parsing::{parse_credential_capability, parse_principal_kind};

pub(crate) fn handle_auth_command(root: &Path, command: AuthCommand) -> Result<()> {
    match command {
        AuthCommand::Init {
            name,
            authority,
            role,
        } => {
            let (session, credentials_path) = load_auth_session(root)?;
            let issued = session.bootstrap_owner_principal(BootstrapOwnerInput {
                authority_id: Some(PrincipalAuthorityId::new(authority)),
                name,
                role,
            })?;
            let mut credentials = CredentialsFile::load(&credentials_path)?;
            let stored = store_issued_credential(&mut credentials, &issued, true);
            credentials.save(&credentials_path)?;
            println!("initialized principal registry");
            print_issued_credential(&stored, &issued);
        }
        AuthCommand::Login {
            profile,
            principal,
            credential,
        } => {
            let (session, credentials_path) = load_auth_session(root)?;
            let mut credentials_file = CredentialsFile::load(&credentials_path)?;
            let selected = credentials_file
                .set_active_by_selector(
                    profile.as_deref(),
                    principal.as_deref(),
                    credential.as_deref(),
                )?
                .clone();
            session.authenticate_principal_credential(
                &CredentialId::new(selected.credential_id.clone()),
                &selected.principal_token,
            )?;
            credentials_file.save(&credentials_path)?;
            println!("logged in");
            println!("profile = {}", selected.profile);
            println!("principal_id = {}", selected.principal_id);
            println!("credential_id = {}", selected.credential_id);
        }
    }

    Ok(())
}

pub(crate) fn handle_principal_command(root: &Path, command: PrincipalCommand) -> Result<()> {
    match command {
        PrincipalCommand::Mint {
            kind,
            name,
            role,
            parent,
            authority,
            metadata_json,
            capabilities,
        } => {
            let (session, credentials_path) = load_auth_session(root)?;
            let mut credentials_file = CredentialsFile::load(&credentials_path)?;
            let active = credentials_file
                .set_active_by_selector(None, None, None)?
                .clone();
            let authenticated = session.authenticate_principal_credential(
                &CredentialId::new(active.credential_id.clone()),
                &active.principal_token,
            )?;
            let kind = parse_principal_kind(&kind)?;
            let parent_principal_id = parent
                .map(PrincipalId::new)
                .or_else(|| default_parent_for_kind(kind, &authenticated.principal.principal_id));
            let issued = session.mint_principal_credential(
                &authenticated,
                MintPrincipalRequest {
                    authority_id: authority.map(PrincipalAuthorityId::new),
                    kind,
                    name,
                    role,
                    parent_principal_id,
                    capabilities: capabilities
                        .iter()
                        .map(|capability| parse_credential_capability(capability))
                        .collect::<Result<Vec<_>>>()?,
                    profile: parse_metadata_json(metadata_json.as_deref())?,
                },
            )?;
            let stored = store_issued_credential(&mut credentials_file, &issued, true);
            credentials_file.save(&credentials_path)?;
            println!("minted principal");
            print_issued_credential(&stored, &issued);
        }
    }

    Ok(())
}

fn load_auth_session(root: &Path) -> Result<(WorkspaceSession, PathBuf)> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let credentials_path = paths.credentials_path()?;
    let session = hydrate_workspace_session_with_options(
        root,
        WorkspaceSessionOptions {
            coordination: false,
            shared_runtime: SharedRuntimeBackend::Sqlite {
                path: paths.shared_runtime_db_path()?,
            },
            hydrate_persisted_projections: false,
        },
    )?;
    Ok((session, credentials_path))
}

fn default_parent_for_kind(kind: PrincipalKind, principal_id: &PrincipalId) -> Option<PrincipalId> {
    match kind {
        PrincipalKind::Agent => Some(principal_id.clone()),
        _ => None,
    }
}

fn parse_metadata_json(value: Option<&str>) -> Result<Value> {
    match value {
        Some(value) => Ok(serde_json::from_str(value)?),
        None => Ok(Value::Null),
    }
}

fn store_issued_credential(
    credentials: &mut CredentialsFile,
    issued: &prism_core::MintedPrincipalCredential,
    set_active: bool,
) -> CredentialProfile {
    credentials
        .upsert_profile(
            CredentialProfile {
                profile: issued.principal.principal_id.0.to_string(),
                authority_id: issued.principal.authority_id.0.to_string(),
                principal_id: issued.principal.principal_id.0.to_string(),
                credential_id: issued.credential.credential_id.0.to_string(),
                principal_token: issued.principal_token.clone(),
            },
            set_active,
        )
        .clone()
}

fn print_issued_credential(
    stored: &CredentialProfile,
    issued: &prism_core::MintedPrincipalCredential,
) {
    println!("profile = {}", stored.profile);
    println!("authority_id = {}", issued.principal.authority_id.0);
    println!("principal_id = {}", issued.principal.principal_id.0);
    println!("principal_kind = {:?}", issued.principal.kind);
    println!("credential_id = {}", issued.credential.credential_id.0);
    println!("principal_token = {}", issued.principal_token);
}
