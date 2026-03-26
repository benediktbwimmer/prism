use prism_ir::{AnchorRef, EventActor, TaskId, Timestamp};

use crate::types::{OutcomeKind, OutcomeResult};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutcomeRecallQuery {
    pub anchors: Vec<AnchorRef>,
    pub task: Option<TaskId>,
    pub kinds: Option<Vec<OutcomeKind>>,
    pub result: Option<OutcomeResult>,
    pub actor: Option<EventActor>,
    pub since: Option<Timestamp>,
    pub limit: usize,
}
