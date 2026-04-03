# Trace the daemon death and auth-drift failure paths in code, add durable observability so future daemon exits are diagnosable, and harden the bridge so dead daemons and stale credentials recover automatically or fail with actionable state.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:7887fb54b15bf98687f217664ec88257369fa7f9121dc5196c8516b4677e4b6c`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn80rqn6t0pgv1dt0zqay9j5`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `6`

## Goal

Trace the daemon death and auth-drift failure paths in code, add durable observability so future daemon exits are diagnosable, and harden the bridge so dead daemons and stale credentials recover automatically or fail with actionable state.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn80rqn6t0pgv1dt0zqay9j5.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`

## Nodes

### Trace daemon lifecycle, supervisor, and authority persistence paths in code

- Node id: `coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`
- Kind: `investigate`
- Status: `completed`
- Summary: Traced the daemon lifecycle, bridge reconnect, auth injection, and principal-registry paths; confirmed missing exit instrumentation, stale runtime-state pruning without observability, bridge retrying dead upstream URIs without restart escalation, and bridge auth reporting bound state without live authority validation.

#### Acceptance

- Relevant daemon, bridge, launcher, runtime-state, and auth/authority code owners are identified with specific failure hypotheses. [any]
- The plan names where to instrument exit, restart, authority lookup, and bridge-auth invalidation paths. [any]

#### Tags

- `auth`
- `daemon`
- `investigation`

### Add durable daemon crash and shutdown observability

- Node id: `coord-task:01kn80s8e7gg2n1czt0jwrh8w5`
- Kind: `edit`
- Status: `completed`
- Summary: Added durable daemon lifecycle observability in runtime state and top-level logging, recorded bridge upstream connection failures during warmup and reconnect, rebuilt the release binaries, and restarted the live daemon successfully. Targeted prism-mcp tests passed; full workspace cargo test is still blocked by unrelated prism-store pragma-count assertions.

#### Acceptance

- Future daemon exits leave durable structured logs or persisted state indicating exit path and restart context. [any]
- Launcher or supervisor paths capture enough metadata to distinguish crash, clean shutdown, and external kill scenarios. [any]

#### Tags

- `daemon`
- `logging`
- `observability`

### Make the bridge self-heal dead upstream daemons instead of retrying a stale URI indefinitely

- Node id: `coord-task:01kn80sf4w82cwjjy9xtsdatq8`
- Kind: `edit`
- Status: `completed`
- Summary: Bootstrapped bridges now resolve reconnect targets through daemon resolution instead of retrying stale URI-file values indefinitely, targeted reconnect tests passed, and the updated release daemon restarted healthy. A full workspace cargo test rerun hit the known flaky runtime-sync timing test again, but the isolated rerun passed and validation was treated as successful under repo policy.

#### Acceptance

- The bridge can detect dead upstream daemon state and trigger a safe restart or respawn flow instead of only retrying a stale URI. [any]
- Recovery behavior is bounded and avoids silent infinite churn or duplicate-daemon storms. [any]

#### Tags

- `bridge`
- `daemon`
- `self-healing`

### Revalidate or invalidate stale bridge auth bindings after daemon or authority changes

- Node id: `coord-task:01kn80sr1ye602pq4smc2t7s6t`
- Kind: `edit`
- Status: `completed`
- Summary: Bridge auth now invalidates stale injected credentials after upstream rejection, exposes stale bridge state with recovery guidance, and is covered by targeted transport tests; full cargo test remains blocked by unrelated prism-store pragma-count assertions.

#### Acceptance

- Bridge auth state is revalidated against the live authority after restart or on first authoritative mutation. [any]
- Stale credential bindings are cleared or refreshed automatically, or fail with accurate user-facing state instead of misleading readiness. [any]

#### Tags

- `auth`
- `bridge`
- `recovery`

### Validate crash logging, bridge auto-recovery, and stale-auth recovery with injected failures

- Node id: `coord-task:01kn80szsnae110gap6ca3gaqp`
- Kind: `validate`
- Status: `completed`
- Summary: Added failure-injection validation coverage for bridge self-healing runtime-state evidence and stale bridge-auth recovery after re-adoption. Targeted tests passed, full cargo test only hit known timing-sensitive prism-mcp flakes, and both isolated reruns passed under repo policy.

#### Acceptance

- Injected daemon death leaves durable forensic evidence and the bridge converges back to a healthy daemon connection. [any]
- Injected stale-auth scenarios produce correct bridge state and successful recovery or accurate user-facing remediation. [any]

#### Tags

- `failure-injection`
- `recovery`
- `validation`

## Edges

- `plan-edge:coord-task:01kn80s8e7gg2n1czt0jwrh8w5:depends-on:coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`: `coord-task:01kn80s8e7gg2n1czt0jwrh8w5` depends on `coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`
- `plan-edge:coord-task:01kn80sf4w82cwjjy9xtsdatq8:depends-on:coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`: `coord-task:01kn80sf4w82cwjjy9xtsdatq8` depends on `coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`
- `plan-edge:coord-task:01kn80sr1ye602pq4smc2t7s6t:depends-on:coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`: `coord-task:01kn80sr1ye602pq4smc2t7s6t` depends on `coord-task:01kn80ry7wvfr9w7gp0dw8s5s6`
- `plan-edge:coord-task:01kn80szsnae110gap6ca3gaqp:depends-on:coord-task:01kn80s8e7gg2n1czt0jwrh8w5`: `coord-task:01kn80szsnae110gap6ca3gaqp` depends on `coord-task:01kn80s8e7gg2n1czt0jwrh8w5`
- `plan-edge:coord-task:01kn80szsnae110gap6ca3gaqp:depends-on:coord-task:01kn80sf4w82cwjjy9xtsdatq8`: `coord-task:01kn80szsnae110gap6ca3gaqp` depends on `coord-task:01kn80sf4w82cwjjy9xtsdatq8`
- `plan-edge:coord-task:01kn80szsnae110gap6ca3gaqp:depends-on:coord-task:01kn80sr1ye602pq4smc2t7s6t`: `coord-task:01kn80szsnae110gap6ca3gaqp` depends on `coord-task:01kn80sr1ye602pq4smc2t7s6t`

