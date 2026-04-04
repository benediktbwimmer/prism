# Implement first working federated runtime via runtime-targeted prism_query

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:b04f30d8e362facad01279e338c67ac43de9abe443a6ee5334dde1a9465a9c8f`
- Source logical timestamp: `unknown`
- Source snapshot: `7 nodes, 11 edges, 7 overlays`

## Overview

- Plan id: `plan:01kncfv6c7tzkc21aemhr0pns2`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `7`
- Edges: `11`

## Goal

Restore shared-ref runtime descriptors, execute authenticated remote prism_query reads keyed by runtime_id, surface them through prism.from("runtime-id"), and validate authority-aware two-runtime federation without relay or blob export.

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
- Snapshot plan shard: `.prism/state/plans/plan:01kncfv6c7tzkc21aemhr0pns2.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kncfw8xh4mephwpkg8ky3mem`

## Nodes

### Restore shared-ref runtime descriptors and live descriptor publication

- Node id: `coord-task:01kncfw8xh4mephwpkg8ky3mem`
- Kind: `edit`
- Status: `completed`
- Summary: Reintroduce runtime descriptors into shared coordination ref state and publish compact per-runtime descriptor shards with runtime_id, capabilities, commit, branch, freshness, and endpoint metadata.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Add operator-configured public URL publication for runtime descriptors

- Node id: `coord-task:01kncfwa2f2xbactnjb5bwmqf6`
- Kind: `edit`
- Status: `completed`
- Summary: Add the runtime configuration surface for an operator-managed public URL and publish that URL through runtime descriptors without introducing a PRISM-owned relay transport.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Implement authenticated remote prism_query execution keyed by runtime_id

- Node id: `coord-task:01kncfwbeab4tk0mq2wk147k9x`
- Kind: `edit`
- Status: `completed`
- Summary: Reintroduce peer runtime query transport as a bounded authenticated read-only prism_query path that resolves a runtime_id through shared coordination descriptors and executes against the remote query runtime under explicit capability checks.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Expose runtime-targeted query chaining as prism.from("runtime-id")

- Node id: `coord-task:01kncfwc8fnzkp12z98axm2ept`
- Kind: `edit`
- Status: `completed`
- Summary: Add query-side runtime targeting so prism_query can chain through prism.from("runtime-id") while default prism reads still execute against the local runtime.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Preserve authority classes, limits, and failure semantics for federated queries

- Node id: `coord-task:01kncfwdcddbj4xw025azyq6hc`
- Kind: `edit`
- Status: `completed`
- Summary: Ensure remote prism_query results remain explicitly peer-enriched, enforce bounded query limits remotely, and surface honest errors for missing runtime_id, capability denial, stale descriptors, and unreachable peers.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Update tool schemas, capabilities, and runtime docs for federated prism_query

- Node id: `coord-task:01kncfwe1x8a5qccafnj6wzamn`
- Kind: `edit`
- Status: `completed`
- Summary: Publish the new runtime-targeted query contract across capabilities, schema docs, and runtime/instructions surfaces so agents know to use runtime_id and prism.from("runtime-id") semantics.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Validate two-runtime local federation and public URL descriptor flow end to end

- Node id: `coord-task:01kncfwfa5ngfp1z2kfcp6vfy0`
- Kind: `edit`
- Status: `completed`
- Summary: Validate the first working federated runtime with two live worktrees by proving descriptor publication, remote prism_query success, capability denial, stale or missing runtime handling, and operator-managed public URL surfacing.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

## Edges

- `plan-edge:coord-task:01kncfwbeab4tk0mq2wk147k9x:depends-on:coord-task:01kncfw8xh4mephwpkg8ky3mem`: `coord-task:01kncfwbeab4tk0mq2wk147k9x` depends on `coord-task:01kncfw8xh4mephwpkg8ky3mem`
- `plan-edge:coord-task:01kncfwc8fnzkp12z98axm2ept:depends-on:coord-task:01kncfwbeab4tk0mq2wk147k9x`: `coord-task:01kncfwc8fnzkp12z98axm2ept` depends on `coord-task:01kncfwbeab4tk0mq2wk147k9x`
- `plan-edge:coord-task:01kncfwdcddbj4xw025azyq6hc:depends-on:coord-task:01kncfwbeab4tk0mq2wk147k9x`: `coord-task:01kncfwdcddbj4xw025azyq6hc` depends on `coord-task:01kncfwbeab4tk0mq2wk147k9x`
- `plan-edge:coord-task:01kncfwdcddbj4xw025azyq6hc:depends-on:coord-task:01kncfwc8fnzkp12z98axm2ept`: `coord-task:01kncfwdcddbj4xw025azyq6hc` depends on `coord-task:01kncfwc8fnzkp12z98axm2ept`
- `plan-edge:coord-task:01kncfwe1x8a5qccafnj6wzamn:depends-on:coord-task:01kncfwc8fnzkp12z98axm2ept`: `coord-task:01kncfwe1x8a5qccafnj6wzamn` depends on `coord-task:01kncfwc8fnzkp12z98axm2ept`
- `plan-edge:coord-task:01kncfwe1x8a5qccafnj6wzamn:depends-on:coord-task:01kncfwdcddbj4xw025azyq6hc`: `coord-task:01kncfwe1x8a5qccafnj6wzamn` depends on `coord-task:01kncfwdcddbj4xw025azyq6hc`
- `plan-edge:coord-task:01kncfwa2f2xbactnjb5bwmqf6:depends-on:coord-task:01kncfw8xh4mephwpkg8ky3mem`: `coord-task:01kncfwa2f2xbactnjb5bwmqf6` depends on `coord-task:01kncfw8xh4mephwpkg8ky3mem`
- `plan-edge:coord-task:01kncfwfa5ngfp1z2kfcp6vfy0:depends-on:coord-task:01kncfwa2f2xbactnjb5bwmqf6`: `coord-task:01kncfwfa5ngfp1z2kfcp6vfy0` depends on `coord-task:01kncfwa2f2xbactnjb5bwmqf6`
- `plan-edge:coord-task:01kncfwfa5ngfp1z2kfcp6vfy0:depends-on:coord-task:01kncfwc8fnzkp12z98axm2ept`: `coord-task:01kncfwfa5ngfp1z2kfcp6vfy0` depends on `coord-task:01kncfwc8fnzkp12z98axm2ept`
- `plan-edge:coord-task:01kncfwfa5ngfp1z2kfcp6vfy0:depends-on:coord-task:01kncfwdcddbj4xw025azyq6hc`: `coord-task:01kncfwfa5ngfp1z2kfcp6vfy0` depends on `coord-task:01kncfwdcddbj4xw025azyq6hc`
- `plan-edge:coord-task:01kncfwfa5ngfp1z2kfcp6vfy0:depends-on:coord-task:01kncfwe1x8a5qccafnj6wzamn`: `coord-task:01kncfwfa5ngfp1z2kfcp6vfy0` depends on `coord-task:01kncfwe1x8a5qccafnj6wzamn`

## Execution Overlays

- Node: `coord-task:01kncfw8xh4mephwpkg8ky3mem`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation-v2`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation-v2`
- Node: `coord-task:01kncfwa2f2xbactnjb5bwmqf6`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation-v2`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation-v2`
- Node: `coord-task:01kncfwbeab4tk0mq2wk147k9x`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation-v2`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation-v2`
- Node: `coord-task:01kncfwc8fnzkp12z98axm2ept`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation-v2`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation-v2`
- Node: `coord-task:01kncfwdcddbj4xw025azyq6hc`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation-v2`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation-v2`
- Node: `coord-task:01kncfwe1x8a5qccafnj6wzamn`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation-v2`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation-v2`
- Node: `coord-task:01kncfwfa5ngfp1z2kfcp6vfy0`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation-v2`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation-v2`

