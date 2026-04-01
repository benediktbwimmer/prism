use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

const CREDENTIALS_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialsFile {
    pub version: u32,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: Vec<CredentialProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialProfile {
    pub profile: String,
    pub authority_id: String,
    pub principal_id: String,
    pub credential_id: String,
    pub principal_token: String,
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
        if file.version == 0 {
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

#[cfg(test)]
mod tests {
    use super::{CredentialProfile, CredentialsFile};

    #[test]
    fn find_by_principal_selects_matching_profile_without_mutating_active() {
        let file = CredentialsFile {
            version: 1,
            active_profile: Some("owner".to_string()),
            profiles: vec![
                CredentialProfile {
                    profile: "owner".to_string(),
                    authority_id: "local-daemon".to_string(),
                    principal_id: "principal:owner".to_string(),
                    credential_id: "credential:owner".to_string(),
                    principal_token: "token:owner".to_string(),
                },
                CredentialProfile {
                    profile: "worker".to_string(),
                    authority_id: "local-daemon".to_string(),
                    principal_id: "principal:worker".to_string(),
                    credential_id: "credential:worker".to_string(),
                    principal_token: "token:worker".to_string(),
                },
            ],
        };

        let selected = file
            .find_by_selector(None, Some("principal:worker"), None)
            .unwrap();

        assert_eq!(selected.profile, "worker");
        assert_eq!(file.active_profile.as_deref(), Some("owner"));
    }
}
