use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

pub type EdgeIndex = usize;
pub type Timestamp = u64;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Language {
    Rust,
    Markdown,
    Json,
    Yaml,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn line(line: usize) -> Self {
        let offset = line.saturating_sub(1);
        Self::new(offset, offset)
    }

    pub fn whole_file(byte_len: usize) -> Self {
        Self::new(0, byte_len)
    }

    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

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

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.path, self.kind)
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct LineageId(#[schemars(with = "String")] pub SmolStr);

impl LineageId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct EventId(#[schemars(with = "String")] pub SmolStr);

impl EventId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct TaskId(#[schemars(with = "String")] pub SmolStr);

impl TaskId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct AgentId(#[schemars(with = "String")] pub SmolStr);

impl AgentId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct SessionId(#[schemars(with = "String")] pub SmolStr);

impl SessionId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct PlanId(#[schemars(with = "String")] pub SmolStr);

impl PlanId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct CoordinationTaskId(#[schemars(with = "String")] pub SmolStr);

impl CoordinationTaskId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct ClaimId(#[schemars(with = "String")] pub SmolStr);

impl ClaimId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct ArtifactId(#[schemars(with = "String")] pub SmolStr);

impl ArtifactId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct ReviewId(#[schemars(with = "String")] pub SmolStr);

impl ReviewId {
    pub fn new(value: impl Into<SmolStr>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
pub struct WorkspaceRevision {
    pub graph_version: u64,
    #[schemars(with = "Option<String>")]
    pub git_commit: Option<SmolStr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanStatus {
    Draft,
    Active,
    Blocked,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CoordinationTaskStatus {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ClaimMode {
    Advisory,
    SoftExclusive,
    HardExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Capability {
    Observe,
    Edit,
    Review,
    Validate,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ClaimStatus {
    Active,
    Released,
    Expired,
    Contended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ConflictSeverity {
    Info,
    Warn,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ArtifactStatus {
    Proposed,
    InReview,
    Approved,
    Rejected,
    Superseded,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ReviewVerdict {
    Approved,
    ChangesRequested,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CoordinationEventKind {
    PlanCreated,
    TaskCreated,
    TaskAssigned,
    TaskStatusChanged,
    TaskBlocked,
    TaskUnblocked,
    ClaimAcquired,
    ClaimRenewed,
    ClaimReleased,
    ClaimContended,
    ArtifactProposed,
    ArtifactReviewed,
    ArtifactSuperseded,
    HandoffRequested,
    HandoffAccepted,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum AnchorRef {
    Node(NodeId),
    Lineage(LineageId),
    File(FileId),
    Kind(NodeKind),
}

impl From<NodeId> for AnchorRef {
    fn from(value: NodeId) -> Self {
        Self::Node(value)
    }
}

impl From<&NodeId> for AnchorRef {
    fn from(value: &NodeId) -> Self {
        Self::Node(value.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventMeta {
    pub id: EventId,
    pub ts: Timestamp,
    pub actor: EventActor,
    pub correlation: Option<TaskId>,
    pub causation: Option<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EventActor {
    User,
    Agent,
    System,
    GitAuthor {
        #[schemars(with = "String")]
        name: SmolStr,
        #[schemars(with = "Option<String>")]
        email: Option<SmolStr>,
    },
    CI,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphChange {
    Added(NodeId),
    Removed(NodeId),
    Modified(NodeId),
    Reanchored { old: NodeId, new: NodeId },
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedNode {
    pub node: Node,
    pub fingerprint: SymbolFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeTrigger {
    ManualReindex,
    FsWatch,
    AgentEdit,
    UserEdit,
    GitCheckout,
    GitCommitImport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedChangeSet {
    pub meta: EventMeta,
    pub trigger: ChangeTrigger,
    pub files: Vec<FileId>,
    pub added: Vec<ObservedNode>,
    pub removed: Vec<ObservedNode>,
    pub updated: Vec<(ObservedNode, ObservedNode)>,
    pub edge_added: Vec<Edge>,
    pub edge_removed: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineageEventKind {
    Born,
    Updated,
    Renamed,
    Moved,
    Reparented,
    Split,
    Merged,
    Died,
    Revived,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineageEvidence {
    ExactNodeId,
    FingerprintMatch,
    SignatureMatch,
    BodyHashMatch,
    SkeletonMatch,
    SameContainerLineage,
    GitRenameHint,
    FileMoveHint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineageEvent {
    pub meta: EventMeta,
    pub lineage: LineageId,
    pub kind: LineageEventKind,
    pub before: Vec<NodeId>,
    pub after: Vec<NodeId>,
    pub confidence: f32,
    pub evidence: Vec<LineageEvidence>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Subgraph {
    pub root: NodeId,
    pub nodes: Vec<NodeId>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
    pub max_depth_reached: Option<usize>,
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new("", "", NodeKind::Module)
    }
}
