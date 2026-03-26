use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{FileId, Language, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum NodeKind {
    Workspace,
    Package,
    Document,
    Module,
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Method,
    Field,
    TypeAlias,
    MarkdownHeading,
    JsonKey,
    TomlKey,
    YamlKey,
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            NodeKind::Workspace => "workspace",
            NodeKind::Package => "package",
            NodeKind::Document => "document",
            NodeKind::Module => "module",
            NodeKind::Function => "function",
            NodeKind::Struct => "struct",
            NodeKind::Enum => "enum",
            NodeKind::Trait => "trait",
            NodeKind::Impl => "impl",
            NodeKind::Method => "method",
            NodeKind::Field => "field",
            NodeKind::TypeAlias => "type-alias",
            NodeKind::MarkdownHeading => "markdown-heading",
            NodeKind::JsonKey => "json-key",
            NodeKind::TomlKey => "toml-key",
            NodeKind::YamlKey => "yaml-key",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct NodeId {
    #[schemars(with = "String")]
    pub crate_name: SmolStr,
    #[schemars(with = "String")]
    pub path: SmolStr,
    pub kind: NodeKind,
}

impl NodeId {
    pub fn new(crate_name: impl Into<SmolStr>, path: impl Into<SmolStr>, kind: NodeKind) -> Self {
        Self {
            crate_name: crate_name.into(),
            path: path.into(),
            kind,
        }
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new("", "", NodeKind::Module)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.path, self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: SmolStr,
    pub kind: NodeKind,
    pub file: FileId,
    pub span: Span,
    pub language: Language,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum EdgeKind {
    Contains,
    Calls,
    References,
    Implements,
    Specifies,
    Validates,
    RelatedTo,
    Defines,
    Imports,
    DependsOn,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum EdgeOrigin {
    Static,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub kind: EdgeKind,
    pub source: NodeId,
    pub target: NodeId,
    pub origin: EdgeOrigin,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Skeleton {
    pub calls: Vec<NodeId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Subgraph {
    pub root: NodeId,
    pub nodes: Vec<NodeId>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
    pub max_depth_reached: Option<usize>,
}
