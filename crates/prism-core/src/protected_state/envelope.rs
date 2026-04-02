use serde::{Deserialize, Serialize};

pub(crate) const PROTECTED_EVENT_ENVELOPE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProtectedSignatureAlgorithm {
    Ed25519,
}

impl Default for ProtectedSignatureAlgorithm {
    fn default() -> Self {
        Self::Ed25519
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProtectedEventEnvelope<T> {
    pub(crate) envelope_version: u32,
    pub(crate) stream: String,
    pub(crate) stream_id: String,
    pub(crate) event_id: String,
    pub(crate) prev_event_id: Option<String>,
    pub(crate) prev_entry_hash: Option<String>,
    pub(crate) sequence: u64,
    pub(crate) runtime_authority_id: String,
    pub(crate) runtime_key_id: String,
    pub(crate) trust_bundle_id: String,
    pub(crate) principal_authority_id: String,
    pub(crate) principal_id: String,
    pub(crate) credential_id: String,
    pub(crate) algorithm: ProtectedSignatureAlgorithm,
    pub(crate) payload_hash: String,
    pub(crate) signature: String,
    pub(crate) payload: T,
}

impl<T> ProtectedEventEnvelope<T> {
    pub(crate) fn is_root_event(&self) -> bool {
        self.prev_event_id.is_none() && self.prev_entry_hash.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProtectedEventEnvelope, ProtectedSignatureAlgorithm, PROTECTED_EVENT_ENVELOPE_VERSION,
    };

    #[test]
    fn root_event_detection_requires_both_predecessor_fields_to_be_absent() {
        let root = ProtectedEventEnvelope {
            envelope_version: PROTECTED_EVENT_ENVELOPE_VERSION,
            stream: "repo_contract_events".to_string(),
            stream_id: "contracts:events".to_string(),
            event_id: "event:1".to_string(),
            prev_event_id: None,
            prev_entry_hash: None,
            sequence: 1,
            runtime_authority_id: "authority:runtime:test".to_string(),
            runtime_key_id: "key:runtime:test".to_string(),
            trust_bundle_id: "trust-bundle:test".to_string(),
            principal_authority_id: "authority:test".to_string(),
            principal_id: "principal:test".to_string(),
            credential_id: "credential:test".to_string(),
            algorithm: ProtectedSignatureAlgorithm::Ed25519,
            payload_hash: "sha256:test".to_string(),
            signature: "base64:test".to_string(),
            payload: serde_json::json!({ "kind": "LegacyImported" }),
        };
        assert!(root.is_root_event());

        let non_root = ProtectedEventEnvelope {
            prev_event_id: Some("event:0".to_string()),
            ..root
        };
        assert!(!non_root.is_root_event());
    }
}
