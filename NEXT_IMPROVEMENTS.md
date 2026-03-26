# Next Improvements

This document captures the next set of PRISM MCP improvements that would materially improve real agent workflows.

The guiding principle is:

- Keep TypeScript as the composition language.
- Keep PRISM as the semantic engine.
- Improve the semantic helpers, discovery affordances, and resource ergonomics so agents need less shell access and less source spelunking.

## Priority 1

Make the interface self-describing and hard to misuse.

- Ensure every documented resource actually resolves from every MCP client path, especially `prism://tool-schemas` and `prism://schema/tool/{toolName}`.
- Add one canonical capabilities resource that lists query methods, resources, enabled features, and version/build info.
- Make every query diagnostic include a concrete next action.
  Example: “lookup was ambiguous; try `path: ...` or `kind: ...`.”
- Add stable examples to schema and resource payloads, not just field types.

Why this matters:

- This removes the remaining need to inspect Rust types or guess payload shapes.

## Priority 2

Add a first-class discovery layer above raw query composition.

- Extend the new owner-biased workflow with:
  - `prism.nextReads(target)`
  - `prism.entrypointsFor(target)`
  - `prism.whereUsed(target, { mode: "behavioral" | "direct" })`
- Add a discovery bundle resource for a symbol or spec that includes:
  - direct links
  - owner paths
  - tests
  - recent failures
  - memory
  - “why these results”
- Make symbol and search resources default to that richer bundle shape.

Why this matters:

- Most agent work is not raw graph traversal.
- The common question is “what should I read next?”

## Priority 3

Absorb repeated agent query patterns into semantic helpers.

These are the patterns that still get reconstructed repeatedly:

- “what implements this in practice?”
- “what code path serves this behavior?”
- “what changed here recently and what failed?”
- “what test should I run?”
- “what should I read before editing this?”

Recommended helpers:

- `prism.editContext(target)`
- `prism.readContext(target)`
- `prism.validationContext(target)`
- `prism.recentChangeContext(target)`

Why this matters:

- TypeScript should remain the substrate for composition.
- Repeated high-value intent should not need to be hand-written every time.

## Priority 4

Improve resource ergonomics for zero-shell workflows.

- Search resources should support all important query options in the URI and echo the applied options back.
- Resource payloads should include small default excerpts for every suggested symbol.
- Related resources should be usefulness-ranked, not merely structurally adjacent.
- Add explicit `suggestedQueries` to payloads.
  Example: exact `prism_query` snippets or narrower resource URIs.

Why this matters:

- This is one of the fastest ways to reduce `rg`, `sed`, and manual file inspection.

## Priority 5

Tighten freshness and trust signals.

- Mark whether a result came from direct graph edges, inferred heuristics, memory, or outcome history.
- Surface indexing freshness and graph revision more prominently.
- Add confidence labels on owner suggestions and drift explanations.

Why this matters:

- Agents need to understand not only what PRISM thinks, but how hard to trust it.

## Recommended Milestone

If only one near-term milestone should be prioritized, it should be:

1. Add `prism.readContext(target)` and `prism.editContext(target)`.
2. Back them with richer symbol and search resource payloads.
3. Make diagnostics suggest those helpers automatically.

Why this milestone:

- It would improve real agent workflows more than another batch of low-level primitives.
- It directly targets the “what should I read before I act?” loop that dominates real usage.
