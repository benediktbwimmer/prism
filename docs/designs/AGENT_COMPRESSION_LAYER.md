# PRISM Agent Compression Layer

## Summary

PRISM keeps a rich semantic/query engine, but stops exposing that richness as the default agent
interaction model.

The architecture has two layers:

- Semantic core / IR:
  - `prism_code`
  - lineage, bundles, semantic ranking, graph-backed reasoning
- Agent surface / default ABI:
  - compact, staged, handle-based MCP tools optimized for the next likely agent action

Design principle:

**Return the minimum sufficient answer for the next likely agent action.**

This milestone optimizes for token reduction and first-hop success, not for preserving the current
query-first UX.

## Why This Is Needed

The current query-first surface is semantically useful, but it encourages too many small
exploratory turns for coding agents operating under real token budgets.

The core semantic engine is still the advantage. The default agent ABI is the problem:

- prompts encourage exploratory composition where the real task is often just `find -> open -> patch`
- repeated option blobs and repeated target metadata consume tokens
- rich query outputs over-fetch for the next likely action
- lack of first-class handles forces rediscovery instead of carrying compact server-side state

The goal is not to remove the expressive query model. The goal is to keep it as the brain and stop
using it as the default mouth.

## Target Default Agent Path

The default agent path becomes:

1. `prism_locate`
2. `prism_gather` when the need is exact text/config/schema slices rather than a semantic symbol
3. `prism_open`
4. `prism_workset`
5. `prism_expand` only when needed
6. ad hoc read-only `prism_code` only when the compact surface cannot express the task

`prism_code` remains the semantic programmable escape hatch.

## Compact Primary Tools

### `prism_locate`

Purpose:
- first-hop target selection

Input:
- `query`
- optional `path`
- optional `glob`
- optional `task_intent`
- optional `limit`
- optional `include_top_preview`

Output:
- `handle`
- `kind`
- `path`
- `name`
- `why_short`
- optional `file_path`
- optional `top_preview`

Default behavior:
- biased toward editable implementation targets
- returns the top 1 to 3 compact candidates only

### `prism_open`

Purpose:
- bounded inspection

Input:
- `handle`
- `mode`

Modes:
- `focus`
- `edit`
- `raw`

Output:
- `handle`
- `file_path`
- `start_line`
- `end_line`
- `text`
- optional `related_handles`

Rules:
- `edit` is narrow and returns the edit slice only
- it does not silently become a mini-bundle

### `prism_gather`

Purpose:
- gather 1 to 3 exact text/file-fragment slices in one hop

Input:
- `query`
- optional `path`
- optional `glob`
- optional `limit`

Output:
- `matches`
- optional `narrowing_hint`

Rules:
- use this for config keys, schema fields, telemetry counters, and other exact text anchors
- gathered matches still return stable handles so the next move can stay on `prism_open` or `prism_workset`

### `prism_workset`

Purpose:
- one-step task context after a likely target is known

Input:
- `handle` or `query`

Output:
- `primary`
- up to 3 `supporting_reads`
- up to 2 `likely_tests`
- one short `why`

Rules:
- keep a hard compactness budget
- do not return nested diagnostics, discovery payloads, or rich extras
- for spec/doc targets, bias `supporting_reads` toward drift follow-ups and let `why` summarize the leading gap
- if it starts to bloat, split it instead of expanding it

### `prism_expand`

Purpose:
- explicit depth-on-demand

Input:
- `handle`
- `kind`
- optional `include_top_preview` for `neighbors`

`kind` values:
- `diagnostics`
- `lineage`
- `neighbors`
- `diff`
- `validation`
- `drift`

Output:
- only the requested expansion
- optional `top_preview` when a bounded neighbor preview is requested

## Handles

Handles are first-class session objects, not thin wrappers around existing symbol JSON.

Requirements:
- every compact tool returns stable opaque handles
- follow-up tools take handles instead of large symbol objects
- handles remain stable within a session
- handle resolution is cheap
- stale handles return a compact remap or one short recovery hint, not a large error envelope
- handles may refer to semantic symbols or lightweight exact text/file fragments

Implementation target:
- handle storage should live in session-local MCP state
- resolution should be cheap in the query runtime

## Default Output Contract

Compact tools omit anything not needed for the next move.

Do not include by default:
- signatures
- full excerpts beyond the requested slice
- lineage IDs
- large diagnostics arrays
- repeated file paths or repo-root metadata
- multi-level nested envelopes

Default result behavior:
- empty results return an empty list or compact status, never ambiguous `null`
- ambiguity returns a compact candidate set plus one narrowing hint
- truncation returns `truncated: true` plus one `next_action`
- rich detail is only available through `prism_expand` or `prism_code`

## Ranking Is Core Milestone Work

Ranking is not secondary. It is core milestone work.

If `prism_locate` does not reliably return the right implementation target in top-1 or top-3,
agents will call it repeatedly and the compression layer will not reduce tokens.

Defaults:
- broad nouns prefer implementation targets
- modules, containers, tests, examples, and docs are demoted unless requested
- behavioral-owner evidence is merged before ranking
- top-1 and top-3 quality are explicit quality targets

## Migration Sequence

1. Add session handle storage and remapping.
2. Implement `prism_locate` and `prism_open` first as thin compact wrappers over existing
   search/file machinery.
3. Add telemetry for payload bytes and dedicated-tool counts before changing prompts.
4. Add `prism_workset` with a hard compactness budget.
5. Switch benchmark prompts from query-first to compact-tool-first.
6. Keep the compact-tool path primary and make `prism_code` the explicit programmable fallback.

## Benchmark And Prompt Guidance

The benchmark and agent guidance should prefer:

1. `prism_locate`
2. `prism_gather` for exact text/config/schema reads
3. `prism_open`
4. `prism_workset`
5. `prism_expand` only when needed
6. `prism_code` only as an explicit fallback

Guidance changes:
- carry handles forward
- avoid rediscovery by text when a handle already exists
- stop recommending `prism_code` or bundle-first discovery as the default path

## Telemetry And Success Criteria

Track separately:
- dedicated compact PRISM tool calls
- `prism_code` calls
- shell read calls
- repeated shell reads
- returned PRISM payload size by tool type

Milestone success criteria:
- lower prompt tokens than the current PRISM arm
- lower completion tokens than the current PRISM arm
- lower PRISM payload size per successful task
- lower `prism_code` usage rate in the benchmark arm

## Test Focus

### Contract tests

- `prism_locate` returns compact fields and stable handles
- `prism_gather` returns 1 to 3 bounded exact-text slices with reusable handles
- `prism_open` returns bounded slices only
- `prism_workset` stays compact and task-shaped
- `prism_expand` returns only the requested expansion
- handles remain stable within a session and remap cleanly when needed

### Ranking tests

Use a fixed dogfood query set for top-1 and top-3 quality:
- `session`
- `helper`
- `status`
- `runtime`
- `validation`
- one benchmark-repo-local symbol query

### Payload-size tests

- snapshot serialized response sizes for compact tools
- compare compact outputs against current bundle outputs for equivalent scenarios
- guard against regressions into verbose nested envelopes

## Scope

This is not a rejection of the semantic query model.

It is a productization step:

- keep PRISM's mind rich
- make its first words short
