use std::fs;
use std::path::Path;

use aes_gcm_siv::aead::{Aead, KeyInit};
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use pbkdf2::pbkdf2_hmac_array;
use prism_ir::{CredentialCapability, CredentialStatus, PrincipalKind, PrincipalStatus};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use crate::util::current_timestamp;

const CREDENTIALS_FILE_VERSION: u32 = 3;
const HUMAN_SESSION_FILE_VERSION: u32 = 1;
const HUMAN_SESSION_IDLE_TIMEOUT_SECS: u64 = 15 * 60;
const HUMAN_SESSION_MAX_LIFETIME_SECS: u64 = 8 * 60 * 60;
const HUMAN_SESSION_FRESH_REAUTH_WINDOW_SECS: u64 = 5 * 60;
const PBKDF2_ROUNDS: u32 = 600_000;
const ENCRYPTION_SALT_LEN: usize = 16;
const ENCRYPTION_KEY_LEN: usize = 32;
const ENCRYPTION_NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialsFile {
    pub version: u32,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: Vec<CredentialProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialProfile {
    pub profile: String,
    pub authority_id: String,
    pub principal_id: String,
    pub credential_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub principal_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_secret: Option<EncryptedCredentialSecret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_metadata: Option<CredentialProfilePrincipalMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_metadata: Option<CredentialProfileCredentialMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedCredentialSecret {
    pub algorithm: String,
    pub kdf: String,
    pub rounds: u32,
    pub salt_b64: String,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialProfilePrincipalMetadata {
    pub kind: PrincipalKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub status: PrincipalStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_principal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub profile: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialProfileCredentialMetadata {
    pub token_verifier: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<CredentialCapability>,
    pub status: CredentialStatus,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HumanSessionFile {
    pub version: u32,
    #[serde(default)]
    pub active_session: Option<HumanSessionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanSessionRecord {
    pub profile: String,
    pub authority_id: String,
    pub principal_id: String,
    pub credential_id: String,
    pub principal_token: String,
    pub unlocked_at: u64,
    pub last_used_at: u64,
    pub idle_timeout_secs: u64,
    pub absolute_expires_at: u64,
    pub fresh_until: u64,
}

impl CredentialsFile {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                version: CREDENTIALS_FILE_VERSION,
                ..Self::default()
            });
        }
        let content = fs::read_to_string(path)?;
        let mut file: Self = toml::from_str(&content)?;
        if matches!(file.version, 0..=2) {
            file.version = CREDENTIALS_FILE_VERSION;
        }
        Ok(file)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn upsert_profile(
        &mut self,
        profile: CredentialProfile,
        set_active: bool,
    ) -> &CredentialProfile {
        let existing_index = self
            .profiles
            .iter()
            .position(|candidate| candidate.profile == profile.profile);
        if let Some(index) = existing_index {
            self.profiles[index] = profile;
        } else {
            self.profiles.push(profile);
        }
        if set_active {
            self.active_profile = Some(
                self.profiles[existing_index.unwrap_or(self.profiles.len() - 1)]
                    .profile
                    .clone(),
            );
        }
        &self.profiles[existing_index.unwrap_or(self.profiles.len() - 1)]
    }

    pub fn find_by_selector(
        &self,
        profile: Option<&str>,
        principal_id: Option<&str>,
        credential_id: Option<&str>,
    ) -> Result<&CredentialProfile> {
        self.profiles
            .iter()
            .find(|candidate| {
                profile.is_some_and(|value| candidate.profile == value)
                    || principal_id.is_some_and(|value| candidate.principal_id == value)
                    || credential_id.is_some_and(|value| candidate.credential_id == value)
            })
            .or_else(|| {
                if profile.is_none() && principal_id.is_none() && credential_id.is_none() {
                    self.active_profile.as_deref().and_then(|active| {
                        self.profiles
                            .iter()
                            .find(|candidate| candidate.profile == active)
                    })
                } else {
                    None
                }
            })
            .or_else(|| {
                (profile.is_none()
                    && principal_id.is_none()
                    && credential_id.is_none()
                    && self.profiles.len() == 1)
                    .then(|| &self.profiles[0])
            })
            .ok_or_else(|| anyhow!("no stored credential matched the requested selector"))
    }

    pub fn find_by_selector_mut(
        &mut self,
        profile: Option<&str>,
        principal_id: Option<&str>,
        credential_id: Option<&str>,
    ) -> Result<&mut CredentialProfile> {
        let selected_profile = self
            .find_by_selector(profile, principal_id, credential_id)?
            .profile
            .clone();
        self.profiles
            .iter_mut()
            .find(|candidate| candidate.profile == selected_profile)
            .ok_or_else(|| anyhow!("selected profile disappeared from the credentials file"))
    }

    pub fn set_active_by_selector(
        &mut self,
        profile: Option<&str>,
        principal_id: Option<&str>,
        credential_id: Option<&str>,
    ) -> Result<&CredentialProfile> {
        let selected_profile = self
            .find_by_selector(profile, principal_id, credential_id)?
            .profile
            .clone();
        self.active_profile = Some(selected_profile.clone());
        self.profiles
            .iter()
            .find(|candidate| candidate.profile == selected_profile)
            .ok_or_else(|| anyhow!("selected profile disappeared from the credentials file"))
    }
}

impl CredentialProfile {
    pub fn has_inline_principal_token(&self) -> bool {
        !self.principal_token.is_empty()
    }

    pub fn has_encrypted_secret(&self) -> bool {
        self.encrypted_secret.is_some()
    }

    pub fn token_verifier(&self) -> Option<&str> {
        self.credential_metadata
            .as_ref()
            .map(|metadata| metadata.token_verifier.as_str())
    }

    pub fn encrypt_principal_token(
        &mut self,
        principal_token: &str,
        passphrase: &str,
    ) -> Result<()> {
        if passphrase.trim().is_empty() {
            bail!("passphrase must not be empty");
        }
        let mut salt = [0u8; ENCRYPTION_SALT_LEN];
        let mut nonce_bytes = [0u8; ENCRYPTION_NONCE_LEN];
        OsRng.fill_bytes(&mut salt);
        OsRng.fill_bytes(&mut nonce_bytes);
        let key = pbkdf2_hmac_array::<Sha256, ENCRYPTION_KEY_LEN>(
            passphrase.as_bytes(),
            &salt,
            PBKDF2_ROUNDS,
        );
        let cipher = Aes256GcmSiv::new_from_slice(&key)
            .map_err(|_| anyhow!("failed to initialize credential cipher"))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, principal_token.as_bytes())
            .map_err(|_| anyhow!("failed to encrypt credential secret"))?;
        self.encrypted_secret = Some(EncryptedCredentialSecret {
            algorithm: "aes-256-gcm-siv".to_string(),
            kdf: "pbkdf2-sha256".to_string(),
            rounds: PBKDF2_ROUNDS,
            salt_b64: BASE64_STANDARD.encode(salt),
            nonce_b64: BASE64_STANDARD.encode(nonce_bytes),
            ciphertext_b64: BASE64_STANDARD.encode(ciphertext),
        });
        self.principal_token.clear();
        Ok(())
    }

    pub fn decrypt_principal_token(&self, passphrase: &str) -> Result<String> {
        if self.has_inline_principal_token() {
            return Ok(self.principal_token.clone());
        }
        let encrypted = self.encrypted_secret.as_ref().ok_or_else(|| {
            anyhow!(
                "profile `{}` does not contain any local credential secret",
                self.profile
            )
        })?;
        if encrypted.algorithm != "aes-256-gcm-siv" {
            bail!(
                "profile `{}` uses unsupported secret algorithm `{}`",
                self.profile,
                encrypted.algorithm
            );
        }
        if encrypted.kdf != "pbkdf2-sha256" {
            bail!(
                "profile `{}` uses unsupported secret kdf `{}`",
                self.profile,
                encrypted.kdf
            );
        }
        let salt = decode_fixed_bytes::<ENCRYPTION_SALT_LEN>(&encrypted.salt_b64, "salt")?;
        let nonce_bytes =
            decode_fixed_bytes::<ENCRYPTION_NONCE_LEN>(&encrypted.nonce_b64, "nonce")?;
        let ciphertext = BASE64_STANDARD
            .decode(encrypted.ciphertext_b64.as_bytes())
            .context("failed to decode credential ciphertext")?;
        let key = pbkdf2_hmac_array::<Sha256, ENCRYPTION_KEY_LEN>(
            passphrase.as_bytes(),
            &salt,
            encrypted.rounds,
        );
        let cipher = Aes256GcmSiv::new_from_slice(&key)
            .map_err(|_| anyhow!("failed to initialize credential cipher"))?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
            .map_err(|_| anyhow!("credential passphrase did not unlock the local secret"))?;
        String::from_utf8(plaintext).context("credential secret was not valid utf-8")
    }
}

impl HumanSessionFile {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                version: HUMAN_SESSION_FILE_VERSION,
                ..Self::default()
            });
        }
        let content = fs::read_to_string(path)?;
        let mut file: Self = toml::from_str(&content)?;
        if file.version == 0 {
            file.version = HUMAN_SESSION_FILE_VERSION;
        }
        Ok(file)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.active_session = None;
    }

    pub fn activate(&mut self, profile: &CredentialProfile, principal_token: String, now: u64) {
        self.version = HUMAN_SESSION_FILE_VERSION;
        self.active_session = Some(HumanSessionRecord {
            profile: profile.profile.clone(),
            authority_id: profile.authority_id.clone(),
            principal_id: profile.principal_id.clone(),
            credential_id: profile.credential_id.clone(),
            principal_token,
            unlocked_at: now,
            last_used_at: now,
            idle_timeout_secs: HUMAN_SESSION_IDLE_TIMEOUT_SECS,
            absolute_expires_at: now.saturating_add(HUMAN_SESSION_MAX_LIFETIME_SECS),
            fresh_until: now.saturating_add(HUMAN_SESSION_FRESH_REAUTH_WINDOW_SECS),
        });
    }

    pub fn active_session(&mut self, now: u64, touch: bool) -> Option<HumanSessionRecord> {
        let session = self.active_session.as_mut()?;
        if session.is_expired_at(now) {
            self.active_session = None;
            return None;
        }
        if touch {
            session.last_used_at = now;
        }
        Some(session.clone())
    }

    pub fn active_session_now(&mut self) -> Option<HumanSessionRecord> {
        self.active_session(current_timestamp(), true)
    }
}

impl HumanSessionRecord {
    pub fn is_expired_at(&self, now: u64) -> bool {
        now > self.absolute_expires_at
            || now.saturating_sub(self.last_used_at) > self.idle_timeout_secs
    }

    pub fn requires_fresh_reauth_at(&self, now: u64) -> bool {
        now > self.fresh_until
    }
}

fn decode_fixed_bytes<const N: usize>(value: &str, label: &str) -> Result<[u8; N]> {
    let decoded = BASE64_STANDARD
        .decode(value.as_bytes())
        .with_context(|| format!("failed to decode credential {label}"))?;
    let decoded_len = decoded.len();
    decoded.try_into().map_err(|_| {
        anyhow!(
            "decoded credential {label} length {} did not match expected length {N}",
            decoded_len
        )
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use prism_ir::{CredentialCapability, CredentialStatus, PrincipalKind, PrincipalStatus};

    use super::{
        CredentialProfile, CredentialProfileCredentialMetadata, CredentialProfilePrincipalMetadata,
        CredentialsFile, HumanSessionFile, HUMAN_SESSION_FILE_VERSION,
    };

    #[test]
    fn find_by_principal_selects_matching_profile_without_mutating_active() {
        let file = CredentialsFile {
            version: 2,
            active_profile: Some("owner".to_string()),
            profiles: vec![
                CredentialProfile {
                    profile: "owner".to_string(),
                    authority_id: "local-daemon".to_string(),
                    principal_id: "principal:owner".to_string(),
                    credential_id: "credential:owner".to_string(),
                    principal_token: "token:owner".to_string(),
                    encrypted_secret: None,
                    principal_metadata: None,
                    credential_metadata: None,
                },
                CredentialProfile {
                    profile: "worker".to_string(),
                    authority_id: "local-daemon".to_string(),
                    principal_id: "principal:worker".to_string(),
                    credential_id: "credential:worker".to_string(),
                    principal_token: "token:worker".to_string(),
                    encrypted_secret: None,
                    principal_metadata: None,
                    credential_metadata: None,
                },
            ],
        };

        let selected = file
            .find_by_selector(None, Some("principal:worker"), None)
            .unwrap();

        assert_eq!(selected.profile, "worker");
        assert_eq!(file.active_profile.as_deref(), Some("owner"));
    }

    #[test]
    fn upsert_profile_without_activation_preserves_active_profile() {
        let mut file = CredentialsFile {
            version: 2,
            active_profile: Some("owner".to_string()),
            profiles: vec![CredentialProfile {
                profile: "owner".to_string(),
                authority_id: "local-daemon".to_string(),
                principal_id: "principal:owner".to_string(),
                credential_id: "credential:owner".to_string(),
                principal_token: "token:owner".to_string(),
                encrypted_secret: None,
                principal_metadata: None,
                credential_metadata: None,
            }],
        };

        file.upsert_profile(
            CredentialProfile {
                profile: "worker".to_string(),
                authority_id: "local-daemon".to_string(),
                principal_id: "principal:worker".to_string(),
                credential_id: "credential:worker".to_string(),
                principal_token: "token:worker".to_string(),
                encrypted_secret: None,
                principal_metadata: None,
                credential_metadata: None,
            },
            false,
        );

        assert_eq!(file.active_profile.as_deref(), Some("owner"));
        assert_eq!(file.profiles.len(), 2);
    }

    #[test]
    fn encrypted_profile_secret_round_trips_without_persisting_inline_token() {
        let mut profile = CredentialProfile {
            profile: "owner".to_string(),
            authority_id: "local-daemon".to_string(),
            principal_id: "principal:owner".to_string(),
            credential_id: "credential:owner".to_string(),
            principal_token: String::new(),
            encrypted_secret: None,
            principal_metadata: None,
            credential_metadata: None,
        };

        profile
            .encrypt_principal_token("token:owner", "correct horse battery staple")
            .unwrap();

        assert!(profile.principal_token.is_empty());
        assert!(profile.encrypted_secret.is_some());
        assert_eq!(
            profile
                .decrypt_principal_token("correct horse battery staple")
                .unwrap(),
            "token:owner"
        );
        assert!(profile.decrypt_principal_token("wrong passphrase").is_err());
    }

    #[test]
    fn human_session_expires_after_idle_timeout() {
        let profile = CredentialProfile {
            profile: "owner".to_string(),
            authority_id: "local-daemon".to_string(),
            principal_id: "principal:owner".to_string(),
            credential_id: "credential:owner".to_string(),
            principal_token: String::new(),
            encrypted_secret: None,
            principal_metadata: None,
            credential_metadata: None,
        };
        let mut session = HumanSessionFile {
            version: HUMAN_SESSION_FILE_VERSION,
            active_session: None,
        };

        session.activate(&profile, "token:owner".to_string(), 100);
        assert!(session.active_session(100 + 60, false).is_some());
        assert!(session.active_session(100 + (15 * 60) + 1, false).is_none());
    }

    #[test]
    fn credential_profile_round_trips_principal_and_credential_metadata() {
        let profile = CredentialProfile {
            profile: "owner".to_string(),
            authority_id: "local-daemon".to_string(),
            principal_id: "principal:owner".to_string(),
            credential_id: "credential:owner".to_string(),
            principal_token: String::new(),
            encrypted_secret: None,
            principal_metadata: Some(CredentialProfilePrincipalMetadata {
                kind: PrincipalKind::Human,
                name: "Owner".to_string(),
                role: Some("repo_owner".to_string()),
                status: PrincipalStatus::Active,
                created_at: 11,
                updated_at: 12,
                parent_principal_id: None,
                profile: json!({ "attestation": { "issuer": "github", "subject": "owner" } }),
            }),
            credential_metadata: Some(CredentialProfileCredentialMetadata {
                token_verifier: "verifier".to_string(),
                capabilities: vec![CredentialCapability::All],
                status: CredentialStatus::Active,
                created_at: 13,
                last_used_at: Some(14),
                revoked_at: None,
            }),
        };

        let encoded = toml::to_string_pretty(&CredentialsFile {
            version: 3,
            active_profile: Some("owner".to_string()),
            profiles: vec![profile.clone()],
        })
        .unwrap();
        let decoded: CredentialsFile = toml::from_str(&encoded).unwrap();

        assert_eq!(decoded.profiles, vec![profile]);
    }
}
