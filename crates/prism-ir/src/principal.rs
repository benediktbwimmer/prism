use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CredentialId, PrincipalAuthorityId, PrincipalId, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Service,
    Agent,
    System,
    Ci,
    External,
}

impl PrincipalKind {
    pub fn is_durable_principal(self) -> bool {
        matches!(self, Self::Human | Self::Service)
    }

    pub fn is_legacy_local_agent(self) -> bool {
        matches!(self, Self::Agent)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HumanAttestationOperation {
    Bootstrap,
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HumanAttestationAssurance {
    High,
    Moderate,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HumanAttestationRecord {
    pub issuer: String,
    pub subject: String,
    pub assurance: HumanAttestationAssurance,
    pub operation: HumanAttestationOperation,
    pub verified_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct HumanPrincipalProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<HumanAttestationRecord>,
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

#[cfg(test)]
mod tests {
    use super::{
        HumanAttestationAssurance, HumanAttestationOperation, HumanAttestationRecord,
        HumanPrincipalProfile, PrincipalKind,
    };

    #[test]
    fn only_human_and_service_are_durable_principal_kinds() {
        assert!(PrincipalKind::Human.is_durable_principal());
        assert!(PrincipalKind::Service.is_durable_principal());
        assert!(!PrincipalKind::Agent.is_durable_principal());
        assert!(!PrincipalKind::System.is_durable_principal());
        assert!(!PrincipalKind::Ci.is_durable_principal());
        assert!(!PrincipalKind::External.is_durable_principal());
    }

    #[test]
    fn only_agent_is_marked_as_legacy_local_agent_kind() {
        assert!(PrincipalKind::Agent.is_legacy_local_agent());
        assert!(!PrincipalKind::Human.is_legacy_local_agent());
        assert!(!PrincipalKind::Service.is_legacy_local_agent());
    }

    #[test]
    fn human_principal_profile_round_trips_attestation_metadata() {
        let profile = HumanPrincipalProfile {
            attestation: Some(HumanAttestationRecord {
                issuer: "github-device-flow".to_string(),
                subject: "bene".to_string(),
                assurance: HumanAttestationAssurance::High,
                operation: HumanAttestationOperation::Bootstrap,
                verified_at: 42,
            }),
        };

        let json = serde_json::to_value(&profile).unwrap();
        let restored: HumanPrincipalProfile = serde_json::from_value(json).unwrap();
        assert_eq!(restored, profile);
    }
}
