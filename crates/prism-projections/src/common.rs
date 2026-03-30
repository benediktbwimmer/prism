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
            OutcomeEvidence::Test { name, .. } => Some(normalize_validation_label(name, "test:")),
            OutcomeEvidence::Build { target, .. } => {
                Some(normalize_validation_label(target, "build:"))
            }
            OutcomeEvidence::Command { argv, passed } if *passed => command_validation_label(argv),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut deduped = HashSet::new();
    labels.retain(|label| deduped.insert(label.clone()));
    labels
}

fn normalize_validation_label(value: &str, default_prefix: &str) -> String {
    let value = value.trim();
    if value.starts_with("test:")
        || value.starts_with("build:")
        || value.starts_with("validation:")
        || value.starts_with("command:")
    {
        value.to_string()
    } else {
        format!("{default_prefix}{value}")
    }
}

fn command_validation_label(argv: &[String]) -> Option<String> {
    match argv {
        [tool, subcommand, ..] if tool == "cargo" && subcommand == "test" => {
            Some(format!("test:{}", argv.join(" ")))
        }
        [tool, subcommand, ..] if tool == "cargo" && subcommand == "build" => {
            Some(format!("build:{}", argv.join(" ")))
        }
        _ => None,
    }
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
