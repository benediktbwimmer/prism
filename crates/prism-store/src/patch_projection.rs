use prism_ir::{AnchorRef, EventId, TaskId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchEventSummaryQuery {
    pub target: Option<AnchorRef>,
    pub task_id: Option<TaskId>,
    pub since: Option<u64>,
    pub path: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFileSummaryQuery {
    pub task_id: Option<TaskId>,
    pub since: Option<u64>,
    pub path: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchEventSummary {
    pub event_id: EventId,
    pub ts: u64,
    pub task_id: Option<String>,
    pub trigger: Option<String>,
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub work_id: Option<String>,
    pub work_title: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFileSummary {
    pub event_id: EventId,
    pub ts: u64,
    pub task_id: Option<String>,
    pub path: String,
    pub trigger: Option<String>,
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub work_id: Option<String>,
    pub work_title: Option<String>,
    pub summary: String,
    pub changed_symbol_count: usize,
    pub added_count: usize,
    pub removed_count: usize,
    pub updated_count: usize,
}
