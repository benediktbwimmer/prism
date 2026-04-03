# refresh-01: make workspace refresh behavior sound and optimized by explaining and reducing full refreshes, fixing pathological deferred behavior, and tightening overall refresh performance semantics

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:3da1007e002a346fdc6eb6a2df746c8615bb25b6edf716a9cc687dd89c51ae8a`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn0sh5q9vv9j2q2n6jwxhtep`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `6`

## Goal

refresh-01: make workspace refresh behavior sound and optimized by explaining and reducing full refreshes, fixing pathological deferred behavior, and tightening overall refresh performance semantics

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn0sh5q9vv9j2q2n6jwxhtep.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn0sh5q9vv9j2q2n6jwxhtep.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn0shaxw8prtd5jrvtbxtn6n`

## Nodes

### Baseline the live refresh paths and quantify full vs deferred triggers

- Node id: `coord-task:01kn0shaxw8prtd5jrvtbxtn6n`
- Kind: `investigate`
- Status: `completed`
- Summary: Baseline captured from live runtimeStatus/runtimeLogs and source inspection: a clean-state fallback probe in `refresh_fs_nonblocking` was surfacing `DeferredBusy` while `fsObserved == fsApplied`, producing false deferred refresh events; recent live full refreshes were about 2563 ms and repeated deferred refreshes also showed snapshot-revision holds up to about 20.9s and 22.7s.

#### Acceptance

- Refresh samples identify where full rescans are coming from and which subphase dominates the slow path. [any]
- Deferred behavior is broken down into fs refresh, snapshot, and reload timing with a clear pathology statement. [any]

### Eliminate unnecessary full refreshes and reduce unavoidable rescan cost

- Node id: `coord-task:01kn0shet61b27jzmdh97t3mmn`
- Kind: `edit`
- Status: `completed`
- Summary: Reduced unavoidable fallback full-refresh cost by teaching `WorkspaceTreeSnapshot` to persist directory stat metadata and using cached file fingerprints plus selective changed-directory subtree scans during `plan_full_refresh`. Added focused core/store regressions, rebuilt the release daemon, and verified live full-refresh cost dropped to about 1215 ms from earlier samples at 2549 ms, 6217 ms, and 25974 ms.

#### Acceptance

- Unnecessary full refresh triggers are removed or downgraded to scoped incremental refreshes. [any]
- Remaining full refreshes have a justified trigger model and materially lower latency. [any]

### Fix pathological deferred refresh behavior

- Node id: `coord-task:01kn0shjk66y4khmr563jwk5nn`
- Kind: `edit`
- Status: `completed`
- Summary: Eliminated the remaining pathological deferred-refresh admission path by making coordination, claim, and artifact mutations fail fast on busy runtime-sync and refresh locks. Added regression coverage for traced coordination admission-busy behavior and claim admission-busy behavior, then rebuilt and restarted the release daemon. Live dirty-worktree validation now shows deferred status with 0 ms hold instead of the earlier multi-second deferred runtime-sync hold.

#### Acceptance

- Deferred refreshes no longer spin or stall pathologically under active edits. [any]
- Slow deferred calls have a clear bounded cause with corrected behavior or explicit fallbacks. [any]

### Harden the refresh state machine and fallback semantics

- Node id: `coord-task:01kn0shpfzn7pdnhw9m0qrchp9`
- Kind: `decide`
- Status: `completed`
- Summary: Separated fallback cached rescans from true full rebuilds by introducing a distinct `rescan` refresh mode/status through prism-core and prism-mcp. Fallback snapshot diff paths now record `rescan` in session/runtime status and keep `fullRebuildCount`/`workspaceReloaded` reserved for true full rebuilds, with focused regressions in prism-core and workspace_runtime plus a release rebuild/restart to refresh the live daemon.

#### Acceptance

- Refresh path semantics are coherent and intentionally separated between scoped refresh, rescan, and deferred runtime sync. [any]
- Fallback behavior is minimal, justified, and measurable. [any]

### Validate refresh performance and regression coverage

- Node id: `coord-task:01kn0sht5tag5vmmjqn127rcvd`
- Kind: `validate`
- Status: `completed`
- Summary: Validated the refresh plan end-to-end with focused regressions across the repaired surfaces: core watch-driven refresh still updates live session state, deferred runtime-sync reporting still skips reload work, the first mutation after a workspace refresh still avoids persisted reload, and persisted notes still reload when freshness requires it. Combined with the new `rescan` regressions and a healthy release rebuild/restart, the plan now has both semantic and behavioral coverage for the refreshed runtime path.

#### Acceptance

- Validation demonstrates fewer or rarer full refreshes, corrected deferred behavior, and improved latency. [any]
- Regression coverage protects the intended refresh semantics going forward. [any]

## Edges

- `plan-edge:coord-task:01kn0shet61b27jzmdh97t3mmn:depends-on:coord-task:01kn0shaxw8prtd5jrvtbxtn6n`: `coord-task:01kn0shet61b27jzmdh97t3mmn` depends on `coord-task:01kn0shaxw8prtd5jrvtbxtn6n`
- `plan-edge:coord-task:01kn0shjk66y4khmr563jwk5nn:depends-on:coord-task:01kn0shaxw8prtd5jrvtbxtn6n`: `coord-task:01kn0shjk66y4khmr563jwk5nn` depends on `coord-task:01kn0shaxw8prtd5jrvtbxtn6n`
- `plan-edge:coord-task:01kn0shpfzn7pdnhw9m0qrchp9:depends-on:coord-task:01kn0shaxw8prtd5jrvtbxtn6n`: `coord-task:01kn0shpfzn7pdnhw9m0qrchp9` depends on `coord-task:01kn0shaxw8prtd5jrvtbxtn6n`
- `plan-edge:coord-task:01kn0sht5tag5vmmjqn127rcvd:depends-on:coord-task:01kn0shet61b27jzmdh97t3mmn`: `coord-task:01kn0sht5tag5vmmjqn127rcvd` depends on `coord-task:01kn0shet61b27jzmdh97t3mmn`
- `plan-edge:coord-task:01kn0sht5tag5vmmjqn127rcvd:depends-on:coord-task:01kn0shjk66y4khmr563jwk5nn`: `coord-task:01kn0sht5tag5vmmjqn127rcvd` depends on `coord-task:01kn0shjk66y4khmr563jwk5nn`
- `plan-edge:coord-task:01kn0sht5tag5vmmjqn127rcvd:depends-on:coord-task:01kn0shpfzn7pdnhw9m0qrchp9`: `coord-task:01kn0sht5tag5vmmjqn127rcvd` depends on `coord-task:01kn0shpfzn7pdnhw9m0qrchp9`

