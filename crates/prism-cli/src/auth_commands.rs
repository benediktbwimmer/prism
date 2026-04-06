use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use prism_core::{
    authenticate_principal_credential_in_registry, bootstrap_owner_principal_in_registry,
    ensure_local_principal_registry_snapshot_with_unlocked_profile,
    mint_principal_credential_in_registry, recover_owner_principal_in_registry,
    AttestedHumanPrincipalInput, CredentialProfile, CredentialProfileCredentialMetadata,
    CredentialProfilePrincipalMetadata, CredentialsFile, HumanSessionFile, MintPrincipalRequest,
};
use prism_ir::{
    CredentialId, HumanAttestationAssurance, HumanAttestationOperation, HumanAttestationRecord,
    HumanPrincipalProfile, PrincipalAuthorityId, PrincipalId, PrincipalKind,
};
use prism_store::Store;
use serde_json::Value;

use crate::cli::{AuthAssuranceArg, AuthCommand, PrincipalCommand};
use crate::github_attestation::{
    issuer_uses_github_device_flow, resolve_github_attested_human_input,
};
use crate::operator_auth::{load_auth_registry_context, load_principal_registry_snapshot};
use crate::parsing::{parse_credential_capability, parse_principal_kind};

const AUTH_PASSPHRASE_ENV: &str = "PRISM_AUTH_PASSPHRASE";

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
            let context = load_auth_registry_context(root)?;
            let mut store = context.store;
            let credentials_path = context.credentials_path;
            let human_session_path = context.human_session_path;
            let mut snapshot = store
                .load_principal_registry_snapshot()?
                .unwrap_or_default();
            let attested_input = resolve_attested_human_input(
                HumanAttestationOperation::Bootstrap,
                name,
                authority,
                role,
                issuer,
                subject,
                assurance,
            )?;
            let issued = bootstrap_owner_principal_in_registry(
                &mut snapshot,
                attested_input,
            )?;
            store.save_principal_registry_snapshot(&snapshot)?;
            let passphrase = prompt_new_passphrase()?;
            let mut credentials = CredentialsFile::load(&credentials_path)?;
            let stored =
                store_issued_credential(&mut credentials, &issued, &passphrase, None, true)?;
            credentials.save(&credentials_path)?;
            activate_human_session(&human_session_path, &stored, &issued.principal_token)?;
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
            let context = load_auth_registry_context(root)?;
            let mut store = context.store;
            let credentials_path = context.credentials_path;
            let human_session_path = context.human_session_path;
            let mut snapshot = store
                .load_principal_registry_snapshot()?
                .unwrap_or_default();
            let attested_input = resolve_attested_human_input(
                HumanAttestationOperation::Recovery,
                name,
                authority,
                role,
                issuer,
                subject,
                assurance,
            )?;
            let issued = recover_owner_principal_in_registry(
                &mut snapshot,
                attested_input,
            )?;
            store.save_principal_registry_snapshot(&snapshot)?;
            let passphrase = prompt_new_passphrase()?;
            let mut credentials = CredentialsFile::load(&credentials_path)?;
            let stored =
                store_issued_credential(&mut credentials, &issued, &passphrase, None, true)?;
            credentials.save(&credentials_path)?;
            activate_human_session(&human_session_path, &stored, &issued.principal_token)?;
            println!("initialized principal registry from recovery attestation");
            print_issued_credential(&stored, &issued);
        }
        AuthCommand::Login {
            profile,
            principal,
            credential,
        } => {
            let context = load_auth_registry_context(root)?;
            let mut store = context.store;
            let credentials_path = context.credentials_path;
            let human_session_path = context.human_session_path;
            let mut credentials_file = CredentialsFile::load(&credentials_path)?;
            let selected = credentials_file
                .set_active_by_selector(
                    profile.as_deref(),
                    principal.as_deref(),
                    credential.as_deref(),
                )?
                .clone();
            let passphrase = prompt_existing_passphrase()?;
            let principal_token = selected.decrypt_principal_token(&passphrase)?;
            let mut snapshot = store.load_principal_registry_snapshot()?.unwrap_or_default();
            if snapshot.principals.is_empty() || snapshot.credentials.is_empty() {
                snapshot = ensure_local_principal_registry_snapshot_with_unlocked_profile(
                    root,
                    &mut store,
                    &selected,
                    &principal_token,
                )?
                .ok_or_else(|| {
                    if selected.is_legacy_local_compatibility_profile() {
                        anyhow!(
                            "selected profile `{}` is a legacy local compatibility credential and cannot be used for human login; bootstrap or recover a human owner profile, then register this worktree for agent execution",
                            selected.profile
                        )
                    } else {
                        anyhow!("principal registry is not initialized")
                    }
                })?;
            }
            let authenticated = authenticate_principal_credential_in_registry(
                &mut snapshot,
                &CredentialId::new(selected.credential_id.clone()),
                &principal_token,
            )?;
            if authenticated.principal.kind != PrincipalKind::Human {
                bail!(
                    "local human login only supports human principals; `{}` is `{:?}`",
                    authenticated.principal.principal_id.0,
                    authenticated.principal.kind
                );
            }
            if !selected.has_encrypted_secret() {
                let selected = credentials_file.find_by_selector_mut(
                    Some(selected.profile.as_str()),
                    None,
                    None,
                )?;
                selected.encrypt_principal_token(&principal_token, &passphrase)?;
            }
            store.save_principal_registry_snapshot(&snapshot)?;
            credentials_file.save(&credentials_path)?;
            activate_human_session(&human_session_path, &selected, &principal_token)?;
            println!("logged in");
            println!("profile = {}", selected.profile);
            println!("principal_id = {}", selected.principal_id);
            println!("credential_id = {}", selected.credential_id);
        }
        AuthCommand::Whoami => {
            let context = load_auth_registry_context(root)?;
            let mut store = context.store;
            let credentials_path = context.credentials_path;
            let human_session_path = context.human_session_path;
            let credentials_file = CredentialsFile::load(&credentials_path)?;
            let selected = credentials_file.find_by_selector(None, None, None)?.clone();
            let mut sessions = HumanSessionFile::load(&human_session_path)?;
            let session = sessions.active_session_now().ok_or_else(|| {
                anyhow!(
                    "no active local human session is available; run `prism auth login` to unlock the stored credential first"
                )
            })?;
            let mut snapshot = load_principal_registry_snapshot(&mut store)?;
            let authenticated = authenticate_principal_credential_in_registry(
                &mut snapshot,
                &CredentialId::new(session.credential_id.clone()),
                &session.principal_token,
            )?;
            store.save_principal_registry_snapshot(&snapshot)?;
            sessions.save(&human_session_path)?;
            println!("profile = {}", selected.profile);
            println!("authority_id = {}", authenticated.principal.authority_id.0);
            println!("principal_id = {}", authenticated.principal.principal_id.0);
            println!("principal_kind = {:?}", authenticated.principal.kind);
            println!(
                "credential_id = {}",
                authenticated.credential.credential_id.0
            );
            println!("session_status = unlocked");
            println!(
                "session_fresh = {}",
                !session.requires_fresh_reauth_at(current_unix_timestamp())
            );
            println!("session_expires_at = {}", session.absolute_expires_at);
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

fn resolve_attested_human_input(
    operation: HumanAttestationOperation,
    name: Option<String>,
    authority: String,
    role: Option<String>,
    issuer: String,
    subject: Option<String>,
    assurance: AuthAssuranceArg,
) -> Result<AttestedHumanPrincipalInput> {
    if issuer_uses_github_device_flow(&issuer) {
        return resolve_github_attested_human_input(
            operation,
            &authority,
            name.as_deref(),
            role,
            subject.as_deref(),
            assurance,
        );
    }

    let name = name
        .map(|candidate| candidate.trim().to_string())
        .filter(|candidate| !candidate.is_empty())
        .ok_or_else(|| anyhow!("`--name` is required for non-GitHub bootstrap flows"))?;
    let subject = subject
        .map(|candidate| candidate.trim().to_string())
        .filter(|candidate| !candidate.is_empty())
        .ok_or_else(|| anyhow!("`--subject` is required for non-GitHub bootstrap flows"))?;

    Ok(AttestedHumanPrincipalInput {
        authority_id: Some(PrincipalAuthorityId::new(authority)),
        name,
        role,
        attestation: HumanAttestationRecord {
            issuer,
            subject,
            assurance: map_assurance_arg(assurance),
            operation,
            verified_at: current_unix_timestamp(),
        },
    })
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
            let context = load_auth_registry_context(root)?;
            let mut store = context.store;
            let credentials_path = context.credentials_path;
            let human_session_path = context.human_session_path;
            let mut credentials_file = CredentialsFile::load(&credentials_path)?;
            let mut sessions = HumanSessionFile::load(&human_session_path)?;
            let session = sessions.active_session_now().ok_or_else(|| {
                anyhow!(
                    "no active local human session is available; run `prism auth login` to unlock the stored credential first"
                )
            })?;
            let mut snapshot = load_principal_registry_snapshot(&mut store)?;
            let authenticated = authenticate_principal_credential_in_registry(
                &mut snapshot,
                &CredentialId::new(session.credential_id.clone()),
                &session.principal_token,
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
            sessions.save(&human_session_path)?;
            let passphrase = prompt_existing_passphrase()?;
            let stored = store_issued_credential(
                &mut credentials_file,
                &issued,
                &passphrase,
                profile.as_deref(),
                false,
            )?;
            credentials_file.save(&credentials_path)?;
            println!("minted principal");
            print_issued_credential(&stored, &issued);
        }
    }

    Ok(())
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
    passphrase: &str,
    profile: Option<&str>,
    set_active: bool,
) -> Result<CredentialProfile> {
    let mut stored = CredentialProfile {
        profile: profile
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(issued.principal.principal_id.0.as_str())
            .to_string(),
        authority_id: issued.principal.authority_id.0.to_string(),
        principal_id: issued.principal.principal_id.0.to_string(),
        credential_id: issued.credential.credential_id.0.to_string(),
        principal_token: String::new(),
        encrypted_secret: None,
        principal_metadata: Some(CredentialProfilePrincipalMetadata {
            kind: issued.principal.kind,
            name: issued.principal.name.clone(),
            role: issued.principal.role.clone(),
            status: issued.principal.status,
            created_at: issued.principal.created_at,
            updated_at: issued.principal.updated_at,
            parent_principal_id: issued
                .principal
                .parent_principal_id
                .as_ref()
                .map(|value| value.0.to_string()),
            profile: issued.principal.profile.clone(),
        }),
        credential_metadata: Some(CredentialProfileCredentialMetadata {
            token_verifier: issued.credential.token_verifier.clone(),
            capabilities: issued.credential.capabilities.clone(),
            status: issued.credential.status,
            created_at: issued.credential.created_at,
            last_used_at: issued.credential.last_used_at,
            revoked_at: issued.credential.revoked_at,
        }),
    };
    stored.encrypt_principal_token(&issued.principal_token, passphrase)?;
    credentials.upsert_profile(stored.clone(), set_active);
    Ok(stored)
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
    println!("local_secret = encrypted");
}

fn activate_human_session(
    human_session_path: &Path,
    stored: &CredentialProfile,
    principal_token: &str,
) -> Result<()> {
    let mut session = HumanSessionFile::load(human_session_path)?;
    session.activate(
        stored,
        principal_token.to_string(),
        current_unix_timestamp(),
    );
    session.save(human_session_path)
}

fn prompt_new_passphrase() -> Result<String> {
    if let Ok(passphrase) = std::env::var(AUTH_PASSPHRASE_ENV) {
        if passphrase.trim().is_empty() {
            bail!("{AUTH_PASSPHRASE_ENV} must not be empty");
        }
        return Ok(passphrase);
    }
    let passphrase = rpassword::prompt_password("PRISM passphrase: ")?;
    if passphrase.trim().is_empty() {
        bail!("passphrase must not be empty");
    }
    let confirmation = rpassword::prompt_password("Confirm PRISM passphrase: ")?;
    if passphrase != confirmation {
        bail!("passphrase confirmation did not match");
    }
    Ok(passphrase)
}

fn prompt_existing_passphrase() -> Result<String> {
    if let Ok(passphrase) = std::env::var(AUTH_PASSPHRASE_ENV) {
        if passphrase.trim().is_empty() {
            bail!("{AUTH_PASSPHRASE_ENV} must not be empty");
        }
        return Ok(passphrase);
    }
    let passphrase = rpassword::prompt_password("PRISM passphrase: ")?;
    if passphrase.trim().is_empty() {
        bail!("passphrase must not be empty");
    }
    Ok(passphrase)
}
