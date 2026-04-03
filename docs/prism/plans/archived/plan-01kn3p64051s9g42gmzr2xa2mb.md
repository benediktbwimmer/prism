# large-edit-follow-up: eliminate the remaining large-edit pathologies in the daemon by preserving better co-change fidelity for oversized change sets, reducing parse/apply and persist cost on large fanout edits, and validating the resulting behavior from the live daemon log.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:1f3fc97f19c5b9818b9a207e1e4953a175012bcab6b2641049b7d88a540e816f`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 4 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn3p64051s9g42gmzr2xa2mb`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `4`
- Edges: `4`

## Goal

large-edit-follow-up: eliminate the remaining large-edit pathologies in the daemon by preserving better co-change fidelity for oversized change sets, reducing parse/apply and persist cost on large fanout edits, and validating the resulting behavior from the live daemon log.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn3p64051s9g42gmzr2xa2mb.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn3p64051s9g42gmzr2xa2mb.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn3p6jbrntr931d9tqn8xrwk`

## Nodes

### Profile the remaining large-edit hot path from live daemon evidence

- Node id: `coord-task:01kn3p6jbrntr931d9tqn8xrwk`
- Kind: `investigate`
- Status: `completed`
- Summary: Profiled the live daemon log and isolated the remaining large-edit tail. Oversized edits repeatedly hit the 128-lineage co-change guardrail in prism_core::indexer::WorkspaceIndexer_S::apply_file_update, while larger fanout refreshes are now dominated by parse_apply_ms and the write-side cost in prism_store::sqlite::SqliteStore::commit_index_persist_batch. Recent large refreshes ranged roughly 259-536ms with parse_apply_ms commonly 152-223 and persist_ms 63-255, while resolve_edges_ms was usually negligible.

#### Acceptance

- Recent large-edit log samples are classified into co-change, parse/apply, and persist contributors [any]
- The next implementation cut is justified by concrete live evidence [any]

### Validate the large-edit follow-up on the live daemon log

- Node id: `coord-task:01kn3p6jdzqwasz42a35dahesz`
- Kind: `validate`
- Status: `completed`
- Summary: Validated the large-edit follow-up on the live daemon after release rebuild and daemon restart recovery. Recent daemon-log evidence shows oversized co-change sampling is active instead of skipping, and large-file refreshes now stay on the in-place persist path with `restore_runtime_ms=0`, `reanchor_memory_ms=0`, `persist_ms` reduced into the teens-to-20s for the controlled large-file probes, and total refreshes reduced from the previous 300-400ms range to roughly 136-183ms on the targeted `crates/prism-mcp/src/tests.rs` workload. Full `cargo test --workspace` and release `mcp status`/`health` are green.

#### Acceptance

- Live daemon validation covers an oversized-change case and a larger-fanout edit case [any]
- The remaining pathological behavior is explicitly documented with current numbers [any]

### Preserve more co-change fidelity for oversized change sets without reintroducing hot-path blowups

- Node id: `coord-task:01kn3p6wyqn4ecaczcsr51wy1m`
- Kind: `edit`
- Status: `completed`
- Summary: Replaced the oversized co-change guardrail with a deterministic bounded fallback. prism_projections::co_change_delta_batch_for_events now samples a capped 128-lineage subset instead of returning no deltas, prism_core::indexer logs `sampling symbol-level co-change deltas for oversized change set`, and live daemon validation on prism-mcp/src/tests.rs showed lineage_event_count=275, sampled_lineage_count=128, co_change_delta_count=16256, total_ms=198 with no drop-to-zero co-change behavior.

#### Acceptance

- Oversized edits no longer collapse all symbol-level co-change detail to zero [any]
- The hot path remains bounded for very large lineage event sets [any]

### Reduce parse/apply and persist cost for large fanout edits

- Node id: `coord-task:01kn3p6x28h7567wcmymy825g3`
- Kind: `edit`
- Status: `completed`
- Summary: Broadened structurally-unchanged file detection to compare node, edge, and unresolved shapes semantically instead of positionally, and kept SQLite file-state persistence on the in-place path for those updates. Focused store/core regressions passed, `cargo test --workspace` passed, and live daemon log validation on `crates/prism-mcp/src/tests.rs` showed the large-file path improving from `in_place_upserted_file_count=0, persist_ms=132, total_ms=410` to `in_place_upserted_file_count=1, persist_ms=23, total_ms=183`, with a second follow-up refresh at `persist_ms=16, total_ms=136`.

#### Acceptance

- Large-file or wider-fanout edits show lower parse/apply or persist cost in targeted validation [any]
- The implementation keeps ordinary tiny-edit performance at least as good as before [any]

## Edges

- `plan-edge:coord-task:01kn3p6jdzqwasz42a35dahesz:depends-on:coord-task:01kn3p6wyqn4ecaczcsr51wy1m`: `coord-task:01kn3p6jdzqwasz42a35dahesz` depends on `coord-task:01kn3p6wyqn4ecaczcsr51wy1m`
- `plan-edge:coord-task:01kn3p6jdzqwasz42a35dahesz:depends-on:coord-task:01kn3p6x28h7567wcmymy825g3`: `coord-task:01kn3p6jdzqwasz42a35dahesz` depends on `coord-task:01kn3p6x28h7567wcmymy825g3`
- `plan-edge:coord-task:01kn3p6wyqn4ecaczcsr51wy1m:depends-on:coord-task:01kn3p6jbrntr931d9tqn8xrwk`: `coord-task:01kn3p6wyqn4ecaczcsr51wy1m` depends on `coord-task:01kn3p6jbrntr931d9tqn8xrwk`
- `plan-edge:coord-task:01kn3p6x28h7567wcmymy825g3:depends-on:coord-task:01kn3p6jbrntr931d9tqn8xrwk`: `coord-task:01kn3p6x28h7567wcmymy825g3` depends on `coord-task:01kn3p6jbrntr931d9tqn8xrwk`

