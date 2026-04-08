# PRISM Shared Runtime SQLite Removal Contract

Status: execution contract for the federated runtime cutover
Audience: PRISM core, storage, coordination, query, and MCP maintainers
Scope: concrete ownership boundaries and removal rules for deleting the shared runtime SQLite backend

---

## 1. Purpose

`docs/PRISM_FEDERATED_RUNTIME_ARCHITECTURE.md` defines the target architecture.

This document locks the concrete cutover contract for implementation. It answers four questions:

- which facts remain authoritative on shared coordination refs
- which facts become strictly worktree-local SQLite state
- which facts may only be fetched through peer or archive enrichment
- which shared runtime SQLite responsibilities must be deleted rather than preserved as a dormant fallback

If older documents still describe a repo-shared runtime database as an authority plane, this document
supersedes those assumptions for implementation work on the federated runtime cutover.

---

## 2. Decision Summary

PRISM is removing the shared runtime SQLite database completely.

The target model is:

- shared runtime SQLite is never the cross-runtime mutable authority plane
- coordination authority currently lives in shared refs and may later be provided by another
  explicit coordination authority backend
- worktree-local SQLite is the only rich hot runtime store
- peer or archive reads are optional enrichment paths, never correctness dependencies
- repo-published `.prism/state/**` remains the repo-published semantic authority plane

The cutover must not leave a dormant shared SQLite backend in the hot path.

That means:

- no authoritative coordination reads from `shared/runtime/state.db`
- no authoritative coordination writes to `shared/runtime/state.db`
- no request-path freshness, revision, or runtime-sync dependency on `shared/runtime/state.db`
- no rich runtime materialization fan-out into `shared/runtime/state.db`
- no startup or recovery path that silently prefers `shared/runtime/state.db` over shared refs plus local state

The only allowed post-cutover relationship to the old shared runtime DB is explicit migration or
cleanup handling for legacy repositories.

---

## 3. Authority Contract

### 3.1 Shared coordination refs

Shared coordination refs remain authoritative for compact cross-runtime facts that must converge:

- plans
- tasks and task lifecycle state
- claims and leases
- shared publication and integration state
- runtime discovery descriptors
- compact shared artifact and review metadata

Shared coordination refs are the only cross-runtime mutable truth that other runtimes may rely on
for correctness.

### 3.2 Worktree-local SQLite

Worktree-local SQLite is authoritative for rich local runtime state:

- append-only journals
- replay inputs and detailed history slices
- detailed outcome and memory event storage
- local serving projections and caches
- local diagnostics and operational traces
- unpublished draft state
- local startup checkpoints and materialized recovery artifacts
- local workspace-tree and refresh bookkeeping

This state may be reconstructed, exported, or served to peers, but it is not shared mutable
authority.

### 3.3 Peer and archive enrichment

Peer runtime reads and archive exports may serve:

- bounded replay slices
- hot execution overlays
- local diagnostics
- local draft context
- rich historical bundles

These are enrichment classes only. They may improve visibility, but they must not decide shared
coordination truth.

### 3.4 Repo-published semantic authority

Tracked `.prism/state/**` remains the repo-published authority plane for branch-published semantic
state and signed published knowledge.

The federated cutover does not move repo-published semantic truth into local SQLite or peer
transport.

---

## 4. Responsibility Map

| State or capability | Target owner after cutover | Notes |
| --- | --- | --- |
| Coordination plans, tasks, claims, leases | shared coordination refs | Local copies are derived caches or checkpoints only. |
| Shared publication and target-integration facts | shared coordination refs | Other runtimes must be able to rely on these without local DB access. |
| Runtime discovery descriptors | shared coordination refs | Peer lookup resolves by `runtime_id` through shared coordination state. |
| Rich runtime journals and replay detail | worktree-local SQLite | Never mirrored into a repo-shared SQLite backend. |
| Outcome and memory event logs | worktree-local SQLite | Shared or repo-published summaries may still be derived and published separately. |
| Local serving projections, caches, and acceleration indexes | worktree-local SQLite | Derived state only. |
| Startup checkpoints and materialized recovery artifacts | worktree-local SQLite | Shared refs may seed these, but do not become a hot local database mirror. |
| Workspace refresh, diagnostics, and runtime timelines | worktree-local SQLite | Request-path reads must not block on shared DB locks. |
| Hot execution overlays and other live rich context | local runtime, optional peer read | Shared refs may publish compact coordination facts, not the rich overlay payload. |
| Cold historical continuity bundles | archive export | Optional and explicitly referenced, not a live mutable source of truth. |
| `shared/runtime/state.db` contents | migrate or delete | No live authority, no live cache dependency, no live write fan-out after cutover. |

---

## 5. Code Boundary Rules

The implementation must enforce these boundaries in code:

1. `SharedRuntimeBackend` must not provide any SQLite-backed shared runtime variant.
   Shared runtime wiring is limited to descriptor-driven remote access or fully disabled.
2. `SharedRuntimeStore` must stop participating in:
   - coordination revision reads
   - coordination snapshot hydration
   - authoritative coordination persistence
   - request-path runtime freshness checks
   - rich runtime materialization fan-out
3. `WorkspaceSession` startup and reload must treat:
   - shared coordination refs as the shared source
   - worktree-local checkpoints and caches as the local recovery source
   - peer and archive reads as optional enrichment only
4. Runtime-targeted query routing must resolve peers through runtime descriptors from shared
   coordination refs rather than through a shared runtime DB lookup path.
5. Legacy shared-runtime SQLite artifacts may be inspected only inside explicit migration or cleanup
   flows. Normal startup and steady-state request handling must not depend on them.

---

## 6. Removal Rules

The cutover must prefer deletion over fallback preservation.

Specifically:

- do not keep a hidden "shared SQLite if present" authority path
- do not keep dual-write fan-out from local runtime materializers into a shared DB
- do not keep `workspace_runtime` freshness logic that consults shared runtime revisions
- do not keep CLI or daemon defaults that silently recreate the shared DB as part of normal startup
- do not preserve the old remote shared runtime backend idea as if it were still part of the
  active architecture contract

If a capability still needs cross-runtime sharing, it must move to one of:

- shared coordination refs
- runtime descriptors plus peer reads
- explicit archive export

---

## 7. Cutover Order

The implementation sequence for this contract is:

1. stop treating shared runtime SQLite as authoritative coordination storage
2. stop using it as a sink for rich runtime materialization
3. stop using it for runtime freshness, revision, and descriptor lookup
4. remove shared-runtime-coupled startup and watch behavior
5. delete the backend abstraction and steady-state open paths
6. leave only explicit migration and cleanup handling for legacy artifacts

This order is intentional. It removes correctness dependencies first, then removes dead storage
paths, and only then deletes the compatibility scaffolding.

---

## 8. Superseded Assumptions

The following older assumptions are no longer valid for implementation work on this cutover:

- repo-shared SQLite is an authority backend
- projections may treat shared runtime state as a first-class authority plane
- append-only operational history must live in one repo-shared runtime database
- future remote runtime layers should be treated as mutable operational truth by default

The companion docs that previously used that language have now been updated. This document and
`docs/PRISM_FEDERATED_RUNTIME_ARCHITECTURE.md` remain the live contract for the removal boundary.

---

## 9. Success Criteria For Task 1

Task 1 is complete when:

- maintainers can point to one explicit responsibility map for the cutover
- later tasks can tell whether a state class belongs on shared refs, local SQLite, peer reads, or
  archive export without re-deciding architecture
- removal work has a crisp "delete vs migrate vs keep local vs keep shared" contract
