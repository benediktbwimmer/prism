# Follow up on remaining PRISM usability gaps by improving compact-tool ranking transparency, strengthening concept-to-doc-to-code continuity, hardening workspace/path edge cases, and tightening stale-context repair loops.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:383c533b0ac8bf7b153be4ee3aa612cc608f89c3cebc02d1be430cbec8a96e25`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn3cceq1sh4s78yj4m34rww2`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `4`
- Edges: `0`

## Goal

Follow up on remaining PRISM usability gaps by improving compact-tool ranking transparency, strengthening concept-to-doc-to-code continuity, hardening workspace/path edge cases, and tightening stale-context repair loops.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn3cceq1sh4s78yj4m34rww2.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn3ccwasnywp14s14dfnpcxw`
- `coord-task:01kn3ccwc6s0q937rgb9h3w4st`
- `coord-task:01kn3ccwdc8vykc0ctjpsxyn3n`
- `coord-task:01kn3ccwek828erfngwv7tjk4d`

## Nodes

### Explain why compact-tool results won ranking

- Node id: `coord-task:01kn3ccwasnywp14s14dfnpcxw`
- Kind: `edit`
- Status: `completed`
- Summary: Added compact locate ranking transparency by surfacing a top-level selectionReason alongside per-candidate whyShort output, preserving stronger ranked reasons instead of discarding them, covering direct locate plus MCP round-trip behavior with regressions, validating focused locate tests and the full workspace suite, and rebuilding/restarting the release MCP daemon.

#### Acceptance

- Compact-tool responses expose a concise, trustworthy explanation for why the top result or candidate won. [any]

### Strengthen concept to doc to code continuity

- Node id: `coord-task:01kn3ccwc6s0q937rgb9h3w4st`
- Kind: `edit`
- Status: `completed`
- Summary: Inspecting concept -> governing doc -> code-owner continuity across compact concept/open/workset flows to reduce manual stitching between those surfaces.

#### Acceptance

- Common concept flows can carry the user from concept packets into governing docs and then into code owners without manual stitching. [any]

### Harden workspace and path edge cases

- Node id: `coord-task:01kn3ccwdc8vykc0ctjpsxyn3n`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Workspace and path-sensitive behavior remains stable across temp roots, canonicalization differences, and parallel test execution. [any]

### Tighten stale-context repair loops

- Node id: `coord-task:01kn3ccwek828erfngwv7tjk4d`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- When PRISM detects stale or detached session context, the user gets a direct repair path rather than explanation only. [any]

## Edges

No published plan edges are currently recorded.

