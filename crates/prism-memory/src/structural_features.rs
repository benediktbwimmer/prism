use std::collections::HashSet;

use serde_json::Value;

use crate::text::{embedding_text, token_set};
use crate::types::MemoryEntry;

#[derive(Debug, Clone)]
pub(crate) struct StructuralFeatures {
    pub tags: HashSet<String>,
    pub terms: HashSet<String>,
    pub rule_kinds: HashSet<String>,
    pub promoted_rule: bool,
    pub evidence_strength: f32,
}

pub(crate) fn derive_structural_features(entry: &MemoryEntry) -> StructuralFeatures {
    let text = embedding_text(&entry.content, &entry.metadata);
    let mut tags = HashSet::new();
    collect_tags_from_text(&text, &mut tags);
    collect_tags_from_metadata(&entry.metadata, &mut tags);
    let terms = token_set(&text);
    StructuralFeatures {
        tags,
        rule_kinds: derive_rule_kinds(&entry.metadata, &terms),
        terms,
        promoted_rule: metadata_promoted_rule(&entry.metadata),
        evidence_strength: metadata_evidence_strength(&entry.metadata),
    }
}

pub(crate) fn derive_query_features(text: &str) -> StructuralFeatures {
    let mut tags = HashSet::new();
    collect_tags_from_text(text, &mut tags);
    let terms = token_set(text);
    let rule_kinds = derive_query_rule_kinds(&tags, &terms);
    StructuralFeatures {
        tags,
        rule_kinds,
        terms,
        promoted_rule: false,
        evidence_strength: 0.0,
    }
}

fn collect_tags_from_text(text: &str, tags: &mut HashSet<String>) {
    let lower = text.to_ascii_lowercase();
    for (needle, tag) in [
        ("invariant", "invariant"),
        ("policy", "policy"),
        ("must", "policy"),
        ("require", "policy"),
        ("migration", "migration"),
        ("validate", "validation"),
        ("test", "validation"),
        ("review", "review"),
        ("together", "cochange"),
        ("co-change", "cochange"),
        ("hot path", "performance"),
        ("perf", "performance"),
    ] {
        if lower.contains(needle) {
            tags.insert(tag.to_string());
        }
    }
}

fn derive_rule_kinds(metadata: &Value, terms: &HashSet<String>) -> HashSet<String> {
    let mut rule_kinds = HashSet::new();
    if let Some(kind) = metadata
        .get("structuralRule")
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
    {
        rule_kinds.insert(kind.to_string());
    }
    if let Some(category) = metadata.get("category").and_then(Value::as_str) {
        rule_kinds.insert(category.to_string());
    }
    if terms.contains("owner") || terms.contains("ownership") || terms.contains("owns") {
        rule_kinds.insert("ownership_rule".to_string());
    }
    rule_kinds
}

fn derive_query_rule_kinds(tags: &HashSet<String>, terms: &HashSet<String>) -> HashSet<String> {
    let mut rule_kinds = HashSet::new();
    if tags.contains("validation") {
        rule_kinds.insert("validation_rule".to_string());
        rule_kinds.insert("validation_recipe".to_string());
    }
    if tags.contains("migration") {
        rule_kinds.insert("migration_rule".to_string());
    }
    if tags.contains("cochange") || tags.contains("review") {
        rule_kinds.insert("co_change_rule".to_string());
    }
    if tags.contains("policy") || tags.contains("invariant") {
        rule_kinds.insert("policy_rule".to_string());
    }
    if terms.contains("owner") || terms.contains("ownership") || terms.contains("owns") {
        rule_kinds.insert("ownership_rule".to_string());
    }
    rule_kinds
}

fn metadata_promoted_rule(metadata: &Value) -> bool {
    metadata
        .get("structuralRule")
        .and_then(|value| value.get("promoted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || metadata
            .get("provenance")
            .and_then(|value| value.get("origin"))
            .and_then(Value::as_str)
            .is_some_and(|origin| origin.eq_ignore_ascii_case("curator"))
}

fn metadata_evidence_strength(metadata: &Value) -> f32 {
    let signal_count = metadata
        .get("structuralRule")
        .and_then(|value| value.get("signalCount"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            let event_count = metadata
                .get("evidence")
                .and_then(|value| value.get("eventIds"))
                .and_then(Value::as_array)
                .map(|values| values.len())
                .unwrap_or(0);
            let validation_count = metadata
                .get("evidence")
                .and_then(|value| value.get("validationChecks"))
                .and_then(Value::as_array)
                .map(|values| values.len())
                .unwrap_or(0);
            let co_change_count = metadata
                .get("evidence")
                .and_then(|value| value.get("coChangeLineages"))
                .and_then(Value::as_array)
                .map(|values| values.len())
                .unwrap_or(0);
            (event_count + validation_count + co_change_count) as u64
        });
    (signal_count as f32 / 4.0).clamp(0.0, 1.0)
}

fn collect_tags_from_metadata(value: &Value, tags: &mut HashSet<String>) {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
        Value::String(value) => collect_tags_from_text(value, tags),
        Value::Array(values) => {
            for value in values {
                collect_tags_from_metadata(value, tags);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                collect_tags_from_text(key, tags);
                collect_tags_from_metadata(value, tags);
            }
        }
    }
}
