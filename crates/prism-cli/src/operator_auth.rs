use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use prism_core::{
    authenticate_principal_credential_in_registry, ensure_local_principal_registry_snapshot,
    AuthenticatedPrincipal, HumanSessionFile, PrismPaths,
};
use prism_ir::{CredentialId, PrincipalKind, PrincipalRegistrySnapshot};
use prism_store::{SqliteStore, Store};

use crate::git_support::ensure_repo_git_support;

pub(crate) struct AuthRegistryContext {
    pub(crate) store: SqliteStore,
    pub(crate) credentials_path: PathBuf,
    pub(crate) human_session_path: PathBuf,
}

pub(crate) struct ActiveHumanSessionContext {
    pub(crate) store: SqliteStore,
    pub(crate) snapshot: PrincipalRegistrySnapshot,
    pub(crate) human_session_path: PathBuf,
    pub(crate) sessions: HumanSessionFile,
    pub(crate) authenticated: AuthenticatedPrincipal,
}

pub(crate) fn load_auth_registry_context(root: &Path) -> Result<AuthRegistryContext> {
    ensure_repo_git_support(root)?;
    let paths = PrismPaths::for_workspace_root(root)?;
    let credentials_path = paths.credentials_path()?;
    let human_session_path = paths.human_session_path()?;
    let mut store = SqliteStore::open(paths.shared_runtime_db_path()?)?;
    let _ = ensure_local_principal_registry_snapshot(root, &mut store)?;
    Ok(AuthRegistryContext {
        store,
        credentials_path,
        human_session_path,
    })
}

pub(crate) fn load_principal_registry_snapshot(
    store: &mut SqliteStore,
) -> Result<PrincipalRegistrySnapshot> {
    store
        .load_principal_registry_snapshot()?
        .ok_or_else(|| anyhow!("principal registry is not initialized"))
}

pub(crate) fn require_active_human_session(root: &Path) -> Result<ActiveHumanSessionContext> {
    let AuthRegistryContext {
        mut store,
        human_session_path,
        ..
    } = load_auth_registry_context(root)?;
    let mut sessions = HumanSessionFile::load(&human_session_path)?;
    let session = sessions.active_session_now().ok_or_else(|| {
        anyhow!(
            "no active local human session is available; run `prism auth login` to unlock the stored credential first"
        )
    })?;
    let mut snapshot = load_principal_registry_snapshot(&mut store)?;
    let authenticated = authenticate_principal_credential_in_registry(
        &mut snapshot,
        &CredentialId::new(session.credential_id.clone()),
        &session.principal_token,
    )?;
    if authenticated.principal.kind != PrincipalKind::Human {
        bail!(
            "active local operator session must authenticate as a human principal; `{}` is `{:?}`",
            authenticated.principal.principal_id.0,
            authenticated.principal.kind
        );
    }
    Ok(ActiveHumanSessionContext {
        store,
        snapshot,
        human_session_path,
        sessions,
        authenticated,
    })
}

impl ActiveHumanSessionContext {
    pub(crate) fn persist(&mut self) -> Result<()> {
        self.store.save_principal_registry_snapshot(&self.snapshot)?;
        self.sessions.save(&self.human_session_path)
    }
}
