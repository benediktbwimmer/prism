# storage-boundary-redesign: separate true shared runtime state from worktree-local semantic cache state, introduce explicit shared-vs-local store ownership and paths, migrate existing repos safely, and validate that cross-worktree shared semantics remain correct while local semantic cache state no longer bloats the shared runtime db.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:429a8a08b9486fe3453978ffe13f278769076b053e201576ee3dcf96f9785b61`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 7 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn46s1pk7s8wdbwrv302ws1g`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `7`

## Goal

storage-boundary-redesign: separate true shared runtime state from worktree-local semantic cache state, introduce explicit shared-vs-local store ownership and paths, migrate existing repos safely, and validate that cross-worktree shared semantics remain correct while local semantic cache state no longer bloats the shared runtime db.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn46s1pk7s8wdbwrv302ws1g.jsonl`

## Root Nodes

- `coord-task:01kn46t33h51asymhpnse5h0g1`

## Nodes

### Classify persisted domains into shared runtime, worktree-local cache, and process-local hot state

- Node id: `coord-task:01kn46t33h51asymhpnse5h0g1`
- Kind: `decide`
- Status: `completed`
- Summary: Classification decision recorded. Shared runtime should stay lean and hold only true cross-worktree continuity: coordination continuity, episodic memory, session/shared concepts, principal registry, and the authoritative outcome journal. Worktree-local cache should own graph/history/projections/workspace tree, curator state, and the heavy derived outcome anchor/index layer. Process-local hot state remains published generations, worker queues, and ephemeral caches.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Every persisted domain has an explicit owner and authority class, including graph, history, projections, workspace tree, coordination, episodic memory, outcomes, curator state, and principal registry. [any]
- The redesign records which domains must remain shared across worktrees and which must become worktree-local. [any]

### Introduce distinct path and bootstrap defaults for worktree cache db vs shared runtime db

- Node id: `coord-task:01kn46t35msk4qky98byxn789j`
- Kind: `edit`
- Status: `completed`
- Summary: Split the worktree cache db path from the shared runtime db path. PrismPaths now exposes a dedicated worktree cache db/bootstrap path, cache_path(root) points the local semantic store at that worktree db, CLI/runtime status surfaces report the worktree cache path, and validations were updated to assert the new ownership boundary between local cache and shared runtime persistence.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Workspace cache paths no longer resolve to the shared runtime db by default. [any]
- Session/bootstrap wiring can open a worktree-local cache store and a separate shared runtime store without aliasing them accidentally. [any]

### Split store interfaces so shared runtime and local semantic cache expose different capabilities

- Node id: `coord-task:01kn46t37cc06aaktw6z4rtsbm`
- Kind: `edit`
- Status: `completed`
- Summary: Completed the shared-runtime interface split. SharedRuntimeStore no longer exposes the full Store surface; it now wraps only narrow shared-runtime capabilities via CoordinationJournal, CoordinationCheckpointStore, ColdQueryStore, EventJournalStore, and MaterializationStore, plus explicit shared-runtime helper methods. WorkspaceSession and memory-refresh paths were updated to use those narrower capabilities, the focused shared-runtime tests passed, cargo test --workspace passed, and the release daemon was rebuilt and restarted successfully on the new split.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Shared runtime code no longer wraps the full local semantic cache store surface as if it were one broad sqlite authority. [any]
- Local semantic cache and shared runtime code paths depend on narrower role-specific traits or types with explicit ownership boundaries. [any]

### Decide ownership, retention, and compaction rules for outcomes and curator state under the split

- Node id: `coord-task:01kn46t38scs0cybmvnbw7ws7r`
- Kind: `decide`
- Status: `completed`
- Summary: Ownership and retention decision recorded. Curator becomes fully worktree-local. Outcomes split into a shared authoritative journal for cross-worktree task replay and continuity, plus a worktree-local derived anchor/index/read-model layer for code-oriented queries. The local outcome layer must include explicit retention/compaction so anchor rows and derived projections stay bounded instead of growing without limit.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- The redesign explicitly decides whether outcomes and curator state are shared, worktree-local, or split across both planes. [any]
- The redesign defines retention or compaction rules that prevent unbounded shared-runtime growth for the chosen ownership model. [any]

### Migrate existing repos by moving worktree-local semantic tables out of the shared runtime db

- Node id: `coord-task:01kn46t3a8m0agn27rwb66gk9h`
- Kind: `edit`
- Status: `completed`
- Summary: Migrated clearly worktree-local semantic state out of the shared runtime DB into the dedicated worktree cache DB, added bootstrap migration, and validated the live repo now holds graph/history/projection/workspace-tree/curator state in the worktree cache while the shared DB is cleared of those local tables.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Existing repos can migrate without losing authoritative shared coordination or memory state. [any]
- The migration relocates worktree-local semantic cache tables and rehydrates bootstrap state from the new split correctly. [any]

### Validate cross-worktree shared semantics, migration safety, and post-split db health

- Node id: `coord-task:01kn46t3bs7y9qppsnf8wkp2tx`
- Kind: `validate`
- Status: `completed`
- Summary: Validated the split with cargo test --workspace, release rebuild and daemon restart, healthy live MCP status, and direct SQLite row checks showing the shared runtime DB no longer contains local graph/history/projection/workspace-tree/curator state while the worktree cache DB does.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Cross-worktree shared memory and coordination semantics still replay correctly after the split. [any]
- Validation demonstrates smaller shared-runtime db scope and correct local semantic cache ownership after migration. [any]

## Edges

- `plan-edge:coord-task:01kn46t35msk4qky98byxn789j:depends-on:coord-task:01kn46t33h51asymhpnse5h0g1`: `coord-task:01kn46t35msk4qky98byxn789j` depends on `coord-task:01kn46t33h51asymhpnse5h0g1`
- `plan-edge:coord-task:01kn46t37cc06aaktw6z4rtsbm:depends-on:coord-task:01kn46t33h51asymhpnse5h0g1`: `coord-task:01kn46t37cc06aaktw6z4rtsbm` depends on `coord-task:01kn46t33h51asymhpnse5h0g1`
- `plan-edge:coord-task:01kn46t38scs0cybmvnbw7ws7r:depends-on:coord-task:01kn46t33h51asymhpnse5h0g1`: `coord-task:01kn46t38scs0cybmvnbw7ws7r` depends on `coord-task:01kn46t33h51asymhpnse5h0g1`
- `plan-edge:coord-task:01kn46t3a8m0agn27rwb66gk9h:depends-on:coord-task:01kn46t35msk4qky98byxn789j`: `coord-task:01kn46t3a8m0agn27rwb66gk9h` depends on `coord-task:01kn46t35msk4qky98byxn789j`
- `plan-edge:coord-task:01kn46t3a8m0agn27rwb66gk9h:depends-on:coord-task:01kn46t37cc06aaktw6z4rtsbm`: `coord-task:01kn46t3a8m0agn27rwb66gk9h` depends on `coord-task:01kn46t37cc06aaktw6z4rtsbm`
- `plan-edge:coord-task:01kn46t3a8m0agn27rwb66gk9h:depends-on:coord-task:01kn46t38scs0cybmvnbw7ws7r`: `coord-task:01kn46t3a8m0agn27rwb66gk9h` depends on `coord-task:01kn46t38scs0cybmvnbw7ws7r`
- `plan-edge:coord-task:01kn46t3bs7y9qppsnf8wkp2tx:depends-on:coord-task:01kn46t3a8m0agn27rwb66gk9h`: `coord-task:01kn46t3bs7y9qppsnf8wkp2tx` depends on `coord-task:01kn46t3a8m0agn27rwb66gk9h`

