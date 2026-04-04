# Curate PRISM concepts for the implemented principal identity and authenticated coordination system

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:ea623f013d3c62d0248d991adefc7e06a4ae0f4f3c6cfe984327492f0ca9cc72`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn6f9bhgq49zweypzysn03rj`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `1`
- Edges: `0`

## Goal

Curate PRISM concepts for the implemented principal identity and authenticated coordination system

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn6f9bhgq49zweypzysn03rj.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn6fa2xeg8f0rayk8n0zwtkm`

## Nodes

### Curate concept packs for principal identity and authenticated coordination

- Node id: `coord-task:01kn6fa2xeg8f0rayk8n0zwtkm`
- Kind: `edit`
- Status: `completed`
- Summary: Curated the principal-identity/authenticated-coordination concept layer, added a reusable principal/provenance concept, refreshed stale mutation/lease packets, and regenerated the published PRISM docs.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:194`
- Anchor: `file:334`
- Anchor: `file:346`
- Anchor: `file:67`

#### Acceptance

- Any missing reusable concepts for principal identity and authenticated coordination are promoted with durable repo scope [any]
- Concept relations reflect the current identity, lease, and coordination boundaries [any]
- Relevant existing concepts are updated or retired to match the implemented identity/coordination model [any]

## Edges

No published plan edges are currently recorded.

