# Canonical Assisted Overlay Follow-Through Phase 6

Status: completed
Audience: core runtime, watch, and coordination maintainers
Scope: remove the remaining legacy-snapshot republish step on the local assisted-lease overlay
path now that published runtime state already carries canonical coordination directly

---

## 1. Summary

The main runtime publish path now carries canonical coordination state directly, but the local
assisted-lease overlay republish path still does one stale thing:

- it reloads runtime coordination state from `prism.coordination_snapshot()`
- then it re-derives canonical state from that legacy snapshot before publishing the overlay

That no longer matches the runtime contract. The published `Prism` already holds both the
continuity snapshot and the first-class canonical snapshot.

This slice removes that republish downgrade and makes the assisted overlay path preserve canonical
coordination state exactly as it exists on the live `Prism`.

## 2. Required changes

- change assisted overlay republish in `watch.rs` to pass both continuity and canonical
  coordination state into `WorkspaceRuntimeState`
- add regression coverage proving overlay republish preserves an explicitly provided canonical
  snapshot

## 3. Non-goals

- do not rewrite assisted-lease selection itself in this slice
- do not remove the deeper mutation-engine dependency on `CoordinationSnapshot`
- do not implement Postgres

## 4. Exit criteria

- local assisted overlay republish no longer re-derives canonical coordination state from the
  legacy snapshot
- a regression test proves the overlay publish path preserves the current canonical snapshot
- targeted tests for `prism-core` pass

## 5. Outcome

- the assisted-lease overlay republish path in `watch.rs` now passes both continuity and canonical
  coordination state into `WorkspaceRuntimeState`
- local overlay republishes no longer degrade live canonical coordination state into a
  legacy-derived v2 snapshot
- regression coverage now proves assisted overlay publication preserves an explicitly provided
  canonical coordination snapshot
