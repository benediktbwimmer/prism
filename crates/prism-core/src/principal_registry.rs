use anyhow::{anyhow, bail, Result};
use prism_ir::{
    new_prefixed_id, new_slugged_id, CredentialCapability, CredentialId, CredentialRecord,
    CredentialStatus, PrincipalAuthorityId, PrincipalId, PrincipalKind, PrincipalProfile,
    PrincipalRegistrySnapshot, PrincipalStatus,
};
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::util::current_timestamp;
use crate::WorkspaceSession;

const DEFAULT_PRINCIPAL_AUTHORITY_ID: &str = "local-daemon";

#[derive(Debug, Clone)]
pub struct BootstrapOwnerInput {
    pub authority_id: Option<PrincipalAuthorityId>,
    pub name: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MintPrincipalRequest {
    pub authority_id: Option<PrincipalAuthorityId>,
    pub kind: PrincipalKind,
    pub name: String,
    pub role: Option<String>,
    pub parent_principal_id: Option<PrincipalId>,
    pub capabilities: Vec<CredentialCapability>,
    pub profile: Value,
}

#[derive(Debug, Clone)]
pub struct MintedPrincipalCredential {
    pub principal: PrincipalProfile,
    pub credential: CredentialRecord,
    pub principal_token: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedPrincipal {
    pub principal: PrincipalProfile,
    pub credential: CredentialRecord,
}

impl WorkspaceSession {
    pub fn bootstrap_owner_principal(
        &self,
        input: BootstrapOwnerInput,
    ) -> Result<MintedPrincipalCredential> {
        let mut snapshot = self.load_principal_registry()?.unwrap_or_default();
        if !snapshot.principals.is_empty() || !snapshot.credentials.is_empty() {
            bail!("principal registry is already initialized");
        }
        let issued = issue_principal_credential(
            &mut snapshot,
            MintPrincipalRequest {
                authority_id: input.authority_id,
                kind: PrincipalKind::Human,
                name: input.name,
                role: input.role,
                parent_principal_id: None,
                capabilities: vec![CredentialCapability::All],
                profile: Value::Null,
            },
        )?;
        self.persist_principal_registry(&snapshot)?;
        Ok(issued)
    }

    pub fn authenticate_principal_credential(
        &self,
        credential_id: &CredentialId,
        principal_token: &str,
    ) -> Result<AuthenticatedPrincipal> {
        self.authenticate_principal_credential_cached(credential_id, principal_token)
    }

    pub fn mint_principal_credential(
        &self,
        actor: &AuthenticatedPrincipal,
        request: MintPrincipalRequest,
    ) -> Result<MintedPrincipalCredential> {
        let mut snapshot = self.load_principal_registry()?.unwrap_or_default();
        verify_actor_still_active(&snapshot, actor)?;
        ensure_mint_capability(actor, &request)?;
        let issued = issue_principal_credential(&mut snapshot, request)?;
        self.persist_principal_registry(&snapshot)?;
        Ok(issued)
    }
}

pub(crate) fn authenticate_principal_credential_without_persist(
    snapshot: &mut PrincipalRegistrySnapshot,
    credential_id: &CredentialId,
    principal_token: &str,
) -> Result<AuthenticatedPrincipal> {
    let credential_index = snapshot
        .credentials
        .iter()
        .position(|credential| credential.credential_id == *credential_id)
        .ok_or_else(|| anyhow!("credential `{}` not found", credential_id.0))?;
    let now = current_timestamp();
    let verifier = credential_verifier(principal_token);
    let credential = snapshot.credentials[credential_index].clone();
    if credential.status != CredentialStatus::Active {
        bail!("credential `{}` is not active", credential.credential_id.0);
    }
    if credential.token_verifier != verifier {
        bail!("credential token did not match verifier");
    }
    let principal = snapshot
        .principals
        .iter()
        .find(|principal| {
            principal.authority_id == credential.authority_id
                && principal.principal_id == credential.principal_id
        })
        .cloned()
        .ok_or_else(|| anyhow!("principal for credential `{}` not found", credential_id.0))?;
    if principal.status != PrincipalStatus::Active {
        bail!("principal `{}` is not active", principal.principal_id.0);
    }
    snapshot.credentials[credential_index].last_used_at = Some(now);
    Ok(AuthenticatedPrincipal {
        principal,
        credential: snapshot.credentials[credential_index].clone(),
    })
}

fn verify_actor_still_active(
    snapshot: &PrincipalRegistrySnapshot,
    actor: &AuthenticatedPrincipal,
) -> Result<()> {
    let Some(principal) = snapshot.principals.iter().find(|principal| {
        principal.authority_id == actor.principal.authority_id
            && principal.principal_id == actor.principal.principal_id
    }) else {
        bail!(
            "principal `{}` no longer exists in the registry",
            actor.principal.principal_id.0
        );
    };
    if principal.status != PrincipalStatus::Active {
        bail!("principal `{}` is not active", principal.principal_id.0);
    }
    let Some(credential) = snapshot.credentials.iter().find(|credential| {
        credential.credential_id == actor.credential.credential_id
            && credential.authority_id == actor.credential.authority_id
            && credential.principal_id == actor.credential.principal_id
    }) else {
        bail!(
            "credential `{}` no longer exists in the registry",
            actor.credential.credential_id.0
        );
    };
    if credential.status != CredentialStatus::Active {
        bail!("credential `{}` is not active", credential.credential_id.0);
    }
    Ok(())
}

fn ensure_mint_capability(
    actor: &AuthenticatedPrincipal,
    request: &MintPrincipalRequest,
) -> Result<()> {
    if has_capability(&actor.credential.capabilities, CredentialCapability::All)
        || has_capability(
            &actor.credential.capabilities,
            CredentialCapability::AdminPrincipals,
        )
    {
        return Ok(());
    }

    if has_capability(
        &actor.credential.capabilities,
        CredentialCapability::MintChildPrincipal,
    ) {
        let Some(parent_principal_id) = request.parent_principal_id.as_ref() else {
            bail!("mint_child_principal requires an explicit parent principal");
        };
        if *parent_principal_id != actor.principal.principal_id {
            bail!("mint_child_principal can only mint children of the authenticated principal");
        }
        if request.kind != PrincipalKind::Agent {
            bail!("mint_child_principal can only mint agent principals");
        }
        return Ok(());
    }

    bail!(
        "credential `{}` cannot mint principals",
        actor.credential.credential_id.0
    )
}

fn issue_principal_credential(
    snapshot: &mut PrincipalRegistrySnapshot,
    request: MintPrincipalRequest,
) -> Result<MintedPrincipalCredential> {
    let authority_id = request
        .authority_id
        .unwrap_or_else(|| PrincipalAuthorityId::new(DEFAULT_PRINCIPAL_AUTHORITY_ID));
    if request.name.trim().is_empty() {
        bail!("principal name cannot be empty");
    }
    if let Some(parent_principal_id) = request.parent_principal_id.as_ref() {
        let parent_exists = snapshot.principals.iter().any(|principal| {
            principal.authority_id == authority_id
                && principal.principal_id == *parent_principal_id
                && principal.status == PrincipalStatus::Active
        });
        if !parent_exists {
            bail!(
                "parent principal `{}` is not active in authority `{}`",
                parent_principal_id.0,
                authority_id.0
            );
        }
    }

    let now = current_timestamp();
    let principal = PrincipalProfile {
        authority_id: authority_id.clone(),
        principal_id: PrincipalId::new(new_slugged_id("principal", &request.name)),
        kind: request.kind,
        name: request.name,
        role: request.role,
        status: PrincipalStatus::Active,
        created_at: now,
        updated_at: now,
        parent_principal_id: request.parent_principal_id,
        profile: request.profile,
    };
    let principal_token = generate_principal_token();
    let credential = CredentialRecord {
        credential_id: CredentialId::new(new_prefixed_id("credential")),
        authority_id,
        principal_id: principal.principal_id.clone(),
        token_verifier: credential_verifier(&principal_token),
        capabilities: normalized_capabilities(request.capabilities),
        status: CredentialStatus::Active,
        created_at: now,
        last_used_at: Some(now),
        revoked_at: None,
    };
    snapshot.principals.push(principal.clone());
    snapshot.credentials.push(credential.clone());
    Ok(MintedPrincipalCredential {
        principal,
        credential,
        principal_token,
    })
}

fn normalized_capabilities(capabilities: Vec<CredentialCapability>) -> Vec<CredentialCapability> {
    if capabilities.is_empty() {
        return vec![
            CredentialCapability::MutateCoordination,
            CredentialCapability::MutateRepoMemory,
        ];
    }
    let mut normalized = Vec::new();
    for capability in capabilities {
        if !normalized.contains(&capability) {
            normalized.push(capability);
        }
    }
    normalized
}

fn has_capability(capabilities: &[CredentialCapability], expected: CredentialCapability) -> bool {
    capabilities.contains(&expected)
}

fn generate_principal_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("prism_ptok_{}", hex_encode(&bytes))
}

fn credential_verifier(principal_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(principal_token.as_bytes());
    format!("sha256:{}", hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        output.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    output
}
