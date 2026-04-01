use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

use super::context::WorkspaceRuntimeContext;
use super::generation::{
    RuntimeDomain, RuntimeDomainState, WorkspaceFileDelta, WorkspaceGenerationId,
    WorkspacePublishedGeneration, WorkspaceRuntimeDeltaBatch, WorkspaceRuntimeDeltaSequence,
};
use super::queue::{
    WorkspaceRuntimeCoalescingKey, WorkspaceRuntimeCommand, WorkspaceRuntimeCommandKind,
    WorkspaceRuntimeQueueDepth, WorkspaceRuntimeQueueSnapshot,
};

#[derive(Debug, Clone)]
pub struct WorkspaceRuntimeEngine {
    context: WorkspaceRuntimeContext,
    published_generation: WorkspacePublishedGeneration,
    next_generation_id: WorkspaceGenerationId,
    next_delta_sequence: WorkspaceRuntimeDeltaSequence,
    recent_deltas: VecDeque<WorkspaceRuntimeDeltaBatch>,
    active_command: Option<WorkspaceRuntimeCommand>,
    queued_commands: VecDeque<WorkspaceRuntimeCommand>,
}

impl WorkspaceRuntimeEngine {
    const RECENT_DELTA_LIMIT: usize = 32;

    pub fn new(context: WorkspaceRuntimeContext) -> Self {
        Self {
            published_generation: WorkspacePublishedGeneration::initial(context.clone()),
            context,
            next_generation_id: WorkspaceGenerationId(1),
            next_delta_sequence: WorkspaceRuntimeDeltaSequence(1),
            recent_deltas: VecDeque::with_capacity(Self::RECENT_DELTA_LIMIT),
            active_command: None,
            queued_commands: VecDeque::new(),
        }
    }

    pub fn context(&self) -> &WorkspaceRuntimeContext {
        &self.context
    }

    pub fn published_generation(&self) -> &WorkspacePublishedGeneration {
        &self.published_generation
    }

    pub fn published_generation_snapshot(&self) -> WorkspacePublishedGeneration {
        self.published_generation.clone()
    }

    pub fn recent_deltas(&self) -> Vec<WorkspaceRuntimeDeltaBatch> {
        self.recent_deltas.iter().cloned().collect()
    }

    pub fn queue_snapshot(&self) -> WorkspaceRuntimeQueueSnapshot {
        let mut seen = BTreeSet::new();
        let mut queued = self
            .queued_commands
            .iter()
            .filter_map(|command| {
                if seen.insert(command.queue_class) {
                    Some(WorkspaceRuntimeQueueDepth {
                        queue_class: command.queue_class,
                        depth: self
                            .queued_commands
                            .iter()
                            .filter(|queued| queued.queue_class == command.queue_class)
                            .count(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        queued.sort_by_key(|depth| depth.queue_class.priority_rank());
        WorkspaceRuntimeQueueSnapshot {
            active: self.active_command.clone(),
            total_depth: self.queued_commands.len(),
            queued,
        }
    }

    pub fn enqueue_command(&mut self, command: WorkspaceRuntimeCommand) -> bool {
        if !matches!(command.coalescing_key, WorkspaceRuntimeCoalescingKey::None) {
            if let Some(existing) = self
                .queued_commands
                .iter_mut()
                .find(|existing| existing.coalescing_key == command.coalescing_key)
            {
                merge_command(existing, command);
                return false;
            }
        }
        self.queued_commands.push_back(command);
        true
    }

    pub fn start_next_command(&mut self) -> Option<WorkspaceRuntimeCommand> {
        if self.active_command.is_some() {
            return None;
        }
        let next_index = self
            .queued_commands
            .iter()
            .enumerate()
            .min_by_key(|(_, command)| command.queue_class.priority_rank())
            .map(|(index, _)| index)?;
        let command = self.queued_commands.remove(next_index)?;
        self.active_command = Some(command.clone());
        Some(command)
    }

    pub fn begin_ad_hoc_command(&mut self, command: WorkspaceRuntimeCommand) -> bool {
        if self.active_command.is_some() {
            return false;
        }
        self.active_command = Some(command);
        true
    }

    pub fn finish_active_command(&mut self) {
        self.complete_active_command(std::iter::empty());
    }

    pub fn complete_active_command<I>(&mut self, follow_up_commands: I)
    where
        I: IntoIterator<Item = WorkspaceRuntimeCommand>,
    {
        self.active_command = None;
        for command in follow_up_commands {
            let _ = self.enqueue_command(command);
        }
    }

    pub fn retry_active_command(&mut self) {
        if let Some(command) = self.active_command.take() {
            let _ = self.enqueue_command(command);
        }
    }

    pub fn has_pending_command_kind(&self, kind: WorkspaceRuntimeCommandKind) -> bool {
        self.active_command
            .as_ref()
            .is_some_and(|command| command.kind == kind)
            || self
                .queued_commands
                .iter()
                .any(|command| command.kind == kind)
    }

    pub fn record_commit(
        &mut self,
        changed_paths: Vec<PathBuf>,
        file_deltas: Vec<WorkspaceFileDelta>,
        domain_states: BTreeMap<RuntimeDomain, RuntimeDomainState>,
    ) -> WorkspaceRuntimeDeltaBatch {
        let parent_generation = self.published_generation.id;
        let committed_generation = self.next_generation_id;
        let delta_sequence = self.next_delta_sequence;
        self.next_generation_id = WorkspaceGenerationId(self.next_generation_id.0 + 1);
        self.next_delta_sequence = WorkspaceRuntimeDeltaSequence(self.next_delta_sequence.0 + 1);
        self.published_generation = WorkspacePublishedGeneration {
            context: self.context.clone(),
            id: committed_generation,
            parent_id: Some(parent_generation),
            committed_delta: Some(delta_sequence),
            domain_states: domain_states.clone(),
        };
        let batch = WorkspaceRuntimeDeltaBatch {
            sequence: delta_sequence,
            parent_generation,
            committed_generation,
            changed_paths,
            file_deltas,
            domain_states,
        };
        if self.recent_deltas.len() == Self::RECENT_DELTA_LIMIT {
            self.recent_deltas.pop_front();
        }
        self.recent_deltas.push_back(batch.clone());
        batch
    }
}

fn merge_command(existing: &mut WorkspaceRuntimeCommand, incoming: WorkspaceRuntimeCommand) {
    existing.kind = incoming.kind;
    existing.queue_class = incoming.queue_class;
    existing.coalescing_key = incoming.coalescing_key;
    if incoming.paths.is_empty() {
        return;
    }
    let mut seen = existing.paths.iter().cloned().collect::<BTreeSet<_>>();
    for path in incoming.paths {
        if seen.insert(path.clone()) {
            existing.paths.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::runtime_engine::{
        RuntimeFreshnessState, RuntimeMaterializationDepth, WorkspaceFileDelta,
        WorkspaceRuntimeCoalescingKey, WorkspaceRuntimeCommand, WorkspaceRuntimeCommandKind,
        WorkspaceRuntimeQueueClass,
    };

    #[test]
    fn runtime_queue_classes_prioritize_interactive_work_first() {
        assert!(
            WorkspaceRuntimeQueueClass::InteractiveMutation.priority_rank()
                < WorkspaceRuntimeQueueClass::Settle.priority_rank()
        );
        assert!(
            WorkspaceRuntimeQueueClass::Settle.priority_rank()
                < WorkspaceRuntimeQueueClass::CheckpointMaterialization.priority_rank()
        );
    }

    #[test]
    fn runtime_engine_records_monotonic_generation_and_delta_sequences() {
        let root = std::env::current_dir().expect("cwd");
        let context = WorkspaceRuntimeContext::from_root(&root);
        let mut engine = WorkspaceRuntimeEngine::new(context.clone());
        let mut domain_states = BTreeMap::new();
        domain_states.insert(
            RuntimeDomain::FileFacts,
            RuntimeDomainState::new(
                RuntimeFreshnessState::Current,
                RuntimeMaterializationDepth::Deep,
            ),
        );
        domain_states.insert(
            RuntimeDomain::CrossFileEdges,
            RuntimeDomainState::new(
                RuntimeFreshnessState::Pending,
                RuntimeMaterializationDepth::Medium,
            ),
        );

        let first = engine.record_commit(
            vec![PathBuf::from("src/lib.rs")],
            vec![WorkspaceFileDelta {
                previous_path: None,
                current_path: Some(PathBuf::from("src/lib.rs")),
                file_count: 1,
                added_nodes: 1,
                removed_nodes: 0,
                updated_nodes: 0,
                edge_added: 0,
                edge_removed: 0,
            }],
            domain_states.clone(),
        );
        assert_eq!(first.sequence.0, 1);
        assert_eq!(first.parent_generation.0, 0);
        assert_eq!(first.committed_generation.0, 1);
        assert_eq!(first.file_deltas.len(), 1);
        assert_eq!(engine.published_generation().id.0, 1);
        assert_eq!(engine.published_generation().domain_states, domain_states);
        assert_eq!(engine.context(), &context);

        let second = engine.record_commit(
            vec![PathBuf::from("src/main.rs")],
            vec![WorkspaceFileDelta {
                previous_path: Some(PathBuf::from("src/lib.rs")),
                current_path: Some(PathBuf::from("src/main.rs")),
                file_count: 1,
                added_nodes: 0,
                removed_nodes: 0,
                updated_nodes: 1,
                edge_added: 1,
                edge_removed: 1,
            }],
            domain_states,
        );
        assert_eq!(second.sequence.0, 2);
        assert_eq!(second.parent_generation.0, 1);
        assert_eq!(second.committed_generation.0, 2);
        assert_eq!(engine.published_generation().id.0, 2);
        assert_eq!(engine.published_generation().parent_id.unwrap().0, 1);
        assert_eq!(engine.published_generation().committed_delta.unwrap().0, 2);
        assert_eq!(engine.recent_deltas().len(), 2);
    }

    #[test]
    fn runtime_engine_coalesces_worktree_refresh_commands() {
        let root = std::env::current_dir().expect("cwd");
        let context = WorkspaceRuntimeContext::from_root(&root);
        let mut engine = WorkspaceRuntimeEngine::new(context);
        assert!(engine.enqueue_command(WorkspaceRuntimeCommand::with_paths(
            WorkspaceRuntimeCommandKind::PreparePaths,
            WorkspaceRuntimeQueueClass::FastPrepare,
            WorkspaceRuntimeCoalescingKey::WorktreeContext,
            vec![PathBuf::from("src/lib.rs")],
        )));
        assert!(!engine.enqueue_command(WorkspaceRuntimeCommand::with_paths(
            WorkspaceRuntimeCommandKind::PreparePaths,
            WorkspaceRuntimeQueueClass::FastPrepare,
            WorkspaceRuntimeCoalescingKey::WorktreeContext,
            vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")],
        )));
        let snapshot = engine.queue_snapshot();
        assert_eq!(snapshot.total_depth, 1);
        assert_eq!(snapshot.queued[0].depth, 1);
        let active = engine
            .start_next_command()
            .expect("queued command should start");
        assert_eq!(active.kind, WorkspaceRuntimeCommandKind::PreparePaths);
        assert_eq!(
            active.paths,
            vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")]
        );
        assert!(engine.has_pending_command_kind(WorkspaceRuntimeCommandKind::PreparePaths));
        engine.finish_active_command();
        assert!(!engine.has_pending_command_kind(WorkspaceRuntimeCommandKind::PreparePaths));
    }

    #[test]
    fn runtime_engine_completion_enqueues_follow_up_commands() {
        let root = std::env::current_dir().expect("cwd");
        let context = WorkspaceRuntimeContext::from_root(&root);
        let mut engine = WorkspaceRuntimeEngine::new(context);
        assert!(engine.enqueue_command(WorkspaceRuntimeCommand::with_paths(
            WorkspaceRuntimeCommandKind::PreparePaths,
            WorkspaceRuntimeQueueClass::FastPrepare,
            WorkspaceRuntimeCoalescingKey::WorktreeContext,
            vec![PathBuf::from("src/lib.rs")],
        )));
        let _active = engine
            .start_next_command()
            .expect("queued command should start");
        engine.complete_active_command([WorkspaceRuntimeCommand::new(
            WorkspaceRuntimeCommandKind::SettleDomain(RuntimeDomain::MemoryReanchor),
            WorkspaceRuntimeQueueClass::Settle,
            WorkspaceRuntimeCoalescingKey::Domain(RuntimeDomain::MemoryReanchor),
        )]);
        let snapshot = engine.queue_snapshot();
        assert_eq!(snapshot.total_depth, 1);
        assert_eq!(
            snapshot.queued[0].queue_class,
            WorkspaceRuntimeQueueClass::Settle
        );
    }

    #[test]
    fn runtime_engine_retries_active_command_after_transient_failure() {
        let root = std::env::current_dir().expect("cwd");
        let context = WorkspaceRuntimeContext::from_root(&root);
        let mut engine = WorkspaceRuntimeEngine::new(context);
        assert!(engine.begin_ad_hoc_command(WorkspaceRuntimeCommand::new(
            WorkspaceRuntimeCommandKind::MaterializeCheckpoint,
            WorkspaceRuntimeQueueClass::CheckpointMaterialization,
            WorkspaceRuntimeCoalescingKey::WorktreeContext,
        )));
        engine.retry_active_command();
        let snapshot = engine.queue_snapshot();
        assert!(snapshot.active.is_none());
        assert_eq!(snapshot.total_depth, 1);
        assert_eq!(
            snapshot.queued[0].queue_class,
            WorkspaceRuntimeQueueClass::CheckpointMaterialization
        );
        assert!(engine.has_pending_command_kind(WorkspaceRuntimeCommandKind::MaterializeCheckpoint));
    }
}
