use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{EdgeKind, NodeId, Span};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolFingerprint {
    pub signature_hash: u64,
    pub body_hash: Option<u64>,
    pub skeleton_hash: Option<u64>,
    pub child_shape_hash: Option<u64>,
}

impl SymbolFingerprint {
    pub fn new(signature_hash: u64) -> Self {
        Self {
            signature_hash,
            body_hash: None,
            skeleton_hash: None,
            child_shape_hash: None,
        }
    }

    pub fn with_parts(
        signature_hash: u64,
        body_hash: Option<u64>,
        skeleton_hash: Option<u64>,
        child_shape_hash: Option<u64>,
    ) -> Self {
        Self {
            signature_hash,
            body_hash,
            skeleton_hash,
            child_shape_hash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedCall {
    pub caller: NodeId,
    pub name: SmolStr,
    pub span: Span,
    pub module_path: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedImport {
    pub importer: NodeId,
    pub path: SmolStr,
    pub span: Span,
    pub module_path: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedImpl {
    pub impl_node: NodeId,
    pub target: SmolStr,
    pub span: Span,
    pub module_path: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedIntent {
    pub source: NodeId,
    pub kind: EdgeKind,
    pub target: SmolStr,
    pub span: Span,
}
