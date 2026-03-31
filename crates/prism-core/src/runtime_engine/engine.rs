use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use super::context::WorkspaceRuntimeContext;
use super::generation::{
    RuntimeDomain, RuntimeDomainState, WorkspaceGenerationId, WorkspacePublishedGeneration,
    WorkspaceRuntimeDeltaBatch, WorkspaceRuntimeDeltaSequence,
};

#[derive(Debug, Clone)]
pub struct WorkspaceRuntimeEngine {
    context: WorkspaceRuntimeContext,
    published_generation: WorkspacePublishedGeneration,
    next_generation_id: WorkspaceGenerationId,
    next_delta_sequence: WorkspaceRuntimeDeltaSequence,
    recent_deltas: VecDeque<WorkspaceRuntimeDeltaBatch>,
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

    pub fn record_commit(
        &mut self,
        changed_paths: Vec<PathBuf>,
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
            domain_states,
        };
        if self.recent_deltas.len() == Self::RECENT_DELTA_LIMIT {
            self.recent_deltas.pop_front();
        }
        self.recent_deltas.push_back(batch.clone());
        batch
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::runtime_engine::{
        RuntimeFreshnessState, RuntimeMaterializationDepth, WorkspaceRuntimeQueueClass,
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

        let first = engine.record_commit(vec![PathBuf::from("src/lib.rs")], domain_states.clone());
        assert_eq!(first.sequence.0, 1);
        assert_eq!(first.parent_generation.0, 0);
        assert_eq!(first.committed_generation.0, 1);
        assert_eq!(engine.published_generation().id.0, 1);
        assert_eq!(engine.published_generation().domain_states, domain_states);
        assert_eq!(engine.context(), &context);

        let second = engine.record_commit(vec![PathBuf::from("src/main.rs")], domain_states);
        assert_eq!(second.sequence.0, 2);
        assert_eq!(second.parent_generation.0, 1);
        assert_eq!(second.committed_generation.0, 2);
        assert_eq!(engine.published_generation().id.0, 2);
        assert_eq!(engine.published_generation().parent_id.unwrap().0, 1);
        assert_eq!(engine.published_generation().committed_delta.unwrap().0, 2);
        assert_eq!(engine.recent_deltas().len(), 2);
    }
}
