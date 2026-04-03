# Complete git execution policy rollout and harden publish semantics

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:b825a988577bff69761184da7d48f765f92c60e3e4cb082573f77b9b03a25e44`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 5 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn8n8v0ns6jcj0xgge5f0adv`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `5`

## Goal

Finish the git execution policy feature by exposing policy surfaces clearly, hardening start and publish transitions, validating failure recovery, and defining the recommended default behavior so agents reliably sync and publish through PRISM.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn8n8v0ns6jcj0xgge5f0adv.jsonl`

## Root Nodes

- `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`

## Nodes

### Audit remaining git execution gaps after the merged implementation

- Node id: `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- Kind: `investigate`
- Status: `ready`
- Priority: `95`

#### Acceptance

- The remaining behavioral, schema, and rollout gaps are enumerated from the current merged state. [any]

### Expose git execution policy and state clearly in MCP, query, and docs surfaces

- Node id: `coord-task:01kn8n9txdczd5dmtf1maheqpw`
- Kind: `edit`
- Status: `ready`
- Priority: `90`

#### Acceptance

- Agents and humans can see active git execution policy, state, and evidence from normal PRISM read paths. [any]

### Harden publish semantics and failure recovery around task completion

- Node id: `coord-task:01kn8n9w0r3ydrfs64cdy58t12`
- Kind: `edit`
- Status: `ready`
- Priority: `95`

#### Acceptance

- Publish failures and partial git transitions are represented and recoverable without silently false completed states. [any]

### Dogfood start and complete flows against real task claims and branch state

- Node id: `coord-task:01kn8n9x2vm8c683vpx5642f3j`
- Kind: `validate`
- Status: `ready`
- Priority: `92`

#### Acceptance

- Claim/start and complete/publish flows are exercised end to end against real repo conditions. [any]

### Set the recommended default policy and rollout guidance

- Node id: `coord-task:01kn8n9y54tz8h72ex327yyt9w`
- Kind: `decide`
- Status: `ready`
- Priority: `88`

#### Acceptance

- The repo has a documented recommendation for default git execution policy behavior and rollout posture. [any]

## Edges

- `plan-edge:coord-task:01kn8n9txdczd5dmtf1maheqpw:depends-on:coord-task:01kn8n9ssejh5kkx8zb4hjxt09`: `coord-task:01kn8n9txdczd5dmtf1maheqpw` depends on `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- `plan-edge:coord-task:01kn8n9w0r3ydrfs64cdy58t12:depends-on:coord-task:01kn8n9ssejh5kkx8zb4hjxt09`: `coord-task:01kn8n9w0r3ydrfs64cdy58t12` depends on `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- `plan-edge:coord-task:01kn8n9x2vm8c683vpx5642f3j:depends-on:coord-task:01kn8n9txdczd5dmtf1maheqpw`: `coord-task:01kn8n9x2vm8c683vpx5642f3j` depends on `coord-task:01kn8n9txdczd5dmtf1maheqpw`
- `plan-edge:coord-task:01kn8n9x2vm8c683vpx5642f3j:depends-on:coord-task:01kn8n9w0r3ydrfs64cdy58t12`: `coord-task:01kn8n9x2vm8c683vpx5642f3j` depends on `coord-task:01kn8n9w0r3ydrfs64cdy58t12`
- `plan-edge:coord-task:01kn8n9y54tz8h72ex327yyt9w:depends-on:coord-task:01kn8n9x2vm8c683vpx5642f3j`: `coord-task:01kn8n9y54tz8h72ex327yyt9w` depends on `coord-task:01kn8n9x2vm8c683vpx5642f3j`

