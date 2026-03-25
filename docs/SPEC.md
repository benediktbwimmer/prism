# PRISM — Rust Implementation Specification

## 0. Philosophy Recap

PRISM is a **deterministic, local-first perception layer** with optional probabilistic augmentation.

Core invariants:

* Base graph must be reproducible and fast
* Agent augmentation must be additive, never authoritative
* Memory is composable and anchored to the perception graph

---

# 1. Crate Architecture

Workspace layout:

```
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
    prism-query/
    prism-cli/
    prism-agent/
    prism-memory/
```

### Dependency Direction

```
prism-cli
  → prism-query → prism-store → prism-ir
  → prism-agent → prism-ir
  → prism-memory → prism-ir
  → prism-parser → prism-ir
      → prism-lang-rust
      → prism-lang-markdown
      → prism-lang-json
      → prism-lang-yaml
```

Critical rule: `prism-memory` depends on `prism-ir` (for `NodeId`, `NodeKind`) but does **not** depend on `prism-store`, `prism-query`, or `prism-parser`. Memory knows about node identities but nothing about how the graph is built, stored, or queried.

Current crate roles:

* `prism-core` is the orchestration layer: workspace discovery, package/member-crate identity, incremental indexing, and deferred edge resolution
* `prism-store` owns persistence boundaries and backend implementations; the in-memory `Graph` is the runtime model, while SQLite is the durable cache in v1
* `prism-query` is the read/query layer over an already-built graph, not the place where indexing or persistence rules live

---

# 2. Core IR (prism-ir)

## 2.1 NodeId (CRITICAL)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub crate_name: SmolStr,
    pub path: SmolStr,      // canonical path: crate::module::symbol
    pub kind: NodeKind,
}
```

Rules:

* Must be stable across runs
* Must survive file movement when possible
* Never rely on byte offsets
* `crate_name` is the owning package/crate namespace, derived from the Cargo package for Rust and reused for co-located docs/config artifacts
* For non-Rust languages, `path` is still package-relative and semantic, not workspace-layout-derived
* Identity is semantic: package/module/symbol path first, file location second
* `impl` nodes must use a canonical path shape: `crate::module::Type::impl::Trait` or `crate::module::Type::impl`

When a node survives a rename, move, split, or reparenting, PRISM should prefer emitting a re-anchoring event over treating it as pure removal.

## 2.2 GraphChange

```rust
pub enum GraphChange {
    Added(NodeId),
    Removed(NodeId),
    Modified(NodeId),
    Reanchored { old: NodeId, new: NodeId },
}
```

`GraphChange::Reanchored` is the critical contract for memory. Invalidating on every identity churn is acceptable for a toy graph, but not for attached recall that users expect to survive code motion.

Implementation note:

* Adapters may emit stable parse fingerprints as an internal re-anchoring aid
* Fingerprints are not public identity and must never replace `NodeId`
* Fingerprints should prefer syntax-local shape over file paths or byte offsets
* They exist to answer "is this probably the same node after a rename or move?" when `NodeId` itself has changed

---

## 2.3 Node

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

---

## 2.4 FileId

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileId(pub u32); // Opaque file identity; v1 may be an arena/index-backed handle
```

Rules:

* `FileId` is intentionally opaque outside the store/indexer boundary
* Path-based identity belongs in `NodeId`, not in `FileId`

---

## 2.5 NodeKind

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

---

## 2.6 Edge

```rust
pub struct Edge {
    pub kind: EdgeKind,
    pub source: NodeId,
    pub target: NodeId,
    pub origin: EdgeOrigin,
    pub confidence: f32,
}
```

---

## 2.7 EdgeKind

```rust
pub enum EdgeKind {
    Contains,
    Calls,
    References,
    Implements,
    Defines,
    Imports,
    // Future:
    // DependsOn,  // crate/package-level
}
```

---

## 2.8 EdgeOrigin

```rust
pub enum EdgeOrigin {
    Static,      // parser-derived
    Inferred,    // agent-derived
}
```

---

# 3. Storage Layer (prism-store)

## 3.1 Requirements

* Fast read-heavy access
* Incremental updates
* Low memory footprint

## 3.2 Approach

Hybrid:

* In-memory graph (primary)
* Disk-backed cache (`sqlite` in v1)

The storage abstraction should be narrow and persistence-focused:

* runtime graph traversal stays in `Graph`
* persistence lives behind a `Store` boundary
* `MemoryStore` is for tests/dev
* `SqliteStore` is the default durable backend

## 3.3 Graph Representation

```rust
pub struct Graph {
    pub nodes: HashMap<NodeId, Node>,
    pub edges: Vec<Edge>,
    pub adjacency: HashMap<NodeId, Vec<EdgeIndex>>,
}
```

Note: `EdgeIndex` is a `usize` into the `edges` vec. This is acceptable for v1 if updates rebuild adjacency, but if edge churn or partial mutation grows, migrate to a `SlotMap`/arena so stable edge handles do not depend on vec layout.

---

## 3.4 Incremental Updates

Track per-file:

```rust
pub struct FileRecord {
    pub file_id: FileId,
    pub hash: u64,
    pub nodes: Vec<NodeId>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
}
```

On change:

* remove old nodes/edges
* reparse file
* insert new nodes/edges
* persist file-local unresolved refs
* rebuild derived edges (`Calls`, `Imports`, `Implements`)
* emit `GraphChange` events for add/remove/modify/re-anchor

---

# 4. Parsing Layer (prism-parser)

## 4.1 Tree-sitter Integration

* One parser per language
* File-level incremental indexing in v1
* Tree reuse/incremental tree-sitter edits are a future optimization, not a current guarantee

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

---

## 4.3 ParseResult

```rust
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
}
```

---

# 5. Rust Adapter (prism-lang-rust)

## 5.1 Responsibilities

* Extract symbols
* Build containment hierarchy
* Detect function calls (heuristic)

---

## 5.2 Symbol Extraction

From AST:

* function_item
* struct_item
* enum_item
* trait_item
* impl_item

Canonicalization rules:

* root module identity is derived from the owning package, not the workspace checkout path
* inline modules extend the containing module path
* `impl Foo for Bar` becomes `...::Bar::impl::Foo`
* inherent impls become `...::Bar::impl`

---

## 5.3 Call Detection (v1)

Detect:

* call_expression
* method_call_expression

Resolution strategy:

* direct name match
* scoped lookup within module

---

## 5.4 Skeleton Extraction

Defined in `prism-ir` so both language adapters and `prism-query` can reference it.

Walk function body:

* collect top-level call expressions
* ignore literals/control flow noise

Output:

```rust
pub struct Skeleton {
    pub calls: Vec<NodeId>,
}
```

---

# 6. Markdown Adapter

* one `Document` node per file
* headings → nodes
* containment is `Package -> Document -> Heading -> Heading`

---

# 7. JSON/YAML Adapters

* one `Document` node per file
* keys → nodes
* containment is `Package -> Document -> Key -> Key`

---

# 8. Query Engine (prism-query)

## 8.1 Entry API

```rust
pub struct Prism {
    graph: Arc<Graph>,
}
```

---

## 8.2 Symbol Handle

```rust
pub struct Symbol<'a> {
    prism: &'a Prism,
    id: NodeId,
}
```

---

## 8.3 Core Methods

```rust
impl Prism {
    pub fn symbol(&self, query: &str) -> Vec<Symbol<'_>>; // best-match / exact-biased resolution
    pub fn search(&self, query: &str, limit: usize, kind: Option<NodeKind>, path: Option<&str>) -> Vec<Symbol<'_>>; // broader ranked lookup with optional filters
}

impl<'a> Symbol<'a> {
    pub fn name(&self) -> &str;
    pub fn signature(&self) -> String;
    pub fn skeleton(&self) -> Skeleton;
    pub fn full(&self) -> String;

    pub fn call_graph(&self, depth: usize) -> Subgraph;
}
```

---

# 9. Agent Layer (prism-agent)

## 9.1 Purpose

* augment graph
* resolve ambiguity

---

## 9.2 Interface

```rust
pub trait Agent {
    fn infer_edges(&self, context: AgentContext) -> Vec<Edge>;
}
```

---

## 9.3 AgentContext

```rust
pub struct AgentContext {
    pub symbol: NodeId,
    pub known_edges: Vec<Edge>,
    pub unresolved_calls: Vec<String>,
}
```

---

## 9.4 Output Contract

* must include confidence
* must not overwrite static edges

---

# 10. Memory Layer (prism-memory)

## 10.1 Purpose

Composable, pluggable memory system anchored to PRISM's perception graph via `NodeId`. Memory is **not** part of the deterministic graph — it is a separate layer that attaches to it.

## 10.2 Architecture

```
┌─────────────────────────────────────────────────┐
│                MemoryComposite                  │
│  (merges + deduplicates results from modules)   │
├─────────────┬──────────────┬────────────────────┤
│  Episodic   │  Structural  │     Semantic       │
│  (events)   │  (patterns)  │     (vectors)      │
└─────────────┴──────────────┴────────────────────┘
         │              │               │
         └──────────────┴───────────────┘
                        │
                    anchored to
                        │
                    NodeId (prism-ir)
```

## 10.3 Core Trait

```rust
/// A single memory module that can store and retrieve memories anchored to nodes.
pub trait MemoryModule: Send + Sync {
    fn name(&self) -> &'static str;

    /// Store a memory, anchored to zero or more NodeIds.
    fn store(&self, entry: MemoryEntry) -> Result<MemoryId>;

    /// Recall memories relevant to a query context.
    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>>;

    /// Apply graph changes so memories can re-anchor instead of being dropped.
    fn apply_changes(&self, changes: &[GraphChange]) -> Result<()>;
}
```

## 10.4 MemoryEntry

```rust
pub struct MemoryEntry {
    pub anchors: Vec<NodeId>,
    pub kind: MemoryKind,
    pub content: String,
    pub metadata: serde_json::Value,
    pub created_at: Timestamp,
    pub source: MemorySource,
    pub trust: f32, // normalized to [0.0, 1.0]
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
* `trust` is explicit so recall can down-rank fuzzy or speculative memories
* At equal score, composite recall should prefer higher-trust memories, then `User`, then `System`, then `Agent`

## 10.5 RecallQuery

```rust
pub struct RecallQuery {
    /// Node context — what are we looking at right now?
    pub focus: Vec<NodeId>,
    /// Free-text query (for semantic search modules)
    pub text: Option<String>,
    /// Maximum results
    pub limit: usize,
    /// Optional memory kinds to include
    pub kinds: Option<Vec<MemoryKind>>,
    /// Optional lower bound on memory creation time
    pub since: Option<Timestamp>,
}
```

Anchor semantics:

* `anchors: Vec<NodeId>` is disjunctive in v1
* recall matches when **any** focus node overlaps
* larger overlap should score higher than a single weak overlap
* multi-anchor entries stay useful for structural memory later without requiring an all-or-nothing match rule today

## 10.6 ScoredMemory

```rust
pub struct ScoredMemory {
    pub id: MemoryId,
    pub entry: MemoryEntry,
    pub score: f32, // normalized module-local score after weighting
    pub source_module: String,
    pub explanation: Option<String>,
}
```

## 10.7 MemoryComposite

```rust
pub struct MemoryComposite {
    modules: Vec<(Box<dyn MemoryModule>, f32)>, // module + weight
}

impl MemoryModule for MemoryComposite {
    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        // query all modules in parallel
        // require each module to return scores normalized to [0.0, 1.0]
        // clamp + weight scores by module weight
        // deduplicate by MemoryId
        // sort by final score
        // truncate to query.limit
    }
}
```

Normalization rule:

* module scores are not assumed comparable by default
* each module must normalize into `[0.0, 1.0]` before composite weighting
* composite ranking is undefined if a module returns raw cosine similarity, recency decay, or heuristic confidence without normalization

## 10.8 Memory Module Types

### Episodic Memory — "what happened here before"

* Stores discrete events anchored to `NodeId`s
* Examples: "Function X changed in commit abc123", "Bug report mentioned null handling in method Y", "User noted that struct Z is performance sensitive"
* Storage: append-only log, indexed by `NodeId`
* Recall: exact or overlap-based anchor match on `focus` nodes, then recency/provenance tie-breaks
* **v1 target — build this first**
* Keep v1 extremely literal; do not infer new world facts from episodic memory

### Structural Memory — "patterns about this code"

* Stores learned invariants and relationships
* Examples: "this module must be updated with module X", "changes to this struct require migration"
* Storage: rule-like entries anchored to `NodeId`s or `NodeKind`s
* Recall: match on `focus` nodes + walk graph neighbors
* **v2 — where outcome-based learning lives**

### Semantic Memory — "fuzzy recall by meaning"

* Stores embedded text chunks (classic RAG)
* Examples: documentation snippets, past conversations, design decisions
* Storage: vector DB (e.g. `hnsw` in-process)
* Recall: embed `text` query, nearest-neighbor search
* **v2 — escape hatch for unstructured knowledge**
* Must help retrieval, not redefine the authoritative world model

## 10.9 Integration with PRISM Graph

PRISM does not depend on memory. Integration points:

1. **NodeId as anchor** — all memories attach to stable node identities
2. **Change events** — when nodes are added/removed/modified/re-anchored during incremental updates, the store emits `GraphChange` events that memory modules can subscribe to
3. **Optional annotation slot** — `prism-store` may expose a `HashMap<NodeId, Vec<Annotation>>` for lightweight metadata attachment without requiring the full memory system

---

# 11. CLI (prism-cli)

## Commands

```
prism entrypoints
prism symbol <name>
prism search <query> [--limit 20] [--kind <kind>] [--path <fragment>]
prism call-graph <name> --depth 3
prism memory recall <symbol> [--text "query"]
prism memory store <symbol> --content "note"
```

---

# 12. Git Integration (v1)

Shell out:

* git blame
* git log -L

V1 note:

* shelling out is acceptable for speed of implementation
* `gix` is the preferred future direction once structured access or process overhead becomes a real constraint

---

# 13. Performance Strategy

* arena allocation for nodes
* string interning (smol_str)
* lazy file loading

---

# 14. Future Hooks

* rust-analyzer integration
* persistent graph store
* distributed indexing

---

# 15. Build Order

```
1. prism-ir          → types, NodeId, NodeKind, Edge, Skeleton
2. prism-parser      → LanguageAdapter trait + ParseResult
3. prism-lang-rust   → first real adapter (tree-sitter-rust)
4. prism-store       → Graph + incremental updates + change events
5. prism-query       → Prism entry point + Symbol handle
6. prism-cli         → wire it all up (vertical slice)
7. prism-lang-{md,json,yaml} → additional adapters
8. prism-memory      → MemoryModule trait + EpisodicMemory + Composite
9. prism-agent       → augmentation layer
```

---

# Final Principle

PRISM is not trying to understand everything.

It builds a **clean, partial map**, then lets intelligence fill in the gaps — and remembers what it learned.
