use std::collections::HashSet;

use serde_json::Value;

use crate::text::{embedding_text, token_set};
use crate::types::MemoryEntry;

#[derive(Debug, Clone)]
pub(crate) struct StructuralFeatures {
    pub tags: HashSet<String>,
    pub terms: HashSet<String>,
}

pub(crate) fn derive_structural_features(entry: &MemoryEntry) -> StructuralFeatures {
    let text = embedding_text(&entry.content, &entry.metadata);
    let mut tags = HashSet::new();
    collect_tags_from_text(&text, &mut tags);
    collect_tags_from_metadata(&entry.metadata, &mut tags);
    StructuralFeatures {
        tags,
        terms: token_set(&text),
    }
}

pub(crate) fn derive_query_features(text: &str) -> StructuralFeatures {
    let mut tags = HashSet::new();
    collect_tags_from_text(text, &mut tags);
    StructuralFeatures {
        tags,
        terms: token_set(text),
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
