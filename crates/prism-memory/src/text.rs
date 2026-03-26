use std::collections::HashSet;

use serde_json::Value;

const EMBEDDING_DIM: usize = 64;

pub(crate) fn embedding_text(content: &str, metadata: &Value) -> String {
    let mut text = content.to_string();
    flatten_json(metadata, &mut text);
    text
}

pub(crate) fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(normalize_token)
        .collect()
}

pub(crate) fn token_set(text: &str) -> HashSet<String> {
    tokenize(text).into_iter().collect()
}

pub(crate) fn hashed_embedding(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0; EMBEDDING_DIM];
    for token in tokenize(text) {
        let hash = stable_hash(&token);
        let index = (hash as usize) % EMBEDDING_DIM;
        let sign = if ((hash >> 8) & 1) == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    normalize(&mut vector);
    vector
}

pub(crate) fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (l, r) in left.iter().zip(right.iter()) {
        dot += l * r;
        left_norm += l * l;
        right_norm += r * r;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        (dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(0.0, 1.0)
    }
}

pub(crate) fn token_overlap(left: &HashSet<String>, right: &HashSet<String>) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let shared = left.intersection(right).count() as f32;
    let union = left.union(right).count() as f32;
    (shared / union).clamp(0.0, 1.0)
}

pub(crate) fn substring_score(content: &str, query: &str) -> f32 {
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return 1.0;
    }
    content
        .to_ascii_lowercase()
        .contains(&normalized_query)
        .then_some(1.0)
        .unwrap_or(0.0)
}

fn flatten_json(value: &Value, output: &mut String) {
    match value {
        Value::Null => {}
        Value::Bool(value) => {
            output.push(' ');
            output.push_str(if *value { "true" } else { "false" });
        }
        Value::Number(value) => {
            output.push(' ');
            output.push_str(&value.to_string());
        }
        Value::String(value) => {
            output.push(' ');
            output.push_str(value);
        }
        Value::Array(values) => {
            for value in values {
                flatten_json(value, output);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                output.push(' ');
                output.push_str(key);
                flatten_json(value, output);
            }
        }
    }
}

fn normalize_token(token: &str) -> Option<String> {
    let lower = token.trim().to_ascii_lowercase();
    if lower.len() < 2 {
        return None;
    }
    let mut stem = lower;
    for suffix in ["ing", "ers", "ies", "ied", "ed", "es", "s"] {
        if stem.len() > suffix.len() + 2 && stem.ends_with(suffix) {
            stem.truncate(stem.len() - suffix.len());
            break;
        }
    }
    Some(stem)
}

fn stable_hash(value: &str) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 {
        return;
    }
    for value in vector {
        *value /= norm;
    }
}
