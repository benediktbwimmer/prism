#[cfg(test)]
use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Context, Result};
use prism_store::migrate_worktree_cache_from_shared_runtime;
use serde::{Deserialize, Serialize};

use crate::util::current_timestamp_millis;
use crate::workspace_identity::{workspace_identity_for_root, WorkspaceIdentity};

const PRISM_HOME_ENV: &str = "PRISM_HOME";
const PATH_METADATA_VERSION: u32 = 1;
const REPO_METADATA_FILE_NAME: &str = "repo.json";
const SESSION_SEED_FILE_NAME: &str = "prism-mcp-session-seed.json";
const WORKTREE_METADATA_FILE_NAME: &str = "worktree.json";

#[cfg(test)]
thread_local! {
    static TEST_PRISM_HOME_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static TEST_TEMP_PRISM_HOME_STATE: RefCell<TestTempPrismHomeState> = RefCell::new(
        TestTempPrismHomeState { path: None }
    );
}

#[cfg(test)]
static NEXT_TEST_PRISM_HOME: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
struct TestTempPrismHomeState {
    path: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for TestTempPrismHomeState {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrismPaths {
    identity: WorkspaceIdentity,
    home_root: PathBuf,
    repo_prism_dir: PathBuf,
    repo_home_dir: PathBuf,
    worktree_cache_dir: PathBuf,
    worktree_cache_db_path: PathBuf,
    worktree_backups_dir: PathBuf,
    shared_runtime_dir: PathBuf,
    shared_runtime_db_path: PathBuf,
    shared_backups_dir: PathBuf,
    feedback_dir: PathBuf,
    validation_feedback_path: PathBuf,
    worktree_dir: PathBuf,
    worktree_mcp_state_dir: PathBuf,
    worktree_mcp_logs_dir: PathBuf,
}

impl PrismPaths {
    pub fn for_workspace_root(root: &Path) -> Result<Self> {
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let identity = workspace_identity_for_root(&canonical_root);
        let home_root = prism_home_root()?;
        let repo_home_dir = home_root
            .join("repos")
            .join(storage_component(&identity.repo_id));
        let worktree_dir = repo_home_dir
            .join("worktrees")
            .join(storage_component(&identity.worktree_id));
        let worktree_cache_dir = worktree_dir.join("cache");
        let shared_runtime_dir = repo_home_dir.join("shared").join("runtime");
        let feedback_dir = repo_home_dir.join("feedback");
        let worktree_mcp_state_dir = worktree_dir.join("mcp").join("state");
        let worktree_mcp_logs_dir = worktree_dir.join("mcp").join("logs");
        Ok(Self {
            identity,
            home_root,
            repo_prism_dir: canonical_root.join(".prism"),
            worktree_cache_dir: worktree_cache_dir.clone(),
            worktree_cache_db_path: worktree_cache_dir.join("state.db"),
            worktree_backups_dir: worktree_dir.join("backups"),
            shared_runtime_db_path: shared_runtime_dir.join("state.db"),
            shared_backups_dir: repo_home_dir.join("shared").join("backups"),
            validation_feedback_path: feedback_dir.join("validation_feedback.jsonl"),
            repo_home_dir,
            shared_runtime_dir,
            feedback_dir,
            worktree_dir,
            worktree_mcp_state_dir,
            worktree_mcp_logs_dir,
        })
    }

    pub fn repo_prism_dir(&self) -> &Path {
        &self.repo_prism_dir
    }

    pub fn home_root(&self) -> &Path {
        &self.home_root
    }

    pub fn repo_home_dir(&self) -> &Path {
        &self.repo_home_dir
    }

    pub fn shared_runtime_dir(&self) -> &Path {
        &self.shared_runtime_dir
    }

    pub fn shared_backups_dir(&self) -> &Path {
        &self.shared_backups_dir
    }

    pub fn feedback_dir(&self) -> &Path {
        &self.feedback_dir
    }

    pub fn worktree_dir(&self) -> &Path {
        &self.worktree_dir
    }

    pub fn worktree_cache_dir(&self) -> &Path {
        &self.worktree_cache_dir
    }

    pub fn worktree_backups_dir(&self) -> &Path {
        &self.worktree_backups_dir
    }

    pub fn worktree_mcp_state_dir(&self) -> &Path {
        &self.worktree_mcp_state_dir
    }

    pub fn worktree_mcp_logs_dir(&self) -> &Path {
        &self.worktree_mcp_logs_dir
    }

    pub fn shared_runtime_db_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.shared_runtime_dir)
            .with_context(|| format!("failed to create {}", self.shared_runtime_dir.display()))?;
        Ok(self.shared_runtime_db_path.clone())
    }

    pub fn worktree_cache_db_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.worktree_cache_dir)
            .with_context(|| format!("failed to create {}", self.worktree_cache_dir.display()))?;
        migrate_legacy_sqlite_store(
            &self.worktree_cache_db_path,
            &self.repo_prism_dir.join("cache.db"),
        )?;
        migrate_legacy_backups(
            &self.worktree_backups_dir,
            &self.repo_prism_dir.join("backups"),
            "cache.db",
            "state.db",
        )?;
        migrate_worktree_cache_from_shared_runtime(
            &self.worktree_cache_db_path,
            &self.shared_runtime_db_path,
        )?;
        Ok(self.worktree_cache_db_path.clone())
    }

    pub fn credentials_path(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.home_root)
            .with_context(|| format!("failed to create {}", self.home_root.display()))?;
        Ok(self.home_root.join("credentials.toml"))
    }

    pub fn trust_dir(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        let path = self.home_root.join("trust");
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(path)
    }

    pub fn trust_bundle_path(&self, bundle_id: &str) -> Result<PathBuf> {
        let path = self
            .trust_dir()?
            .join("bundles")
            .join(format!("{}.json", storage_component(bundle_id)));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(path)
    }

    pub fn trusted_root_path(&self, authority_root_id: &str) -> Result<PathBuf> {
        let path = self
            .trust_dir()?
            .join("roots")
            .join(format!("{}.json", storage_component(authority_root_id)));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(path)
    }

    pub fn trusted_root_key_path(&self, issuer_key_id: &str) -> Result<PathBuf> {
        let path = self
            .trust_dir()?
            .join("root-keys")
            .join(format!("{}.json", storage_component(issuer_key_id)));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(path)
    }

    pub fn runtime_signing_key_path(&self, runtime_key_id: &str) -> Result<PathBuf> {
        let path = self
            .trust_dir()?
            .join("runtime-keys")
            .join(format!("{}.json", storage_component(runtime_key_id)));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(path)
    }

    pub fn runtime_authority_state_path(&self) -> Result<PathBuf> {
        let path = self.trust_dir()?.join("runtime-authority.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(path)
    }

    pub fn validation_feedback_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        migrate_legacy_file(
            &self.validation_feedback_path,
            &self.repo_prism_dir.join("validation_feedback.jsonl"),
        )?;
        Ok(self.validation_feedback_path.clone())
    }

    pub fn mcp_http_uri_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.worktree_mcp_state_dir).with_context(|| {
            format!("failed to create {}", self.worktree_mcp_state_dir.display())
        })?;
        let path = self.worktree_mcp_state_dir.join("prism-mcp-http-uri");
        migrate_legacy_file(&path, &self.repo_prism_dir.join("prism-mcp-http-uri"))?;
        Ok(path)
    }

    pub fn mcp_runtime_state_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.worktree_mcp_state_dir).with_context(|| {
            format!("failed to create {}", self.worktree_mcp_state_dir.display())
        })?;
        let path = self.worktree_mcp_state_dir.join("prism-mcp-runtime.json");
        migrate_legacy_file(&path, &self.repo_prism_dir.join("prism-mcp-runtime.json"))?;
        Ok(path)
    }

    pub fn mcp_session_seed_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.worktree_mcp_state_dir).with_context(|| {
            format!("failed to create {}", self.worktree_mcp_state_dir.display())
        })?;
        let path = self.worktree_mcp_state_dir.join(SESSION_SEED_FILE_NAME);
        migrate_legacy_file(&path, &self.repo_prism_dir.join(SESSION_SEED_FILE_NAME))?;
        Ok(path)
    }

    pub fn mcp_startup_marker_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.worktree_mcp_state_dir).with_context(|| {
            format!("failed to create {}", self.worktree_mcp_state_dir.display())
        })?;
        let path = self.worktree_mcp_state_dir.join("prism-mcp-startup");
        migrate_legacy_file(&path, &self.repo_prism_dir.join("prism-mcp-startup"))?;
        Ok(path)
    }

    pub fn mcp_daemon_log_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.worktree_mcp_logs_dir).with_context(|| {
            format!("failed to create {}", self.worktree_mcp_logs_dir.display())
        })?;
        let path = self.worktree_mcp_logs_dir.join("prism-mcp-daemon.log");
        migrate_legacy_file(&path, &self.repo_prism_dir.join("prism-mcp-daemon.log"))?;
        Ok(path)
    }

    pub fn mcp_call_log_path(&self) -> Result<PathBuf> {
        self.ensure_home_metadata()?;
        fs::create_dir_all(&self.worktree_mcp_logs_dir).with_context(|| {
            format!("failed to create {}", self.worktree_mcp_logs_dir.display())
        })?;
        let path = self.worktree_mcp_logs_dir.join("prism-mcp-call-log.jsonl");
        migrate_legacy_file(&path, &self.repo_prism_dir.join("prism-mcp-call-log.jsonl"))?;
        Ok(path)
    }

    fn ensure_home_metadata(&self) -> Result<()> {
        fs::create_dir_all(&self.repo_home_dir)
            .with_context(|| format!("failed to create {}", self.repo_home_dir.display()))?;
        fs::create_dir_all(&self.worktree_dir)
            .with_context(|| format!("failed to create {}", self.worktree_dir.display()))?;
        write_repo_metadata(
            &self.repo_home_dir.join(REPO_METADATA_FILE_NAME),
            &self.identity,
        )?;
        write_worktree_metadata(
            &self.worktree_dir.join(WORKTREE_METADATA_FILE_NAME),
            &self.identity,
        )?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoMetadata {
    version: u32,
    repo_id: String,
    locator_kind: String,
    locator_path: String,
    canonical_root_hint: String,
    created_at: u64,
    last_seen_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorktreeMetadata {
    version: u32,
    repo_id: String,
    worktree_id: String,
    canonical_root: String,
    branch_ref: Option<String>,
    created_at: u64,
    last_seen_at: u64,
}

fn prism_home_root() -> Result<PathBuf> {
    #[cfg(test)]
    {
        if let Some(override_dir) = TEST_PRISM_HOME_OVERRIDE.with(|slot| slot.borrow().clone()) {
            return Ok(override_dir);
        }
        if let Some(override_dir) = env::var_os(PRISM_HOME_ENV) {
            return Ok(PathBuf::from(override_dir));
        }
        return ensure_test_prism_home_root();
    }

    #[cfg(not(test))]
    {
        if let Some(override_dir) = env::var_os(PRISM_HOME_ENV) {
            return Ok(PathBuf::from(override_dir));
        }
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("could not resolve home directory; set PRISM_HOME"))?;
        return Ok(home.join(".prism"));
    }

    #[allow(unreachable_code)]
    Err(anyhow!("unreachable prism home resolution branch"))
}

#[cfg(test)]
fn ensure_test_prism_home_root() -> Result<PathBuf> {
    TEST_TEMP_PRISM_HOME_STATE.with(|slot| {
        if let Some(path) = slot.borrow().path.clone() {
            return Ok(path);
        }
        let unique = NEXT_TEST_PRISM_HOME.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "prism-test-home-{}-{}-{unique}",
            std::process::id(),
            current_timestamp_millis()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        TEST_PRISM_HOME_OVERRIDE.with(|override_slot| {
            *override_slot.borrow_mut() = Some(path.clone());
        });
        slot.borrow_mut().path = Some(path.clone());
        Ok(path)
    })
}

#[cfg(test)]
pub(crate) struct TestPrismHomeOverrideGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
pub(crate) fn set_test_prism_home_override(path: &Path) -> TestPrismHomeOverrideGuard {
    let previous = TEST_PRISM_HOME_OVERRIDE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let previous = slot.clone();
        *slot = Some(path.to_path_buf());
        previous
    });
    TestPrismHomeOverrideGuard { previous }
}

#[cfg(test)]
impl Drop for TestPrismHomeOverrideGuard {
    fn drop(&mut self) {
        TEST_PRISM_HOME_OVERRIDE.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

fn storage_component(id: &str) -> String {
    id.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .collect()
}

fn write_repo_metadata(path: &Path, identity: &WorkspaceIdentity) -> Result<()> {
    let existing = read_json_file::<RepoMetadata>(path);
    let now = current_timestamp_millis();
    write_json_file(
        path,
        &RepoMetadata {
            version: PATH_METADATA_VERSION,
            repo_id: identity.repo_id.clone(),
            locator_kind: identity.repo_locator_kind.to_string(),
            locator_path: identity.repo_locator_path.to_string_lossy().to_string(),
            canonical_root_hint: identity.canonical_root.to_string_lossy().to_string(),
            created_at: existing
                .as_ref()
                .map_or(now, |metadata| metadata.created_at),
            last_seen_at: now,
        },
    )
}

fn write_worktree_metadata(path: &Path, identity: &WorkspaceIdentity) -> Result<()> {
    let existing = read_json_file::<WorktreeMetadata>(path);
    let now = current_timestamp_millis();
    write_json_file(
        path,
        &WorktreeMetadata {
            version: PATH_METADATA_VERSION,
            repo_id: identity.repo_id.clone(),
            worktree_id: identity.worktree_id.clone(),
            canonical_root: identity.canonical_root.to_string_lossy().to_string(),
            branch_ref: identity.branch_ref.clone(),
            created_at: existing
                .as_ref()
                .map_or(now, |metadata| metadata.created_at),
            last_seen_at: now,
        },
    )
}

fn read_json_file<T>(path: &Path) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut bytes =
        serde_json::to_vec_pretty(value).context("failed to serialize PRISM path metadata")?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn migrate_legacy_file(target: &Path, legacy: &Path) -> Result<()> {
    if target.exists() || !legacy.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    rename_or_copy(legacy, target)
}

fn migrate_legacy_sqlite_store(target: &Path, legacy: &Path) -> Result<()> {
    let legacy_exists = ["", "-shm", "-wal"]
        .into_iter()
        .map(|suffix| with_suffix(legacy, suffix))
        .any(|path| path.exists());
    if !legacy_exists || target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    for suffix in ["", "-shm", "-wal"] {
        let legacy_path = with_suffix(legacy, suffix);
        if !legacy_path.exists() {
            continue;
        }
        let target_path = with_suffix(target, suffix);
        rename_or_copy(&legacy_path, &target_path)?;
    }
    Ok(())
}

fn migrate_legacy_backups(
    target_dir: &Path,
    legacy_dir: &Path,
    old_prefix: &str,
    new_prefix: &str,
) -> Result<()> {
    if !legacy_dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create {}", target_dir.display()))?;
    for entry in fs::read_dir(legacy_dir)
        .with_context(|| format!("failed to read {}", legacy_dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", legacy_dir.display()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(old_prefix) {
            continue;
        }
        let suffix = &name[old_prefix.len()..];
        let target = target_dir.join(format!("{new_prefix}{suffix}"));
        if target.exists() {
            continue;
        }
        rename_or_copy(&entry.path(), &target)?;
    }
    Ok(())
}

fn rename_or_copy(source: &Path, target: &Path) -> Result<()> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            fs::copy(source, target).with_context(|| {
                format!(
                    "failed to copy legacy path {} to {} after rename error: {rename_error}",
                    source.display(),
                    target.display()
                )
            })?;
            if source.is_file() {
                fs::remove_file(source).with_context(|| {
                    format!("failed to remove legacy file {}", source.display())
                })?;
            }
            Ok(())
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), suffix))
}
