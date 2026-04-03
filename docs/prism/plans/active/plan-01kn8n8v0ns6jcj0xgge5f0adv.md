# Complete git execution policy rollout and harden publish semantics

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:1a06204170643778964d56e0232645fd5a8d5e6e02c548cdb1eca417aa7c4a43`
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

Finish the git execution policy feature by exposing policy surfaces clearly, separating task lifecycle from publish lifecycle, hardening start and publish transitions, validating strict/manual publication recovery, proving strict `require` through dogfooding, and making strict `require` the recommended default while removing `auto` after the migration path is no longer needed.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn8n8v0ns6jcj0xgge5f0adv.jsonl`

## Root Nodes

- `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`

## Nodes

### Audit remaining git execution gaps after the merged implementation

- Node id: `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- Kind: `investigate`
- Status: `completed`
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

### Split task lifecycle from publish lifecycle and harden strict completion semantics

- Node id: `coord-task:01kn8n9w0r3ydrfs64cdy58t12`
- Kind: `edit`
- Status: `ready`
- Summary: Make publish intent, publish-pending, publish-failed, and publish-ack semantics explicit so strict require mode can guarantee repo-published `.prism` state is manually committed and verified without automatic git mutations.
- Priority: `95`

#### Acceptance

- PRISM can represent publish-pending and publish-failed states without falsely marking a task completed before the repo-published `.prism` state is actually committed and verified. [any]
- Publish acknowledgement and verification state live outside the repo-published projection so confirming publication does not re-dirty `.prism` artifacts or create a commit loop. [any]
- Task lifecycle and publish lifecycle are represented separately enough for strict `require` mode to avoid automatic branch, commit, and push operations while still tracking desired completion. [any]

### Dogfood start and complete flows against real task claims and branch state

- Node id: `coord-task:01kn8n9x2vm8c683vpx5642f3j`
- Kind: `validate`
- Status: `ready`
- Priority: `92`

#### Acceptance

- Claim/start and complete/publish flows are exercised end to end against real repo conditions. [any]

### Make strict require the default and remove auto after proven dogfooding

- Node id: `coord-task:01kn8n9y54tz8h72ex327yyt9w`
- Kind: `decide`
- Status: `ready`
- Summary: Document strict `require` as the recommended default once the publish-state model is proven, and delete `auto` rather than keeping it as a normal mode because broad auto-commit behavior is too risky for multi-task agent work.
- Priority: `88`

#### Acceptance

- The final policy recommendation explains why commit-scope responsibility should remain with the agent or human rather than an automatic publish mode. [any]
- The repo guidance names strict `require` as the recommended default mode for normal PRISM agent work. [any]
- The rollout plan only keeps `auto` as a temporary migration aid, and explicitly removes it once strict `require` has been proven through dogfooding and validation. [any]

## Edges

- `plan-edge:coord-task:01kn8n9txdczd5dmtf1maheqpw:depends-on:coord-task:01kn8n9ssejh5kkx8zb4hjxt09`: `coord-task:01kn8n9txdczd5dmtf1maheqpw` depends on `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- `plan-edge:coord-task:01kn8n9w0r3ydrfs64cdy58t12:depends-on:coord-task:01kn8n9ssejh5kkx8zb4hjxt09`: `coord-task:01kn8n9w0r3ydrfs64cdy58t12` depends on `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- `plan-edge:coord-task:01kn8n9x2vm8c683vpx5642f3j:depends-on:coord-task:01kn8n9txdczd5dmtf1maheqpw`: `coord-task:01kn8n9x2vm8c683vpx5642f3j` depends on `coord-task:01kn8n9txdczd5dmtf1maheqpw`
- `plan-edge:coord-task:01kn8n9x2vm8c683vpx5642f3j:depends-on:coord-task:01kn8n9w0r3ydrfs64cdy58t12`: `coord-task:01kn8n9x2vm8c683vpx5642f3j` depends on `coord-task:01kn8n9w0r3ydrfs64cdy58t12`
- `plan-edge:coord-task:01kn8n9y54tz8h72ex327yyt9w:depends-on:coord-task:01kn8n9x2vm8c683vpx5642f3j`: `coord-task:01kn8n9y54tz8h72ex327yyt9w` depends on `coord-task:01kn8n9x2vm8c683vpx5642f3j`

