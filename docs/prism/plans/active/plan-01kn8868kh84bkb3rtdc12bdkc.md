# Address the highest-priority remaining PRISM validation-feedback issues across task-brief guidance, compact ranking and follow-through, runtime diagnostics, freshness for newly added files, and deterministic validation stability.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:57530e8821b816d713111857b7f97a75275fb22f66d6a1293a1c81e8c21f0932`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 5 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn8868kh84bkb3rtdc12bdkc`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `5`

## Goal

Address the highest-priority remaining PRISM validation-feedback issues across task-brief guidance, compact ranking and follow-through, runtime diagnostics, freshness for newly added files, and deterministic validation stability.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn8868kh84bkb3rtdc12bdkc.jsonl`

## Root Nodes

- `coord-task:01kn88705a26qcc46r29w6zy5k`
- `coord-task:01kn887c7vzqbqeq28g6dpejs7`
- `coord-task:01kn887qhy3394rwdsqj43tzz3`
- `coord-task:01kn8881wy1ezws32kge141gk7`
- `coord-task:01kn888c0tajwyg12b26d389nf`

## Nodes

### Fix prism_task_brief next-action drift for completed and self-contained tasks

- Node id: `coord-task:01kn88705a26qcc46r29w6zy5k`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Active tasks prefer task-local next steps over sibling-plan redirects [any]
- Completed tasks stop emitting unrelated follow-up nextAction guidance [any]
- Task-brief regressions cover completed-task and self-contained-task cases [any]

### Improve compact ranking and docs-to-code follow-through for repo-awareness queries

- Node id: `coord-task:01kn887c7vzqbqeq28g6dpejs7`
- Kind: `edit`
- Status: `completed`
- Summary: Improved compact locate ranking by handling camelCase API names, treating contrast tails like `over ...` as non-positive ranking context, supplementing exact-identifier candidates with text-hit remaps, and downweighting field-only exact matches relative to owner code. Validated with focused regressions, `cargo test -p prism-mcp --quiet`, workspace validation plus isolated rerun of the accepted query-history flake, and a rebuilt/restarted live daemon showing `compact_open related_handles` now resolves to `compact_open`.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Broad doc/spec and subsystem queries rank real owner code ahead of helper and schema noise [any]
- Doc headings and concept packets hand off to useful workset or open targets instead of dead ends [any]
- Focused ranking regressions cover the recurring validation-feedback queries [any]

### Make runtimeStatus trustworthy for process and memory diagnostics

- Node id: `coord-task:01kn887qhy3394rwdsqj43tzz3`
- Kind: `edit`
- Status: `completed`
- Summary: Made runtimeStatus trustworthy for process and memory diagnostics by merging real `ps` snapshots into runtime-state process records instead of synthesizing zeroed process metadata. Validated with focused runtime_views tests, `cargo test -p prism-mcp --quiet`, `cargo test --workspace --quiet`, release rebuild/restart, and live daemon verification showing runtimeStatus RSS/PPID closely matches direct ps output.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Runtime diagnostics distinguish live daemons from stale ledger entries consistently [any]
- Status regressions cover memory and process-reporting accuracy [any]
- runtimeStatus exposes real per-process RSS and no longer zeros active daemon memory fields [any]

### Tighten freshness for newly added files and docs in anchors and compact reads

- Node id: `coord-task:01kn8881wy1ezws32kge141gk7`
- Kind: `edit`
- Status: `completed`
- Summary: Improved freshness for newly added files and docs by teaching anchor conversion to trigger scoped workspace refreshes for previously unseen files and by adding workspace-path fallbacks for prism_locate and compact text reads before semantic indexing catches up. Validation passed via targeted anchor and compact-locate regressions, cargo test -p prism-mcp --quiet, cargo test --workspace --quiet, and the required release build plus daemon restart/status/health sequence.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Compact locate or read flows can surface newly added docs without manual fallbacks [any]
- Freshness regressions cover new-file and new-doc cases [any]
- Newly added files become anchorable and queryable without stale-path surprises [any]

### Eliminate the remaining known workspace validation flakes

- Node id: `coord-task:01kn888c0tajwyg12b26d389nf`
- Kind: `edit`
- Status: `completed`
- Summary: Hardened the two remaining workspace-parallel PRISM MCP flakes by removing brittle wall-clock timing from the refresh-lock test, making query-history assertions uniquely scoped and order-independent, and invalidating cached runtimeStatus after query-log writes so the live status surface no longer reports stale mcpCallLogBytes. Validation passed via targeted reruns, short stress loops, cargo test -p prism-mcp --quiet, cargo test --workspace --quiet, and the required release build plus daemon restart/status/health sequence.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Flake-specific regressions or harness hardening cover the resolved root causes [any]
- Full workspace validation stops depending on isolated rerun exceptions for those tests [any]
- The recurring prism-mcp query-history and runtime-sync suite flakes stop failing under normal full-suite parallel runs [any]

### Validate the feedback-driven repo-awareness improvements end to end

- Node id: `coord-task:01kn888p792g26q4napd7sy96b`
- Kind: `edit`
- Status: `completed`
- Summary: Validated the feedback-driven repo-awareness improvements end to end after completing task-brief guidance fixes, compact ranking follow-through, runtimeStatus diagnostics cleanup, fresh-file/doc freshness handling, and deterministic flake elimination. Validation evidence includes targeted PRISM MCP regressions, cargo test -p prism-mcp --quiet, cargo test --workspace --quiet, and the required release build plus daemon restart/status/health sequence on the updated binaries.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Release daemon rebuild, restart, status, and health pass on the final state [any]
- Targeted regressions cover each implemented improvement track [any]
- cargo test --workspace --quiet passes, or only accepted non-regression flakes remain until the flake task lands [any]

## Edges

- `plan-edge:coord-task:01kn888p792g26q4napd7sy96b:depends-on:coord-task:01kn88705a26qcc46r29w6zy5k`: `coord-task:01kn888p792g26q4napd7sy96b` depends on `coord-task:01kn88705a26qcc46r29w6zy5k`
- `plan-edge:coord-task:01kn888p792g26q4napd7sy96b:depends-on:coord-task:01kn887c7vzqbqeq28g6dpejs7`: `coord-task:01kn888p792g26q4napd7sy96b` depends on `coord-task:01kn887c7vzqbqeq28g6dpejs7`
- `plan-edge:coord-task:01kn888p792g26q4napd7sy96b:depends-on:coord-task:01kn887qhy3394rwdsqj43tzz3`: `coord-task:01kn888p792g26q4napd7sy96b` depends on `coord-task:01kn887qhy3394rwdsqj43tzz3`
- `plan-edge:coord-task:01kn888p792g26q4napd7sy96b:depends-on:coord-task:01kn8881wy1ezws32kge141gk7`: `coord-task:01kn888p792g26q4napd7sy96b` depends on `coord-task:01kn8881wy1ezws32kge141gk7`
- `plan-edge:coord-task:01kn888p792g26q4napd7sy96b:depends-on:coord-task:01kn888c0tajwyg12b26d389nf`: `coord-task:01kn888p792g26q4napd7sy96b` depends on `coord-task:01kn888c0tajwyg12b26d389nf`

