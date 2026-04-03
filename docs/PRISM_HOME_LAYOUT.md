# PRISM Home Layout

This document defines the filesystem split between repo-owned PRISM state and
user-local PRISM state.

It extends the persistence rules in
[PERSISTENCE_STATE_CLASSIFICATION.md](/Users/bene/code/prism/docs/PERSISTENCE_STATE_CLASSIFICATION.md)
with a concrete on-disk layout.

## Goal

PRISM currently mixes two different kinds of state inside repo-local `.prism`:

- published repo knowledge that should travel with the repository
- machine-local runtime state that should stay out of git and out of the repo tree

The target design is:

- keep only repo-owned published knowledge in `<repo>/.prism`
- move non-published state to `~/.prism`
- model scope by explicit identities such as `repo_id` and `worktree_id`, not by
  accidental file placement

This split must preserve PRISM's three persistence planes:

- repo-published authority in `<repo>/.prism`
- shared runtime authority in `~/.prism`
- worktree-local hot acceleration in `~/.prism`

## Ownership Rule

Repo `.prism` is for published repo truth only.

That means the repo tree should contain exactly the PRISM artifacts that are safe
to commit, review, clone, and inherit in a fresh checkout:

- `.prism/plans/`
- `.prism/concepts/`
- `.prism/contracts/`
- `.prism/memory/`

Everything else moves out of the repo tree.

In particular, the following are not repo-published truth and therefore do not
belong in repo `.prism`:

- SQLite runtime stores such as `cache.db`
- WAL or SHM sidecars
- local database backups
- daemon logs
- HTTP URI handoff files
- runtime status snapshots
- MCP session seeds
- MCP call logs
- dogfooding and validation feedback logs
- future imported or cached external data

## Scope Model

`~/.prism` should separate state along two axes:

- logical repo scope: shared across clones and worktrees of the same repository
- worktree scope: specific to one local checkout path and its running processes

The filesystem layout should be driven by explicit identities:

- `project_id`: optional higher-level coordination scope spanning multiple repos
- `repo_id`: stable identity for the logical repository
- `worktree_id`: stable identity for one local checkout or worktree
- `instance_id`: runtime process identity for rotating logs and runtime records
- `source_kind`: where the information came from

The important rule is that storage paths do not replace these identities. The
data model must still record `project_id`, `repo_id`, `worktree_id`,
`branch_ref`, `session_id`, and `instance_id` internally when that scope matters.

## Repo Tree Layout

The repo tree stays intentionally small:

```text
<repo>/
  .prism/
    concepts/
      events.jsonl
      relations.jsonl
    contracts/
      events.jsonl
    memory/
      events.jsonl
    plans/
      index.jsonl
      active/
      archived/
```

Design constraints:

- No logs or caches in repo `.prism`.
- No user-local diagnostics in repo `.prism`.
- No worktree-specific runtime files in repo `.prism`.
- Promotion into repo `.prism` must remain explicit and reviewable.

## Home Layout

`~/.prism` becomes the home for user-local PRISM state.

```text
~/.prism/
  VERSION
  auth/
    principals/
    tokens/
  config/
    user.toml
  projects/
    <project_id>/
      project.json
      coordination/
        state.db
        state.db-shm
        state.db-wal
  repos/
    <repo_id>/
      repo.json
      shared/
        runtime/
          state.db
          state.db-shm
          state.db-wal
        backups/
          state.db.*.bak
      feedback/
        validation_feedback.jsonl
      imports/
        github/
        benchmarks/
        replay/
      worktrees/
        <worktree_id>/
          worktree.json
          acceleration/
            checkpoints/
            projections/
            query-cache/
          mcp/
            state/
              prism-mcp-http-uri
              prism-mcp-runtime.json
              prism-mcp-session-seed.json
            logs/
              prism-mcp-daemon.log
              prism-mcp-daemon.<instance_id>.log
              prism-mcp-call-log.jsonl
              prism-mcp-call-log.<instance_id>.jsonl
```

## Home Resolution

`~/.prism` is the default visible home, not a hardcoded invariant.

Resolution rule:

- use `PRISM_HOME` when it is set
- otherwise default to `$HOME/.prism`
- keep the scope model independent from the concrete root path so XDG or other
  platform-specific homes can be added later without redefining repo semantics

This matters immediately for tests, CI, containers, and future multi-platform
support.

## Current Implementation Status

The current codebase already implements the first half of this split:

- `PRISM_HOME` override support with `$HOME/.prism` as the default
- repo-scoped shared runtime storage at
  `~/.prism/repos/<repo_id>/shared/runtime/state.db`
- repo-scoped validation feedback under `feedback/`
- worktree-scoped MCP URI, runtime, session-seed, and log paths under
  `worktrees/<worktree_id>/mcp/`
- opportunistic migration from repo-local `.prism` runtime files to the new home
  layout
- `repo.json` and `worktree.json` manifests for discovery and cleanup metadata

The following remain architectural targets rather than fully landed behavior:

- project-scoped coordination storage under `projects/<project_id>/`
- first-class writers for worktree-local acceleration artifacts
- principal/auth storage under `auth/`
- automated cleanup and garbage-collection workflows

## Source Buckets

Each subtree under `~/.prism/repos/<repo_id>/` has one owner and one kind of
information source.

## Three Local Planes Under `~/.prism`

`~/.prism` should not be treated as one undifferentiated local bucket. It should
mirror the remaining two persistence planes explicitly:

- `shared/`: shared runtime authority for the logical repo on this machine
- `worktrees/<worktree_id>/acceleration/`: optional worktree-local hot acceleration
- `worktrees/<worktree_id>/mcp/`: process-facing MCP runtime files and logs

That means the local design is:

- repo `.prism` for published truth
- `~/.prism/projects/<project_id>/` for optional cross-repo coordination scope
- `~/.prism/.../shared/` for authoritative mutable runtime state
- `~/.prism/.../worktrees/<worktree_id>/acceleration/` for rebuildable fast paths

The project scope is intentionally above repo scope:

- a project may include multiple repos
- a repo may participate in zero or one project in the simple local model
- project-scoped data should contain only genuinely cross-repo coordination state
- repo-scoped runtime truth should not be promoted upward just because a repo is
  part of a project

### `shared/`

This is the local home of shared runtime authority.

It contains:

- the authoritative shared runtime database, proposed as `state.db`
- WAL and SHM sidecars
- local backup files
- future authoritative runtime journals or compaction outputs that belong to the
  shared runtime plane

This directory is repo-scoped rather than worktree-scoped.

This is where the optional local shared-runtime backend lives. On a single machine
with multiple worktrees, PRISM should have one authoritative shared runtime store
per `repo_id`, not one SQLite authority per checkout path.

The important consequence is:

- `state.db` is not a cache
- `state.db` should hold shared mutable runtime truth
- worktree divergence must be represented inside the data model with `worktree_id`
  and related scope fields, not by creating separate authoritative databases

Representative contents of shared runtime authority:

- coordination runtime state
- shared mutable plans or execution overlays that are not yet published repo truth
- claims, handoffs, reviews, and other continuity state
- crash-sensitive journals for authored local/session state
- persisted state needed so different local runtimes can see the same repo-scoped
  live truth

### `projects/<project_id>/coordination/`

This is the optional higher-level coordination scope for work that spans multiple
repos.

It exists so PRISM can support cross-repo plans and workflow continuity without
collapsing all runtime state into one machine-global store.

Representative contents:

- cross-repo plans
- project-scoped claims, handoffs, and reviews
- shared artifacts that belong to a multi-repo workflow
- coordination records that reference more than one `repo_id`

Rules:

- this is coordination scope, not a replacement for repo runtime state
- repo graph/runtime truth remains under `repo_id`
- project state should reference repos explicitly rather than absorb their local
  runtime authority
- this scope is the right semantic bridge to a future shared Postgres backend

### `worktrees/<worktree_id>/acceleration/`

This is optional worktree-local hot acceleration state.

It is explicitly not authoritative. It exists to make one checkout fast without
changing the semantic truth of the shared runtime plane.

Representative contents:

- worktree-local checkpoints
- hydrated projections
- materialized read models
- cold-query acceleration files
- request-path helper caches

Rules:

- safe to delete and rebuild
- scoped to one checkout path
- may depend on branch or current filesystem reality
- must never become the only source of mutable truth
- should be invalidated aggressively when worktree identity or branch reality changes

### `feedback/`

This is user-local but repo-scoped evidence about PRISM quality.

It contains:

- dogfooding entries
- validation feedback
- future replay-seed material that should not be committed to the repo

This should be shared across worktrees of the same repo. A feedback case is about
PRISM behavior on the logical repository, even when it was observed from a single
checkout.

### `imports/`

This is for non-authored data copied or cached from external sources.

Examples:

- GitHub issue or PR material cached for local workflows
- benchmark corpora and replay fixtures
- imported validation bundles
- future remote retrieval caches

The point of this bucket is to avoid mixing imported external evidence with either
published repo truth or PRISM's own runtime database.

### `worktrees/<worktree_id>/mcp/state/`

This is checkout-specific MCP runtime state.

It contains:

- the HTTP URI handoff file
- runtime status JSON
- session seed state

These files are tied to one local checkout path and the processes attached to it.
They must not bleed across worktrees.

### `worktrees/<worktree_id>/mcp/logs/`

This is checkout-specific process output.

It contains:

- current daemon log
- rotated daemon logs
- current call log
- rotated call logs

Logs should live next to the worktree runtime state they describe, not next to
published repo knowledge.

## Current To Target Mapping

| Current path | Target path | Scope | Why |
| --- | --- | --- | --- |
| `<repo>/.prism/plans/**` | unchanged | repo | published repo truth |
| `<repo>/.prism/concepts/**` | unchanged | repo | published repo truth |
| `<repo>/.prism/contracts/**` | unchanged | repo | published repo truth |
| `<repo>/.prism/memory/**` | unchanged | repo | published repo truth |
| `<repo>/.prism/cache.db*` | `~/.prism/repos/<repo_id>/shared/runtime/state.db*` | shared runtime authority | repo-scoped local mutable truth |
| `<repo>/.prism/backups/cache.db*.bak` | `~/.prism/repos/<repo_id>/shared/backups/state.db*.bak` | shared runtime authority | local recovery material for shared runtime state |
| `<repo>/.prism/validation_feedback.jsonl` | `~/.prism/repos/<repo_id>/feedback/validation_feedback.jsonl` | repo-local user state | not publishable repo truth |
| `<repo>/.prism/prism-mcp-http-uri` | `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/state/prism-mcp-http-uri` | worktree | process handoff file |
| `<repo>/.prism/prism-mcp-runtime.json` | `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/state/prism-mcp-runtime.json` | worktree | local runtime snapshot |
| `<repo>/.prism/prism-mcp-session-seed.json` | `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/state/prism-mcp-session-seed.json` | worktree | local session continuity |
| `<repo>/.prism/prism-mcp-daemon.log` | `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/logs/prism-mcp-daemon.log` | worktree | process log |
| `<repo>/.prism/prism-mcp-daemon.<id>.log` | `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/logs/` | worktree | rotated process log |
| `<repo>/.prism/prism-mcp-call-log.jsonl` | `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/logs/prism-mcp-call-log.jsonl` | worktree | local call trace |
| `<repo>/.prism/prism-mcp-call-log.<id>.jsonl` | `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/logs/` | worktree | rotated call trace |

## Identity Files

Two small metadata files make the hierarchy manageable:

- `project.json`
  - stores `project_id`
  - stores repo membership or project descriptor metadata
  - stores creation and last-seen timestamps
- `repo.json`
  - stores `repo_id`
  - stores canonical root or origin metadata used to recognize the repo
  - stores creation and last-seen timestamps
- `worktree.json`
  - stores `worktree_id`
  - stores canonical absolute path
  - stores optional branch and last-seen timestamps

These files are for discovery, cleanup, and debugging. They do not replace the
actual runtime records inside the database.

They are also an explicit exception to the repo-portable path contract in
[`PATH_IDENTITY_CONTRACT.md`](PATH_IDENTITY_CONTRACT.md):
`repo.json` and `worktree.json` may store canonical locator paths because they
are local discovery metadata, not portable semantic repo state.

## Identity Derivation

`repo_id` and `worktree_id` need stricter rules than simple path-joins.

The intended policy is:

- `worktree_id` may be derived from the canonical checkout root because it is
  explicitly checkout-local
- `repo_id` should represent the logical repo, not one specific worktree path
- the filesystem layout must not be the only place that scope is inferred

The current implementation keeps the existing local-ID scheme for compatibility:

- `repo_id` is derived from the Git common dir path when available
- otherwise `repo_id` falls back to the canonical repo root path
- `worktree_id` is derived from the canonical repo root path
- `repo.json` records the locator kind and locator path that produced the current
  `repo_id`

That is good enough for the current local-home migration, but it is intentionally
not the final logical identity story. If PRISM later adopts a stronger repo
identity policy, it should migrate or alias these ids explicitly rather than
silently changing the meaning of an existing `repo_id`.

## Lifecycle And Cleanup

This layout will accumulate stale local state over time, so cleanup semantics are
part of the design rather than an afterthought.

The concrete retention policy for those semantics lives in
[PRISM_HOME_RETENTION_AND_GC.md](/Users/bene/code/prism/docs/PRISM_HOME_RETENTION_AND_GC.md).

Rules:

- `repo.json`, `worktree.json`, and `project.json` should carry `last_seen`
  metadata so cleanup can identify cold directories safely
- old worktree directories are cleanup candidates once the checkout path no
  longer exists or has not been seen for a long time
- `acceleration/` contents are always disposable and should be the first eviction
  target under storage pressure
- rotated logs, crash leftovers, and old backups should be bounded and reclaimable
- cleanup must never delete repo-published truth in `<repo>/.prism`
- cleanup must never treat imported evidence or feedback logs as authoritative
  runtime truth

## Compatibility Rules

The migration should be single-write and fallback-read.

Rules:

- New code writes only to the new target path.
- During migration, reads may fall back to old repo-local paths when the new file
  does not exist yet.
- PRISM should migrate old files forward opportunistically on startup.
- Repo `.prism` should stop receiving new non-published files immediately once the
  path layer exists.
- The path resolution logic should live in one dedicated module instead of being
  reconstructed in `prism-core`, `prism-cli`, `prism-mcp`, tests, and scripts.
- The same module should understand `project_id`, `repo_id`, and `worktree_id`
  scopes explicitly instead of inferring them from ad hoc path joins.
- The module should expose separate APIs for repo-published paths, shared-runtime
  paths, project-coordination paths, and worktree-acceleration paths so call
  sites cannot silently blur them.

## Migration Plan

### Phase 1: Introduce a path layer

Add one shared path module that exposes:

- project-scoped coordination paths under `~/.prism/projects/<project_id>/...`
- repo-published paths under `<repo>/.prism`
- repo-scoped home paths under `~/.prism/repos/<repo_id>/...`
- worktree-scoped home paths under `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/...`

No call site should keep hardcoding `root.join(".prism")` for runtime files after
this phase.

### Phase 2: Move non-published files

Switch the following call sites first:

- shared runtime `cache.db` resolution, renamed to `state.db`
- validation feedback
- MCP daemon URI and runtime files
- daemon log resolution
- call log resolution
- session seed resolution
- launcher scripts and benchmark helpers

In the same phase, introduce explicit homes for worktree-local acceleration
artifacts even if the first implementation only creates the directories and does
not yet write new checkpoint files there.

Project-scoped coordination storage can land later as an additive phase once the
repo/worktree split is stable. The important thing now is to reserve the scope in
the architecture and path model.

### Phase 3: Migrate old data

On startup:

- create `repo_id` and `worktree_id` directories as needed
- move existing repo-local runtime files into the new home hierarchy
- leave repo-published plans, concepts, contracts, and memories in place
- tolerate missing old files and partial migrations

### Phase 4: Tighten the contract

After the migration has settled:

- reject creation of new non-published files in repo `.prism`
- update docs and tests to treat repo `.prism` as publishable-only
- if implementation phases temporarily preserve the old `cache.db` filename during
  migration, complete the rename to `state.db` once the location split is stable

Given the three-plane model, `state.db` is the better target name than
`runtime.db` or `cache.db`, because this file is authoritative shared runtime
state rather than a disposable cache.

## Non-Goals

- This design does not move published plans, concepts, contracts, or repo memory
  out of the repo tree.
- This design does not force a remote shared backend; local SQLite remains
  first-class.
- This design does not require one database per worktree.
- This design does not require one MCP daemon per worktree.
- This design does not make all local runtime state machine-global by default.

## Summary

The split is intentionally strict:

- repo `.prism` becomes publishable repo knowledge only
- `~/.prism/projects/<project_id>/` is the future home for optional cross-repo
  coordination state
- `~/.prism/repos/<repo_id>/shared/` holds authoritative shared runtime state
- `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/acceleration/` holds optional
  rebuildable hot acceleration state
- worktree-specific MCP runtime artifacts live under `worktrees/<worktree_id>/mcp/`
- external or user-local evidence gets its own buckets instead of being mixed into
  runtime storage or repo truth
