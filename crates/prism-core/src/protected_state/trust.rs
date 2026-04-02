use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use prism_ir::{new_prefixed_id, new_sortable_token};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::protected_state::canonical::canonical_json_bytes;
use crate::protected_state::envelope::ProtectedSignatureAlgorithm;
use crate::util::current_timestamp;
use crate::PrismPaths;

const TRUST_BUNDLE_VERSION: u32 = 1;
const TRUST_ROOT_VERSION: u32 = 1;
const PRIVATE_KEY_RECORD_VERSION: u32 = 1;
const RUNTIME_AUTHORITY_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeKeyRevocationKind {
    RevokedForFutureUse,
    HistoricallyCompromised,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeAuthorityRecord {
    pub(crate) runtime_authority_id: String,
    pub(crate) authority_root_id: String,
    pub(crate) activated_at: u64,
    pub(crate) revoked_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeKeyRecord {
    pub(crate) runtime_key_id: String,
    pub(crate) runtime_authority_id: String,
    pub(crate) algorithm: ProtectedSignatureAlgorithm,
    pub(crate) public_key: String,
    pub(crate) activated_at: u64,
    pub(crate) revoked_at: Option<u64>,
    pub(crate) revocation_kind: Option<RuntimeKeyRevocationKind>,
    pub(crate) historically_compromised_from: Option<u64>,
}

impl RuntimeKeyRecord {
    pub(crate) fn is_active_for_new_signatures(&self, now: u64) -> bool {
        self.activated_at <= now
            && self.revoked_at.is_none()
            && self.historically_compromised_from.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TrustBundle {
    pub(crate) bundle_version: u32,
    pub(crate) bundle_id: String,
    pub(crate) authority_root_id: String,
    pub(crate) issued_at: u64,
    pub(crate) issuer_key_id: String,
    pub(crate) runtime_authorities: Vec<RuntimeAuthorityRecord>,
    pub(crate) runtime_keys: Vec<RuntimeKeyRecord>,
    pub(crate) signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TrustedAuthorityRoot {
    pub(crate) root_version: u32,
    pub(crate) authority_root_id: String,
    pub(crate) issuer_key_id: String,
    pub(crate) algorithm: ProtectedSignatureAlgorithm,
    pub(crate) public_key: String,
    pub(crate) created_at: u64,
    pub(crate) trusted_at: u64,
    pub(crate) pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredPrivateKey {
    file_version: u32,
    key_id: String,
    owner_id: String,
    algorithm: ProtectedSignatureAlgorithm,
    secret_key: String,
    public_key: String,
    created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeAuthorityState {
    pub(crate) state_version: u32,
    pub(crate) authority_root_id: String,
    pub(crate) issuer_key_id: String,
    pub(crate) runtime_authority_id: String,
    pub(crate) active_runtime_key_id: String,
    pub(crate) active_trust_bundle_id: String,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRuntimeSigningKey {
    pub(crate) state: RuntimeAuthorityState,
    pub(crate) bundle: TrustBundle,
    pub(crate) runtime_key: RuntimeKeyRecord,
    pub(crate) signing_key: SigningKey,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedTrustedRuntimeKey {
    pub(crate) verifying_key: VerifyingKey,
}

pub(crate) fn ensure_local_runtime_trust(paths: &PrismPaths) -> Result<RuntimeAuthorityState> {
    if let Some(state) = load_runtime_authority_state(paths)? {
        return Ok(state);
    }

    let now = current_timestamp();
    let authority_root_id = format!("authority-root:{}", new_sortable_token());
    let issuer_key_id = new_prefixed_id("key-root").to_string();
    let runtime_authority_id = format!("authority:runtime:{}", new_sortable_token());
    let runtime_key_id = new_prefixed_id("key-runtime").to_string();
    let bundle_id = new_prefixed_id("trust-bundle").to_string();

    let root_signing_key = SigningKey::generate(&mut OsRng);
    let runtime_signing_key = SigningKey::generate(&mut OsRng);

    let trusted_root = TrustedAuthorityRoot {
        root_version: TRUST_ROOT_VERSION,
        authority_root_id: authority_root_id.clone(),
        issuer_key_id: issuer_key_id.clone(),
        algorithm: ProtectedSignatureAlgorithm::Ed25519,
        public_key: verifying_key_to_base64(&root_signing_key.verifying_key()),
        created_at: now,
        trusted_at: now,
        pinned: true,
    };
    let runtime_authority = RuntimeAuthorityRecord {
        runtime_authority_id: runtime_authority_id.clone(),
        authority_root_id: authority_root_id.clone(),
        activated_at: now,
        revoked_at: None,
    };
    let runtime_key = RuntimeKeyRecord {
        runtime_key_id: runtime_key_id.clone(),
        runtime_authority_id: runtime_authority_id.clone(),
        algorithm: ProtectedSignatureAlgorithm::Ed25519,
        public_key: verifying_key_to_base64(&runtime_signing_key.verifying_key()),
        activated_at: now,
        revoked_at: None,
        revocation_kind: None,
        historically_compromised_from: None,
    };
    let mut bundle = TrustBundle {
        bundle_version: TRUST_BUNDLE_VERSION,
        bundle_id: bundle_id.clone(),
        authority_root_id: authority_root_id.clone(),
        issued_at: now,
        issuer_key_id: issuer_key_id.clone(),
        runtime_authorities: vec![runtime_authority],
        runtime_keys: vec![runtime_key.clone()],
        signature: String::new(),
    };
    sign_bundle(&mut bundle, &root_signing_key)?;

    let state = RuntimeAuthorityState {
        state_version: RUNTIME_AUTHORITY_STATE_VERSION,
        authority_root_id,
        issuer_key_id,
        runtime_authority_id,
        active_runtime_key_id: runtime_key_id.clone(),
        active_trust_bundle_id: bundle_id,
        created_at: now,
        updated_at: now,
    };

    save_json(
        &paths.trusted_root_path(&trusted_root.authority_root_id)?,
        &trusted_root,
    )?;
    save_json(
        &paths.trusted_root_key_path(&state.issuer_key_id)?,
        &StoredPrivateKey {
            file_version: PRIVATE_KEY_RECORD_VERSION,
            key_id: state.issuer_key_id.clone(),
            owner_id: state.authority_root_id.clone(),
            algorithm: ProtectedSignatureAlgorithm::Ed25519,
            secret_key: signing_key_to_base64(&root_signing_key),
            public_key: verifying_key_to_base64(&root_signing_key.verifying_key()),
            created_at: now,
        },
    )?;
    save_json(
        &paths.runtime_signing_key_path(&runtime_key_id)?,
        &StoredPrivateKey {
            file_version: PRIVATE_KEY_RECORD_VERSION,
            key_id: runtime_key_id,
            owner_id: state.runtime_authority_id.clone(),
            algorithm: ProtectedSignatureAlgorithm::Ed25519,
            secret_key: signing_key_to_base64(&runtime_signing_key),
            public_key: verifying_key_to_base64(&runtime_signing_key.verifying_key()),
            created_at: now,
        },
    )?;
    save_json(&paths.trust_bundle_path(&bundle.bundle_id)?, &bundle)?;
    save_json(&paths.runtime_authority_state_path()?, &state)?;
    Ok(state)
}

pub(crate) fn load_runtime_authority_state(
    paths: &PrismPaths,
) -> Result<Option<RuntimeAuthorityState>> {
    load_json_optional(&paths.runtime_authority_state_path()?)
}

pub(crate) fn load_trust_bundle(
    paths: &PrismPaths,
    bundle_id: &str,
) -> Result<Option<TrustBundle>> {
    load_json_optional(&paths.trust_bundle_path(bundle_id)?)
}

pub(crate) fn load_trusted_root(
    paths: &PrismPaths,
    authority_root_id: &str,
) -> Result<Option<TrustedAuthorityRoot>> {
    load_json_optional(&paths.trusted_root_path(authority_root_id)?)
}

pub(crate) fn export_trust_bundle(paths: &PrismPaths, bundle_id: &str) -> Result<TrustBundle> {
    load_trust_bundle(paths, bundle_id)?
        .ok_or_else(|| anyhow!("trust bundle `{bundle_id}` was not found"))
}

pub(crate) fn import_trust_bundle(
    paths: &PrismPaths,
    bundle: &TrustBundle,
    pinned_root: Option<&TrustedAuthorityRoot>,
) -> Result<()> {
    let trusted_root = if let Some(root) = load_trusted_root(paths, &bundle.authority_root_id)? {
        root
    } else if let Some(root) = pinned_root {
        ensure!(
            root.authority_root_id == bundle.authority_root_id,
            "pinned root does not match trust bundle authority root"
        );
        save_json(&paths.trusted_root_path(&root.authority_root_id)?, root)?;
        root.clone()
    } else {
        bail!(
            "trust bundle `{}` uses unknown authority root `{}`; explicit trust pinning is required",
            bundle.bundle_id,
            bundle.authority_root_id
        );
    };

    verify_bundle(bundle, &trusted_root)?;
    let destination = paths.trust_bundle_path(&bundle.bundle_id)?;
    if destination.exists() {
        let existing: TrustBundle = load_json(&destination)?;
        ensure!(
            existing == *bundle,
            "trust bundle `{}` already exists with different contents",
            bundle.bundle_id
        );
        return Ok(());
    }
    save_json(&destination, bundle)
}

pub(crate) fn load_active_runtime_signing_key(
    paths: &PrismPaths,
) -> Result<ActiveRuntimeSigningKey> {
    let state = ensure_local_runtime_trust(paths)?;
    let bundle = export_trust_bundle(paths, &state.active_trust_bundle_id)?;
    let runtime_key = bundle
        .runtime_keys
        .iter()
        .find(|key| key.runtime_key_id == state.active_runtime_key_id)
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "active runtime key `{}` missing from trust bundle",
                state.active_runtime_key_id
            )
        })?;
    ensure!(
        runtime_key.is_active_for_new_signatures(current_timestamp()),
        "runtime key `{}` is not active for new signatures",
        runtime_key.runtime_key_id
    );
    let stored: StoredPrivateKey =
        load_json(&paths.runtime_signing_key_path(&runtime_key.runtime_key_id)?)?;
    let signing_key = signing_key_from_base64(&stored.secret_key)?;
    ensure!(
        stored.public_key == runtime_key.public_key,
        "runtime key record `{}` does not match stored private key material",
        runtime_key.runtime_key_id
    );
    Ok(ActiveRuntimeSigningKey {
        state,
        bundle,
        runtime_key,
        signing_key,
    })
}

pub(crate) fn resolve_trusted_runtime_key(
    paths: &PrismPaths,
    bundle_id: &str,
    runtime_authority_id: &str,
    runtime_key_id: &str,
) -> Result<ResolvedTrustedRuntimeKey> {
    let bundle = load_trust_bundle(paths, bundle_id)?
        .ok_or_else(|| anyhow!("trust bundle `{bundle_id}` is not imported locally"))?;
    let trusted_root = load_trusted_root(paths, &bundle.authority_root_id)?
        .ok_or_else(|| anyhow!("unknown authority root `{}`", bundle.authority_root_id))?;
    verify_bundle(&bundle, &trusted_root)?;

    let runtime_authority = bundle
        .runtime_authorities
        .iter()
        .find(|authority| authority.runtime_authority_id == runtime_authority_id)
        .ok_or_else(|| {
            anyhow!(
                "runtime authority `{runtime_authority_id}` is absent from trust bundle `{bundle_id}`"
            )
        })?;
    ensure!(
        runtime_authority.authority_root_id == bundle.authority_root_id,
        "runtime authority `{runtime_authority_id}` is bound to the wrong authority root"
    );

    let runtime_key = bundle
        .runtime_keys
        .iter()
        .find(|key| key.runtime_key_id == runtime_key_id)
        .cloned()
        .ok_or_else(|| {
            anyhow!("runtime key `{runtime_key_id}` is absent from trust bundle `{bundle_id}`")
        })?;
    ensure!(
        runtime_key.runtime_authority_id == runtime_authority_id,
        "runtime key `{runtime_key_id}` does not belong to runtime authority `{runtime_authority_id}`"
    );
    let verifying_key = verifying_key_from_base64(&runtime_key.public_key)?;
    Ok(ResolvedTrustedRuntimeKey { verifying_key })
}

fn sign_bundle(bundle: &mut TrustBundle, root_signing_key: &SigningKey) -> Result<()> {
    bundle.signature.clear();
    let signature = root_signing_key.sign(&bundle_signing_bytes(bundle)?);
    bundle.signature = format!("base64:{}", BASE64_STANDARD.encode(signature.to_bytes()));
    Ok(())
}

pub(crate) fn verify_bundle(
    bundle: &TrustBundle,
    trusted_root: &TrustedAuthorityRoot,
) -> Result<()> {
    ensure!(
        bundle.authority_root_id == trusted_root.authority_root_id,
        "trust bundle authority root does not match trusted root"
    );
    ensure!(
        bundle.issuer_key_id == trusted_root.issuer_key_id,
        "trust bundle issuer key does not match trusted root issuer key"
    );
    let verifying_key = verifying_key_from_base64(&trusted_root.public_key)?;
    let signature = signature_from_base64(&bundle.signature)?;
    verifying_key
        .verify(&bundle_signing_bytes(bundle)?, &signature)
        .map_err(|error| anyhow!("trust bundle signature verification failed: {error}"))
}

#[derive(Serialize)]
struct TrustBundleSigningView<'a> {
    bundle_version: u32,
    bundle_id: &'a str,
    authority_root_id: &'a str,
    issued_at: u64,
    issuer_key_id: &'a str,
    runtime_authorities: &'a [RuntimeAuthorityRecord],
    runtime_keys: &'a [RuntimeKeyRecord],
}

fn bundle_signing_bytes(bundle: &TrustBundle) -> Result<Vec<u8>> {
    canonical_json_bytes(&TrustBundleSigningView {
        bundle_version: bundle.bundle_version,
        bundle_id: &bundle.bundle_id,
        authority_root_id: &bundle.authority_root_id,
        issued_at: bundle.issued_at,
        issuer_key_id: &bundle.issuer_key_id,
        runtime_authorities: &bundle.runtime_authorities,
        runtime_keys: &bundle.runtime_keys,
    })
}

fn save_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn load_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))
}

fn load_json_optional<T>(path: &Path) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(None);
    }
    load_json(path).map(Some)
}

fn signing_key_to_base64(signing_key: &SigningKey) -> String {
    format!("base64:{}", BASE64_STANDARD.encode(signing_key.to_bytes()))
}

fn signing_key_from_base64(value: &str) -> Result<SigningKey> {
    let bytes = decode_base64_bytes(value, "signing key")?;
    let secret: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("signing key must decode to 32 bytes"))?;
    Ok(SigningKey::from_bytes(&secret))
}

fn verifying_key_to_base64(verifying_key: &VerifyingKey) -> String {
    format!(
        "base64:{}",
        BASE64_STANDARD.encode(verifying_key.to_bytes())
    )
}

fn verifying_key_from_base64(value: &str) -> Result<VerifyingKey> {
    let bytes = decode_base64_bytes(value, "verifying key")?;
    let public_key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("verifying key must decode to 32 bytes"))?;
    VerifyingKey::from_bytes(&public_key)
        .map_err(|error| anyhow!("invalid Ed25519 verifying key bytes: {error}"))
}

fn signature_from_base64(value: &str) -> Result<ed25519_dalek::Signature> {
    let bytes = decode_base64_bytes(value, "signature")?;
    ed25519_dalek::Signature::try_from(bytes.as_slice())
        .map_err(|error| anyhow!("invalid Ed25519 signature bytes: {error}"))
}

fn decode_base64_bytes(value: &str, label: &str) -> Result<Vec<u8>> {
    let encoded = value
        .strip_prefix("base64:")
        .ok_or_else(|| anyhow!("{label} must use `base64:` prefix"))?;
    BASE64_STANDARD
        .decode(encoded)
        .map_err(|error| anyhow!("{label} is not valid base64: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        ensure_local_runtime_trust, export_trust_bundle, import_trust_bundle,
        load_active_runtime_signing_key, TrustedAuthorityRoot,
    };
    use crate::{
        prism_paths::set_test_prism_home_override, protected_state::trust::load_trusted_root,
        PrismPaths,
    };

    fn temp_workspace(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "prism-protected-state-{label}-{}",
            prism_ir::new_sortable_token()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        root
    }

    #[test]
    fn bootstrapping_local_runtime_trust_creates_bundle_and_active_key() {
        let home = temp_workspace("home-a");
        let _guard = set_test_prism_home_override(&home);
        let workspace = temp_workspace("workspace-a");
        let paths = PrismPaths::for_workspace_root(&workspace).unwrap();

        let state = ensure_local_runtime_trust(&paths).unwrap();
        assert!(paths.runtime_authority_state_path().unwrap().exists());
        assert!(paths
            .trust_bundle_path(&state.active_trust_bundle_id)
            .unwrap()
            .exists());
        assert!(paths
            .runtime_signing_key_path(&state.active_runtime_key_id)
            .unwrap()
            .exists());

        let loaded = load_active_runtime_signing_key(&paths).unwrap();
        assert_eq!(
            loaded.state.active_runtime_key_id,
            state.active_runtime_key_id
        );
        assert_eq!(loaded.bundle.bundle_id, state.active_trust_bundle_id);
    }

    #[test]
    fn import_rejects_unknown_roots_without_explicit_pinning() {
        let home_a = temp_workspace("home-b1");
        let _guard_a = set_test_prism_home_override(&home_a);
        let workspace_a = temp_workspace("workspace-b1");
        let paths_a = PrismPaths::for_workspace_root(&workspace_a).unwrap();
        let state_a = ensure_local_runtime_trust(&paths_a).unwrap();
        let bundle = export_trust_bundle(&paths_a, &state_a.active_trust_bundle_id).unwrap();
        let root = load_trusted_root(&paths_a, &state_a.authority_root_id)
            .unwrap()
            .unwrap();

        let home_b = temp_workspace("home-b2");
        let _guard_b = set_test_prism_home_override(&home_b);
        let workspace_b = temp_workspace("workspace-b2");
        let paths_b = PrismPaths::for_workspace_root(&workspace_b).unwrap();

        let error = import_trust_bundle(&paths_b, &bundle, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("explicit trust pinning"));

        import_trust_bundle(&paths_b, &bundle, Some(&root)).unwrap();
        let imported_root: TrustedAuthorityRoot =
            load_trusted_root(&paths_b, &root.authority_root_id)
                .unwrap()
                .unwrap();
        assert_eq!(imported_root.authority_root_id, root.authority_root_id);
    }
}
