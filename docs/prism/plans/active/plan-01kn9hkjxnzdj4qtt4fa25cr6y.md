# Rewrite tracked .prism to signed snapshot publication

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:457ee4883227b043af2210bffc71eee48cbec89070f0dbdd69eefcbe18d3c347`
- Source logical timestamp: `unknown`
- Source snapshot: `11 nodes, 18 edges, 5 overlays`

## Overview

- Plan id: `plan:01kn9hkjxnzdj4qtt4fa25cr6y`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `11`
- Edges: `18`

## Goal

Replace repo-committed append-log authority with compact signed snapshot state in tracked .prism, move fine-grained operational history into runtime/shared journals, and preserve cold-clone correctness, Git-native time travel, and trusted publisher attribution.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn9hkjxnzdj4qtt4fa25cr6y.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn9hmd10c4x9nzzmp1b5h53a`

## Nodes

### Lock the tracked .prism snapshot authority contract

- Node id: `coord-task:01kn9hmd10c4x9nzzmp1b5h53a`
- Kind: `edit`
- Status: `completed`
- Summary: Locked the tracked .prism snapshot authority contract in SPEC, persistence classification, and protected-signature docs; retired the stale repo-log replay contract; promoted snapshot-authority semantics into repo concepts and contracts.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Spec states Git is the durable history, branching, and time-travel substrate for tracked .prism [any]
- Spec states runtime/shared state owns the fine-grained append-only operational journal [any]
- Spec states tracked .prism is the repo-published authoritative snapshot format, not a repo-committed operational event log [any]

### Define the tracked snapshot file layout and shard strategy

- Node id: `coord-task:01kn9hmj8kajwtre6xq83jbt12`
- Kind: `edit`
- Status: `completed`
- Summary: Defined the tracked snapshot shard strategy: stable object-oriented files, a single stable manifest boundary, and rebuildable indexes separated from authority files.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- File layout rules preserve cold-clone self-containment without forcing repo-committed append logs [any]
- Manifest and index responsibilities are separated cleanly from per-object snapshot files [any]
- Snapshot layout favors stable shard- or object-oriented files over monolithic per-domain blobs [any]

### Define signed publish manifests, chaining, and publish atomicity

- Node id: `coord-task:01kn9hmqncfe5nfm4chtm6dt2s`
- Kind: `edit`
- Status: `completed`
- Summary: Defined the publish-manifest contract: one stable manifest per publish boundary, required attestation fields, previous-manifest chaining, and atomic publication semantics.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Manifest handling stays snapshot-shaped in HEAD instead of becoming a new append pile [any]
- Publish boundary is defined as one coherent PRISM publication step rather than a fuzzy sequence of writes [any]
- Publish manifest schema covers publisher identity, work context, snapshot file digests, and prior-manifest continuity [any]

### Define the runtime or shared journal model and snapshot linkage

- Node id: `coord-task:01kn9hn12c7s4bymzxdwea8qfn`
- Kind: `edit`
- Status: `completed`
- Summary: Defined the runtime/shared journal contract, local-versus-shared behavior, and coarse snapshot-to-journal linkage without making published snapshots depend on journal availability.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Publish manifests may reference runtime journal ranges without making snapshots depend on journal availability [any]
- Retention and compaction rules preserve high-resolution audit where desired without pushing that payload back into Git [any]
- Runtime or shared journal is the canonical home for fine-grained operational history between publish boundaries [any]

### Define which coordination state is durable enough to publish

- Node id: `coord-task:01kn9hn84cet00qdmq429cc7cg`
- Kind: `edit`
- Status: `completed`
- Summary: Defined the durable coordination subset for tracked snapshots and explicitly excluded leases, heartbeats, watcher churn, and short-lived runtime continuity noise.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Ephemeral lease, heartbeat, and short-lived runtime continuity data are explicitly excluded from tracked snapshots [any]
- Published coordination scope is limited to state later agents need to understand current repo intent and execution state [any]
- Snapshot contents for plans and coordination are stable enough for Git review and narrow merge surfaces [any]

### Implement tracked snapshot writers and signed publish manifests

- Node id: `coord-task:01kn9hnfwp7s16np7cf557cxwr`
- Kind: `edit`
- Status: `completed`
- Summary: Tracing the current tracked publication writers and protected-state signing path so snapshot writers and signed publish manifests can replace repo-committed append-log publication.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Manifest signing and verification replace per-event repo-log signing on the tracked repo path [any]
- Snapshot publication is deterministic enough for reviewable Git diffs and reproducible regeneration [any]
- Tracked .prism publication writes stable snapshot files and one updated manifest for each publish boundary [any]

### Replace tracked append-log surfaces with snapshot publication and snapshot-backed reads

- Node id: `coord-task:01kn9hnq7j6p60y8arjfrccw5r`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Fine-grained operational detail remains available through runtime or shared journals instead of being pushed back into Git [any]
- New repo-published writes stop growing the tracked append-log surfaces they replace [any]
- Tracked read paths resolve published state from snapshot artifacts and manifests rather than replaying repo-committed event logs [any]

### Rework hot and cold startup around snapshot restore and cheap journal reconciliation

- Node id: `coord-task:01kn9hnzqwjgfenrc5tf2jdxsw`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Cold startup verifies manifest and snapshot digests directly from the repo and loads current published state without historical repo replay [any]
- Hot startup restores from current snapshot state instead of replaying tracked repo event streams [any]
- Runtime or shared journal reconciliation is incremental and cheap enough to preserve practical hot restart performance [any]

### Build the migration bridge from legacy tracked event logs to the first snapshot publish

- Node id: `coord-task:01kn9hp5yrtkn0dh3fv6anbwb8`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Compatibility readers exist only as a temporary migration bridge and not as permanent dual authority [any]
- First snapshot-format publish records continuity to the final legacy tracked event-log checkpoint or digest [any]
- Migration path is deterministic and safe for repos that already carry event-log-era .prism history [any]

### Update projections, tooling, docs, and validation around the new authority model

- Node id: `coord-task:01kn9hpcdqsrjqcg23rate5ps8`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Docs and repo-facing surfaces describe tracked .prism as signed snapshot authority with Git-native history [any]
- Tooling and diagnostics reflect runtime-journal detail as operational state rather than tracked repo truth [any]
- Validation recipes cover snapshot publication, migration, startup restore, and Git-time-travel semantics [any]

### Validate cold-clone restore, Git time travel, startup performance, and published trust semantics

- Node id: `coord-task:01kn9hphz797055gppgx8dwfwa`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Cold clone can verify and load current published state from tracked snapshots and manifests without historical repo replay [any]
- Git checkout across commits and branches yields correct historical snapshot state [any]
- Hot restart and cold startup meet explicit post-rewrite performance targets while preserving signed publisher attribution [any]

## Edges

- `plan-edge:coord-task:01kn9hmj8kajwtre6xq83jbt12:depends-on:coord-task:01kn9hmd10c4x9nzzmp1b5h53a`: `coord-task:01kn9hmj8kajwtre6xq83jbt12` depends on `coord-task:01kn9hmd10c4x9nzzmp1b5h53a`
- `plan-edge:coord-task:01kn9hmqncfe5nfm4chtm6dt2s:depends-on:coord-task:01kn9hmd10c4x9nzzmp1b5h53a`: `coord-task:01kn9hmqncfe5nfm4chtm6dt2s` depends on `coord-task:01kn9hmd10c4x9nzzmp1b5h53a`
- `plan-edge:coord-task:01kn9hn12c7s4bymzxdwea8qfn:depends-on:coord-task:01kn9hmd10c4x9nzzmp1b5h53a`: `coord-task:01kn9hn12c7s4bymzxdwea8qfn` depends on `coord-task:01kn9hmd10c4x9nzzmp1b5h53a`
- `plan-edge:coord-task:01kn9hn84cet00qdmq429cc7cg:depends-on:coord-task:01kn9hmd10c4x9nzzmp1b5h53a`: `coord-task:01kn9hn84cet00qdmq429cc7cg` depends on `coord-task:01kn9hmd10c4x9nzzmp1b5h53a`
- `plan-edge:coord-task:01kn9hnfwp7s16np7cf557cxwr:depends-on:coord-task:01kn9hmj8kajwtre6xq83jbt12`: `coord-task:01kn9hnfwp7s16np7cf557cxwr` depends on `coord-task:01kn9hmj8kajwtre6xq83jbt12`
- `plan-edge:coord-task:01kn9hnfwp7s16np7cf557cxwr:depends-on:coord-task:01kn9hmqncfe5nfm4chtm6dt2s`: `coord-task:01kn9hnfwp7s16np7cf557cxwr` depends on `coord-task:01kn9hmqncfe5nfm4chtm6dt2s`
- `plan-edge:coord-task:01kn9hnfwp7s16np7cf557cxwr:depends-on:coord-task:01kn9hn12c7s4bymzxdwea8qfn`: `coord-task:01kn9hnfwp7s16np7cf557cxwr` depends on `coord-task:01kn9hn12c7s4bymzxdwea8qfn`
- `plan-edge:coord-task:01kn9hnfwp7s16np7cf557cxwr:depends-on:coord-task:01kn9hn84cet00qdmq429cc7cg`: `coord-task:01kn9hnfwp7s16np7cf557cxwr` depends on `coord-task:01kn9hn84cet00qdmq429cc7cg`
- `plan-edge:coord-task:01kn9hnq7j6p60y8arjfrccw5r:depends-on:coord-task:01kn9hnfwp7s16np7cf557cxwr`: `coord-task:01kn9hnq7j6p60y8arjfrccw5r` depends on `coord-task:01kn9hnfwp7s16np7cf557cxwr`
- `plan-edge:coord-task:01kn9hnzqwjgfenrc5tf2jdxsw:depends-on:coord-task:01kn9hn12c7s4bymzxdwea8qfn`: `coord-task:01kn9hnzqwjgfenrc5tf2jdxsw` depends on `coord-task:01kn9hn12c7s4bymzxdwea8qfn`
- `plan-edge:coord-task:01kn9hnzqwjgfenrc5tf2jdxsw:depends-on:coord-task:01kn9hnfwp7s16np7cf557cxwr`: `coord-task:01kn9hnzqwjgfenrc5tf2jdxsw` depends on `coord-task:01kn9hnfwp7s16np7cf557cxwr`
- `plan-edge:coord-task:01kn9hp5yrtkn0dh3fv6anbwb8:depends-on:coord-task:01kn9hmd10c4x9nzzmp1b5h53a`: `coord-task:01kn9hp5yrtkn0dh3fv6anbwb8` depends on `coord-task:01kn9hmd10c4x9nzzmp1b5h53a`
- `plan-edge:coord-task:01kn9hp5yrtkn0dh3fv6anbwb8:depends-on:coord-task:01kn9hnfwp7s16np7cf557cxwr`: `coord-task:01kn9hp5yrtkn0dh3fv6anbwb8` depends on `coord-task:01kn9hnfwp7s16np7cf557cxwr`
- `plan-edge:coord-task:01kn9hpcdqsrjqcg23rate5ps8:depends-on:coord-task:01kn9hnq7j6p60y8arjfrccw5r`: `coord-task:01kn9hpcdqsrjqcg23rate5ps8` depends on `coord-task:01kn9hnq7j6p60y8arjfrccw5r`
- `plan-edge:coord-task:01kn9hpcdqsrjqcg23rate5ps8:depends-on:coord-task:01kn9hnzqwjgfenrc5tf2jdxsw`: `coord-task:01kn9hpcdqsrjqcg23rate5ps8` depends on `coord-task:01kn9hnzqwjgfenrc5tf2jdxsw`
- `plan-edge:coord-task:01kn9hpcdqsrjqcg23rate5ps8:depends-on:coord-task:01kn9hp5yrtkn0dh3fv6anbwb8`: `coord-task:01kn9hpcdqsrjqcg23rate5ps8` depends on `coord-task:01kn9hp5yrtkn0dh3fv6anbwb8`
- `plan-edge:coord-task:01kn9hphz797055gppgx8dwfwa:depends-on:coord-task:01kn9hnzqwjgfenrc5tf2jdxsw`: `coord-task:01kn9hphz797055gppgx8dwfwa` depends on `coord-task:01kn9hnzqwjgfenrc5tf2jdxsw`
- `plan-edge:coord-task:01kn9hphz797055gppgx8dwfwa:depends-on:coord-task:01kn9hp5yrtkn0dh3fv6anbwb8`: `coord-task:01kn9hphz797055gppgx8dwfwa` depends on `coord-task:01kn9hp5yrtkn0dh3fv6anbwb8`

## Execution Overlays

- Node: `coord-task:01kn9hnq7j6p60y8arjfrccw5r`
  git execution status: `published`
  source ref: `task/prism-snapshot-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-snapshot-rewrite`
- Node: `coord-task:01kn9hnzqwjgfenrc5tf2jdxsw`
  git execution status: `published`
  source ref: `task/prism-snapshot-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-snapshot-rewrite`
- Node: `coord-task:01kn9hp5yrtkn0dh3fv6anbwb8`
  git execution status: `published`
  source ref: `task/prism-snapshot-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-snapshot-rewrite`
- Node: `coord-task:01kn9hpcdqsrjqcg23rate5ps8`
  git execution status: `published`
  source ref: `task/prism-snapshot-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-snapshot-rewrite`
- Node: `coord-task:01kn9hphz797055gppgx8dwfwa`
  git execution status: `published`
  source ref: `task/prism-snapshot-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-snapshot-rewrite`

