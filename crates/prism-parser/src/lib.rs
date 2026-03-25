use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, FileId, Language, Node, NodeId};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub struct ParseInput<'a> {
    pub crate_name: &'a str,
    pub workspace_root: &'a Path,
    pub path: &'a Path,
    pub file_id: FileId,
    pub source: &'a str,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnresolvedCall {
    pub source: NodeId,
    pub name: SmolStr,
    pub module_path: SmolStr,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnresolvedImport {
    pub source: NodeId,
    pub name: SmolStr,
    pub module_path: SmolStr,
    pub target_path: SmolStr,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnresolvedImpl {
    pub source: NodeId,
    pub name: SmolStr,
    pub module_path: SmolStr,
    pub trait_path: SmolStr,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
}

impl ParseResult {
    pub fn merge(&mut self, other: Self) {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
        self.unresolved_calls.extend(other.unresolved_calls);
        self.unresolved_imports.extend(other.unresolved_imports);
        self.unresolved_impls.extend(other.unresolved_impls);
    }
}

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

pub fn relative_file(input: &ParseInput<'_>) -> PathBuf {
    input
        .path
        .strip_prefix(input.workspace_root)
        .unwrap_or(input.path)
        .to_path_buf()
}
