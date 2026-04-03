# Complete git execution policy rollout and harden publish semantics

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:b3049a8a6cbc0b30f8c06ad68da6c29d21e3419f0ab964bd525d9c2ff8f17f0c`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn8n8v0ns6jcj0xgge5f0adv`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `6`

## Goal

Finish the git execution policy feature by exposing policy surfaces clearly, separating task lifecycle from publish lifecycle, making publish-pending and publication acknowledgement semantics explicit, finalizing strict `require` so agents own source-code commit scope while PRISM only finalizes allowlisted PRISM-managed projection files, making freshness policy explicit, validating the flow through dogfooding, and removing `auto` once strict `require` is proven.

## Git Execution Policy

- Start mode: `auto`
- Completion mode: `auto`
- Target branch: `main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn8n8v0ns6jcj0xgge5f0adv.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

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
- Status: `in_progress`
- Priority: `90`

#### Acceptance

- Agents and humans can see active git execution policy, state, and evidence from normal PRISM read paths. [any]

### Split task lifecycle from publish lifecycle and harden strict completion semantics

- Node id: `coord-task:01kn8n9w0r3ydrfs64cdy58t12`
- Kind: `edit`
- Status: `completed`
- Summary: Make publish intent, publish-pending, publish-failed, and publish-ack semantics explicit so strict `require` can separate logical task completion from repo publication, prevent false completion during push failures, and keep user-code commit scope manual.
- Priority: `95`

#### Acceptance

- PRISM can represent publish-pending and publish-failed states without falsely marking a task completed before repo publication succeeds. [any]
- Publish acknowledgement and verification state live outside the repo-published projection so confirming publication does not re-dirty `.prism` artifacts or create a commit loop. [any]
- Task lifecycle and publish lifecycle are represented separately enough for `require` to track desired completion before final publication while keeping source-code commit scope with the agent or human. [any]

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

### Finalize strict require mode with explicit freshness policy and PRISM-only projection commits

- Node id: `coord-task:01kn8yzs3354nger6td7q3x24r`
- Kind: `edit`
- Status: `completed`
- Summary: Define `require` as the recommended safe mode: verify explicit git/workflow invariants, require intentionally committed user-code changes, let PRISM finalize only allowlisted PRISM-managed projection files, and eliminate any remaining broad auto-publication behavior.
- Priority: `94`

#### Acceptance

- `require` never selects user-code commit scope and blocks completion when uncommitted non-PRISM changes make publication ambiguous. [any]
- Freshness enforcement is expressed as explicit policy fields such as target ref, max commits behind target, and optional fetch-age thresholds rather than hidden heuristics. [any]
- Any automatic follow-up commit or push in `require` is restricted to allowlisted PRISM-managed paths such as `.prism/**`, `docs/prism/**`, and `PRISM.md`. [any]
- The path from pending publication to completed is idempotent and cannot report a false completed state when publication fails. [any]

## Edges

- `plan-edge:coord-task:01kn8n9txdczd5dmtf1maheqpw:depends-on:coord-task:01kn8n9ssejh5kkx8zb4hjxt09`: `coord-task:01kn8n9txdczd5dmtf1maheqpw` depends on `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- `plan-edge:coord-task:01kn8n9w0r3ydrfs64cdy58t12:depends-on:coord-task:01kn8n9ssejh5kkx8zb4hjxt09`: `coord-task:01kn8n9w0r3ydrfs64cdy58t12` depends on `coord-task:01kn8n9ssejh5kkx8zb4hjxt09`
- `plan-edge:coord-task:01kn8n9x2vm8c683vpx5642f3j:depends-on:coord-task:01kn8n9txdczd5dmtf1maheqpw`: `coord-task:01kn8n9x2vm8c683vpx5642f3j` depends on `coord-task:01kn8n9txdczd5dmtf1maheqpw`
- `plan-edge:coord-task:01kn8n9x2vm8c683vpx5642f3j:depends-on:coord-task:01kn8yzs3354nger6td7q3x24r`: `coord-task:01kn8n9x2vm8c683vpx5642f3j` depends on `coord-task:01kn8yzs3354nger6td7q3x24r`
- `plan-edge:coord-task:01kn8n9y54tz8h72ex327yyt9w:depends-on:coord-task:01kn8n9x2vm8c683vpx5642f3j`: `coord-task:01kn8n9y54tz8h72ex327yyt9w` depends on `coord-task:01kn8n9x2vm8c683vpx5642f3j`
- `plan-edge:coord-task:01kn8yzs3354nger6td7q3x24r:depends-on:coord-task:01kn8n9w0r3ydrfs64cdy58t12`: `coord-task:01kn8yzs3354nger6td7q3x24r` depends on `coord-task:01kn8n9w0r3ydrfs64cdy58t12`

