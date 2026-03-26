use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, FileId, Language, Node, NodeId, SymbolFingerprint};
use serde::{Deserialize, Serialize};

pub use prism_ir::{UnresolvedCall, UnresolvedImpl, UnresolvedImport, UnresolvedIntent};

#[derive(Debug, Clone)]
pub struct ParseInput<'a> {
    pub package_name: &'a str,
    pub crate_name: &'a str,
    pub package_root: &'a Path,
    pub path: &'a Path,
    pub file_id: FileId,
    pub source: &'a str,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub fingerprints: HashMap<NodeId, SymbolFingerprint>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
    pub unresolved_intents: Vec<UnresolvedIntent>,
}

impl ParseResult {
    pub fn merge(&mut self, other: Self) {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
        self.fingerprints.extend(other.fingerprints);
        self.unresolved_calls.extend(other.unresolved_calls);
        self.unresolved_imports.extend(other.unresolved_imports);
        self.unresolved_impls.extend(other.unresolved_impls);
        self.unresolved_intents.extend(other.unresolved_intents);
    }

    pub fn record_fingerprint(&mut self, id: &NodeId, fingerprint: SymbolFingerprint) {
        self.fingerprints.insert(id.clone(), fingerprint);
    }
}

pub type NodeFingerprint = SymbolFingerprint;

#[derive(Debug, Clone)]
pub struct SymbolTarget<'a> {
    pub kind: EdgeKind,
    pub source: &'a NodeId,
    pub module_path: &'a str,
    pub name: &'a str,
    pub target_path: &'a str,
}

pub trait LanguageAdapter {
    fn language(&self) -> Language;
    fn supports_path(&self, path: &Path) -> bool;
    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult>;
}

pub fn relative_package_file(input: &ParseInput<'_>) -> PathBuf {
    input
        .path
        .strip_prefix(input.package_root)
        .unwrap_or(input.path)
        .to_path_buf()
}

pub fn document_path(input: &ParseInput<'_>) -> String {
    let relative = relative_package_file(input);
    let mut parts = vec![input.crate_name.to_owned(), "document".to_owned()];
    for component in relative.components() {
        let value = component.as_os_str().to_string_lossy();
        parts.push(sanitize_path_segment(&value));
    }
    parts.join("::")
}

pub fn document_name(input: &ParseInput<'_>) -> String {
    relative_package_file(input).display().to_string()
}

pub fn fingerprint_from_parts<I, S>(parts: I) -> NodeFingerprint
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let joined = parts
        .into_iter()
        .map(|part| part.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join("|");
    NodeFingerprint::new(stable_hash(&joined))
}

pub fn normalized_shape_hash(value: &str) -> String {
    format!("{:016x}", stable_hash(&normalize_shape(value)))
}

pub fn intent_kind_for_context(context: &str, default_kind: EdgeKind) -> EdgeKind {
    let lower = context.to_ascii_lowercase();
    if contains_any(
        &lower,
        &[
            "test",
            "tests",
            "validate",
            "validation",
            "assert",
            "check",
            "coverage",
        ],
    ) {
        EdgeKind::Validates
    } else if contains_any(
        &lower,
        &[
            "spec",
            "specification",
            "requirement",
            "requirements",
            "contract",
            "behavior",
            "invariant",
            "guarantee",
            "design",
            "adr",
        ],
    ) {
        EdgeKind::Specifies
    } else {
        default_kind
    }
}

pub fn extract_intent_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for candidate in extract_backticked_targets(text)
        .into_iter()
        .chain(extract_plain_targets(text))
    {
        let normalized = normalize_intent_target(&candidate);
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            targets.push(normalized);
        }
    }

    targets
}

fn sanitize_path_segment(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_underscore = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_underscore = false;
        } else if !previous_underscore {
            normalized.push('_');
            previous_underscore = true;
        }
    }

    let normalized = normalized.trim_matches('_').to_owned();
    if normalized.is_empty() {
        "file".to_owned()
    } else {
        normalized
    }
}

fn extract_backticked_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut inside = false;
    let mut current = String::new();

    for ch in text.chars() {
        if ch == '`' {
            if inside {
                if !current.trim().is_empty() {
                    targets.push(current.trim().to_owned());
                }
                current.clear();
            }
            inside = !inside;
            continue;
        }
        if inside {
            current.push(ch);
        }
    }

    targets
}

fn extract_plain_targets(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|token| {
            let normalized = normalize_intent_target(token);
            is_plain_symbol_candidate(&normalized).then_some(normalized)
        })
        .collect()
}

fn normalize_intent_target(value: &str) -> String {
    value
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | ':' | ';'
            )
        })
        .trim_end_matches("()")
        .trim()
        .to_owned()
}

fn is_plain_symbol_candidate(value: &str) -> bool {
    if value.len() < 3
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == ':')
    {
        return false;
    }

    if value.contains("::") || value.contains('_') {
        return true;
    }

    let has_upper = value.chars().any(|ch| ch.is_ascii_uppercase());
    let has_lower = value.chars().any(|ch| ch.is_ascii_lowercase());
    has_upper && has_lower
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn normalize_shape(value: &str) -> String {
    let mut normalized = String::new();
    let mut ident = false;
    let mut digits = false;
    let mut spacing = false;

    for ch in value.chars() {
        if ch.is_ascii_alphabetic() || ch == '_' {
            if !ident {
                normalized.push('i');
                ident = true;
                digits = false;
                spacing = false;
            }
            continue;
        }

        if ch.is_ascii_digit() {
            if !digits {
                normalized.push('n');
                digits = true;
                ident = false;
                spacing = false;
            }
            continue;
        }

        ident = false;
        digits = false;
        if ch.is_whitespace() {
            if !spacing {
                normalized.push(' ');
                spacing = true;
            }
            continue;
        }

        spacing = false;
        normalized.push(ch);
    }

    normalized.trim().to_owned()
}

fn stable_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
