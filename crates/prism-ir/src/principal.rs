use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CredentialId, PrincipalAuthorityId, PrincipalId, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Agent,
    System,
    Ci,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalStatus {
    Active,
    Suspended,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialCapability {
    MutateCoordination,
    MutateRepoMemory,
    ReadPeerRuntime,
    MintChildPrincipal,
    AdminPrincipals,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PrincipalRef {
    pub authority_id: PrincipalAuthorityId,
    pub principal_id: PrincipalId,
    pub kind: PrincipalKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PrincipalActor {
    pub authority_id: PrincipalAuthorityId,
    pub principal_id: PrincipalId,
    #[serde(default)]
    pub kind: Option<PrincipalKind>,
    #[serde(default)]
    pub name: Option<String>,
}

impl PrincipalActor {
    pub fn scoped_id(&self) -> String {
        format!("principal:{}:{}", self.authority_id.0, self.principal_id.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrincipalProfile {
    pub authority_id: PrincipalAuthorityId,
    pub principal_id: PrincipalId,
    pub kind: PrincipalKind,
    pub name: String,
    #[serde(default)]
    pub role: Option<String>,
    pub status: PrincipalStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default)]
    pub parent_principal_id: Option<PrincipalId>,
    #[serde(default)]
    pub profile: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CredentialRecord {
    pub credential_id: CredentialId,
    pub authority_id: PrincipalAuthorityId,
    pub principal_id: PrincipalId,
    pub token_verifier: String,
    #[serde(default)]
    pub capabilities: Vec<CredentialCapability>,
    pub status: CredentialStatus,
    pub created_at: Timestamp,
    #[serde(default)]
    pub last_used_at: Option<Timestamp>,
    #[serde(default)]
    pub revoked_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct PrincipalRegistrySnapshot {
    #[serde(default)]
    pub principals: Vec<PrincipalProfile>,
    #[serde(default)]
    pub credentials: Vec<CredentialRecord>,
}
