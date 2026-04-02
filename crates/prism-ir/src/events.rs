use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{
    CredentialId, EventId, PrincipalActor, PrincipalAuthorityId, PrincipalId, PrincipalKind,
    TaskId, Timestamp,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventMeta {
    pub id: EventId,
    pub ts: Timestamp,
    pub actor: EventActor,
    pub correlation: Option<TaskId>,
    pub causation: Option<EventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_context: Option<EventExecutionContext>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventExecutionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<CredentialId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_context: Option<WorkContextSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkContextKind {
    Undeclared,
    AdHoc,
    Coordination,
    Delegated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkContextSnapshot {
    pub work_id: String,
    pub kind: WorkContextKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_work_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordination_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservedChangeCheckpointTrigger {
    MutationBoundary,
    WorkTransition,
    Disconnect,
    ExplicitCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObservedChangeCheckpointEntry {
    pub trigger: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    pub file_count: usize,
    pub added_nodes: usize,
    pub removed_nodes: usize,
    pub updated_nodes: usize,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObservedChangeCheckpoint {
    pub flush_trigger: ObservedChangeCheckpointTrigger,
    pub changed_paths: Vec<String>,
    pub entries: Vec<ObservedChangeCheckpointEntry>,
    pub window_started_at: Timestamp,
    pub window_ended_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EventActor {
    User,
    Agent,
    System,
    Principal(PrincipalActor),
    GitAuthor {
        #[schemars(with = "String")]
        name: SmolStr,
        #[schemars(with = "Option<String>")]
        email: Option<SmolStr>,
    },
    CI,
}

impl EventActor {
    pub fn canonical_identity_actor(&self) -> Self {
        match self {
            Self::Principal(actor) => Self::Principal(actor.clone()),
            Self::Agent => Self::Principal(PrincipalActor {
                authority_id: PrincipalAuthorityId::new("legacy"),
                principal_id: PrincipalId::new("legacy_agent_fallback"),
                kind: Some(PrincipalKind::Agent),
                name: Some("legacy_agent_fallback".to_string()),
            }),
            Self::User => Self::Principal(PrincipalActor {
                authority_id: PrincipalAuthorityId::new("legacy"),
                principal_id: PrincipalId::new("legacy_human_fallback"),
                kind: Some(PrincipalKind::Human),
                name: Some("legacy_human_fallback".to_string()),
            }),
            _ => self.clone(),
        }
    }

    pub fn canonical_identity_key(&self) -> String {
        match self.canonical_identity_actor() {
            Self::User => "user".to_string(),
            Self::Agent => "agent".to_string(),
            Self::System => "system".to_string(),
            Self::Principal(actor) => actor.scoped_id(),
            Self::CI => "ci".to_string(),
            Self::GitAuthor { name, email } => {
                format!("git:{}:{}", name, email.as_deref().unwrap_or(""))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EventActor;
    use crate::{PrincipalActor, PrincipalAuthorityId, PrincipalId, PrincipalKind};

    #[test]
    fn principal_actor_scoped_id_uses_authority_and_principal_id() {
        let actor = EventActor::Principal(PrincipalActor {
            authority_id: PrincipalAuthorityId::new("local-daemon"),
            principal_id: PrincipalId::new("agent-7"),
            kind: None,
            name: None,
        });

        let EventActor::Principal(principal) = actor else {
            panic!("expected principal actor");
        };
        assert_eq!(principal.scoped_id(), "principal:local-daemon:agent-7");
    }

    #[test]
    fn legacy_agent_actor_canonicalizes_to_fallback_principal() {
        let canonical = EventActor::Agent.canonical_identity_actor();
        let EventActor::Principal(principal) = canonical else {
            panic!("expected principal actor");
        };
        assert_eq!(principal.authority_id, PrincipalAuthorityId::new("legacy"));
        assert_eq!(
            principal.principal_id,
            PrincipalId::new("legacy_agent_fallback")
        );
        assert_eq!(principal.kind, Some(PrincipalKind::Agent));
        assert_eq!(principal.name.as_deref(), Some("legacy_agent_fallback"));
    }

    #[test]
    fn legacy_user_actor_canonicalizes_to_fallback_principal() {
        let canonical = EventActor::User.canonical_identity_actor();
        let EventActor::Principal(principal) = canonical else {
            panic!("expected principal actor");
        };
        assert_eq!(principal.authority_id, PrincipalAuthorityId::new("legacy"));
        assert_eq!(
            principal.principal_id,
            PrincipalId::new("legacy_human_fallback")
        );
        assert_eq!(principal.kind, Some(PrincipalKind::Human));
        assert_eq!(principal.name.as_deref(), Some("legacy_human_fallback"));
    }
}
