# PRISM — Perceptual Representation & Intelligence System for Machines

## Rust Implementation Specification

Current implementation priorities live in [ROADMAP.md](ROADMAP.md).

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
    prism-projections/
    prism-coordination/
    prism-curator/
    prism-agent/
    prism-js/
    prism-mcp/
    prism-cli/
```

`prism-history` is the new conceptual layer that turns raw observed graph change into deterministic temporal lineage. It may begin life inside `prism-store::history`, but the intended boundary is separate.

Additional current crates:

* `prism-projections` owns derived read models such as co-change and validation signals
* `prism-coordination` owns shared plans, tasks, claims, artifacts, and coordination event state
* `prism-curator` owns background enrichment and proposal-oriented curation triggers

Dependency direction (simplified):

```text
prism-cli
  → prism-core
  → prism-query
prism-core
  → prism-store → prism-ir
  → prism-history → prism-ir
  → prism-memory → prism-ir
  → prism-projections
  → prism-query
  → prism-coordination → prism-ir
  → prism-curator
  → prism-parser → prism-ir
      → prism-lang-rust
      → prism-lang-markdown
      → prism-lang-json
      → prism-lang-yaml
prism-query
  → prism-store → prism-ir
  → prism-history → prism-ir
  → prism-memory → prism-ir
  → prism-projections
  → prism-coordination → prism-ir
prism-js
  → prism-memory → prism-ir
  → prism-coordination → prism-ir
prism-mcp
  → prism-core
  → prism-js
  → prism-query
  → prism-store → prism-ir
  → prism-memory → prism-ir
  → prism-coordination → prism-ir
  → prism-curator
  → prism-agent → prism-ir
```

Critical boundaries:

* `prism-core` is the orchestration layer that assembles a usable `Prism` from store, parser, history, memory, and runtime configuration
* `prism-core` owns workspace loading, adapter registration, incremental reindex coordination, and long-lived service construction
* `prism-core` is intentionally thin: it wires subsystems together, but does not redefine their storage, query, or memory semantics
* `prism-store` owns persistence and raw observed change capture
* `prism-history` owns lineage assignment and time-aware projection
* `prism-memory` owns structured memory and outcomes, but not graph construction
* `prism-projections` owns derived signals built from history and outcomes
* `prism-query` is the join layer over graph, lineage, and memory
* `prism-coordination` owns shared multi-session workflow state and policy checks
* `prism-curator` owns background proposal generation over Prism state
* `prism-js` owns the JavaScript/TypeScript-facing API contract and runtime shim
* `prism-mcp` owns agent transport, session lifecycle, and embedded query execution

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

Supporting types used by nodes and adapters:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Markdown,
    Json,
    Yaml,
}
```

Rules:

* `Span` is byte-offset based in v1 and is half-open: `[start, end)`
* `Language` is the parser-selected document language, not an inferred semantic category

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

Unresolved reference records are deterministic parser outputs. They capture gaps in static resolution without guessing the answer:

```rust
pub struct UnresolvedCall {
    pub caller: NodeId,
    pub name: SmolStr,
    pub span: Span,
}

pub struct UnresolvedImport {
    pub importer: NodeId,
    pub path: SmolStr,
    pub span: Span,
}

pub struct UnresolvedImpl {
    pub impl_node: NodeId,
    pub target: SmolStr,
    pub span: Span,
}
```

Rules:

* unresolved records are file-local parse artifacts, not durable identities
* they are allowed to be lossy as long as they are deterministic and useful for later resolution or augmentation
* agent augmentation may use them as input signals, but may not overwrite them in the authoritative parse result

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
* v1 should treat filesystem-detected edits as `FsWatch` by default
* `AgentEdit` and `UserEdit` are reserved for future explicit attribution paths; the store should not guess based on the writer process alone

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

# 8. Agent Augmentation Model

## 8.1 Four Kinds of State

PRISM maintains four distinct kinds of state, each with different creation rules:

1. **Deterministic structure** — parser, store, and history build this automatically. Never agent-written.
2. **Raw outcome memory** — what happened during a task: patch, test, failure, validation, note. Written by the foreground task agent.
3. **Ephemeral inference** — temporary agent guesses useful for the current task or session. Session-scoped by default.
4. **Durable inferred knowledge** — persistent inferred edges or structural memories that survived enough evidence. Requires promotion.

## 8.2 Automated Pipeline (Non-Agent)

These run automatically on every reindex or file change, with no agent involvement:

* parse files
* rebuild current graph
* emit `ObservedChangeSet`
* resolve lineage in `prism-history`
* re-anchor memory via `apply_lineage`

## 8.3 Foreground Task Agent Augmentation

The agent currently using PRISM over MCP is the primary augmentation actor because it has the missing ingredient the background system does not: **intent**.

It knows what the user is trying to do, what uncertainty matters right now, what evidence it just consulted, what patch it attempted, and what failed or succeeded.

### Query-Time Inference

When the static graph is not enough, the agent can infer extra edges or associations for the current task:

* likely callee for unresolved call
* likely related module
* probable doc or spec section tied to a symbol
* likely risk neighbors for a change plan

These should begin as session-scoped or low-trust persisted inference.

### Outcome Logging

The highest-value direct write. After or during work, the task agent should write structured outcomes:

* `TestRan`
* `FailureObserved`
* `FixValidated`
* `ReviewFeedback`

Patch observation is automatic — PRISM detects file changes via `ObservedChangeSet` and records them without agent involvement. The agent should only record outcomes that require semantic interpretation: what a test result means, what failed, why a fix worked.

### Episodic Notes

Small repo-specific notes anchored to nodes or lineages:

* "null handling here caused a regression"
* "changing this struct required updating serializer tests"
* "this module is migration-sensitive"

Trust must be explicit. Notes must not masquerade as facts.

## 8.4 What the Foreground Agent Should Not Do

The foreground agent should not directly create durable structural knowledge such as:

* stable inferred edges used globally
* policy memories
* hard "this always implies that" rules

Those require more evidence or later promotion. Otherwise every task agent leaves behind a trail of overfit guesses.

## 8.5 Trigger Model

### Trigger 1: Query-Time, On Demand (Primary)

Use augmentation when:

* the deterministic graph cannot answer a query well
* the result set is ambiguous
* there are unresolved calls, imports, or impls
* the agent is planning a change and wants a richer risk picture

Flow:

1. agent queries PRISM normally
2. PRISM returns structure plus ambiguity or unresolvedness
3. agent decides augmentation is worth it
4. agent synthesizes a task-local interpretation
5. agent optionally stores: session-scoped inferred edges, low-trust notes, outcome context

### Trigger 2: Task Lifecycle Events

At major task boundaries, the task agent should write structured memory:

* when it forms a plan
* when it applies a patch
* when tests or builds run
* when a failure appears
* when the fix is validated
* when the task ends or is abandoned

### Trigger 3: Post-Change Enrichment

After a meaningful `ObservedChangeSet`, PRISM can enqueue a bounded augmentation job. Good enqueue conditions:

* large change set in a hotspot
* many unresolved imports or calls
* ambiguous lineage mapping
* repeated failures on the same lineage
* task completed with rich evidence worth distilling

## 8.6 Background Curator

The background curator is event-driven, not loop-driven. It is never an always-on autonomous agent roaming the repo.

It reads:

* accumulated outcome events
* repeated ambiguities
* lineage histories
* co-change patterns

It produces:

* candidate structural memories
* candidate durable inferred edges
* candidate risk summaries
* candidate validation recipes

Crucially: **the curator proposes, it does not silently rewrite the world.**

Its outputs land as:

* `Inferred` edges with evidence and confidence
* structural memories with explicit trust
* promotion candidates, not authoritative structure

The foreground agent is the historian of the current task. The background curator is the librarian over many tasks.

## 8.7 Inferred Edge Overlay

Graph augmentation is an overlay, not a mutation of the base graph.

Queries can operate over:

* static graph only
* static plus inferred edges
* static plus inferred plus memory and risk views

Inferred edges are visible, useful, and composable, but they never contaminate the authoritative parse-derived structure.

```rust
pub enum InferredEdgeScope {
    SessionOnly,
    Persisted,
    Rejected,
    Expired,
}
```

This lets agents be aggressive in-session without poisoning long-term state.

## 8.8 Agent Trait Interface

```rust
pub trait Agent {
    fn infer_edges(&self, context: AgentContext) -> Vec<Edge>;
}

pub struct AgentContext {
    pub symbol: NodeId,
    pub known_edges: Vec<Edge>,
    pub unresolved_calls: Vec<String>,
    pub task: Option<TaskId>,
}
```

## 8.9 Output Contract

* inferred edges must include confidence and `EdgeOrigin::Inferred`
* static edges are never overwritten
* agent-authored outcome events should carry `TaskId` correlation when possible
* recommendations and patches should be traceable back to graph, lineage, memory, or runtime evidence

## 8.10 Core Principle

Let the active agent write raw experience. Let a background curator distill repeated patterns. Let neither rewrite authoritative structure.

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

# 10. Coordination Framework

## 10.1 Purpose

PRISM's multi-agent model is a shared coordination layer over the same graph, lineage, and memory model.

Agents do not need direct chat to collaborate. They coordinate through shared plans, claims, artifacts, reviews, and handoffs anchored to the same code entities.

Core principles:

* reads stay query-centric: most coordination inspection happens through `prism_query`
* mutations stay explicit: shared coordination state changes must be validated and audited
* coordination state is shared across sessions connected to the same PRISM server
* speculative inference remains session-scoped unless explicitly promoted
* coordination objects anchor through `AnchorRef` and survive code motion through lineage when possible
* claims are leases with policy, not blind global locks

This is a first-class conceptual layer. It may begin life inside `prism-mcp` or `prism-memory`, but the long-term intended boundary is separate.

## 10.2 Identity and Snapshot Model

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlanId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoordinationTaskId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClaimId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactId(pub smol_str::SmolStr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReviewId(pub smol_str::SmolStr);

pub struct WorkspaceRevision {
    pub graph_version: u64,
    pub git_commit: Option<String>,
}
```

Rules:

* `TaskId` remains the correlation ID for outcome history and task-local memory
* `CoordinationTaskId` is the shared plan node for multi-agent work; many `TaskId`s may contribute to one coordination task
* `SessionId` identifies the MCP connection that currently holds live claims or authored coordination mutations
* `AgentId` identifies a logical actor across many sessions when the client can provide one
* `WorkspaceRevision` captures the code state a coordination decision assumed
* coordination records should store both structural anchors and the base revision they were made against

## 10.3 Shared Plan Graph

```rust
pub enum PlanStatus {
    Draft,
    Active,
    Blocked,
    Completed,
    Abandoned,
}

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

pub enum ClaimMode {
    Advisory,
    SoftExclusive,
    HardExclusive,
}

pub struct CoordinationPolicy {
    pub default_claim_mode: ClaimMode,
    pub max_parallel_editors_per_anchor: u16,
    pub require_review_for_completion: bool,
    pub stale_after_graph_change: bool,
}

pub struct AcceptanceCriterion {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
}

pub struct Plan {
    pub id: PlanId,
    pub goal: String,
    pub status: PlanStatus,
    pub policy: CoordinationPolicy,
    pub root_tasks: Vec<CoordinationTaskId>,
}

pub struct CoordinationTask {
    pub id: CoordinationTaskId,
    pub plan: PlanId,
    pub title: String,
    pub status: CoordinationTaskStatus,
    pub assignee: Option<AgentId>,
    pub session: Option<SessionId>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<CoordinationTaskId>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub base_revision: WorkspaceRevision,
}
```

Rules:

* plans are shared DAGs of work, not private prompts
* dependencies are explicit and queryable
* acceptance criteria must be structured enough for handoff, validation, and review
* task status is server-authoritative and replayable
* assignment is advisory unless plan policy says otherwise
* one coordination task may be executed by several local `TaskId`s over time, but it has one shared lifecycle

## 10.4 Claims, Conflicts, and Contention

```rust
pub enum Capability {
    Observe,
    Edit,
    Review,
    Validate,
    Merge,
}

pub enum ClaimStatus {
    Active,
    Released,
    Expired,
    Contended,
}

pub enum ConflictSeverity {
    Info,
    Warn,
    Block,
}

pub struct WorkClaim {
    pub id: ClaimId,
    pub holder: SessionId,
    pub agent: Option<AgentId>,
    pub task: Option<CoordinationTaskId>,
    pub anchors: Vec<AnchorRef>,
    pub capability: Capability,
    pub mode: ClaimMode,
    pub since: Timestamp,
    pub expires_at: Timestamp,
    pub status: ClaimStatus,
    pub base_revision: WorkspaceRevision,
}

pub struct CoordinationConflict {
    pub severity: ConflictSeverity,
    pub anchors: Vec<AnchorRef>,
    pub summary: String,
    pub blocking_claims: Vec<ClaimId>,
}
```

Rules:

* claims are anchored, time-bounded leases
* overlap is detected at file, node, lineage, and nearby graph neighborhood levels
* conflict severity derives from capability, mode, overlap, and plan policy
* hard-exclusive conflicts may block coordination mutations, but read queries still succeed
* claim acquisition never silently steals ownership; it returns conflict detail
* lease expiry and renewal are explicit so abandoned sessions do not pin the repo forever

## 10.5 Artifacts, Reviews, and Handoffs

```rust
pub enum ArtifactStatus {
    Proposed,
    InReview,
    Approved,
    Rejected,
    Superseded,
    Merged,
}

pub enum ReviewVerdict {
    Approved,
    ChangesRequested,
    Rejected,
}

pub struct Artifact {
    pub id: ArtifactId,
    pub task: CoordinationTaskId,
    pub anchors: Vec<AnchorRef>,
    pub base_revision: WorkspaceRevision,
    pub diff_ref: Option<String>,
    pub status: ArtifactStatus,
    pub evidence: Vec<EventId>,
}
```

Rules:

* file change observation is still automatic, but an artifact records the coordination meaning of a patch or deliverable
* artifacts bind outputs to coordination tasks, anchors, and base revision
* review is first-class and may gate task completion by policy
* handoffs move task responsibility without losing the local `TaskId` history that led up to them
* if the base revision is stale relative to current graph state, PRISM should surface that before approval or merge-like completion

## 10.6 Coordination Event Log

```rust
pub enum CoordinationEventKind {
    PlanCreated,
    PlanUpdated,
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
    MutationRejected,
}
```

Rules:

* coordination is event-sourced the same way history and outcomes are event-sourced
* replay must answer why a task is blocked, why a claim was denied, and what review state gated completion
* coordination events use the same attribution discipline as other PRISM mutations: timestamped, actor-attributed, and task-correlated when possible

## 10.7 Read Model Through `prism_query`

Coordination reads should primarily happen through `prism_query`, not through a growing list of bespoke MCP tools.

The query surface should expose first-class coordination views such as:

* `prism.plan(planId)`
* `prism.task(taskId)`
* `prism.readyTasks(planId)`
* `prism.claims(anchor)`
* `prism.conflicts(anchor)`
* `prism.blockers(taskId)`
* `prism.pendingReviews(planId?)`
* `prism.artifacts(taskId)`
* `prism.simulateClaim(input)`

This keeps multi-agent reasoning programmable in one round-trip while preserving an explicit audit boundary for mutations.

---

# 11. MCP Server (prism-mcp)

## 11.1 Purpose

`prism-mcp` is the primary agent-facing integration surface.

The design goal is not a large catalog of narrowly typed tools. The design goal is one programmable query surface over a live in-memory `Prism` instance.

That means:

* the MCP server loads and retains the graph for the session
* queries compose in one round-trip instead of chaining many tool calls
* API discovery happens through an MCP resource, not repeated system prompt text

Session and task model:

* an MCP session may exist with no active `TaskId` while it is only reading
* every MCP session has a stable `SessionId` for attribution and live claim ownership
* on the first mutation in a session with no active task, the server auto-creates a `TaskId` and binds it as the active task
* agents may explicitly create and label task context with `prism_session { action: "start_task", ... }`
* one session may create many tasks over time; at most one task is the session default at a time
* mutation tools inherit the active session `TaskId` when `task_id` is omitted
* mutation tools may override attribution with an explicit `task_id`, so unrelated work can coexist in one session without opening a second MCP connection

## 11.2 Primary Tool

```text
prism_query { code: string, language?: "ts" } -> QueryResult
```

Rules:

* v1 is TypeScript-first
* `language` may default to `"ts"` in v1
* the query executes with a pre-bound `prism` object over structure, lineage, memory, and coordination state
* the final value returned by the snippet must be JSON-serializable
* execution happens against the already-loaded in-memory graph for the active MCP session
* `prism_query` is read-only
* mutations such as memory writes, outcome logging, inference persistence, plan updates, and claim acquisition are handled through explicit MCP mutation tools, not through the query runtime

Expected query shape:

```ts
const sym = prism.symbol("handle_request");
const cg = sym?.callGraph(3);
const lineage = sym?.lineage();

return { sym, cg, lineage };
```

Structured output:

```ts
interface QueryResult {
  result: unknown;
  diagnostics: QueryDiagnostic[];
}

interface QueryDiagnostic {
  code:
    | "ambiguous_symbol"
    | "result_truncated"
    | "depth_limited"
    | "unknown_method"
    | "lineage_uncertain"
    | "anchor_unresolved"
    | "task_blocked"
    | "stale_revision";
  message: string;
  data?: Record<string, unknown>;
}
```

The goal is that agents can repair and narrow queries from machine-readable diagnostics instead of guessing from free-form error text.

## 11.3 Discovery Resource

The MCP server must expose at least one resource:

```text
prism://api-reference
```

This resource should document:

* a short conceptual overview
* a `d.ts`-style surface contract
* the `prism` global
* available methods and return types
* supported query-language conventions and limits
* runnable examples and recipes
* current limitations

The resource is the canonical discovery path for agents. The tool description should stay short and point to the resource instead of embedding the full API in prompt text.

The resource should feel more like a tiny SDK README plus type definition file than a dry protocol appendix.

## 11.4 Runtime Model

The runtime has three layers:

```text
TypeScript snippet
  ↓
prism-js runtime shim
  ↓
embedded JS engine host calls
  ↓
live Prism API
```

Execution requirements:

* the server owns the live `Prism` session state
* the embedded runtime must not shell out for each query
* TypeScript should transpile to JavaScript before evaluation
* runtime bindings must expose structured data, not formatted CLI text
* query results must serialize back to JSON for MCP tool output
* the query runtime should apply hard safety limits for breadth, depth, and output size

Security and determinism constraints:

* the runtime should expose only PRISM query capabilities, not arbitrary filesystem or process access
* host-call boundaries should be explicit and auditable
* query errors must return structured diagnostics
* broad or expensive queries should fail or truncate deterministically instead of degrading silently

Default v1 safety limits:

* max result nodes: `500`
* max call-graph depth: `10`
* max serialized JSON result size: `256 KiB`
* limits should be configurable per session, but the defaults must exist even when the client provides no overrides

## 11.5 Binding Layer (prism-js)

`prism-js` is the language-facing facade over `prism-query`.

Responsibilities:

* define the `prism` object surface for JS/TS queries
* provide the runtime shim loaded into the embedded engine
* publish the API reference resource text
* keep the JS-visible contract stable even as Rust internals evolve
* present plain structured views rather than leaking Rust internals or opaque runtime handles

Representative surface:

```ts
interface PrismApi {
  symbol(query: string): SymbolView | null;
  symbols(query: string): SymbolView[];
  search(query: string, options?: SearchOptions): SymbolView[];
  entrypoints(): SymbolView[];
  plan(planId: string): PlanView | null;
  task(taskId: string): CoordinationTaskView | null;
  readyTasks(planId: string): CoordinationTaskView[];
  claims(anchor: AnchorRefView): ClaimView[];
  conflicts(anchor: AnchorRefView): ConflictView[];
  blockers(taskId: string): BlockerView[];
  pendingReviews(planId?: string): ArtifactView[];
  artifacts(taskId: string): ArtifactView[];
  simulateClaim(input: ClaimProposal): ConflictView[];
  diagnostics(): QueryDiagnostic[];
}

interface NodeId {
  crateName: string;
  path: string;
  kind: NodeKind;
}

type NodeKind =
  | "Workspace"
  | "Package"
  | "Document"
  | "Module"
  | "Function"
  | "Struct"
  | "Enum"
  | "Trait"
  | "Impl"
  | "Method"
  | "Field"
  | "TypeAlias"
  | "MarkdownHeading"
  | "JsonKey"
  | "YamlKey";

type EdgeKind =
  | "Contains"
  | "Calls"
  | "References"
  | "Implements"
  | "Defines"
  | "Imports";

type EdgeOrigin = "Static" | "Inferred";

interface SymbolView {
  id: NodeId;
  name: string;
  kind: NodeKind;
  signature: string;
  full(): string;
  relations(): Relations;
  callGraph(depth: number): Subgraph;
  lineage(): LineageView | null;
}

interface SearchOptions {
  limit?: number;
  kind?: NodeKind;
  path?: string;
  includeInferred?: boolean;
}

interface Relations {
  contains: SymbolView[];
  callers: SymbolView[];
  callees: SymbolView[];
  references: SymbolView[];
  imports: SymbolView[];
  implements: SymbolView[];
}

interface Subgraph {
  nodes: SymbolView[];
  edges: {
    kind: EdgeKind;
    source: NodeId;
    target: NodeId;
    origin: EdgeOrigin;
    confidence: number;
  }[];
  truncated?: boolean;
  maxDepthReached?: number;
}

interface LineageView {
  lineageId: string;
  current: SymbolView;
  status: "active" | "dead" | "ambiguous";
  history: {
    eventId: string;
    ts: string;
    kind: string;
    confidence: number;
  }[];
}

type AnchorRefView =
  | { type: "Node"; node: NodeId }
  | { type: "Lineage"; lineageId: string }
  | { type: "File"; fileId: number }
  | { type: "Kind"; kind: NodeKind };

type PlanStatus = "Draft" | "Active" | "Blocked" | "Completed" | "Abandoned";

type CoordinationTaskStatus =
  | "Proposed"
  | "Ready"
  | "InProgress"
  | "Blocked"
  | "InReview"
  | "Validating"
  | "Completed"
  | "Abandoned";

type Capability = "Observe" | "Edit" | "Review" | "Validate" | "Merge";
type ClaimMode = "Advisory" | "SoftExclusive" | "HardExclusive";
type ConflictSeverity = "Info" | "Warn" | "Block";

interface WorkspaceRevisionView {
  graphVersion: number;
  gitCommit?: string;
}

interface PlanView {
  id: string;
  goal: string;
  status: PlanStatus;
  rootTaskIds: string[];
}

interface CoordinationTaskView {
  id: string;
  planId: string;
  title: string;
  status: CoordinationTaskStatus;
  assignee?: string;
  anchors: AnchorRefView[];
  dependsOn: string[];
  baseRevision: WorkspaceRevisionView;
}

interface ClaimView {
  id: string;
  holder: string;
  taskId?: string;
  capability: Capability;
  mode: ClaimMode;
  status: "Active" | "Released" | "Expired" | "Contended";
  anchors: AnchorRefView[];
  expiresAt: string;
}

interface ConflictView {
  severity: ConflictSeverity;
  summary: string;
  anchors: AnchorRefView[];
  overlapKinds: string[];
  blockingClaimIds: string[];
}

interface BlockerView {
  kind: "Dependency" | "ClaimConflict" | "ReviewRequired" | "StaleRevision";
  summary: string;
  relatedTaskId?: string;
  relatedArtifactId?: string;
}

interface ArtifactView {
  id: string;
  taskId: string;
  status: "Proposed" | "InReview" | "Approved" | "Rejected" | "Superseded" | "Merged";
  anchors: AnchorRefView[];
  baseRevision: WorkspaceRevisionView;
}

interface ClaimProposal {
  anchors: AnchorRefView[];
  capability: Capability;
  mode?: ClaimMode;
  taskId?: string;
}
```

Rules:

* the JS API should prefer plain data plus a small set of ergonomic methods
* methods should compose naturally inside one snippet
* the JS contract should reflect `prism-query`, not the CLI
* TypeScript is for composition; Prism is where semantic meaning should live
* high-value semantic operations should graduate into first-class `prism-query` methods instead of being reimplemented ad hoc in snippets

Examples of good semantic methods to expose over time:

* `prism.relatedFailures(nodeId)`
* `prism.blastRadius(nodeId)`
* `prism.validationRecipe(nodeId)`
* `prism.resumeTask(taskId)`
* `prism.readyTasks(planId)`
* `prism.conflicts(anchor)`
* `prism.simulateClaim(input)`

## 11.6 Recipes And Examples

The API reference should ship with concrete copy-pastable recipes such as:

* find a symbol and return its call graph plus lineage
* search for likely risky neighbors
* compare prior failures for one lineage
* summarize entrypoints in one package
* explain why a query was truncated and how to narrow it
* inspect blockers for a coordination task
* check who is actively editing a lineage before proposing a patch
* simulate an edit claim before acquiring it

Agents learn these surfaces best from examples. Recipes are not auxiliary documentation; they are part of the product surface.

## 11.7 Mutation Tools

The MCP server exposes explicit mutation tools alongside the read-only `prism_query`:

```text
prism_session { action: "start_task", input: { description: string, tags?: string[] } } -> { action: "start_task", task_id: string, session: SessionView }
prism_session { action: "configure", input: { ... } } -> { action: "configure", task_id?: string, session: SessionView }

prism_mutate { action: "outcome", input: { kind: OutcomeKind, anchors: AnchorRef[], summary: string, result?: OutcomeResult, evidence?: OutcomeEvidence[], task_id?: string } } -> EventMutationResult
prism_mutate { action: "memory", input: { action: "store", payload: { anchors: AnchorRef[], kind: MemoryKind, content: string, trust?: float, source?: MemorySource, metadata?: object }, task_id?: string } } -> MemoryMutationResult
prism_mutate { action: "infer_edge", input: { source: NodeId, target: NodeId, kind: EdgeKind, confidence: float, scope?: InferredEdgeScope, task_id?: string } } -> EdgeMutationResult
prism_mutate { action: "coordination", input: { kind: "plan_create" | "plan_update" | "task_create" | "task_update" | "handoff", payload: object, task_id?: string } } -> CoordinationMutationResult
prism_mutate { action: "claim", input: { action: "acquire" | "renew" | "release", payload: object, task_id?: string } } -> ClaimMutationResult
prism_mutate { action: "artifact", input: { action: "propose" | "supersede" | "review", payload: object, task_id?: string } } -> ArtifactMutationResult
prism_mutate { action: "test_ran" | "failure_observed" | "fix_validated", input: { ... } } -> EventMutationResult
prism_mutate { action: "curator_promote_edge" | "curator_promote_memory" | "curator_reject_proposal", input: { ... } } -> CuratorProposalDecisionResult
```

These fill in `EventMeta` automatically from the session context. The lower the friction, the more reliably agents will record outcomes.

Patch observation is not exposed as a mutation tool. PRISM detects file changes automatically via `ObservedChangeSet` and records them without agent involvement. Only outcomes that require semantic interpretation belong in the MCP mutation surface.

Rules:

* mutation tools are separate from `prism_query` to keep the query surface pure and predictable
* all mutations produce structured confirmation and the resulting authoritative state for the mutated object
* `prism_session { action: "start_task" }` creates a task record and makes it the active session task
* if the session has no active task, the first mutation auto-creates one before the mutation is recorded
* outcome events inherit the session's `TaskId` automatically when available
* explicit `task_id` arguments override the active session task without changing the session default
* inferred edges default to `SessionOnly` scope unless explicitly promoted
* the MCP surface exposes one coarse session mutation tool and one coarse general mutation tool
* `prism_mutate` owns shared plan, task, handoff, claim, artifact, outcome, memory, inference, and curator decision changes via tagged actions
* coordination actions inside `prism_mutate` must attribute mutations to the current `SessionId` and current or explicit `TaskId`
* coordination mutations must validate policy, dependency state, and base revision before they commit
* `prism_query` remains the primary read surface for plans, claims, blockers, conflicts, artifacts, and review queues
* the MCP server must support a `--no-coordination` mode where coordination is disabled end to end
* when coordination is entirely disabled for a workspace session, coordination state should not be loaded or persisted for that session
* coordination feature flags should gate both mutation tools and coordination read helpers so the advertised MCP surface matches what the server actually allows
* workflow, claim, and artifact capabilities should be independently enableable for gradual rollout

## 11.8 Convenience Query Tools

Optional convenience tools may exist later for high-frequency lookups:

But they are secondary and are not part of the preferred public surface. The programmable `prism_query` tool is the primary interface.

---

# 12. CLI (prism-cli)

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

# 13. Git Integration

Shelling out is acceptable in v1 for:

* `git blame`
* `git log -L`
* rename and move hints used as lineage evidence

`gix` is the preferred future direction once structured access or process overhead becomes a real constraint.

---

# 14. Performance Strategy

* arena-style allocation for graph-heavy structures where needed
* string interning with `smol_str`
* lazy file loading
* append-only event storage for history and outcomes
* derived projections built incrementally rather than recomputed from scratch when possible
* MCP sessions should reuse one loaded graph and one initialized runtime shim across many queries

---

# 15. Future Hooks

These are explicitly not v1 requirements, but the architecture should leave room for them:

* intent graph over docs, specs, ADRs, tests, and symbols
* runtime grounding via coverage, traces, logs, stack traces, and profiler samples
* first-class uncertainty tracking
* policy and invariant layers
* drift detection between implementation and intent
* additional language runtimes once the TypeScript surface is stable
* hosted HTTP MCP server for distributed coordination across different clones and machines; the graph stays local per clone, but lineage, memory, outcomes, and coordination state are shared through the server; the architecture should assume single-server multi-session from v1 so the step to networked multi-session is a transport change, not an architecture change

---

# 16. Build Order

Recommended sequence:

1. land `LineageId`, `EventId`, `TaskId`, and `AnchorRef` in `prism-ir`
2. make `prism-store` emit `ObservedChangeSet`
3. implement deterministic lineage resolution in `prism-history`
4. add structured `OutcomeEvent` logging in `prism-memory`
5. expose `lineage_of`, `related_failures`, `blast_radius`, and `resume_task` in `prism-query`
6. land `prism-js` as the stable JS/TS binding contract over `prism-query`
7. add `prism-mcp` with `prism_query` and `prism://api-reference`
8. land coordination identities, plan, task, claim, and artifact event types, and `WorkspaceRevision`
9. expose coordination reads and claim simulation through `prism-query` and `prism-js`
10. add coarse mutation actions under `prism_mutate` for coordination, claims, and artifacts
11. add derived projections such as co-change, hotspot, validation recipes, and drift detection

---

# Final Principle

PRISM should remember code as a living thing, not just a static graph.

The system model is:

* structure tells PRISM what exists now
* history tells PRISM what persisted through change
* outcome memory tells PRISM what happened when it changed
* coordination tells PRISM who is doing what, on which anchors, under which policy
* queries and agents turn that into evidence-backed action
