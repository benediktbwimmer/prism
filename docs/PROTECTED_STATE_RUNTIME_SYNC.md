# Protected State Runtime Sync

## Problem

PRISM currently treats repo-published `.prism` state inconsistently at runtime.

- The normal source watcher ignores `.prism` entirely in [`watch.rs`](../crates/prism-core/src/watch.rs#L978).
- Memory is a special case: some session read paths opportunistically import repo memory events through [`sync_repo_memory_events_locked(...)`](../crates/prism-core/src/session.rs#L3068).
- Other repo-published streams such as changes, concepts, contracts, relations, and plans are primarily hydrated at startup or through explicit rebuild paths, not through one uniform live import mechanism.

That produces behavior that is hard to explain:

- upstream merged `.prism` memory may appear during a live session
- upstream merged `.prism` changes or concepts may not
- freshness depends on which read path was used
- `.prism` state is not governed by one coherent runtime contract

This doc defines the replacement design.

## Decision

PRISM should use one explicit runtime import model for repo-published protected state.

The design is:

- remove the memory-only lazy sync special case
- keep `.prism` ignored by the normal source-tree watcher
- add a dedicated protected-state watcher/importer for `.prism/**`
- import changed protected streams into live runtime state through one uniform sync path
- suppress self-generated writes so PRISM does not react to its own repo-publish operations as if they were upstream changes
- keep a bounded fallback reconciliation sweep as a safety net, not as the primary freshness mechanism

## Non-Goals

This design does not:

- make `.prism` changes trigger normal source indexing
- reintroduce full runtime rebuilds as a steady-state read-path mechanism
- preserve the current memory-only lazy import behavior
- expose separate user-facing query APIs for repo-published versus runtime-local state

## Current State

### Source Watch Path

The existing source watcher excludes `.prism` by policy in [`watch.rs`](../crates/prism-core/src/watch.rs#L978).

That is still correct for the source refresh pipeline. Repo-published protected streams need different handling than source code edits.

### Memory Special Case

Several session reads opportunistically call [`sync_repo_memory_events_locked(...)`](../crates/prism-core/src/session.rs#L3068), for example in [`load_episodic_snapshot(...)`](../crates/prism-core/src/session.rs#L1242).

That creates one-off freshness behavior for memory only.

### Desired Runtime Rule

Repo-published `.prism` state should follow one runtime rule:

1. bootstrap imports published state into runtime
2. live `.prism` changes are imported by one dedicated protected-state sync mechanism
3. read paths consume live runtime state only

Read paths must not contain stream-specific import exceptions.

## Architecture

### 1. Separate Watchers by Responsibility

PRISM should have two watcher classes:

- source watcher
  - watches normal repo files
  - drives source graph refresh and observed change processing
  - continues to ignore `.prism`
- protected-state watcher
  - watches repo-published `.prism` protected streams only
  - drives targeted protected-state import into runtime
  - does not trigger source indexing

This avoids conflating source refresh with protected-state hydration.

### 2. One Protected-State Sync Entry Point

Introduce one explicit sync path for repo-published protected state.

Suggested shape:

- `sync_repo_protected_state(...)`
- `sync_repo_protected_stream(stream, ...)`

The sync path should:

- determine which protected stream changed
- verify the stream through the protected-state verification machinery
- read only the changed stream
- import only the affected domain into live runtime state
- update runtime freshness/materialization metadata for that domain
- avoid full rebuild unless repair/admin logic explicitly requests it

For shared coordination refs, this sync path must also own the expensive authority import work:

- fetch or refresh the shared ref head when needed
- verify the shared manifest and snapshot shard digests
- rebuild one local materialized coordination snapshot or checkpoint keyed by shared-ref identity
- update runtime freshness markers that tell startup whether the local materialized snapshot is still valid

The shared ref remains the authority and replication source, but daemon startup should consume the
local materialized coordination snapshot rather than rehydrating the shared ref directly on the
critical path.

#### Shared coordination startup artifact

The shared-coordination sync path should write one local materialized startup artifact with:

- the fully hydrated `CoordinationSnapshot`
- authored `plan_graphs`
- `execution_overlays`
- authority metadata for the imported shared ref
  - shared ref name
  - source head commit when known
  - canonical manifest digest when known
  - schema version
  - materialized timestamp

The initial implementation should reuse the local checkpoint store and checkpoint materializer
surface that already persists coordination read models and compaction state. This keeps the startup
artifact local to the daemon's selected runtime/cache environment instead of treating the shared ref
itself as the startup store.

The explicit shared-coordination sync path is the only owner that should rewrite this startup
artifact from shared-ref authority input.

### 3. Stream-Oriented Domain Imports

Each protected stream class should map to one runtime import handler.

Initial tracked publication classes:

- `.prism/state/manifest.json`
- `.prism/state/memory/**/*.json`
- `.prism/state/changes/**/*.json`
- `.prism/state/concepts/**/*.json`
- `.prism/state/relations/**/*.json`
- `.prism/state/contracts/**/*.json`
- `.prism/state/plans/**/*.json`
- `.prism/state/coordination/**/*.json`

Legacy migration-only compatibility inputs may still include:

- `.prism/memory/*.jsonl`
- `.prism/changes/events.jsonl`
- `.prism/concepts/events.jsonl`
- `.prism/concepts/relations.jsonl`
- `.prism/contracts/events.jsonl`
- `.prism/plans/streams/*.jsonl`

Each handler should:

- validate manifest and snapshot digest integrity
- decode the current shard set for the affected domain
- use legacy stream replay only during explicit migration compatibility
- patch the corresponding in-memory/runtime-backed domain
- optionally update local/shared durability projections

### 4. Self-Write Suppression

The protected-state watcher must not import PRISM's own repo-publish writes as if they were external updates needing another round of work.

The importer should suppress self-writes using stream-local freshness markers such as:

- last imported event id
- last imported sequence
- last verified entry hash
- per-stream file fingerprint or mtime/size tuple

The exact mechanism can be implementation-driven, but the requirement is strict:

- upstream merge or external file replacement should import
- PRISM's own just-written publish step should not cause redundant churn

### 5. Bootstrap and Safety-Net Reconciliation

Startup should still hydrate repo-published protected state into runtime.

In addition:

- a bounded periodic reconciliation sweep may exist as a safety net
- it should verify whether observed stream heads still match runtime import markers
- it must not be the primary freshness path

For shared coordination specifically, startup should prefer a local materialized coordination
snapshot or checkpoint that was produced by the explicit sync path. That startup artifact should be
keyed by a verified shared-ref identity such as the current head commit, canonical manifest digest,
or both. If the local artifact is missing or stale, the explicit shared-coordination sync path
rebuilds it; startup should not treat the shared ref as a live startup database.

On the startup critical path:

- load the local shared-coordination startup artifact directly
- build in-memory coordination state once from that artifact
- mark freshness using the artifact's last imported authority key
- let the dedicated sync path decide later whether the shared ref has advanced and whether a new
  local artifact must be materialized

If the local startup artifact is absent, startup may fall back to local runtime durability
mechanisms such as the coordination journal or a previous local compaction snapshot, but that
fallback is still local. The recovery path should schedule or trigger explicit shared-ref sync
instead of fetching and rehydrating the shared ref inline.

That keeps freshness event-driven while still covering missed watcher events.

## Runtime Contract

### Read Paths

Read paths should:

- read live runtime state
- report freshness when useful
- never opportunistically import `.prism` streams on a per-domain special basis

### Mutation Paths

Mutation paths that publish repo-protected state should:

- write repo-protected streams
- update self-write suppression markers
- patch live runtime state directly when they already know the authored event

That preserves the rule from [`REFRESH_RUNTIME_REDESIGN.md`](REFRESH_RUNTIME_REDESIGN.md#L182): writes should not rely on "persist then reload to see your write."

## Migration Plan

### Phase 1

Unify semantics without live watching yet.

- remove the memory-only lazy import calls from read paths
- keep bootstrap hydration for all protected-state domains
- add one internal sync helper covering all protected streams

This yields a consistent rule immediately, even before live `.prism` sync lands.

### Phase 2

Add dedicated protected-state watching.

- watch `.prism` protected streams
- trigger targeted per-stream import
- suppress self-writes

### Phase 3

Add recovery and diagnostics polish.

- report per-stream freshness in runtime status
- expose protected-state sync diagnostics in internal developer surfaces
- add missed-event reconciliation safety net

## Testing Requirements

The implementation should add explicit coverage for:

- startup hydration from repo-published streams
- upstream merge simulation while runtime is already running
- memory, changes, concepts, contracts, relations, and plan streams all following the same import model
- no read path importing memory specially
- self-write suppression for PRISM-authored stream writes
- external `.prism` edits importing correctly after watcher events
- degraded but correct recovery when a watcher event is missed

## TODOs

TODO-PSYNC-1: Add a new runtime design section to the core docs set that states the single rule: repo-published `.prism` state is imported only by bootstrap or the dedicated protected-state sync path, never by ad hoc read-path hooks.

TODO-PSYNC-2: Remove all session read-path calls to `sync_repo_memory_events_locked(...)` in [`session.rs`](../crates/prism-core/src/session.rs).

TODO-PSYNC-3: Replace `sync_repo_memory_events_locked(...)` with a generalized protected-state sync surface, likely centered on `sync_repo_protected_state(...)` plus per-stream helpers.

TODO-PSYNC-4: Introduce a dedicated protected-state watcher for `.prism/**` that is separate from the normal source watcher in [`watch.rs`](../crates/prism-core/src/watch.rs).

TODO-PSYNC-5: Keep `.prism` ignored by the normal source watcher and document that this is intentional, not a bug.

TODO-PSYNC-6: Add per-stream import handlers for memory, changes, concepts, concept relations, contracts, and published plan streams.

TODO-PSYNC-7: Add self-write suppression markers so PRISM does not re-import its own protected-state publish writes as external updates.

TODO-PSYNC-8: Extend runtime freshness/status surfaces to report protected-state sync status per domain or per stream.

TODO-PSYNC-9: Add internal developer diagnostics for protected-state watcher events, suppressed self-writes, verified imports, and failed imports.

TODO-PSYNC-10: Add tests that simulate an upstream merge of `.prism` state into a live repo and verify the runtime imports it without restart.

TODO-PSYNC-11: Add tests that prove memory no longer has a unique lazy import path and now follows the same behavior as the other protected streams.

TODO-PSYNC-12: Add a bounded reconciliation sweep as a fallback safety net and test that it repairs missed watcher notifications without becoming the primary sync mechanism.
