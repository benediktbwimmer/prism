use std::error::Error;
use std::fmt;

use tracing::warn;

use crate::workspace_identity::workspace_identity_for_root;
use crate::{AuthenticatedPrincipal, WorkspaceSession};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundWorktreePrincipal {
    pub authority_id: String,
    pub principal_id: String,
    pub principal_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePrincipalConflict {
    pub worktree_id: String,
    pub bound_principal: BoundWorktreePrincipal,
    pub attempted_principal: BoundWorktreePrincipal,
}

impl fmt::Display for WorktreePrincipalConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "worktree `{}` is already bound to principal `{}` (`{}`) and cannot accept principal `{}` (`{}`)",
            self.worktree_id,
            self.bound_principal.principal_id,
            self.bound_principal.principal_name,
            self.attempted_principal.principal_id,
            self.attempted_principal.principal_name,
        )
    }
}

impl Error for WorktreePrincipalConflict {}

impl WorkspaceSession {
    pub fn bind_or_validate_worktree_principal(
        &self,
        authenticated: &AuthenticatedPrincipal,
    ) -> Result<(), WorktreePrincipalConflict> {
        let attempted = BoundWorktreePrincipal::from_authenticated(authenticated);
        let mut guard = self
            .worktree_principal_binding
            .lock()
            .expect("worktree principal binding lock poisoned");
        let result = match guard.as_mut() {
            None => {
                *guard = Some(attempted);
                Ok(())
            }
            Some(bound) if bound.same_identity(&attempted) => {
                bound.principal_name = attempted.principal_name;
                Ok(())
            }
            Some(bound) => Err(WorktreePrincipalConflict {
                worktree_id: workspace_identity_for_root(&self.root).worktree_id,
                bound_principal: bound.clone(),
                attempted_principal: attempted,
            }),
        };
        drop(guard);

        if result.is_ok() {
            if let Err(error) = self.publish_pending_repo_patch_provenance_for_active_work() {
                warn!(
                    root = %self.root.display(),
                    error = %error,
                    "failed to publish pending repo patch provenance after binding worktree principal"
                );
            }
        }

        result
    }

    pub fn bound_worktree_principal(&self) -> Option<BoundWorktreePrincipal> {
        self.worktree_principal_binding
            .lock()
            .expect("worktree principal binding lock poisoned")
            .clone()
    }
}

impl BoundWorktreePrincipal {
    fn from_authenticated(authenticated: &AuthenticatedPrincipal) -> Self {
        Self {
            authority_id: authenticated.principal.authority_id.0.to_string(),
            principal_id: authenticated.principal.principal_id.0.to_string(),
            principal_name: authenticated.principal.name.clone(),
        }
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.authority_id == other.authority_id && self.principal_id == other.principal_id
    }
}
