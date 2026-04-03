# Eliminate absolute path leakage and make path identity repo-relative across PRISM

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:4f7fdd8577c24e69033c3cadbbfe538afd476caef113e9d31172852e99aeb484`
- Source logical timestamp: `unknown`
- Source snapshot: `0 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn9vcavs4jft9eqw82ysmghq`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `0`
- Edges: `0`

## Goal

Define one correct path identity model for PRISM so tracked snapshots, shared/runtime state, APIs, docs, and projections never persist absolute filesystem paths; use file ids plus repo-relative paths as canonical identity and anchors as the primary semantic reference.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn9vcavs4jft9eqw82ysmghq.json`
- Legacy migration log path: unavailable in the current projection

## Nodes

No published plan nodes are currently recorded.

## Edges

No published plan edges are currently recorded.

