use std::collections::HashSet;

use prism_ir::{NodeId, NodeKind};
use prism_memory::{OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeResult};

pub fn event_weight(event: &OutcomeEvent) -> f32 {
    let kind_weight = match event.kind {
        OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved => 2.5,
        OutcomeKind::FixValidated => 2.0,
        OutcomeKind::BuildRan | OutcomeKind::TestRan => 1.25,
        _ => 1.0,
    };
    let result_weight = match event.result {
        OutcomeResult::Failure => 2.0,
        OutcomeResult::Success => 1.0,
        OutcomeResult::Partial => 0.75,
        OutcomeResult::Unknown => 0.5,
    };
    kind_weight * result_weight
}

pub fn validation_labels(evidence: &[OutcomeEvidence]) -> Vec<String> {
    let mut labels = evidence
        .iter()
        .filter_map(|evidence| match evidence {
            OutcomeEvidence::Test { name, .. } => Some(format!("test:{name}")),
            OutcomeEvidence::Build { target, .. } => Some(format!("build:{target}")),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut deduped = HashSet::new();
    labels.retain(|label| deduped.insert(label.clone()));
    labels
}

pub fn is_intent_source_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Document
            | NodeKind::MarkdownHeading
            | NodeKind::JsonKey
            | NodeKind::TomlKey
            | NodeKind::YamlKey
    )
}

pub fn push_unique(values: &mut Vec<NodeId>, value: NodeId) {
    if !values.contains(&value) {
        values.push(value);
    }
}
