# Canonical Task Handoff Follow-Through Phase 6

Status: completed
Audience: coordination, query, MCP, and runtime maintainers
Scope: move pending-handoff and assisted-lease reader paths onto canonical v2 task records so they no longer need the legacy task projection

---

## 1. Summary

After the reader-side v2 migration, the next blocker is that several active paths still depend on
legacy `CoordinationTask` only because canonical task records did not carry the handoff state that
those paths need.

That keeps assisted-lease logic, task-brief rendering, and session-context readers partially tied
to the legacy projection even though the underlying feature is still a real part of the live task
model.

This slice fixes that mismatch by making pending handoff state canonical in v2 and then moving the
affected reader-side paths over to canonical task and plan lookups.

## 2. Required changes

- add `pending_handoff_to` to canonical task records and derive it from the legacy task snapshot
- make canonical derived task status treat pending handoff as a blocked state
- expose the canonical handoff field through `CoordinationTaskV2View`
- switch assisted-lease, task-brief, and session-task reader paths that only needed handoff or
  lease metadata onto canonical v2 task lookups

## 3. Non-goals

- do not rewrite mutation-heavy coordination flows in this slice
- do not remove the remaining legacy mutation helpers from `prism-query` or `prism-mcp`
- do not implement Postgres work in this slice

## 4. Exit criteria

- pending handoff is first-class data on canonical task records rather than hidden in legacy-only
  state or metadata
- task brief and assisted-lease readers no longer need legacy task lookups to detect handoff state
- v2 task views expose the handoff field for product surfaces that need it
- targeted `prism-coordination`, `prism-query`, `prism-core`, and `prism-mcp` validation passes
