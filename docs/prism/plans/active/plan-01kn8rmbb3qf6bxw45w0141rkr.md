# Explicit logical repo identity layer for clone-safe shared runtime

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:6a4f8f94ddc1a222d8b0c9a481eb0185db4a44dacb18ea4c348eaa1e19571822`
- Source logical timestamp: `unknown`
- Source snapshot: `9 nodes, 16 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn8rmbb3qf6bxw45w0141rkr`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `9`
- Edges: `16`

## Goal

Implement a stable repo-published logical_repo_id in .prism, separate it cleanly from local repo-instance and worktree identity, and use it as the future namespace boundary for shared runtime coordination across clones and machines.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn8rmbb3qf6bxw45w0141rkr.jsonl`

## Root Nodes

- `coord-task:01kn8rmqrchgcb1ym5btdszded`

## Nodes

### Lock the logical repo identity contract and naming invariants

- Node id: `coord-task:01kn8rmqrchgcb1ym5btdszded`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Capture the namespace rule that future shared runtime backends key coordination by logical_repo_id while local caches remain repo-instance/worktree scoped. [any]
- Define logical_repo_id, repo_instance_id, and worktree_id as separate identity layers with one clear owner each. [any]
- State explicitly that clones only converge when they share a repo-published logical identity, not by path or remote heuristics. [any]

### Define the protected repo-published logical identity artifact in .prism

- Node id: `coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Choose the authoritative .prism artifact and schema that carries logical_repo_id plus any issuance/version metadata. [any]
- Make the artifact self-contained on cold clones and compatible with protected-state signing and verification. [any]
- Specify how clones read, trust, and refuse conflicting logical identities from repo state. [any]

### Specify logical identity bootstrap and clone propagation workflow

- Node id: `coord-task:01kn8rnk0ek3mjnmpbx4d3atz2`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Cover explicit repair or override flows for malformed, missing, or conflicting identity artifacts. [any]
- Define how a brand-new repo initializes logical_repo_id and how later clones inherit it from .prism without operator ambiguity. [any]
- Keep the bootstrap ergonomic for the human while preserving durable authorship and authority boundaries. [any]

### Thread logical identity through runtime context, provenance, logs, and diagnostics

- Node id: `coord-task:01kn8rz9qnzcwan5vd2cs7c99j`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Published provenance and diagnostics stop overloading the current local repo_id as if it were clone-stable. [any]
- Runtime/session/query surfaces expose logical_repo_id separately from local repo-instance and worktree identity. [any]

### Namespace shared runtime backends by logical repo identity

- Node id: `coord-task:01kn8rzmv5sa7xj8xt557rtzrg`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Map local repo-instance migration and compatibility rules around the new logical identity

- Node id: `coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Add mismatch detection and repair surfaces for logical identity conflicts

- Node id: `coord-task:01kn8s096yzxzw3cwvyc4t3gg3`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Validate multi-worktree and multi-clone convergence on one logical repo identity

- Node id: `coord-task:01kn8s0kxdap9nc8376dvaap5q`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

### Update specs, instructions, and release validation for logical repo identity

- Node id: `coord-task:01kn8s0zewczf185w547hbf46x`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

## Edges

- `plan-edge:coord-task:01kn8rn49cxj6cs4rdhxxg0j8c:depends-on:coord-task:01kn8rmqrchgcb1ym5btdszded`: `coord-task:01kn8rn49cxj6cs4rdhxxg0j8c` depends on `coord-task:01kn8rmqrchgcb1ym5btdszded`
- `plan-edge:coord-task:01kn8rnk0ek3mjnmpbx4d3atz2:depends-on:coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`: `coord-task:01kn8rnk0ek3mjnmpbx4d3atz2` depends on `coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`
- `plan-edge:coord-task:01kn8rz9qnzcwan5vd2cs7c99j:depends-on:coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`: `coord-task:01kn8rz9qnzcwan5vd2cs7c99j` depends on `coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`
- `plan-edge:coord-task:01kn8rz9qnzcwan5vd2cs7c99j:depends-on:coord-task:01kn8rnk0ek3mjnmpbx4d3atz2`: `coord-task:01kn8rz9qnzcwan5vd2cs7c99j` depends on `coord-task:01kn8rnk0ek3mjnmpbx4d3atz2`
- `plan-edge:coord-task:01kn8rzmv5sa7xj8xt557rtzrg:depends-on:coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`: `coord-task:01kn8rzmv5sa7xj8xt557rtzrg` depends on `coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`
- `plan-edge:coord-task:01kn8rzmv5sa7xj8xt557rtzrg:depends-on:coord-task:01kn8rnk0ek3mjnmpbx4d3atz2`: `coord-task:01kn8rzmv5sa7xj8xt557rtzrg` depends on `coord-task:01kn8rnk0ek3mjnmpbx4d3atz2`
- `plan-edge:coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex:depends-on:coord-task:01kn8rmqrchgcb1ym5btdszded`: `coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex` depends on `coord-task:01kn8rmqrchgcb1ym5btdszded`
- `plan-edge:coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex:depends-on:coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`: `coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex` depends on `coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`
- `plan-edge:coord-task:01kn8s096yzxzw3cwvyc4t3gg3:depends-on:coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`: `coord-task:01kn8s096yzxzw3cwvyc4t3gg3` depends on `coord-task:01kn8rn49cxj6cs4rdhxxg0j8c`
- `plan-edge:coord-task:01kn8s096yzxzw3cwvyc4t3gg3:depends-on:coord-task:01kn8rnk0ek3mjnmpbx4d3atz2`: `coord-task:01kn8s096yzxzw3cwvyc4t3gg3` depends on `coord-task:01kn8rnk0ek3mjnmpbx4d3atz2`
- `plan-edge:coord-task:01kn8s096yzxzw3cwvyc4t3gg3:depends-on:coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex`: `coord-task:01kn8s096yzxzw3cwvyc4t3gg3` depends on `coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex`
- `plan-edge:coord-task:01kn8s0kxdap9nc8376dvaap5q:depends-on:coord-task:01kn8rz9qnzcwan5vd2cs7c99j`: `coord-task:01kn8s0kxdap9nc8376dvaap5q` depends on `coord-task:01kn8rz9qnzcwan5vd2cs7c99j`
- `plan-edge:coord-task:01kn8s0kxdap9nc8376dvaap5q:depends-on:coord-task:01kn8rzmv5sa7xj8xt557rtzrg`: `coord-task:01kn8s0kxdap9nc8376dvaap5q` depends on `coord-task:01kn8rzmv5sa7xj8xt557rtzrg`
- `plan-edge:coord-task:01kn8s0kxdap9nc8376dvaap5q:depends-on:coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex`: `coord-task:01kn8s0kxdap9nc8376dvaap5q` depends on `coord-task:01kn8rzyqxvzpnw7m4c0zzk9ex`
- `plan-edge:coord-task:01kn8s0kxdap9nc8376dvaap5q:depends-on:coord-task:01kn8s096yzxzw3cwvyc4t3gg3`: `coord-task:01kn8s0kxdap9nc8376dvaap5q` depends on `coord-task:01kn8s096yzxzw3cwvyc4t3gg3`
- `plan-edge:coord-task:01kn8s0zewczf185w547hbf46x:depends-on:coord-task:01kn8s0kxdap9nc8376dvaap5q`: `coord-task:01kn8s0zewczf185w547hbf46x` depends on `coord-task:01kn8s0kxdap9nc8376dvaap5q`

