# runtime-status-cleanup: make runtimeStatus cheap, accurate, and trustworthy by pruning stale runtime-state process records, tightening process/count reporting, and validating the cleaned status surface under real daemon usage.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:99cdbad9ceadaccef3fa4a9e8d2efd24feb413285808eddf3af43a77673718de`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn2nhs48pj2kqeqqgtrqsded`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `4`
- Edges: `0`

## Goal

runtime-status-cleanup: make runtimeStatus cheap, accurate, and trustworthy by pruning stale runtime-state process records, tightening process/count reporting, and validating the cleaned status surface under real daemon usage.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn2nhs48pj2kqeqqgtrqsded.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn2nhs48pj2kqeqqgtrqsded.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn2nj1rsdv7bs5vws1fynq7g`
- `coord-task:01kn2nj1t95nwaytv4gd7hd2k8`
- `coord-task:01kn2nj1wbag9jvvajz4ma081y`
- `coord-task:01kn2nj70jmf7vkecd8jdjn8b5`

## Nodes

### Audit runtime-state process lifecycle and define the canonical pruning/update rules for cheap multi-instance-safe runtimeStatus reporting

- Node id: `coord-task:01kn2nj1rsdv7bs5vws1fynq7g`
- Kind: `edit`
- Status: `completed`
- Summary: Canonical runtime-state rule: only trust persisted process records after exact per-PID `ps` command validation, and prune stale or PID-reused records as part of runtime-state reads.

### Implement stale runtime-state process pruning and live-process filtering so daemon and bridge counts stay accurate without expensive broad process scans

- Node id: `coord-task:01kn2nj1t95nwaytv4gd7hd2k8`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented runtime-state pruning on read plus exact per-PID `ps` snapshots for state-backed runtimeStatus process reporting, with fallback to broad process scans only when needed.

### Validate runtimeStatus end to end under daemon restart, stale runtime-state records, and live dogfooding before closing the cleanup plan

- Node id: `coord-task:01kn2nj1wbag9jvvajz4ma081y`
- Kind: `edit`
- Status: `completed`
- Summary: Validated with targeted runtime-state and runtime-status tests, a full `cargo test` workspace run, and a release rebuild plus `prism-cli mcp restart/status/health` cycle against the updated daemon.

### Tighten runtimeStatus process and connection reporting semantics so cheap introspection stays correct for one or many local MCP daemons sharing the machine

- Node id: `coord-task:01kn2nj70jmf7vkecd8jdjn8b5`
- Kind: `edit`
- Status: `completed`
- Summary: runtimeStatus now computes bridge connection state even when structured runtime-state exists, so connected/orphaned bridge counts are based on live connection probes instead of the presence of the sidecar file.

## Edges

No published plan edges are currently recorded.

