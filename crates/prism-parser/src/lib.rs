use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, FileId, Language, Node, NodeId, SymbolFingerprint};
use serde::{Deserialize, Serialize};

pub use prism_ir::{UnresolvedCall, UnresolvedImpl, UnresolvedImport};

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
}

impl ParseResult {
    pub fn merge(&mut self, other: Self) {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
        self.fingerprints.extend(other.fingerprints);
        self.unresolved_calls.extend(other.unresolved_calls);
        self.unresolved_imports.extend(other.unresolved_imports);
        self.unresolved_impls.extend(other.unresolved_impls);
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
