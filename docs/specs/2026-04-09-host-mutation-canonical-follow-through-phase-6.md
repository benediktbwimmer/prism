# Host Mutation Canonical Follow-Through Phase 6

Status: completed
Audience: MCP, coordination, query, and runtime maintainers
Scope: move host-side coordination mutation readers and session-binding helpers onto canonical v2 task and plan records where legacy-only lease-holder helpers are not required

---

## 1. Summary

After the canonical handoff slice, the biggest remaining reader-heavy legacy cluster is
`crates/prism-mcp/src/host_mutations.rs`.

Many paths in that file still load legacy `CoordinationTask` and `Plan` records even when they
only need canonical task title, anchors, branch ref, plan id, or git execution state. That keeps
git-execution follow-through and session binding tied to the old projection longer than necessary.

This slice moves those read-heavy mutation helpers onto canonical v2 task and plan records while
leaving the smaller set of lease-holder and stale-same-holder checks isolated until the deeper
runtime mutation purge.

## 2. Required changes

- add local host-mutation helpers for canonical task and plan lookup by coordination ids
- move git-execution follow-through readers onto canonical task and plan records
- move session binding and work declaration readers onto canonical task and plan records
- replace legacy existence checks in workflow-target resolution where v2 presence is sufficient

## 3. Non-goals

- do not rewrite the deeper lease-holder admissibility helpers in this slice
- do not remove `CoordinationSnapshot` from the mutation runtime in this slice
- do not implement Postgres work in this slice

## 4. Exit criteria

- read-heavy host-mutation helpers no longer call legacy `coordination_task(...)` or
  `coordination_plan(...)` when canonical v2 task and plan data is sufficient
- git-execution mutation follow-through and session binding use canonical task and plan state
- the remaining legacy host-mutation callers are explicitly limited to lease-holder or mutation
  paths that still depend on legacy-only helpers
- targeted `prism-query` and `prism-mcp` validation passes

## 5. Outcome

This slice is complete.

Completed follow-through:

- `host_mutations.rs` now uses canonical v2 task and plan lookups for session binding,
  work declaration, workflow update reads, artifact default-anchor lookup, review-artifact
  follow-through, and most git-execution reloads
- post-update git-execution helper methods now reload canonical v2 task views rather than
  returning legacy task projections
- the MCP coordination mutation test surface was updated to the v2 plan-status contract

Intentional residual legacy sites after this slice:

- `maybe_auto_resume_stale_same_holder_task(...)` still uses legacy task lease-holder helpers
- the initial git-execution admissibility gate still loads the legacy task shape because
  `ensure_git_execution_task_admissible(...)` and related holder checks are not yet canonicalized
