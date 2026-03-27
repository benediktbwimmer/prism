use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use prism_agent::{InferenceStore, InferredEdgeRecord, InferredEdgeScope};
use prism_ir::{AgentId, EventId, SessionId, TaskId};
use prism_memory::SessionMemory;
use prism_query::QueryLimits;

use crate::NEXT_SESSION_ID;

#[derive(Debug, Clone)]
pub(crate) struct SessionTaskState {
    pub(crate) id: TaskId,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
}

pub(crate) struct SessionState {
    session_id: SessionId,
    pub(crate) notes: Arc<SessionMemory>,
    pub(crate) inferred_edges: SessionInferenceStore,
    current_task: Mutex<Option<SessionTaskState>>,
    current_agent: Mutex<Option<AgentId>>,
    next_event: Arc<AtomicU64>,
    next_task: Arc<AtomicU64>,
    limits: Mutex<QueryLimits>,
}

impl SessionState {
    pub(crate) fn new(
        notes: Arc<SessionMemory>,
        inferred_edges: Arc<InferenceStore>,
        next_event: Arc<AtomicU64>,
        next_task: Arc<AtomicU64>,
        limits: QueryLimits,
    ) -> Self {
        Self {
            session_id: SessionId::new(format!(
                "session:{}",
                NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            )),
            notes,
            inferred_edges: SessionInferenceStore::new(inferred_edges),
            current_task: Mutex::new(None),
            current_agent: Mutex::new(None),
            next_event,
            next_task,
            limits: Mutex::new(limits),
        }
    }

    pub(crate) fn next_event_id(&self, prefix: &str) -> EventId {
        let sequence = self.next_event.fetch_add(1, Ordering::Relaxed) + 1;
        EventId::new(format!("{prefix}:{sequence}"))
    }

    pub(crate) fn current_task(&self) -> Option<TaskId> {
        self.current_task
            .lock()
            .expect("session task lock poisoned")
            .as_ref()
            .map(|task| task.id.clone())
    }

    pub(crate) fn session_id(&self) -> SessionId {
        self.session_id.clone()
    }

    pub(crate) fn current_agent(&self) -> Option<AgentId> {
        self.current_agent
            .lock()
            .expect("session agent lock poisoned")
            .clone()
    }

    pub(crate) fn current_task_state(&self) -> Option<SessionTaskState> {
        self.current_task
            .lock()
            .expect("session task lock poisoned")
            .clone()
    }

    pub(crate) fn set_current_task(
        &self,
        task: TaskId,
        description: Option<String>,
        tags: Vec<String>,
    ) {
        *self
            .current_task
            .lock()
            .expect("session task lock poisoned") = Some(SessionTaskState {
            id: task,
            description,
            tags,
        });
    }

    pub(crate) fn update_current_task_metadata(
        &self,
        description: Option<Option<String>>,
        tags: Option<Vec<String>>,
    ) {
        if let Some(task) = self
            .current_task
            .lock()
            .expect("session task lock poisoned")
            .as_mut()
        {
            if let Some(description) = description {
                task.description = description;
            }
            if let Some(tags) = tags {
                task.tags = tags;
            }
        }
    }

    pub(crate) fn clear_current_task(&self) {
        *self
            .current_task
            .lock()
            .expect("session task lock poisoned") = None;
    }

    pub(crate) fn set_current_agent(&self, agent: AgentId) {
        *self
            .current_agent
            .lock()
            .expect("session agent lock poisoned") = Some(agent);
    }

    pub(crate) fn clear_current_agent(&self) {
        *self
            .current_agent
            .lock()
            .expect("session agent lock poisoned") = None;
    }

    pub(crate) fn start_task(&self, description: &str, tags: &[String]) -> TaskId {
        let sequence = self.next_task.fetch_add(1, Ordering::Relaxed) + 1;
        let mut slug = description
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>();
        while slug.contains("--") {
            slug = slug.replace("--", "-");
        }
        slug = slug.trim_matches('-').to_owned();
        let prefix = if slug.is_empty() { "task" } else { &slug };
        let task = TaskId::new(format!("task:{prefix}:{sequence}"));
        self.set_current_task(task.clone(), Some(description.to_string()), tags.to_vec());
        task
    }

    pub(crate) fn task_for_mutation(&self, explicit: Option<TaskId>) -> TaskId {
        if let Some(task) = explicit {
            return task;
        }
        if let Some(task) = self.current_task() {
            return task;
        }
        self.start_task("session", &[])
    }

    pub(crate) fn limits(&self) -> QueryLimits {
        *self.limits.lock().expect("session limits lock poisoned")
    }

    pub(crate) fn set_limits(&self, limits: QueryLimits) {
        *self.limits.lock().expect("session limits lock poisoned") = limits;
    }
}

pub(crate) struct SessionInferenceStore {
    persisted: Arc<InferenceStore>,
    session_only: InferenceStore,
}

impl SessionInferenceStore {
    fn new(persisted: Arc<InferenceStore>) -> Self {
        Self {
            persisted,
            session_only: InferenceStore::new(),
        }
    }

    pub(crate) fn record(&self, id: &prism_agent::EdgeId) -> Option<InferredEdgeRecord> {
        self.session_only
            .record(id)
            .or_else(|| self.persisted.record(id))
    }

    pub(crate) fn edges_from(
        &self,
        source: &prism_ir::NodeId,
        kind: Option<prism_ir::EdgeKind>,
    ) -> Vec<InferredEdgeRecord> {
        let mut records = self.persisted.edges_from(source, kind);
        records.extend(self.session_only.edges_from(source, kind));
        records
    }

    pub(crate) fn edges_to(
        &self,
        target: &prism_ir::NodeId,
        kind: Option<prism_ir::EdgeKind>,
    ) -> Vec<InferredEdgeRecord> {
        let mut records = self.persisted.edges_to(target, kind);
        records.extend(self.session_only.edges_to(target, kind));
        records
    }

    pub(crate) fn store_edge(
        &self,
        edge: prism_ir::Edge,
        scope: InferredEdgeScope,
        task: Option<TaskId>,
        evidence: Vec<String>,
    ) -> prism_agent::EdgeId {
        match scope {
            InferredEdgeScope::SessionOnly => {
                self.session_only.store_edge(edge, scope, task, evidence)
            }
            _ => self.persisted.store_edge(edge, scope, task, evidence),
        }
    }
}
