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
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryDiagnostic {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

pub fn api_reference_markdown() -> &'static str {
    r#"# PRISM Query API

`prism_query` executes a TypeScript snippet against a live in-memory PRISM graph.

## Mental model

Treat this like a repo-specific read-only query shell.

- TypeScript is for composition.
- Prism is where semantic meaning should live.
- Return the final value with `return ...`.
- The returned value must be JSON-serializable.
- `language` currently supports only `"ts"`.
- `prism_query` is read-only in this implementation.

## Result shape

```ts
interface QueryResult {
  result: unknown;
  diagnostics: QueryDiagnostic[];
}

interface QueryDiagnostic {
  code: string;
  message: string;
  data?: Record<string, unknown>;
}
```

Diagnostics are how the server tells you a query was ambiguous, truncated, or capped.

## Type surface

```ts
type NodeId = {
  crate_name: string;
  path: string;
  kind: string;
};

type SearchOptions = {
  limit?: number;
  kind?: string;
  path?: string;
};

type PrismApi = {
  symbol(query: string): SymbolView | null;
  symbols(query: string): SymbolView[];
  search(query: string, options?: SearchOptions): SymbolView[];
  entrypoints(): SymbolView[];
  lineage(target: SymbolView | NodeId): LineageView | null;
  diagnostics(): QueryDiagnostic[];
};

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

type RelationsView = {
  outgoing_calls: NodeId[];
  incoming_calls: NodeId[];
  outgoing_imports: NodeId[];
  incoming_imports: NodeId[];
  outgoing_implements: NodeId[];
  incoming_implements: NodeId[];
};

type LineageView = {
  lineage: string;
  events: unknown[];
};
```

## Limits and determinism

- Search results are capped.
- Call graph depth is capped.
- Results are deterministically ordered by Prism before they reach the JS layer.
- The graph stays live in memory for the MCP session, but the JS runtime is recreated per query in this initial implementation.

## Recipes

### 1. Find a symbol and show call graph plus lineage

```ts
const sym = prism.symbol("main");
return {
  symbol: sym,
  callGraph: sym?.callGraph(2),
  lineage: sym?.lineage(),
};
```

### 2. Search only functions

```ts
return prism.search("request", { limit: 5, kind: "function" });
```

### 3. Find callers of the best symbol match

```ts
const sym = prism.symbol("handle_request");
return {
  symbol: sym,
  callers: sym?.relations().incoming_calls ?? [],
};
```

### 4. Fall back from exact-ish lookup to search

```ts
const sym = prism.symbol("RequestContext") ?? prism.search("RequestContext", { limit: 1 })[0];
return sym;
```

### 5. Summarize entrypoints

```ts
return prism.entrypoints().map((sym) => ({
  path: sym.id.path,
  file: sym.file_path,
}));
```

### 6. Pull source plus relations in one round-trip

```ts
const sym = prism.symbol("main");
return {
  symbol: sym,
  source: sym?.full(),
  relations: sym?.relations(),
};
```

### 7. Inspect diagnostics after an ambiguous lookup

```ts
const sym = prism.symbol("parse");
return {
  symbol: sym,
  diagnostics: prism.diagnostics(),
};
```

### 8. Narrow by path fragment

```ts
return prism.search("config", {
  kind: "struct",
  path: "src/settings",
  limit: 10,
});
```

### 9. Compare two related symbols

```ts
const left = prism.symbol("handle_request");
const right = prism.symbol("handle_response");
return {
  left,
  right,
  sharedCallers:
    left && right
      ? left
          .relations()
          .incoming_calls
          .filter((caller) =>
            right.relations().incoming_calls.some((other) => other.path === caller.path)
          )
      : [],
};
```

### 10. Return both data and repair hints

```ts
const results = prism.search("parse", { limit: 1000 });
return {
  results,
  diagnostics: prism.diagnostics(),
};
```

## Current implementation surface

- Available now: symbol lookup, search, entrypoints, relations, call graphs, source extraction, and lineage history.
- Not exposed yet: memory recall, related failures, blast radius, validation recipes, and task replay.
- Keep query logic small. If you find yourself reconstructing semantics from raw low-level fields every time, that method probably belongs in Prism itself.
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
  diagnostics() {
    return __prismHost("diagnostics", {});
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
        assert!(docs.contains("type PrismApi"));
        assert!(docs.contains("### 10. Return both data and repair hints"));
    }

    #[test]
    fn prelude_exposes_global_prism() {
        let prelude = runtime_prelude();
        assert!(prelude.contains("globalThis.prism"));
        assert!(prelude.contains("__prismHostCall"));
    }
}
