# Decouple shared-ref authority from daemon startup

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:5b0410b49e3b4a3dc7f966615f6877561fb379bd17eccbb24cbca6b11a9e6ae3`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 6 edges, 6 overlays`

## Overview

- Plan id: `plan:01knb0f1xztpmmbjjzetw9xbwg`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `6`

## Goal

Separate shared coordination refs as the authority/replication source from daemon startup by introducing one local materialized coordination snapshot/checkpoint for bootstrap, moving shared-ref fetch/verify/import onto an explicit sync path, reusing one in-memory coordination load across bootstrap layers, and validating that hot restart falls back to low-single-digit seconds without regressing correctness.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`
- Max fetch age seconds: `300`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01knb0f1xztpmmbjjzetw9xbwg.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01knb0g511dtavrvd5ban1x6jq`

## Nodes

### Define the authority vs materialized-runtime contract

- Node id: `coord-task:01knb0g511dtavrvd5ban1x6jq`
- Kind: `investigate`
- Status: `completed`
- Summary: Map the current daemon bootstrap, shared-ref hydration, and runtime-host reload paths; define which state remains authoritative in the shared ref versus which state becomes a local materialized startup snapshot keyed by shared-ref identity.

#### Bindings

- Anchor: `file:177`
- Anchor: `file:188`
- Anchor: `file:329`
- Anchor: `file:330`
- Anchor: `file:39`
- Anchor: `file:421`
- Anchor: `file:424`
- Anchor: `file:487`

#### Acceptance

- The current startup and reload path is reduced to one concrete contract boundary between shared-ref authority and local materialized startup state. [any]
- The contract identifies the shared-ref identity key needed to validate or invalidate the local materialized snapshot. [any]

### Design the local materialized coordination snapshot/checkpoint

- Node id: `coord-task:01knb0hdvk3pmkwcjkq0zzjgmq`
- Kind: `decide`
- Status: `completed`
- Summary: Define the on-disk startup artifact, invalidation keying, freshness rules, and rebuild path so daemon bootstrap can consume one local materialized coordination snapshot instead of rehydrating the shared ref directly.

#### Acceptance

- The snapshot/checkpoint shape, storage owner, and invalidation key are explicit enough to implement without re-reading the shared ref during startup. [any]
- The design states when startup can trust local materialized state and when an explicit sync/import must rebuild it. [any]

### Move shared-ref fetch, verification, and import into an explicit sync path

- Node id: `coord-task:01knb0hxhtfxwqc1nm64dzjckx`
- Kind: `edit`
- Status: `completed`
- Summary: Refactor shared-ref refresh, manifest verification, and record import so the expensive authority sync runs outside the daemon startup critical path and materializes the local startup snapshot as its output.

#### Acceptance

- Shared-ref authority ingestion is no longer required during daemon startup to serve a healthy runtime. [any]
- The sync path is the sole owner of shared-ref fetch, verification, and materialized snapshot rebuild work. [any]

### Remove repeated coordination reloads across bootstrap layers

- Node id: `coord-task:01knb0k8skvp53g3hswbh5tyf2`
- Kind: `edit`
- Status: `completed`
- Summary: Thread one already-loaded coordination runtime state through session bootstrap, workspace host, workspace runtime, and session construction so later layers reuse state instead of reloading coordination a second or third time.

#### Acceptance

- Bootstrap, runtime host, and session layers reuse one startup-loaded coordination state instead of rehydrating it again. [any]
- The extra post-build startup gap caused by repeated coordination reloads is removed or made directly observable if any residual work remains. [any]

### Make daemon startup consume only the local materialized snapshot

- Node id: `coord-task:01knb0nbyyzqs1bs768mxfmmvq`
- Kind: `edit`
- Status: `completed`
- Summary: Change session bootstrap and indexer startup so one local materialized coordination snapshot/checkpoint seeds the runtime, without direct shared-ref hydration on the critical path.

#### Acceptance

- Startup can reach a healthy daemon state by loading one local materialized coordination snapshot and building in-memory state once. [any]
- Bootstrap code no longer treats the shared coordination ref as a startup database. [any]

### Add startup boundary observability and validate hot restart timing

- Node id: `coord-task:01knb0nvvkchbygemh7gxc9b6a`
- Kind: `validate`
- Status: `completed`
- Summary: Instrument the new sync/startup boundaries, then validate with release builds and daemon restarts that hot restart drops from ~27-32s to low-single-digit seconds while daemon health remains correct.

#### Acceptance

- Startup logs expose separate timing for sync/import, local snapshot load, in-memory coordination build, and post-bootstrap runtime reuse. [any]
- Release restart validation shows hot restart returning to low-single-digit seconds or clearly isolates the remaining structural blocker. [any]

## Edges

- `plan-edge:coord-task:01knb0hdvk3pmkwcjkq0zzjgmq:depends-on:coord-task:01knb0g511dtavrvd5ban1x6jq`: `coord-task:01knb0hdvk3pmkwcjkq0zzjgmq` depends on `coord-task:01knb0g511dtavrvd5ban1x6jq`
- `plan-edge:coord-task:01knb0hxhtfxwqc1nm64dzjckx:depends-on:coord-task:01knb0hdvk3pmkwcjkq0zzjgmq`: `coord-task:01knb0hxhtfxwqc1nm64dzjckx` depends on `coord-task:01knb0hdvk3pmkwcjkq0zzjgmq`
- `plan-edge:coord-task:01knb0k8skvp53g3hswbh5tyf2:depends-on:coord-task:01knb0nbyyzqs1bs768mxfmmvq`: `coord-task:01knb0k8skvp53g3hswbh5tyf2` depends on `coord-task:01knb0nbyyzqs1bs768mxfmmvq`
- `plan-edge:coord-task:01knb0nbyyzqs1bs768mxfmmvq:depends-on:coord-task:01knb0hdvk3pmkwcjkq0zzjgmq`: `coord-task:01knb0nbyyzqs1bs768mxfmmvq` depends on `coord-task:01knb0hdvk3pmkwcjkq0zzjgmq`
- `plan-edge:coord-task:01knb0nvvkchbygemh7gxc9b6a:depends-on:coord-task:01knb0hxhtfxwqc1nm64dzjckx`: `coord-task:01knb0nvvkchbygemh7gxc9b6a` depends on `coord-task:01knb0hxhtfxwqc1nm64dzjckx`
- `plan-edge:coord-task:01knb0nvvkchbygemh7gxc9b6a:depends-on:coord-task:01knb0nbyyzqs1bs768mxfmmvq`: `coord-task:01knb0nvvkchbygemh7gxc9b6a` depends on `coord-task:01knb0nbyyzqs1bs768mxfmmvq`

## Execution Overlays

- Node: `coord-task:01knb0g511dtavrvd5ban1x6jq`
  git execution status: `coordination_published`
  source ref: `task/startup-materialized-snapshot`
  target ref: `origin/main`
  publish ref: `task/startup-materialized-snapshot`
- Node: `coord-task:01knb0hdvk3pmkwcjkq0zzjgmq`
  git execution status: `coordination_published`
  source ref: `task/startup-materialized-snapshot`
  target ref: `origin/main`
  publish ref: `task/startup-materialized-snapshot`
- Node: `coord-task:01knb0hxhtfxwqc1nm64dzjckx`
  git execution status: `coordination_published`
  source ref: `task/startup-materialized-snapshot`
  target ref: `origin/main`
  publish ref: `task/startup-materialized-snapshot`
- Node: `coord-task:01knb0k8skvp53g3hswbh5tyf2`
  git execution status: `coordination_published`
  source ref: `task/startup-materialized-snapshot`
  target ref: `origin/main`
  publish ref: `task/startup-materialized-snapshot`
- Node: `coord-task:01knb0nbyyzqs1bs768mxfmmvq`
  git execution status: `coordination_published`
  source ref: `task/startup-materialized-snapshot`
  target ref: `origin/main`
  publish ref: `task/startup-materialized-snapshot`
- Node: `coord-task:01knb0nvvkchbygemh7gxc9b6a`
  git execution status: `coordination_published`
  source ref: `task/startup-materialized-snapshot`
  target ref: `origin/main`
  publish ref: `task/startup-materialized-snapshot`

