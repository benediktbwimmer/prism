use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

const CREDENTIALS_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct CredentialsFile {
    pub version: u32,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: Vec<CredentialProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CredentialProfile {
    pub profile: String,
    pub authority_id: String,
    pub principal_id: String,
    pub credential_id: String,
    pub principal_token: String,
}

impl CredentialsFile {
    pub(crate) fn load(path: &Path) -> Result<Self> {
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

    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub(crate) fn upsert_profile(
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

    pub(crate) fn set_active_by_selector(
        &mut self,
        profile: Option<&str>,
        principal_id: Option<&str>,
        credential_id: Option<&str>,
    ) -> Result<&CredentialProfile> {
        let selected_index = if let Some(profile) = profile {
            self.profiles
                .iter()
                .position(|candidate| candidate.profile == profile)
        } else if let Some(credential_id) = credential_id {
            self.profiles
                .iter()
                .position(|candidate| candidate.credential_id == credential_id)
        } else if let Some(principal_id) = principal_id {
            self.profiles
                .iter()
                .position(|candidate| candidate.principal_id == principal_id)
        } else if let Some(active_profile) = self.active_profile.as_deref() {
            self.profiles
                .iter()
                .position(|candidate| candidate.profile == active_profile)
        } else if self.profiles.len() == 1 {
            Some(0)
        } else {
            None
        }
        .ok_or_else(|| anyhow!("no stored credential matched the requested selector"))?;
        self.active_profile = Some(self.profiles[selected_index].profile.clone());
        Ok(&self.profiles[selected_index])
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialProfile, CredentialsFile};

    #[test]
    fn set_active_by_principal_selects_matching_profile() {
        let mut file = CredentialsFile {
            version: 1,
            active_profile: None,
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
            .set_active_by_selector(None, Some("principal:worker"), None)
            .unwrap();

        assert_eq!(selected.profile, "worker");
        assert_eq!(file.active_profile.as_deref(), Some("worker"));
    }
}
