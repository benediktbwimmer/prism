# PRISM — Rust Implementation Specification

## 0. Philosophy Recap

PRISM is a **deterministic, local-first perception layer** with optional probabilistic augmentation.

Core invariant:

* Base graph must be reproducible and fast
* Agent augmentation must be additive, never authoritative

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
```

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

---

## 2.2 Node

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

## 2.3 NodeKind

```rust
pub enum NodeKind {
    Workspace,
    Package,
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

## 2.4 Edge

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

## 2.5 EdgeKind

```rust
pub enum EdgeKind {
    Contains,
    Calls,
    References,
    Implements,
    Defines,
}
```

---

## 2.6 EdgeOrigin

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
* Disk-backed cache (sled or sqlite)

## 3.3 Graph Representation

```rust
pub struct Graph {
    pub nodes: HashMap<NodeId, Node>,
    pub edges: Vec<Edge>,
    pub adjacency: HashMap<NodeId, Vec<EdgeIndex>>,
}
```

---

## 3.4 Incremental Updates

Track per-file:

```rust
pub struct FileRecord {
    pub file_id: FileId,
    pub hash: u64,
    pub nodes: Vec<NodeId>,
}
```

On change:

* remove old nodes/edges
* reparse file
* insert new nodes/edges

---

# 4. Parsing Layer (prism-parser)

## 4.1 Tree-sitter Integration

* One parser per language
* Incremental parsing enabled

## 4.2 Trait Interface

```rust
pub trait LanguageAdapter {
    fn parse(&self, file: &str) -> ParseResult;
}
```

---

## 4.3 ParseResult

```rust
pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
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

* headings → nodes
* hierarchy via heading levels

---

# 7. JSON/YAML Adapters

* keys → nodes
* nesting → Contains edges

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

# 10. CLI (prism-cli)

## Commands

```
prism entrypoints
prism symbol <name>
prism call-graph <name> --depth 3
```

---

# 11. Git Integration (v1)

Shell out:

* git blame
* git log -L

---

# 12. Performance Strategy

* arena allocation for nodes
* string interning (smol_str)
* lazy file loading

---

# 13. Future Hooks

* rust-analyzer integration
* persistent graph store
* distributed indexing

---

# Final Principle

PRISM is not trying to understand everything.

It builds a **clean, partial map**, then lets intelligence fill in the gaps.
