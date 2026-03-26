use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{EventId, TaskId, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventMeta {
    pub id: EventId,
    pub ts: Timestamp,
    pub actor: EventActor,
    pub correlation: Option<TaskId>,
    pub causation: Option<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EventActor {
    User,
    Agent,
    System,
    GitAuthor {
        #[schemars(with = "String")]
        name: SmolStr,
        #[schemars(with = "Option<String>")]
        email: Option<SmolStr>,
    },
    CI,
}
