use prism_ir::{Language, LineageEvent, LineageId, NodeId, NodeKind, Span};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API_REFERENCE_URI: &str = "prism://api-reference";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolView {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    pub signature: String,
    pub file_path: Option<String>,
    pub span: Span,
    pub language: Language,
    pub lineage_id: Option<LineageId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RelationsView {
    pub outgoing_calls: Vec<NodeId>,
    pub incoming_calls: Vec<NodeId>,
    pub outgoing_imports: Vec<NodeId>,
    pub incoming_imports: Vec<NodeId>,
    pub outgoing_implements: Vec<NodeId>,
    pub incoming_implements: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineageView {
    pub lineage: LineageId,
    pub events: Vec<LineageEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryEnvelope {
    pub result: Value,
}

pub fn api_reference_markdown() -> &'static str {
    r#"# PRISM Query API

`prism_query` executes a TypeScript snippet against a live in-memory PRISM graph.

Rules:

- The snippet runs with a global `prism` object.
- Return the final value with `return ...`.
- The returned value must be JSON-serializable.
- `language` currently supports only `"ts"`.

## Global API

```ts
const prism: {
  symbol(query: string): SymbolView | null;
  symbols(query: string): SymbolView[];
  search(
    query: string,
    options?: { limit?: number; kind?: string; path?: string }
  ): SymbolView[];
  entrypoints(): SymbolView[];
  lineage(target: SymbolView | NodeId): LineageView | null;
};
```

## SymbolView

```ts
type SymbolView = {
  id: NodeId;
  name: string;
  kind: string;
  signature: string;
  file_path?: string;
  span: { start_line: number; start_col: number; end_line: number; end_col: number };
  language: string;
  lineage_id?: string;
  full(): string;
  relations(): RelationsView;
  callGraph(depth?: number): Subgraph;
  lineage(): LineageView | null;
};
```

## Examples

```ts
const sym = prism.symbol("main");
return {
  symbol: sym,
  lineage: sym?.lineage(),
  callers: sym?.relations().incoming_calls ?? [],
};
```

```ts
return prism.search("request", { limit: 5, kind: "function" });
```

## Current limitations

- The graph stays live in memory for the MCP session, but the JS runtime is recreated per query in this initial implementation.
- The query surface currently covers symbol lookup, search, entrypoints, relations, call graphs, source extraction, and lineage history.
- Memory recall, blast radius, and task replay are not exposed yet.
"#
}

pub fn runtime_prelude() -> &'static str {
    r#""use strict";

function __prismDecode(raw) {
  const envelope = JSON.parse(raw);
  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  return envelope.value;
}

function __prismHost(operation, args) {
  const payload = args === undefined ? "{}" : JSON.stringify(args);
  return __prismDecode(__prismHostCall(operation, payload));
}

function __prismNormalizeTarget(target) {
  if (target == null) {
    return null;
  }
  if (typeof target === "object" && target.id != null) {
    return target.id;
  }
  return target;
}

function __prismEnrichSymbol(raw) {
  if (raw == null) {
    return null;
  }

  return {
    ...raw,
    full() {
      return __prismHost("full", { id: this.id });
    },
    relations() {
      return __prismHost("relations", { id: this.id });
    },
    callGraph(depth = 3) {
      return __prismHost("callGraph", { id: this.id, depth });
    },
    lineage() {
      return __prismHost("lineage", { id: this.id });
    },
  };
}

function __prismEnrichSymbols(values) {
  return Array.isArray(values) ? values.map(__prismEnrichSymbol) : [];
}

globalThis.prism = Object.freeze({
  symbol(query) {
    return __prismEnrichSymbol(__prismHost("symbol", { query }));
  },
  symbols(query) {
    return __prismEnrichSymbols(__prismHost("symbols", { query }));
  },
  search(query, options = {}) {
    return __prismEnrichSymbols(
      __prismHost("search", Object.assign({ query }, options))
    );
  },
  entrypoints() {
    return __prismEnrichSymbols(__prismHost("entrypoints", {}));
  },
  lineage(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismHost("lineage", { id });
  },
});
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_reference_mentions_primary_tool() {
        let docs = api_reference_markdown();
        assert!(docs.contains("prism_query"));
        assert!(docs.contains("symbol(query: string)"));
    }

    #[test]
    fn prelude_exposes_global_prism() {
        let prelude = runtime_prelude();
        assert!(prelude.contains("globalThis.prism"));
        assert!(prelude.contains("__prismHostCall"));
    }
}
