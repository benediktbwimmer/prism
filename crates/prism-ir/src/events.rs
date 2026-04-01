use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{CredentialId, EventId, PrincipalActor, TaskId, Timestamp};

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
    pub credential_id: Option<CredentialId>,
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

#[cfg(test)]
mod tests {
    use super::EventActor;
    use crate::{PrincipalActor, PrincipalAuthorityId, PrincipalId};

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
}
