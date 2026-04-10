use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use prism_agent::{InferenceStore, InferredEdgeRecord, InferredEdgeScope};
use prism_ir::{
    AgentId, EventId, NodeId, NodeKind, SessionId, TaskId, WorkContextKind, new_prefixed_id,
    new_slugged_id, new_sortable_token,
};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryModule, RecallQuery, ScoredMemory,
    SessionMemory,
};
use prism_query::QueryLimits;

#[derive(Debug, Clone)]
pub(crate) struct SessionTaskState {
    pub(crate) id: TaskId,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) coordination_task_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionWorkState {
    pub(crate) id: TaskId,
    pub(crate) kind: WorkContextKind,
    pub(crate) title: String,
    pub(crate) summary: Option<String>,
    pub(crate) parent_work_id: Option<TaskId>,
    pub(crate) coordination_task_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) plan_title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionHandleCategory {
    Symbol,
    TextFragment,
    Concept,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionHandleTarget {
    pub(crate) id: NodeId,
    pub(crate) lineage_id: Option<String>,
    pub(crate) handle_category: SessionHandleCategory,
    pub(crate) name: String,
    pub(crate) kind: NodeKind,
    pub(crate) file_path: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) why_short: String,
    pub(crate) start_line: Option<usize>,
    pub(crate) end_line: Option<usize>,
    pub(crate) start_column: Option<usize>,
    pub(crate) end_column: Option<usize>,
}

pub(crate) struct SessionState {
    session_id: SessionId,
    pub(crate) notes: SessionNotes,
    pub(crate) inferred_edges: SessionInferenceStore,
    current_task: Mutex<Option<SessionTaskState>>,
    current_work: Mutex<Option<SessionWorkState>>,
    current_agent: Mutex<Option<AgentId>>,
    next_event: Arc<AtomicU64>,
    next_handle: AtomicU64,
    handle_targets: Mutex<HashMap<String, SessionHandleTarget>>,
    handle_keys: Mutex<HashMap<String, String>>,
    limits: Mutex<QueryLimits>,
}

impl SessionState {
    pub(crate) fn new(
        notes: Option<Arc<SessionMemory>>,
        inferred_edges: Option<Arc<InferenceStore>>,
        next_event: Arc<AtomicU64>,
        limits: QueryLimits,
    ) -> Self {
        Self {
            session_id: SessionId::new(new_prefixed_id("session")),
            notes: SessionNotes::new(notes),
            inferred_edges: SessionInferenceStore::new(inferred_edges),
            current_task: Mutex::new(None),
            current_work: Mutex::new(None),
            current_agent: Mutex::new(None),
            next_event,
            next_handle: AtomicU64::new(1),
            handle_targets: Mutex::new(HashMap::new()),
            handle_keys: Mutex::new(HashMap::new()),
            limits: Mutex::new(limits),
        }
    }

    pub(crate) fn next_event_id(&self, prefix: &str) -> EventId {
        let sequence = self.next_event.fetch_add(1, Ordering::Relaxed) + 1;
        let session_fragment = format!("{:?}", self.session_id)
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect::<String>();
        EventId::new(format!(
            "{prefix}:{}:{session_fragment}:{sequence}",
            new_sortable_token()
        ))
    }

    pub(crate) fn current_task(&self) -> Option<TaskId> {
        self.current_task
            .lock()
            .expect("session task lock poisoned")
            .as_ref()
            .map(|task| task.id.clone())
    }

    pub(crate) fn effective_current_task(&self) -> Option<TaskId> {
        self.effective_current_task_state().map(|task| task.id)
    }

    pub(crate) fn current_work(&self) -> Option<TaskId> {
        self.current_work
            .lock()
            .expect("session work lock poisoned")
            .as_ref()
            .map(|work| work.id.clone())
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

    pub(crate) fn effective_current_task_state(&self) -> Option<SessionTaskState> {
        let task = self.current_task_state()?;
        match self.current_work_state() {
            Some(work) if session_task_matches_work(&task, &work) => Some(task),
            Some(_) => session_task_is_plan_node_focus(&task).then_some(task),
            None => (session_task_is_coordination_focus(&task)
                || session_task_is_plan_node_focus(&task))
            .then_some(task),
        }
    }

    pub(crate) fn persistable_current_task_state(&self) -> Option<SessionTaskState> {
        self.current_work_state()
            .and_then(|_| self.effective_current_task_state())
    }

    pub(crate) fn current_work_state(&self) -> Option<SessionWorkState> {
        self.current_work
            .lock()
            .expect("session work lock poisoned")
            .clone()
    }

    pub(crate) fn set_current_task(
        &self,
        task: TaskId,
        description: Option<String>,
        tags: Vec<String>,
        coordination_task_id: Option<String>,
    ) {
        *self
            .current_task
            .lock()
            .expect("session task lock poisoned") = Some(SessionTaskState {
            id: task,
            description,
            tags,
            coordination_task_id,
        });
    }

    pub(crate) fn set_current_work(&self, work: SessionWorkState) {
        *self
            .current_work
            .lock()
            .expect("session work lock poisoned") = Some(work);
    }

    pub(crate) fn update_current_work<F>(&self, update: F)
    where
        F: FnOnce(&mut SessionWorkState),
    {
        if let Some(work) = self
            .current_work
            .lock()
            .expect("session work lock poisoned")
            .as_mut()
        {
            update(work);
        }
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

    #[allow(dead_code)]
    pub(crate) fn clear_current_work(&self) {
        *self
            .current_work
            .lock()
            .expect("session work lock poisoned") = None;
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

    pub(crate) fn start_task(
        &self,
        description: &str,
        tags: &[String],
        explicit_task_id: Option<TaskId>,
        coordination_task_id: Option<String>,
    ) -> TaskId {
        let task = explicit_task_id.unwrap_or_else(|| self.next_described_task_id(description));
        let coordination_task_id = coordination_task_id.or_else(|| {
            task.0
                .as_str()
                .starts_with("coord-task:")
                .then(|| task.0.to_string())
        });
        self.set_current_task(
            task.clone(),
            Some(description.to_string()),
            tags.to_vec(),
            coordination_task_id,
        );
        task
    }

    fn next_described_task_id(&self, description: &str) -> TaskId {
        TaskId::new(new_slugged_id("task", description))
    }

    fn next_described_work_id(&self, title: &str) -> TaskId {
        TaskId::new(new_slugged_id("work", title))
    }

    pub(crate) fn declare_work(
        &self,
        title: &str,
        kind: WorkContextKind,
        summary: Option<String>,
        parent_work_id: Option<TaskId>,
        coordination_task_id: Option<String>,
        plan_id: Option<String>,
        plan_title: Option<String>,
    ) -> TaskId {
        let work = SessionWorkState {
            id: self.next_described_work_id(title),
            kind,
            title: title.to_string(),
            summary,
            parent_work_id,
            coordination_task_id,
            plan_id,
            plan_title,
        };
        let work_id = work.id.clone();
        self.set_current_work(work);
        work_id
    }

    pub(crate) fn task_for_mutation(&self, explicit: Option<TaskId>) -> TaskId {
        if let Some(task) = explicit {
            return task;
        }
        if let Some(work) = self.current_work() {
            return work;
        }
        if let Some(task) = self.current_task() {
            return task;
        }
        self.start_task("session", &[], None, None)
    }

    pub(crate) fn limits(&self) -> QueryLimits {
        *self.limits.lock().expect("session limits lock poisoned")
    }

    pub(crate) fn set_limits(&self, limits: QueryLimits) {
        *self.limits.lock().expect("session limits lock poisoned") = limits;
    }

    pub(crate) fn intern_target_handle(&self, target: SessionHandleTarget) -> String {
        let key = session_handle_key(&target);
        if let Some(existing) = self
            .handle_keys
            .lock()
            .expect("session handle keys lock poisoned")
            .get(&key)
            .cloned()
        {
            self.handle_targets
                .lock()
                .expect("session handle targets lock poisoned")
                .insert(existing.clone(), target);
            return existing;
        }

        let sequence = self.next_handle.fetch_add(1, Ordering::Relaxed);
        let handle = format!("handle:{sequence}");
        self.handle_targets
            .lock()
            .expect("session handle targets lock poisoned")
            .insert(handle.clone(), target);
        self.handle_keys
            .lock()
            .expect("session handle keys lock poisoned")
            .insert(key, handle.clone());
        handle
    }

    pub(crate) fn handle_target(&self, handle: &str) -> Option<SessionHandleTarget> {
        self.handle_targets
            .lock()
            .expect("session handle targets lock poisoned")
            .get(handle)
            .cloned()
    }

    pub(crate) fn refresh_target_handle(&self, handle: &str, target: SessionHandleTarget) {
        let key = session_handle_key(&target);
        self.handle_targets
            .lock()
            .expect("session handle targets lock poisoned")
            .insert(handle.to_string(), target);
        self.handle_keys
            .lock()
            .expect("session handle keys lock poisoned")
            .insert(key, handle.to_string());
    }
}

#[derive(Clone)]
pub(crate) enum SessionNotes {
    Enabled(Arc<SessionMemory>),
    Disabled,
}

impl SessionNotes {
    fn new(notes: Option<Arc<SessionMemory>>) -> Self {
        notes.map_or(Self::Disabled, Self::Enabled)
    }

    #[cfg(test)]
    pub(crate) fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }

    pub(crate) fn entry(&self, id: &MemoryId) -> Option<MemoryEntry> {
        match self {
            Self::Enabled(notes) => notes.entry(id),
            Self::Disabled => None,
        }
    }

    pub(crate) fn persisted_snapshot(&self) -> EpisodicMemorySnapshot {
        match self {
            Self::Enabled(notes) => notes.persisted_snapshot(),
            Self::Disabled => EpisodicMemorySnapshot {
                entries: Vec::new(),
            },
        }
    }

    pub(crate) fn snapshot(&self) -> EpisodicMemorySnapshot {
        match self {
            Self::Enabled(notes) => notes.snapshot(),
            Self::Disabled => EpisodicMemorySnapshot {
                entries: Vec::new(),
            },
        }
    }

    pub(crate) fn replace_from_snapshot(&self, snapshot: EpisodicMemorySnapshot) {
        if let Self::Enabled(notes) = self {
            notes.replace_from_snapshot(snapshot);
        }
    }

    pub(crate) fn store(&self, entry: MemoryEntry) -> Result<MemoryId> {
        match self {
            Self::Enabled(notes) => notes.store(entry),
            Self::Disabled => Err(anyhow!(
                "session memory is unavailable in coordination-only mode"
            )),
        }
    }

    pub(crate) fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        match self {
            Self::Enabled(notes) => notes.recall(query),
            Self::Disabled => Ok(Vec::new()),
        }
    }
}

fn session_task_is_coordination_focus(task: &SessionTaskState) -> bool {
    task.coordination_task_id.is_some() || task.id.0.starts_with("coord-task:")
}

fn session_task_is_plan_node_focus(task: &SessionTaskState) -> bool {
    task.id.0.starts_with("plan-node:")
}

fn session_task_matches_work(task: &SessionTaskState, work: &SessionWorkState) -> bool {
    if session_task_is_plan_node_focus(task) {
        return true;
    }

    let task_coordination_id = task.coordination_task_id.as_deref().or_else(|| {
        task.id
            .0
            .starts_with("coord-task:")
            .then_some(task.id.0.as_str())
    });
    let work_coordination_id = work.coordination_task_id.as_deref();

    matches!(
        (task_coordination_id, work_coordination_id),
        (Some(task_id), Some(work_id)) if task_id == work_id
    )
}

fn session_handle_key(target: &SessionHandleTarget) -> String {
    if let (Some(file_path), Some(start_line), Some(end_line)) = (
        target.file_path.as_ref(),
        target.start_line,
        target.end_line,
    ) {
        return format!(
            "fragment:{file_path}:{start_line}:{}:{end_line}:{}:{}",
            target.start_column.unwrap_or(1),
            target.end_column.unwrap_or(1),
            target.query.as_deref().unwrap_or_default()
        );
    }
    if let Some(lineage_id) = target.lineage_id.as_ref().filter(|value| !value.is_empty()) {
        format!("lineage:{lineage_id}")
    } else {
        format!(
            "node:{}:{}:{}",
            target.id.crate_name, target.id.path, target.id.kind
        )
    }
}

pub(crate) struct SessionInferenceStore {
    persisted: Option<Arc<InferenceStore>>,
    session_only: Option<InferenceStore>,
}

impl SessionInferenceStore {
    fn new(persisted: Option<Arc<InferenceStore>>) -> Self {
        let enabled = persisted.is_some();
        Self {
            persisted,
            session_only: enabled.then(InferenceStore::new),
        }
    }

    #[cfg(test)]
    pub(crate) fn is_enabled(&self) -> bool {
        self.persisted.is_some()
    }

    pub(crate) fn record(&self, id: &prism_agent::EdgeId) -> Option<InferredEdgeRecord> {
        self.session_only
            .as_ref()
            .and_then(|store| store.record(id))
            .or_else(|| self.persisted.as_ref().and_then(|store| store.record(id)))
    }

    pub(crate) fn edges_from(
        &self,
        source: &prism_ir::NodeId,
        kind: Option<prism_ir::EdgeKind>,
    ) -> Vec<InferredEdgeRecord> {
        let mut records = self
            .persisted
            .as_ref()
            .map(|store| store.edges_from(source, kind))
            .unwrap_or_default();
        if let Some(store) = self.session_only.as_ref() {
            records.extend(store.edges_from(source, kind));
        }
        records
    }

    pub(crate) fn edges_to(
        &self,
        target: &prism_ir::NodeId,
        kind: Option<prism_ir::EdgeKind>,
    ) -> Vec<InferredEdgeRecord> {
        let mut records = self
            .persisted
            .as_ref()
            .map(|store| store.edges_to(target, kind))
            .unwrap_or_default();
        if let Some(store) = self.session_only.as_ref() {
            records.extend(store.edges_to(target, kind));
        }
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
            InferredEdgeScope::SessionOnly => self
                .session_only
                .as_ref()
                .map(|store| store.store_edge(edge, scope, task, evidence))
                .unwrap_or_else(|| {
                    prism_agent::EdgeId(prism_ir::new_prefixed_id("edge").to_string())
                }),
            _ => self
                .persisted
                .as_ref()
                .map(|store| store.store_edge(edge, scope, task, evidence))
                .unwrap_or_else(|| {
                    prism_agent::EdgeId(prism_ir::new_prefixed_id("edge").to_string())
                }),
        }
    }
}
