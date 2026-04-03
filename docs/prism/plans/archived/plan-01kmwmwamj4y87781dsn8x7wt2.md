# Refactor the prism-mcp test suite so routine feedback loops are faster and the integration-heavy cases are less flaky and more deterministic.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:625fd7a83d1146b95174d2a627d362272c103e932d6add7b954727ff598b88d5`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 5 edges, 0 overlays`

## Overview

- Plan id: `plan:01kmwmwamj4y87781dsn8x7wt2`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `5`

## Goal

Refactor the prism-mcp test suite so routine feedback loops are faster and the integration-heavy cases are less flaky and more deterministic.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kmwmwamj4y87781dsn8x7wt2.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kmwmwamj4y87781dsn8x7wt2.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kmwmwtsvsb4fnsekv4hd29fd`

## Nodes

### Phase 1: Split the prism-mcp test monolith into focused modules and shared support

- Node id: `coord-task:01kmwmwtsvsb4fnsekv4hd29fd`
- Kind: `edit`
- Status: `ready`
- Summary: Extract the 16k-line tests.rs file into cohesive test modules plus a shared support layer so routine edits stop rebuilding one giant unit-test binary.
- Priority: `1`

#### Acceptance

- tests.rs is decomposed into focused modules and common helpers live in shared support [any]
- Small surface edits no longer require touching one monolithic test source file [any]

#### Tags

- `modularity`
- `phase-1`
- `tests`

### Phase 2: Introduce reusable workspace, host, and server fixtures

- Node id: `coord-task:01kmwmx0mgvnar7ttdgbx5vqp7`
- Kind: `edit`
- Status: `proposed`
- Summary: Replace repeated temp workspace and indexing boilerplate with fixture builders for indexed workspaces, in-process hosts, and slow transport scenarios.
- Priority: `2`

#### Acceptance

- Common fixture builders cover basic workspace, indexed host, and server cases [any]
- Repeated temp_workspace and index_workspace_session setup is materially reduced across the suite [any]

#### Tags

- `fixtures`
- `phase-2`
- `tests`

### Phase 3: Replace sleep-based waits with deterministic polling helpers

- Node id: `coord-task:01kmwmx66n1p6yd7ynxefdxmwm`
- Kind: `edit`
- Status: `proposed`
- Summary: Remove raw thread and tokio sleeps from tests in favor of wait-until helpers tied to real readiness and refresh conditions.
- Priority: `3`

#### Acceptance

- Raw sleep calls in ordinary prism-mcp tests are removed or isolated behind deterministic helpers [any]
- Timing-sensitive tests wait on explicit readiness predicates instead of fixed delays [any]

#### Tags

- `phase-3`
- `stability`
- `tests`

### Phase 4: Isolate transport and bridge tests into a slower integration layer

- Node id: `coord-task:01kmwmxdydjdfqt637qws4020s`
- Kind: `decide`
- Status: `proposed`
- Summary: Keep only a small explicit set of real HTTP and bridge end-to-end tests in a dedicated slow layer, while moving most surface tests to in-process execution.
- Priority: `4`

#### Acceptance

- Transport and bridge scenarios are separated from the routine fast suite [any]
- Most MCP surface behavior is verified without sockets or bridge startup [any]

#### Tags

- `integration`
- `phase-4`
- `tests`

### Phase 5: Push pure logic tests down into the owning crates

- Node id: `coord-task:01kmwmxn7rrwce389gmwgw91r9`
- Kind: `edit`
- Status: `proposed`
- Summary: Move log-store, filtering, serialization, and other non-surface behavior tests into their owning crates so prism-mcp stays focused on wiring and MCP behavior.
- Priority: `5`

#### Acceptance

- Pure logic tests live with the modules and crates that own the behavior [any]
- prism-mcp retains primarily surface and wiring coverage instead of lower-layer rule tests [any]

#### Tags

- `ownership`
- `phase-5`
- `tests`

### Phase 6: Add explicit CI lanes for fast, indexed, and slow integration tests

- Node id: `coord-task:01kmwmxw55q5xvpqg4r5g5x393`
- Kind: `validate`
- Status: `proposed`
- Summary: Define separate execution classes so local and PR feedback loops stay fast while still covering indexed-workspace and transport-heavy scenarios in the right place.
- Priority: `6`

#### Acceptance

- CI distinguishes routine fast tests from indexed-workspace and slow integration suites [any]
- Developers can run the fast lane without paying for the full slow matrix [any]

#### Tags

- `ci`
- `phase-6`
- `tests`

## Edges

- `plan-edge:coord-task:01kmwmx0mgvnar7ttdgbx5vqp7:depends-on:coord-task:01kmwmwtsvsb4fnsekv4hd29fd`: `coord-task:01kmwmx0mgvnar7ttdgbx5vqp7` depends on `coord-task:01kmwmwtsvsb4fnsekv4hd29fd`
- `plan-edge:coord-task:01kmwmx66n1p6yd7ynxefdxmwm:depends-on:coord-task:01kmwmx0mgvnar7ttdgbx5vqp7`: `coord-task:01kmwmx66n1p6yd7ynxefdxmwm` depends on `coord-task:01kmwmx0mgvnar7ttdgbx5vqp7`
- `plan-edge:coord-task:01kmwmxdydjdfqt637qws4020s:depends-on:coord-task:01kmwmx66n1p6yd7ynxefdxmwm`: `coord-task:01kmwmxdydjdfqt637qws4020s` depends on `coord-task:01kmwmx66n1p6yd7ynxefdxmwm`
- `plan-edge:coord-task:01kmwmxn7rrwce389gmwgw91r9:depends-on:coord-task:01kmwmxdydjdfqt637qws4020s`: `coord-task:01kmwmxn7rrwce389gmwgw91r9` depends on `coord-task:01kmwmxdydjdfqt637qws4020s`
- `plan-edge:coord-task:01kmwmxw55q5xvpqg4r5g5x393:depends-on:coord-task:01kmwmxn7rrwce389gmwgw91r9`: `coord-task:01kmwmxw55q5xvpqg4r5g5x393` depends on `coord-task:01kmwmxn7rrwce389gmwgw91r9`

