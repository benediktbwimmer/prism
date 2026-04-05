use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use prism_core::{
    authenticate_principal_credential_in_registry, bootstrap_owner_principal_in_registry,
    mint_principal_credential_in_registry, recover_owner_principal_in_registry,
    AttestedHumanPrincipalInput, CredentialProfile, CredentialsFile, MintPrincipalRequest,
    PrismPaths,
};
use prism_ir::{
    CredentialId, HumanAttestationAssurance, HumanAttestationOperation, HumanAttestationRecord,
    HumanPrincipalProfile, PrincipalAuthorityId, PrincipalId, PrincipalKind,
    PrincipalRegistrySnapshot,
};
use prism_store::{SqliteStore, Store};
use serde_json::Value;

use crate::cli::{AuthAssuranceArg, AuthCommand, PrincipalCommand};
use crate::git_support::ensure_repo_git_support;
use crate::parsing::{parse_credential_capability, parse_principal_kind};

pub(crate) fn handle_auth_command(root: &Path, command: AuthCommand) -> Result<()> {
    match command {
        AuthCommand::Bootstrap {
            name,
            authority,
            role,
            issuer,
            subject,
            assurance,
        } => {
            let (mut store, credentials_path) = load_auth_registry_store(root)?;
            let mut snapshot = store
                .load_principal_registry_snapshot()?
                .unwrap_or_default();
            let issued = bootstrap_owner_principal_in_registry(
                &mut snapshot,
                AttestedHumanPrincipalInput {
                    authority_id: Some(PrincipalAuthorityId::new(authority)),
                    name,
                    role,
                    attestation: HumanAttestationRecord {
                        issuer,
                        subject,
                        assurance: map_assurance_arg(assurance),
                        operation: HumanAttestationOperation::Bootstrap,
                        verified_at: current_unix_timestamp(),
                    },
                },
            )?;
            store.save_principal_registry_snapshot(&snapshot)?;
            let mut credentials = CredentialsFile::load(&credentials_path)?;
            let stored = store_issued_credential(&mut credentials, &issued, None, false);
            credentials.save(&credentials_path)?;
            println!("initialized principal registry");
            print_issued_credential(&stored, &issued);
        }
        AuthCommand::Recover {
            name,
            authority,
            role,
            issuer,
            subject,
            assurance,
        } => {
            let (mut store, credentials_path) = load_auth_registry_store(root)?;
            let mut snapshot = store
                .load_principal_registry_snapshot()?
                .unwrap_or_default();
            let issued = recover_owner_principal_in_registry(
                &mut snapshot,
                AttestedHumanPrincipalInput {
                    authority_id: Some(PrincipalAuthorityId::new(authority)),
                    name,
                    role,
                    attestation: HumanAttestationRecord {
                        issuer,
                        subject,
                        assurance: map_assurance_arg(assurance),
                        operation: HumanAttestationOperation::Recovery,
                        verified_at: current_unix_timestamp(),
                    },
                },
            )?;
            store.save_principal_registry_snapshot(&snapshot)?;
            let mut credentials = CredentialsFile::load(&credentials_path)?;
            let stored = store_issued_credential(&mut credentials, &issued, None, false);
            credentials.save(&credentials_path)?;
            println!("initialized principal registry from recovery attestation");
            print_issued_credential(&stored, &issued);
        }
        AuthCommand::Login {
            profile,
            principal,
            credential,
        } => {
            let (mut store, credentials_path) = load_auth_registry_store(root)?;
            let mut credentials_file = CredentialsFile::load(&credentials_path)?;
            let selected = credentials_file
                .set_active_by_selector(
                    profile.as_deref(),
                    principal.as_deref(),
                    credential.as_deref(),
                )?
                .clone();
            let mut snapshot = load_principal_registry_snapshot(&mut store)?;
            authenticate_principal_credential_in_registry(
                &mut snapshot,
                &CredentialId::new(selected.credential_id.clone()),
                &selected.principal_token,
            )?;
            store.save_principal_registry_snapshot(&snapshot)?;
            credentials_file.save(&credentials_path)?;
            println!("logged in");
            println!("profile = {}", selected.profile);
            println!("principal_id = {}", selected.principal_id);
            println!("credential_id = {}", selected.credential_id);
        }
        AuthCommand::Whoami => {
            let (mut store, credentials_path) = load_auth_registry_store(root)?;
            let credentials_file = CredentialsFile::load(&credentials_path)?;
            let selected = credentials_file.find_by_selector(None, None, None)?.clone();
            let mut snapshot = load_principal_registry_snapshot(&mut store)?;
            let authenticated = authenticate_principal_credential_in_registry(
                &mut snapshot,
                &CredentialId::new(selected.credential_id.clone()),
                &selected.principal_token,
            )?;
            store.save_principal_registry_snapshot(&snapshot)?;
            println!("profile = {}", selected.profile);
            println!("authority_id = {}", authenticated.principal.authority_id.0);
            println!("principal_id = {}", authenticated.principal.principal_id.0);
            println!("principal_kind = {:?}", authenticated.principal.kind);
            println!(
                "credential_id = {}",
                authenticated.credential.credential_id.0
            );
            if let Ok(profile) =
                serde_json::from_value::<HumanPrincipalProfile>(authenticated.principal.profile)
            {
                if let Some(attestation) = profile.attestation {
                    println!("attestation_issuer = {}", attestation.issuer);
                    println!("attestation_subject = {}", attestation.subject);
                    println!("attestation_assurance = {:?}", attestation.assurance);
                    println!("attestation_operation = {:?}", attestation.operation);
                }
            }
        }
    }

    Ok(())
}

fn map_assurance_arg(value: AuthAssuranceArg) -> HumanAttestationAssurance {
    match value {
        AuthAssuranceArg::High => HumanAttestationAssurance::High,
        AuthAssuranceArg::Moderate => HumanAttestationAssurance::Moderate,
        AuthAssuranceArg::Legacy => HumanAttestationAssurance::Legacy,
    }
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time should be after unix epoch")
        .as_secs()
}

pub(crate) fn handle_principal_command(root: &Path, command: PrincipalCommand) -> Result<()> {
    match command {
        PrincipalCommand::Mint {
            profile,
            kind,
            name,
            role,
            parent,
            authority,
            metadata_json,
            capabilities,
        } => {
            let (mut store, credentials_path) = load_auth_registry_store(root)?;
            let mut credentials_file = CredentialsFile::load(&credentials_path)?;
            let active = credentials_file.find_by_selector(None, None, None)?.clone();
            let mut snapshot = load_principal_registry_snapshot(&mut store)?;
            let authenticated = authenticate_principal_credential_in_registry(
                &mut snapshot,
                &CredentialId::new(active.credential_id.clone()),
                &active.principal_token,
            )?;
            let kind = parse_principal_kind(&kind)?;
            if !kind.is_durable_principal() {
                if kind.is_legacy_local_agent() {
                    bail!(
                        "legacy local agent principals are no longer mintable; use worktree execution identity instead"
                    );
                }
                bail!(
                    "principal kind `{:?}` is not supported for durable principal minting",
                    kind
                );
            }
            let parent_principal_id = parent
                .map(PrincipalId::new)
                .or_else(|| default_parent_for_kind(kind, &authenticated.principal.principal_id));
            let issued = mint_principal_credential_in_registry(
                &mut snapshot,
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
            store.save_principal_registry_snapshot(&snapshot)?;
            let stored =
                store_issued_credential(&mut credentials_file, &issued, profile.as_deref(), false);
            credentials_file.save(&credentials_path)?;
            println!("minted principal");
            print_issued_credential(&stored, &issued);
        }
    }

    Ok(())
}

fn load_auth_registry_store(root: &Path) -> Result<(SqliteStore, PathBuf)> {
    ensure_repo_git_support(root)?;
    let paths = PrismPaths::for_workspace_root(root)?;
    let credentials_path = paths.credentials_path()?;
    let store = SqliteStore::open(paths.shared_runtime_db_path()?)?;
    Ok((store, credentials_path))
}

fn load_principal_registry_snapshot(store: &mut SqliteStore) -> Result<PrincipalRegistrySnapshot> {
    store
        .load_principal_registry_snapshot()?
        .ok_or_else(|| anyhow::anyhow!("principal registry is not initialized"))
}

fn default_parent_for_kind(kind: PrincipalKind, principal_id: &PrincipalId) -> Option<PrincipalId> {
    match kind {
        PrincipalKind::Service => Some(principal_id.clone()),
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
    profile: Option<&str>,
    set_active: bool,
) -> CredentialProfile {
    credentials
        .upsert_profile(
            CredentialProfile {
                profile: profile
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(issued.principal.principal_id.0.as_str())
                    .to_string(),
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
