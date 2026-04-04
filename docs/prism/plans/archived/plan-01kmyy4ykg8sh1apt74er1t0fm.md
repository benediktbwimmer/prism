# contracts-01: implement the first PRISM contracts primitive as a concept-like sibling pipeline with explicit schema and event storage, hosted MCP mutations and resources, query/read integration, and contract-aware impact or after-edit behavior, while keeping derived policy or guardrail enforcement out of scope for this first rollout.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:7e08e847c928b7415ea91503bf51bf12adfdc6e2ba6d73971318508e16e5b1c0`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 4 edges, 0 overlays`

## Overview

- Plan id: `plan:01kmyy4ykg8sh1apt74er1t0fm`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `4`

## Goal

contracts-01: implement the first PRISM contracts primitive as a concept-like sibling pipeline with explicit schema and event storage, hosted MCP mutations and resources, query/read integration, and contract-aware impact or after-edit behavior, while keeping derived policy or guardrail enforcement out of scope for this first rollout.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kmyy4ykg8sh1apt74er1t0fm.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kmyy5z8vc59p39fyp82qh98h`

## Nodes

### Milestone 0: Lock the contracts V1 ontology and rollout boundaries

- Node id: `coord-task:01kmyy5z8vc59p39fyp82qh98h`
- Kind: `investigate`
- Status: `completed`
- Summary: Milestone 0 is now codified in the implementation: contracts landed as a concept-like sibling surface with a narrow V1 model centered on contract packets, append-only contract events, repo event publication, basic query-side resolution, and explicit deferral of MCP mutation/resource exposure and richer contract-aware impact integration to later milestones.
- Priority: `0`

#### Acceptance

- The contracts-01 rollout has an explicit V1 scope that includes schema or storage, read surfaces, mutations, and limited query integration but excludes derived policy enforcement [any]
- The minimum contract object and event model are narrow enough to ship without a large second taxonomy pass [any]
- Open questions that would block implementation are reduced to a small explicit list or folded into concrete V1 choices [any]

#### Validation Refs

- `contracts:m0-v1-shape`

#### Tags

- `contracts`
- `ontology`
- `scope`
- `spec`

### Milestone 1: Add contract IR, event storage, and projection loading

- Node id: `coord-task:01kmyy5z9e5b2qv0j9gkq9db84`
- Kind: `edit`
- Status: `completed`
- Summary: Milestone 1 is complete: contract IR, event storage, projection loading, repo event publication, and query/runtime lookup are all implemented and validated.
- Priority: `1`

#### Acceptance

- Contract types, enums, and event records exist as first-class IR rather than ad hoc JSON blobs [any]
- Repo-scoped contracts round-trip through a dedicated append-only contract event log and reload into live state [any]
- The implementation follows the existing modular knowledge pipeline shape instead of embedding core contract logic in facade files or coordination-only code [any]

#### Validation Refs

- `contracts:m1-storage-roundtrip`

#### Tags

- `contracts`
- `ir`
- `projection`
- `storage`

### Milestone 2: Expose contract mutations, schemas, and resource surfaces

- Node id: `coord-task:01kmyy5z9yg4m41vgvsp52jjhb`
- Kind: `edit`
- Status: `completed`
- Summary: Milestone 2 is complete: contract mutations, schemas, examples, and the prism://contracts resource surface are implemented and validated.
- Priority: `1`

#### Acceptance

- prism_mutate exposes explicit contract lifecycle operations with validated payloads and actionable schema examples [any]
- The MCP resource surface exposes inspectable contract state without reading raw event logs directly [any]
- Hosted contract mutations update the live runtime consistently with persisted contract state [any]

#### Validation Refs

- `contracts:m2-mcp-surface`

#### Tags

- `contracts`
- `mcp`
- `mutation`
- `resources`

### Milestone 3: Integrate contracts into normal query and review flows

- Node id: `coord-task:01kmyy5zaha779zg71a25vgpb9`
- Kind: `edit`
- Status: `completed`
- Summary: Milestone 3 is complete: contracts now participate in normal query and review flows through contract-aware impact, afterEdit, validationPlan, readContext, workset, taskRisk, artifactRisk, and task-brief guidance.
- Priority: `1`

#### Acceptance

- Agents can ask which contracts govern a target and inspect those contracts through standard read flows [any]
- At least one existing impact-oriented surface reports touched contracts, affected consumers, or contract-linked validations when relevant [any]
- The first integration distinguishes implementation-only edits from contract-affecting edits more clearly than the current baseline [any]

#### Validation Refs

- `contracts:m3-query-integration`

#### Tags

- `contracts`
- `impact`
- `query`
- `review`

### Milestone 4: Validate, dogfood, and restart the live MCP runtime

- Node id: `coord-task:01kmyy5zb6c4dexq044bz52jvj`
- Kind: `validate`
- Status: `completed`
- Summary: Milestone 4 is complete: targeted core, query, and MCP contract tests ran, contract rollout dogfooding feedback was recorded, and the release prism-cli/prism-mcp binaries were rebuilt with a healthy live daemon restart.
- Priority: `1`

#### Acceptance

- Core storage, query lookup, and MCP mutation or resource surfaces have targeted test coverage for the first contracts path [any]
- Dogfooding yields at least one recorded validation outcome or feedback item about the contracts rollout [any]
- After meaningful MCP or query-surface changes, the release binaries are rebuilt and the live MCP daemon is restarted and health-checked [any]

#### Validation Refs

- `contracts:m4-validation-and-runtime`

#### Tags

- `contracts`
- `dogfood`
- `runtime`
- `validation`

## Edges

- `plan-edge:coord-task:01kmyy5z9e5b2qv0j9gkq9db84:depends-on:coord-task:01kmyy5z8vc59p39fyp82qh98h`: `coord-task:01kmyy5z9e5b2qv0j9gkq9db84` depends on `coord-task:01kmyy5z8vc59p39fyp82qh98h`
- `plan-edge:coord-task:01kmyy5z9yg4m41vgvsp52jjhb:depends-on:coord-task:01kmyy5z9e5b2qv0j9gkq9db84`: `coord-task:01kmyy5z9yg4m41vgvsp52jjhb` depends on `coord-task:01kmyy5z9e5b2qv0j9gkq9db84`
- `plan-edge:coord-task:01kmyy5zaha779zg71a25vgpb9:depends-on:coord-task:01kmyy5z9yg4m41vgvsp52jjhb`: `coord-task:01kmyy5zaha779zg71a25vgpb9` depends on `coord-task:01kmyy5z9yg4m41vgvsp52jjhb`
- `plan-edge:coord-task:01kmyy5zb6c4dexq044bz52jvj:depends-on:coord-task:01kmyy5zaha779zg71a25vgpb9`: `coord-task:01kmyy5zb6c4dexq044bz52jvj` depends on `coord-task:01kmyy5zaha779zg71a25vgpb9`

