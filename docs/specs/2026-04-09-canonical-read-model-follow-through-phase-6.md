# Canonical Read-Model Follow-Through Phase 6

Status: completed
Audience: coordination, core, MCP, and UI-read-model maintainers
Scope: make the coordination read-model builders and their materialization path consume
`CoordinationSnapshotV2`, and remove legacy plan/task payloads from those read-model shapes

---

## 1. Summary

The live coordination broker is still carrying a legacy `CoordinationSnapshot` for one narrow
reason:

- `CoordinationReadModel` and `CoordinationQueueReadModel` are still shaped around legacy
  `Plan` and `CoordinationTask` payloads

That keeps an avoidable migration dependency alive on the production read path even after the
broker and adjacent query surfaces have moved to canonical v2 snapshots.

This slice closes that gap by:

- making the read-model builders operate directly on `CoordinationSnapshotV2`
- storing only canonical identifiers where the read models previously kept legacy plan/task
  payloads
- switching broker and materialization fallback rebuilds to the canonical builders

## 2. Required changes

- add `coordination_read_model_from_snapshot_v2(...)`
- add `coordination_queue_read_model_from_snapshot_v2(...)`
- change `CoordinationReadModel.active_plans` to `active_plan_ids`
- change `CoordinationQueueReadModel.pending_handoff_tasks` to `pending_handoff_task_ids`
- rebuild fallback read and queue models from `CoordinationSnapshotV2` in the MCP broker
- rebuild effective persisted read and queue models from `CoordinationSnapshotV2` in the
  coordination materialized store and checkpoint materializer
- update UI summary and queue readers to consume the renamed identifier-based fields

## 3. Non-goals

- do not remove every remaining legacy incremental seed helper in this slice
- do not implement Postgres
- do not change higher-level UI summary wording beyond the read-model field cutover

## 4. Exit criteria

- the broker no longer needs a legacy snapshot to derive current read and queue models
- persisted effective read-model rebuilds consume `CoordinationSnapshotV2`
- coordination read models no longer embed legacy `Plan` payloads only to count active plans
- coordination queue read models no longer embed legacy `CoordinationTask` payloads only to track
  pending handoffs
- targeted tests for `prism-coordination`, `prism-core`, and `prism-mcp` pass

## 5. Outcome

- canonical read-model builders now exist alongside the legacy entrypoints
- the read-model structs now carry `active_plan_ids` and `pending_handoff_task_ids`
- MCP broker fallback reads and persisted materialization fallback reads now rebuild from
  `CoordinationSnapshotV2`
- UI overview coordination summaries and queues consume the identifier-based read-model fields
