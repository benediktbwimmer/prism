# Coordination Query Reader V2 Follow-Through Phase 6

Status: completed
Audience: coordination, query, MCP, and runtime maintainers
Scope: move reader-side query and MCP task/plan lookups off legacy coordination model projections and onto canonical v2 task and plan records

---

## 1. Summary

Phase 6 already removed the public compatibility surface, but several active reader paths still
looked up legacy `CoordinationTask` and `Plan` projections internally even when they only needed
canonical task title, summary, anchors, tags, validation checks, or plan policy.

That keeps the old model alive longer than necessary and makes the v2-only cut incomplete in
practice.

This slice pushes the next layer inward:

- `prism-query` reader helpers load canonical task records through a v2 lookup path
- risk, intent, and impact calculations stop depending on legacy task and plan projections where
  v2 data already exists
- MCP task-context and provenance readers use canonical v2 task and plan data rather than legacy
  compatibility lookups

This is intentionally a reader-side slice only. It does not yet remove deeper runtime and mutation
paths that still depend on legacy-only fields.

## 2. Required changes

- add a canonical coordination-task v2 lookup helper in `crates/prism-query/src/coordination.rs`
- switch reader-side `prism-query` task and plan lookups in `impact.rs` and `intent.rs` to v2
  helpers
- switch MCP read-model/task-context helpers that only need canonical task metadata to v2 lookups
- keep the remaining legacy-model uses explicit in files that still depend on legacy-only state so
  the next slice can target them directly

## 3. Non-goals

- do not remove legacy task or plan projections from mutation code in this slice
- do not rewrite watch or lease-handoff logic that still depends on legacy-only fields
- do not change external query or MCP schema in this slice

## 4. Exit criteria

- the touched reader-side query and MCP modules no longer call legacy `coordination_task(...)` or
  `coordination_plan(...)` helpers
- task-anchor, task-intent, task-risk, and related provenance readers use canonical v2 task data
  where possible
- targeted `prism-query` and `prism-mcp` validation passes
