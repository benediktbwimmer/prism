use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::{Edge, FileId, Language, Node, NodeId};
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub struct ParseInput<'a> {
    pub crate_name: &'a str,
    pub workspace_root: &'a Path,
    pub path: &'a Path,
    pub file_id: FileId,
    pub source: &'a str,
}

#[derive(Debug, Clone, Default)]
pub struct UnresolvedCall {
    pub source: NodeId,
    pub name: SmolStr,
    pub module_path: SmolStr,
}

#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub unresolved_calls: Vec<UnresolvedCall>,
}

impl ParseResult {
    pub fn merge(&mut self, other: Self) {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
        self.unresolved_calls.extend(other.unresolved_calls);
    }
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
