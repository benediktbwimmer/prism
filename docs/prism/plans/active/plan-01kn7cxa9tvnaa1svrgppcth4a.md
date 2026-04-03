# Investigate and reduce the next observed PRISM runtime and MCP performance hotspots from live daemon and MCP logs by tackling compact-task latency, refresh-lock admission stalls, oversized co-change work, startup/recovery fixed costs, and noisy bridge transport behavior, while preserving correctness and proving improvements with log-backed validation.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:584fa378ca5c433140e891331fd37fbabb16d9f480a8b4d94e4f9bb10542e308`
- Source logical timestamp: `unknown`
- Source snapshot: `7 nodes, 10 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn7cxa9tvnaa1svrgppcth4a`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `7`
- Edges: `10`

## Goal

Investigate and reduce the next observed PRISM runtime and MCP performance hotspots from live daemon and MCP logs by tackling compact-task latency, refresh-lock admission stalls, oversized co-change work, startup/recovery fixed costs, and noisy bridge transport behavior, while preserving correctness and proving improvements with log-backed validation.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn7cxa9tvnaa1svrgppcth4a.jsonl`

## Root Nodes

- `coord-task:01kn7cxtgg1r28ntzg41pgzt2r`

## Nodes

### Turn the current log audit into a reproducible hotspot baseline

- Node id: `coord-task:01kn7cxtgg1r28ntzg41pgzt2r`
- Kind: `investigate`
- Status: `completed`
- Summary: Captured the April 2 live daemon and MCP hotspot baseline in docs/PERFORMANCE_MILESTONE.md with current runtime status, startup and refresh timings, slow-call leaders, recurring warning classes, and explicit optimization hypotheses for prism_task_brief, refresh-lock admission, co-change fanout, startup fixed costs, and bridge transport noise.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- There is a documented baseline for recent daemon startup, incremental refresh timings, slow MCP call classes, slow internal query classes, and recurring warning families from live logs or PRISM query surfaces. [any]
- Each target hotspot is tied to a named subsystem and an intended optimization hypothesis, including prism_task_brief, refresh-lock admission, co-change sampling, startup or recovery costs, and transport or bridge noise. [any]

#### Validation Refs

- `log:mcp-hotspot-baseline`
- `log:runtime-hotspot-baseline`

### Slim the prism_task_brief critical path and degrade gracefully under stale runtime state

- Node id: `coord-task:01kn7cy67chqrcz18kvnpk7gmm`
- Kind: `edit`
- Status: `completed`
- Summary: Attack the historical compact-task latency hotspot by separating cheap task-summary data from expensive refresh or enrichment work, so task briefs stay responsive even when runtime sync is deferred or stale.
- Priority: `2`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Typical prism_task_brief calls no longer wait on heavyweight runtime refresh or broad enrichment to return a minimally useful brief. [any]
- Slow-call traces and query traces for prism_task_brief clearly attribute any remaining expensive phases instead of collapsing into opaque compact handler time. [any]

#### Validation Refs

- `bench:prism-task-brief-latency`
- `log:task-brief-traces`

### Remove long refresh-lock stalls from coordination mutation admission

- Node id: `coord-task:01kn7cym8fe0dxg3ehd19fp9ge`
- Kind: `edit`
- Status: `completed`
- Summary: Redesign the coordination mutation admission path so refresh activity cannot strand mutateCoordination callers behind long-held locks and then fail after minutes of waiting.
- Priority: `3`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Coordination mutations either proceed on a lighter-weight snapshot path or fail fast with structured retry guidance instead of blocking behind refresh_lock for long intervals. [any]
- Live slow MCP call history no longer shows multi-second or minute-scale mutateCoordination waits attributed primarily to refresh lock admission. [any]

#### Validation Refs

- `bench:coordination-mutation-admission`
- `log:refresh-lock-admission`

### Bound oversized co-change work during incremental indexing

- Node id: `coord-task:01kn7cywwqynsppzxgn5rqawmc`
- Kind: `edit`
- Status: `completed`
- Summary: Reduce the recurring oversized co-change sampling warnings and the underlying indexing cost by capping or approximating symbol-level co-change work when change sets or hot files explode fanout.
- Priority: `4`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Incremental refreshes with large test or broad refactor files no longer emit repeated co-change oversize warnings under ordinary developer workflows. [any]
- Indexer timing for small edit batches no longer spends disproportionate time or fanout volume on co-change delta generation relative to parse and persist work. [any]

#### Validation Refs

- `bench:indexer-cochange-hot-path`
- `log:cochange-oversize-warnings`

### Trim daemon startup and recovery fixed costs

- Node id: `coord-task:01kn7cz3g0ks84qsqz565pfen9`
- Kind: `edit`
- Status: `completed`
- Summary: Writer-open full projection co-change prune is now a one-time backfill guarded by metadata. Live steady-state daemon restarts dropped worktree writer-open prune_ms from ~342ms to 0ms and full indexer prep from ~814ms to ~730ms.
- Priority: `5`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Daemon startup or recovery traces show materially reduced fixed-cost time in one or more of store open, prune, graph load, coordination load, projection restore, or replay volume. [any]
- The optimization preserves correct runtime hydration and does not reintroduce stale projections or recovery divergence. [any]

#### Validation Refs

- `bench:daemon-startup-timeline`
- `log:runtime-startup-timeline`

### Reduce noisy bridge transport warnings and make bridge lifecycle state clearer

- Node id: `coord-task:01kn7cz9ysdjmzcdr8sqtk388j`
- Kind: `edit`
- Status: `completed`
- Summary: Default MCP logging now suppresses generic rmcp transport warnings while keeping rmcp::service warnings visible. Fresh restart windows no longer emit new benign SSE decode or cancelled-stream warnings from the new binaries.
- Priority: `6`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Expected bridge reconnect or restart churn no longer produces repeated high-severity transport warnings that look like data corruption or service faults. [any]
- Runtime status or logs make it easier to distinguish healthy active bridges from stale or idle long-lived bridge sessions. [any]

#### Validation Refs

- `log:bridge-lifecycle-visibility`
- `log:bridge-transport-warnings`

### Validate log-backed runtime and MCP performance improvements end to end

- Node id: `coord-task:01kn7czn5h7s8396kc9a2jcgpf`
- Kind: `validate`
- Status: `completed`
- Summary: Validated the hotspot improvements end to end. cargo test -p prism-store --quiet and cargo test -p prism-mcp --quiet passed; cargo test --workspace --quiet hit only the two known flakes and both passed in isolated reruns; cargo build --release -p prism-cli -p prism-mcp, prism-cli mcp restart --internal-developer, prism-cli mcp status, and prism-cli mcp health all passed. Live logs confirm task_brief latency collapse, steady-state startup prune_ms dropped to 0ms after one-time backfill, and fresh restarts no longer emit new benign rmcp transport warnings.
- Priority: `7`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- The same runtimeStatus, runtimeLogs, runtimeTimeline, slowMcpCalls, mcpStats, slowQueries, and queryLog surfaces are re-run and compared against the baseline with explicit before and after evidence. [any]
- Validation includes targeted tests for changed areas, a full cargo test workspace run under repo policy, and rebuilt-release daemon restart or health verification for any runtime or MCP changes. [any]

#### Validation Refs

- `./target/release/prism-cli mcp health`
- `./target/release/prism-cli mcp restart --internal-developer`
- `./target/release/prism-cli mcp status`
- `cargo build --release -p prism-cli -p prism-mcp`
- `cargo test --workspace --quiet`
- `log:post-fix-mcp-audit`
- `log:post-fix-runtime-audit`

## Edges

- `plan-edge:coord-task:01kn7cy67chqrcz18kvnpk7gmm:depends-on:coord-task:01kn7cxtgg1r28ntzg41pgzt2r`: `coord-task:01kn7cy67chqrcz18kvnpk7gmm` depends on `coord-task:01kn7cxtgg1r28ntzg41pgzt2r`
- `plan-edge:coord-task:01kn7cym8fe0dxg3ehd19fp9ge:depends-on:coord-task:01kn7cxtgg1r28ntzg41pgzt2r`: `coord-task:01kn7cym8fe0dxg3ehd19fp9ge` depends on `coord-task:01kn7cxtgg1r28ntzg41pgzt2r`
- `plan-edge:coord-task:01kn7cywwqynsppzxgn5rqawmc:depends-on:coord-task:01kn7cxtgg1r28ntzg41pgzt2r`: `coord-task:01kn7cywwqynsppzxgn5rqawmc` depends on `coord-task:01kn7cxtgg1r28ntzg41pgzt2r`
- `plan-edge:coord-task:01kn7cz3g0ks84qsqz565pfen9:depends-on:coord-task:01kn7cxtgg1r28ntzg41pgzt2r`: `coord-task:01kn7cz3g0ks84qsqz565pfen9` depends on `coord-task:01kn7cxtgg1r28ntzg41pgzt2r`
- `plan-edge:coord-task:01kn7cz9ysdjmzcdr8sqtk388j:depends-on:coord-task:01kn7cxtgg1r28ntzg41pgzt2r`: `coord-task:01kn7cz9ysdjmzcdr8sqtk388j` depends on `coord-task:01kn7cxtgg1r28ntzg41pgzt2r`
- `plan-edge:coord-task:01kn7czn5h7s8396kc9a2jcgpf:depends-on:coord-task:01kn7cy67chqrcz18kvnpk7gmm`: `coord-task:01kn7czn5h7s8396kc9a2jcgpf` depends on `coord-task:01kn7cy67chqrcz18kvnpk7gmm`
- `plan-edge:coord-task:01kn7czn5h7s8396kc9a2jcgpf:depends-on:coord-task:01kn7cym8fe0dxg3ehd19fp9ge`: `coord-task:01kn7czn5h7s8396kc9a2jcgpf` depends on `coord-task:01kn7cym8fe0dxg3ehd19fp9ge`
- `plan-edge:coord-task:01kn7czn5h7s8396kc9a2jcgpf:depends-on:coord-task:01kn7cywwqynsppzxgn5rqawmc`: `coord-task:01kn7czn5h7s8396kc9a2jcgpf` depends on `coord-task:01kn7cywwqynsppzxgn5rqawmc`
- `plan-edge:coord-task:01kn7czn5h7s8396kc9a2jcgpf:depends-on:coord-task:01kn7cz3g0ks84qsqz565pfen9`: `coord-task:01kn7czn5h7s8396kc9a2jcgpf` depends on `coord-task:01kn7cz3g0ks84qsqz565pfen9`
- `plan-edge:coord-task:01kn7czn5h7s8396kc9a2jcgpf:depends-on:coord-task:01kn7cz9ysdjmzcdr8sqtk388j`: `coord-task:01kn7czn5h7s8396kc9a2jcgpf` depends on `coord-task:01kn7cz9ysdjmzcdr8sqtk388j`

