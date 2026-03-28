use std::collections::HashSet;

use serde_json::Value;

const EMBEDDING_DIM: usize = 64;
const TOKEN_ALIAS_GROUPS: &[&[&str]] = &[
    &[
        "auth",
        "authenticate",
        "authentication",
        "authorization",
        "authorize",
        "credential",
        "credentials",
        "login",
        "signin",
        "signon",
    ],
    &["owner", "ownership", "owns", "maintainer", "maintainers"],
    &[
        "validate",
        "validation",
        "verify",
        "verification",
        "check",
        "checks",
    ],
    &["config", "configuration", "setting", "settings"],
];

pub(crate) fn embedding_text(content: &str, metadata: &Value) -> String {
    let mut text = content.to_string();
    flatten_json(metadata, &mut text);
    text
}

pub(crate) fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .flat_map(split_identifier_like)
        .filter_map(|token| normalize_token(&token))
        .collect()
}

pub(crate) fn token_set(text: &str) -> HashSet<String> {
    tokenize(text).into_iter().collect()
}

pub(crate) fn expanded_token_set(text: &str) -> HashSet<String> {
    expand_token_set(&token_set(text))
}

pub(crate) fn hashed_embedding(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0; EMBEDDING_DIM];
    for token in expanded_token_set(text) {
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

fn split_identifier_like(token: &str) -> Vec<String> {
    if token.is_empty() {
        return Vec::new();
    }

    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous_is_lower = false;
    for ch in token.chars() {
        let boundary = !current.is_empty() && previous_is_lower && ch.is_ascii_uppercase();
        if boundary {
            parts.push(current.clone());
            current.clear();
        }
        current.push(ch);
        previous_is_lower = ch.is_ascii_lowercase();
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn expand_token_set(tokens: &HashSet<String>) -> HashSet<String> {
    let mut expanded = tokens.clone();
    for token in tokens {
        for alias_group in TOKEN_ALIAS_GROUPS {
            if alias_group.iter().any(|alias| token == alias) {
                expanded.extend(alias_group.iter().map(|alias| alias.to_string()));
            }
        }
        if token.starts_with("auth") {
            expanded.extend(
                ["auth", "authenticate", "authentication", "login", "signin"]
                    .into_iter()
                    .map(str::to_string),
            );
        }
        if token.starts_with("own") {
            expanded.extend(
                ["owner", "ownership", "owns"]
                    .into_iter()
                    .map(str::to_string),
            );
        }
    }
    expanded
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
