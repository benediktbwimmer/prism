# PRISM — Rust Implementation Specification

## 0. Philosophy

PRISM is a deterministic, local-first perception layer with optional probabilistic augmentation.

Core invariants:

* Structure is authoritative
* Time and memory are layered on top of structure, not baked into it
* Agents are additive, never authoritative
* Every durable attachment must be evidence-backed and anchorable

The central design move is to separate:

* current-snapshot identity
* temporal identity across change
* experiential memory about outcomes

That means:

* `NodeId` answers "what is this symbol in the current graph?"
* `LineageId` answers "what is the same thing through time?"
* memory and outcomes attach through `AnchorRef`, not raw file offsets or transient parser details

---

# 1. Crate Architecture

Workspace layout:

```text
prism/
  Cargo.toml
  crates/
    prism-core/
    prism-ir/
    prism-parser/
    prism-lang-rust/
    prism-lang-markdown/
    prism-lang-json/
    prism-lang-yaml/
    prism-store/
    prism-history/
    prism-query/
    prism-memory/
    prism-agent/
    prism-cli/
```

`prism-history` is the new conceptual layer that turns raw observed graph change into deterministic temporal lineage. It may begin life inside `prism-store::history`, but the intended boundary is separate.

Dependency direction:

```text
prism-cli
  → prism-query
      → prism-store → prism-ir
      → prism-history → prism-ir
      → prism-memory → prism-ir
  → prism-agent → prism-ir
  → prism-parser → prism-ir
      → prism-lang-rust
      → prism-lang-markdown
      → prism-lang-json
      → prism-lang-yaml
```

Critical boundaries:

* `prism-store` owns persistence and raw observed change capture
* `prism-history` owns lineage assignment and time-aware projection
* `prism-memory` owns structured memory and outcomes, but not graph construction
* `prism-query` is the join layer over graph, lineage, and memory

---

# 2. Core IR (prism-ir)

## 2.1 NodeId

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub crate_name: SmolStr,
    pub path: SmolStr,
    pub kind: NodeKind,
}
```

Rules:

* `NodeId` identifies a node in the current snapshot
* it must be semantic, not byte-offset-based
* it must not encode temporal identity
* rename, move, split, or reparenting may legitimately change `NodeId`
* cross-time continuity is handled by `LineageId`, not by stretching `NodeId`

Canonicalization rules:

* `crate_name` is the owning package or crate namespace
* `path` is semantic and package-relative, not workspace-layout-derived
* `impl` nodes use `crate::module::Type::impl::Trait` or `crate::module::Type::impl`

## 2.2 Stable Cross-Time and Task Identity

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineageId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskId(pub smol_str::SmolStr);
```

Rules:

* `LineageId` is stable across renames, moves, and other re-anchorable changes
* `EventId` identifies a single observed or outcome event
* `TaskId` groups related agent, user, build, review, and validation actions into one resumable story

## 2.3 AnchorRef

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnchorRef {
    Node(NodeId),
    Lineage(LineageId),
    File(FileId),
    Kind(NodeKind),
}
```

Rules:

* `AnchorRef::Node` is the v1-compatible anchor for current graph entities
* `AnchorRef::Lineage` is the preferred durable anchor for cross-time memory
* `AnchorRef::File` is for file-scoped events, diagnostics, and coarse outcome attachment
* `AnchorRef::Kind` supports broad structural or policy rules

## 2.4 Event Envelope

```rust
pub struct EventMeta {
    pub id: EventId,
    pub ts: Timestamp,
    pub actor: EventActor,
    pub correlation: Option<TaskId>,
    pub causation: Option<EventId>,
}

pub enum EventActor {
    User,
    Agent,
    System,
    GitAuthor { name: String, email: Option<String> },
    CI,
}
```

Rules:

* `correlation` groups the events of one task or incident
* `causation` points to the event that immediately led to this event
* event streams may differ in semantics, but they share the same envelope

## 2.5 Node

```rust
pub struct Node {
    pub id: NodeId,
    pub name: SmolStr,
    pub kind: NodeKind,
    pub file: FileId,
    pub span: Span,
    pub language: Language,
}
```

## 2.6 FileId

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);
```

Rules:

* `FileId` is opaque outside the store and indexer boundary
* path-based meaning belongs in `NodeId`, not in `FileId`

## 2.7 NodeKind

```rust
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
```

## 2.8 Edge

```rust
pub struct Edge {
    pub kind: EdgeKind,
    pub source: NodeId,
    pub target: NodeId,
    pub origin: EdgeOrigin,
    pub confidence: f32,
}
```

## 2.9 EdgeKind

```rust
pub enum EdgeKind {
    Contains,
    Calls,
    References,
    Implements,
    Defines,
    Imports,
}
```

## 2.10 EdgeOrigin

```rust
pub enum EdgeOrigin {
    Static,
    Inferred,
}
```

## 2.11 Skeleton

```rust
pub struct Skeleton {
    pub calls: Vec<NodeId>,
}
```

## 2.12 SymbolFingerprint

```rust
pub struct SymbolFingerprint {
    pub signature_hash: u64,
    pub body_hash: Option<u64>,
    pub skeleton_hash: Option<u64>,
    pub child_shape_hash: Option<u64>,
}
```

Rules:

* no byte offsets
* no whitespace sensitivity
* no path sensitivity
* derived from normalized structure and content only
* deterministic and language-adapter-produced

For Rust:

* `signature_hash` is name-stripped signature shape plus generics, params, and return type
* `body_hash` is normalized body content when available
* `skeleton_hash` is normalized call skeleton
* `child_shape_hash` captures fields, variants, or method names when relevant

---

# 3. Storage Layer (prism-store)

## 3.1 Requirements

* fast read-heavy access
* incremental updates
* low memory footprint
* raw change capture without speculative historical interpretation

## 3.2 Approach

Hybrid:

* in-memory graph for runtime traversal
* disk-backed cache with SQLite in v1

The store boundary stays narrow:

* runtime graph traversal stays in `Graph`
* persistence and update bookkeeping live in `Store`
* lineage inference does not live here

## 3.3 Graph Representation

```rust
pub struct Graph {
    pub nodes: HashMap<NodeId, Node>,
    pub edges: Vec<Edge>,
    pub adjacency: HashMap<NodeId, Vec<EdgeIndex>>,
}
```

`EdgeIndex` may remain a `usize` in v1 if adjacency rebuilds on update. If edge churn grows, move to a stable arena or slot-map-backed handle.

## 3.4 File Records

```rust
pub struct FileRecord {
    pub file_id: FileId,
    pub hash: u64,
    pub nodes: Vec<NodeId>,
    pub fingerprints: HashMap<NodeId, SymbolFingerprint>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
}
```

## 3.5 Observed Change Stream

The store emits raw observed facts only. It does not decide whether something was a rename, move, split, or merge.

```rust
pub struct ObservedNode {
    pub node: Node,
    pub fingerprint: SymbolFingerprint,
}

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

pub enum ChangeTrigger {
    ManualReindex,
    FsWatch,
    AgentEdit,
    UserEdit,
    GitCheckout,
    GitCommitImport,
}
```

Rules:

* `ObservedChangeSet` is the canonical change stream for temporal projection
* `updated` means "same current `NodeId`, content changed" and is not a rename claim
* rename and move semantics are assigned later by `prism-history`

If older code still expects `GraphChange`, derive it at the boundary from `ObservedChangeSet` as a compatibility convenience. It is not the canonical time model.

## 3.6 Incremental Update Flow

On file change:

* remove prior file-local nodes and edges from the current graph
* reparse the file
* update unresolved references
* rebuild derived edges such as `Calls`, `Imports`, and `Implements`
* emit one `ObservedChangeSet`

---

# 4. Parsing Layer (prism-parser)

## 4.1 Tree-sitter Integration

* one parser per language
* file-level incremental indexing in v1
* tree reuse and tree-sitter incremental edit reuse are future optimizations

## 4.2 Trait Interface

```rust
pub trait LanguageAdapter {
    fn language(&self) -> Language;
    fn supports_path(&self, path: &Path) -> bool;
    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult>;
}
```

```rust
pub struct ParseInput<'a> {
    pub package_name: &'a str,
    pub crate_name: &'a str,
    pub package_root: &'a Path,
    pub path: &'a Path,
    pub file_id: FileId,
    pub source: &'a str,
}
```

## 4.3 ParseResult

```rust
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub fingerprints: HashMap<NodeId, SymbolFingerprint>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
}
```

Rules:

* language adapters produce fingerprints, but do not assign lineage
* unresolved reference capture stays file-local and deterministic

---

# 5. Language Adapters

## 5.1 Rust Adapter (prism-lang-rust)

Responsibilities:

* extract symbols
* build containment hierarchy
* detect calls heuristically
* emit stable fingerprints for lineage resolution

Symbol extraction:

* `function_item`
* `struct_item`
* `enum_item`
* `trait_item`
* `impl_item`

Canonicalization:

* root module identity is derived from the owning package
* inline modules extend the containing module path
* `impl Foo for Bar` becomes `...::Bar::impl::Foo`
* inherent impls become `...::Bar::impl`

Call detection in v1:

* `call_expression`
* `method_call_expression`
* direct name match
* scoped lookup within module

Skeleton extraction:

* collect top-level calls
* ignore literal and control-flow noise

Fingerprint extraction:

* signature shape
* normalized body
* call skeleton
* child shape for structs, enums, and container-like symbols

## 5.2 Markdown Adapter

* one `Document` node per file
* headings become nodes
* containment is `Package -> Document -> Heading -> Heading`

## 5.3 JSON and YAML Adapters

* one `Document` node per file
* keys become nodes
* containment is `Package -> Document -> Key -> Key`

These adapters matter for intent grounding later, so docs and config should be treated as first-class perception inputs rather than blob text.

---

# 6. History Layer (prism-history)

## 6.1 Purpose

`prism-history` consumes `ObservedChangeSet` and produces:

* `NodeId -> LineageId` mappings for the current graph
* append-only lineage events
* deterministic explanations for re-anchoring decisions

This is the layer that makes PRISM temporal without overloading the graph identity model.

## 6.2 Lineage Events

```rust
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

pub struct LineageEvent {
    pub meta: EventMeta,
    pub lineage: LineageId,
    pub kind: LineageEventKind,
    pub before: Vec<NodeId>,
    pub after: Vec<NodeId>,
    pub confidence: f32,
    pub evidence: Vec<LineageEvidence>,
}

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
```

Rules:

* every re-anchoring decision must be explainable
* ambiguous matches must remain ambiguous rather than being overcommitted
* lineage events are deterministic projections over observed change, not agent guesses

## 6.3 Resolver Strategy

The resolver should be deterministic and staged:

1. exact `NodeId` match
2. same kind plus strong fingerprint match in the same container lineage
3. same kind plus strong fingerprint match with git rename or file-move hints
4. one removed node matching many added nodes yields a split candidate
5. many removed nodes matching one added node yields a merge candidate
6. multiple equally strong candidates emit `Ambiguous`

Trust rule:

* prefer a graceful "not sure" over a confident but wrong lineage claim

## 6.4 Derived Projections

Once lineage exists, `prism-history` can project:

* co-change maps
* hotspot indices
* lineage-local change frequency
* ambiguity hot zones

Those projections are derived views, not primary storage.

---

# 7. Query Layer (prism-query)

## 7.1 Entry API

```rust
pub struct Prism {
    graph: Arc<Graph>,
    history: Arc<HistoryStore>,
    memory: Arc<dyn MemoryModule>,
}
```

## 7.2 Symbol Handle

```rust
pub struct Symbol<'a> {
    prism: &'a Prism,
    id: NodeId,
}
```

## 7.3 Core Methods

```rust
impl Prism {
    pub fn symbol(&self, query: &str) -> Vec<Symbol<'_>>;
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        kind: Option<NodeKind>,
        path: Option<&str>,
    ) -> Vec<Symbol<'_>>;

    pub fn lineage_of(&self, node: &NodeId) -> Option<LineageId>;
    pub fn lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent>;
    pub fn outcomes_for(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent>;
    pub fn related_failures(&self, node: &NodeId) -> Vec<OutcomeEvent>;
    pub fn blast_radius(&self, node: &NodeId) -> ChangeImpact;
    pub fn resume_task(&self, task: &TaskId) -> TaskReplay;
}

impl<'a> Symbol<'a> {
    pub fn name(&self) -> &str;
    pub fn signature(&self) -> String;
    pub fn skeleton(&self) -> Skeleton;
    pub fn full(&self) -> String;
    pub fn call_graph(&self, depth: usize) -> Subgraph;
}
```

High-value query behaviors:

* "why is this risky?"
* "what usually changes with this?"
* "what failed here before?"
* "what happened the last time we touched this?"

## 7.4 ChangeImpact

`ChangeImpact` should be a structured answer, not just a flat edge walk. It should be able to include:

* directly affected nodes
* historically co-changing neighbors
* relevant lineages
* likely validations
* attached outcome risks
* unresolved ambiguities

## 7.5 TaskReplay

`TaskReplay` reconstructs the correlated story for a `TaskId`:

* plan or hypothesis
* patch events
* tests or builds run
* failures or review feedback
* fix validation or rollback

---

# 8. Agent Layer (prism-agent)

## 8.1 Purpose

* augment the graph
* resolve ambiguity where allowed
* produce evidence-backed actions and notes

## 8.2 Interface

```rust
pub trait Agent {
    fn infer_edges(&self, context: AgentContext) -> Vec<Edge>;
}
```

```rust
pub struct AgentContext {
    pub symbol: NodeId,
    pub known_edges: Vec<Edge>,
    pub unresolved_calls: Vec<String>,
}
```

## 8.3 Output Contract

* inferred edges must include confidence
* static edges are never overwritten
* agent-authored memory and outcome events should carry `TaskId` correlation when possible
* recommendations and patches should be traceable back to graph, lineage, memory, or runtime evidence

---

# 9. Memory Layer (prism-memory)

## 9.1 Purpose

Memory is a composable layer over PRISM's world model. It is not part of the deterministic graph, but it must remain anchorable to graph entities and durable across code motion when possible.

The long-term anchor model is `AnchorRef`, not raw `NodeId`.

## 9.2 Architecture

```text
┌──────────────────────────────────────────────────────┐
│                   MemoryComposite                    │
│    merges and deduplicates results from modules      │
├────────────────┬─────────────────┬───────────────────┤
│ OutcomeMemory  │ StructuralMemory│   SemanticMemory  │
│  (events)      │   (patterns)    │   (vectors/text)  │
└────────────────┴─────────────────┴───────────────────┘
                         │
                     anchored to
                         │
                     AnchorRef
```

## 9.3 Core Trait

```rust
pub trait MemoryModule: Send + Sync {
    fn name(&self) -> &'static str;
    fn store(&self, entry: MemoryEntry) -> Result<MemoryId>;
    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>>;
    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()>;
}
```

Rules:

* v1 modules may only use `AnchorRef::Node`
* the trait still accepts `AnchorRef` so later re-anchoring does not require a breaking redesign

## 9.4 MemoryEntry

```rust
pub struct MemoryEntry {
    pub id: MemoryId,
    pub anchors: Vec<AnchorRef>,
    pub kind: MemoryKind,
    pub content: String,
    pub metadata: serde_json::Value,
    pub created_at: Timestamp,
    pub source: MemorySource,
    pub trust: f32,
}

pub enum MemoryKind {
    Episodic,
    Structural,
    Semantic,
}

pub enum MemorySource {
    Agent,
    User,
    System,
}
```

Rules:

* `MemoryId` is store-generated in v1
* `source` is provenance, not ranking by itself
* `trust` is explicit so fuzzy or speculative memory can be down-ranked
* at equal score, prefer higher trust, then `User`, then `System`, then `Agent`

## 9.5 RecallQuery

```rust
pub struct RecallQuery {
    pub focus: Vec<AnchorRef>,
    pub text: Option<String>,
    pub limit: usize,
    pub kinds: Option<Vec<MemoryKind>>,
    pub since: Option<Timestamp>,
}
```

Anchor semantics:

* anchor matching is disjunctive in v1
* larger overlap should score higher than weak overlap
* `Node` and `Lineage` anchors may both participate in the same entry

## 9.6 ScoredMemory

```rust
pub struct ScoredMemory {
    pub id: MemoryId,
    pub entry: MemoryEntry,
    pub score: f32,
    pub source_module: String,
    pub explanation: Option<String>,
}
```

## 9.7 MemoryComposite

```rust
pub struct MemoryComposite {
    modules: Vec<(Box<dyn MemoryModule>, f32)>,
}
```

Normalization rules:

* each module returns scores normalized into `[0.0, 1.0]`
* composite weighting clamps and reweights module-local scores
* deduplication happens on `MemoryId`

## 9.8 Outcome Memory

Outcome memory is the first specialized structured memory worth building. It records what happened when code was touched.

```rust
pub struct OutcomeEvent {
    pub meta: EventMeta,
    pub anchors: Vec<AnchorRef>,
    pub kind: OutcomeKind,
    pub result: OutcomeResult,
    pub summary: String,
    pub evidence: Vec<OutcomeEvidence>,
    pub metadata: serde_json::Value,
}

pub enum OutcomeKind {
    NoteAdded,
    HypothesisProposed,
    PlanCreated,
    PatchApplied,
    BuildRan,
    TestRan,
    ReviewFeedback,
    FailureObserved,
    RegressionObserved,
    FixValidated,
    RollbackPerformed,
    MigrationRequired,
    IncidentLinked,
    PerfSignalObserved,
}

pub enum OutcomeResult {
    Success,
    Failure,
    Partial,
    Unknown,
}

pub enum OutcomeEvidence {
    Commit { sha: String },
    Test { name: String, passed: bool },
    Build { target: String, passed: bool },
    Reviewer { author: String },
    Issue { id: String },
    StackTrace { hash: String },
    DiffSummary { text: String },
}
```

Recommended v1.5 starter kinds:

* `NoteAdded`
* `PatchApplied`
* `TestRan`
* `FailureObserved`
* `FixValidated`

Index outcome events by:

* `AnchorRef`
* `TaskId`
* `OutcomeKind`
* recency
* result
* actor

## 9.9 Module Types

### OutcomeMemory

* append-only structured event log
* indexed by anchor, task, kind, and recency
* source of "what happened here before?"

### StructuralMemory

* stores rules, invariants, and policy-like knowledge
* examples: "changes here require migration", "these modules evolve together"

### SemanticMemory

* stores embedded text for fuzzy recall
* escape hatch for unstructured knowledge, not the authoritative model

## 9.10 Integration Points

PRISM does not depend on memory. Integration happens at the boundaries:

1. graph entities expose `AnchorRef`
2. store emits `ObservedChangeSet`
3. history emits `LineageEvent`
4. memory can re-anchor or enrich recall with both

The highest-value join is:

* current node
* mapped lineage
* prior outcomes

That is how memories survive renames, moves, and restructures.

---

# 10. CLI (prism-cli)

Commands:

```text
prism entrypoints
prism symbol <name>
prism search <query> [--limit 20] [--kind <kind>] [--path <fragment>]
prism call-graph <name> --depth 3
prism lineage <symbol>
prism risk <symbol>
prism task resume <task-id>
prism memory recall <symbol> [--text "query"]
prism memory store <symbol> --content "note"
```

---

# 11. Git Integration

Shelling out is acceptable in v1 for:

* `git blame`
* `git log -L`
* rename and move hints used as lineage evidence

`gix` is the preferred future direction once structured access or process overhead becomes a real constraint.

---

# 12. Performance Strategy

* arena-style allocation for graph-heavy structures where needed
* string interning with `smol_str`
* lazy file loading
* append-only event storage for history and outcomes
* derived projections built incrementally rather than recomputed from scratch when possible

---

# 13. Future Hooks

These are explicitly not v1 requirements, but the architecture should leave room for them:

* intent graph over docs, specs, ADRs, tests, and symbols
* runtime grounding via coverage, traces, logs, stack traces, and profiler samples
* first-class uncertainty tracking
* policy and invariant layers
* drift detection between implementation and intent

---

# 14. Build Order

Recommended sequence:

1. land `LineageId`, `EventId`, `TaskId`, and `AnchorRef` in `prism-ir`
2. make `prism-store` emit `ObservedChangeSet`
3. implement deterministic lineage resolution in `prism-history`
4. add structured `OutcomeEvent` logging in `prism-memory`
5. expose `lineage_of`, `related_failures`, `blast_radius`, and `resume_task` in `prism-query`
6. add derived projections such as co-change, hotspot, validation recipes, and drift detection

---

# Final Principle

PRISM should remember code as a living thing, not just a static graph.

The system model is:

* structure tells PRISM what exists now
* history tells PRISM what persisted through change
* outcome memory tells PRISM what happened when it changed
* queries and agents turn that into evidence-backed action
