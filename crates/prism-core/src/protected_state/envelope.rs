use anyhow::{anyhow, ensure, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::protected_state::canonical::{canonical_json_bytes, sha256_prefixed};

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

#[derive(Serialize)]
struct ProtectedEnvelopeSigningView<'a, T> {
    envelope_version: u32,
    stream: &'a str,
    stream_id: &'a str,
    event_id: &'a str,
    prev_event_id: &'a Option<String>,
    prev_entry_hash: &'a Option<String>,
    sequence: u64,
    runtime_authority_id: &'a str,
    runtime_key_id: &'a str,
    trust_bundle_id: &'a str,
    principal_authority_id: &'a str,
    principal_id: &'a str,
    credential_id: &'a str,
    algorithm: ProtectedSignatureAlgorithm,
    payload_hash: &'a str,
    payload: &'a T,
}

impl<T> ProtectedEventEnvelope<T>
where
    T: Serialize,
{
    pub(crate) fn canonical_payload_bytes(&self) -> Result<Vec<u8>> {
        canonical_json_bytes(&self.payload)
    }

    pub(crate) fn computed_payload_hash(&self) -> Result<String> {
        Ok(sha256_prefixed(&self.canonical_payload_bytes()?))
    }

    pub(crate) fn canonical_signing_bytes(&self) -> Result<Vec<u8>> {
        let view = ProtectedEnvelopeSigningView {
            envelope_version: self.envelope_version,
            stream: &self.stream,
            stream_id: &self.stream_id,
            event_id: &self.event_id,
            prev_event_id: &self.prev_event_id,
            prev_entry_hash: &self.prev_entry_hash,
            sequence: self.sequence,
            runtime_authority_id: &self.runtime_authority_id,
            runtime_key_id: &self.runtime_key_id,
            trust_bundle_id: &self.trust_bundle_id,
            principal_authority_id: &self.principal_authority_id,
            principal_id: &self.principal_id,
            credential_id: &self.credential_id,
            algorithm: self.algorithm,
            payload_hash: &self.payload_hash,
            payload: &self.payload,
        };
        canonical_json_bytes(&view)
    }

    pub(crate) fn canonical_entry_bytes(&self) -> Result<Vec<u8>> {
        canonical_json_bytes(self)
    }

    pub(crate) fn computed_entry_hash(&self) -> Result<String> {
        Ok(sha256_prefixed(&self.canonical_entry_bytes()?))
    }

    pub(crate) fn refresh_payload_hash(&mut self) -> Result<()> {
        self.payload_hash = self.computed_payload_hash()?;
        Ok(())
    }

    pub(crate) fn sign_with(&mut self, signing_key: &SigningKey) -> Result<()> {
        self.refresh_payload_hash()?;
        let signature = signing_key.sign(&self.canonical_signing_bytes()?);
        self.signature = signature_bytes_to_prefixed_base64(signature.to_bytes());
        Ok(())
    }

    pub(crate) fn verify_hashes(&self) -> Result<()> {
        ensure!(
            self.payload_hash == self.computed_payload_hash()?,
            "protected payload hash does not match canonical payload bytes"
        );
        Ok(())
    }

    pub(crate) fn verify_signature(&self, verifying_key: &VerifyingKey) -> Result<()> {
        self.verify_hashes()?;
        let signature = signature_from_prefixed_base64(&self.signature)?;
        verifying_key
            .verify(&self.canonical_signing_bytes()?, &signature)
            .map_err(|error| anyhow!("protected envelope signature verification failed: {error}"))
    }
}

fn signature_bytes_to_prefixed_base64(bytes: [u8; 64]) -> String {
    format!("base64:{}", BASE64_STANDARD.encode(bytes))
}

fn signature_from_prefixed_base64(value: &str) -> Result<Signature> {
    let encoded = value
        .strip_prefix("base64:")
        .ok_or_else(|| anyhow!("protected envelope signature must use `base64:` prefix"))?;
    let decoded = BASE64_STANDARD
        .decode(encoded)
        .map_err(|error| anyhow!("protected envelope signature is not valid base64: {error}"))?;
    Signature::try_from(decoded.as_slice())
        .map_err(|error| anyhow!("protected envelope signature has invalid Ed25519 bytes: {error}"))
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    use super::{
        ProtectedEventEnvelope, ProtectedSignatureAlgorithm, PROTECTED_EVENT_ENVELOPE_VERSION,
    };

    fn sample_envelope() -> ProtectedEventEnvelope<serde_json::Value> {
        ProtectedEventEnvelope {
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
            payload_hash: String::new(),
            signature: String::new(),
            payload: json!({
                "z": "last",
                "a": {
                    "kind": "LegacyImported",
                    "legacy_record_count": 4
                }
            }),
        }
    }

    #[test]
    fn root_event_detection_requires_both_predecessor_fields_to_be_absent() {
        let root = sample_envelope();
        assert!(root.is_root_event());

        let non_root = ProtectedEventEnvelope {
            prev_event_id: Some("event:0".to_string()),
            ..root
        };
        assert!(!non_root.is_root_event());
    }

    #[test]
    fn canonical_signing_bytes_omit_signature_and_use_jcs_ordering() {
        let mut envelope = sample_envelope();
        envelope.refresh_payload_hash().unwrap();
        let signing = String::from_utf8(envelope.canonical_signing_bytes().unwrap()).unwrap();
        assert!(!signing.contains("\"signature\""));
        assert!(signing.contains(r#""stream":"repo_contract_events""#));
        assert!(signing.contains(
            r#""payload":{"a":{"kind":"LegacyImported","legacy_record_count":4},"z":"last"}"#
        ));
    }

    #[test]
    fn sign_and_verify_round_trip_uses_canonical_bytes() {
        let mut envelope = sample_envelope();
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let verifying_key = signing_key.verifying_key();

        envelope.sign_with(&signing_key).unwrap();
        assert!(envelope.signature.starts_with("base64:"));
        envelope.verify_signature(&verifying_key).unwrap();

        let entry_hash = envelope.computed_entry_hash().unwrap();
        assert!(entry_hash.starts_with("sha256:"));
    }

    #[test]
    fn verify_signature_rejects_payload_tampering() {
        let mut envelope = sample_envelope();
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let verifying_key = signing_key.verifying_key();
        envelope.sign_with(&signing_key).unwrap();

        envelope.payload = json!({ "kind": "tampered" });
        let error = envelope
            .verify_signature(&verifying_key)
            .unwrap_err()
            .to_string();
        assert!(error.contains("payload hash"));
    }
}
