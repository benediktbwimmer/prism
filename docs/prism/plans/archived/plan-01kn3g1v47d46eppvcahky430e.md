# Improve PRISM MCP day-to-day agent ergonomics after the latest dogfooding round

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:1add3f813ac22a1e75cc55ba16624361e5a974d74394bb9b768f0d7bb4df3a81`
- Source logical timestamp: `unknown`
- Source snapshot: `9 nodes, 8 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn3g1v47d46eppvcahky430e`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `9`
- Edges: `8`

## Goal

Improve PRISM MCP day-to-day agent ergonomics after the latest dogfooding round

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn3g1v47d46eppvcahky430e.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`

## Nodes

### Prioritize the next PRISM MCP agent-UX improvements by day-to-day impact

- Node id: `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- Kind: `decide`
- Status: `completed`

### Lean more on active task context in compact-tool ranking and follow-through

- Node id: `coord-task:01kn3g2axtp38kdahjc7p1nrad`
- Kind: `edit`
- Status: `completed`
- Summary: Implement coordination-aware compact-tool ranking via optional taskId inputs instead of ambient session current-task state.

### Strengthen concept -> doc -> code continuity across compact flows

- Node id: `coord-task:01kn3g2az4kq3g8nahzdnxeaw7`
- Kind: `edit`
- Status: `completed`
- Summary: Strengthen concept -> doc -> code continuity by supplementing code-side follow-through candidates when phrase search misses owners and by preventing weak incidental docs from outranking stronger code targets.

### Explain why close alternatives lost in compact-tool ranking

- Node id: `coord-task:01kn3g2b0a14g01m2w768mawpp`
- Kind: `edit`
- Status: `completed`
- Summary: Add whyNotTop runner-up explanations to compact locate results, backed by ranking-signal comparisons so close alternatives explain which stronger winner signal beat them without overwhelming the normal winner-oriented selectionReason surface.

### Surface confidence and freshness more explicitly in compact results

- Node id: `coord-task:01kn3g2b1hje6ht1tbmfcz93mp`
- Kind: `edit`
- Status: `completed`
- Summary: Expose confidenceLabel on ranked compact handle candidates and freshness on compact open/workset/expand results, keeping the new labels aligned with existing remapped behavior across stale-handle and concept-remap paths.

### Generalize empty-string optional-input normalization across MCP-facing enums

- Node id: `coord-task:01kn3g2b2vd1e8q3hmshsf6p88`
- Kind: `edit`
- Status: `completed`
- Summary: Apply the optional empty-string enum deserializer across MCP-facing tool args and nested mutation payloads so blank enum fields deserialize as omitted instead of failing validation.

### Strengthen doc-governance and adjacent-spec navigation

- Node id: `coord-task:01kn3g2b41q01esvn3spmhxe3w`
- Kind: `edit`
- Status: `completed`
- Summary: Strengthen doc/spec follow-through by ranking sibling governance docs and adjacent specs ahead of owner hops, updating compact next-action guidance to mention adjacent spec navigation, and aligning spec-open regressions with the new doc-first behavior.

### Improve frontend-language degradation behavior when semantic coverage is thin

- Node id: `coord-task:01kn3g2b58twgvqcb0ftsra75n`
- Kind: `edit`
- Status: `completed`
- Summary: Improved compact-tool degradation when semantic coverage is thin by letting UI-shaped frontend queries fall back into text-hit discovery and by boosting frontend-like paths for matched UI-oriented text candidates. Also fixed follow-on workspace integration drift uncovered during validation: added principal-registry support to MemoryStore, re-exported CoordinationEventStream, taught Store trait forwarding through MutexGuard, added SharedRuntimeStore forwards for projection concept persistence, and normalized prism-core shared-runtime call sites so full workspace validation and release daemon restart succeed again.

### Turn repairAction guidance into one-hop safe session repairs

- Node id: `coord-task:01kn3g2b6expme2z26e91r8n9a`
- Kind: `edit`
- Status: `completed`
- Summary: Replace detached-session repairAction guidance with a narrow prism_mutate session_repair path, expose the new session_repair mutate action and input schema, and validate that clear_current_task safely clears leftover current-task bindings without depending on prism_session.

## Edges

- `plan-edge:coord-task:01kn3g2axtp38kdahjc7p1nrad:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2axtp38kdahjc7p1nrad` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- `plan-edge:coord-task:01kn3g2az4kq3g8nahzdnxeaw7:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2az4kq3g8nahzdnxeaw7` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- `plan-edge:coord-task:01kn3g2b0a14g01m2w768mawpp:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2b0a14g01m2w768mawpp` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- `plan-edge:coord-task:01kn3g2b1hje6ht1tbmfcz93mp:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2b1hje6ht1tbmfcz93mp` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- `plan-edge:coord-task:01kn3g2b2vd1e8q3hmshsf6p88:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2b2vd1e8q3hmshsf6p88` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- `plan-edge:coord-task:01kn3g2b41q01esvn3spmhxe3w:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2b41q01esvn3spmhxe3w` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- `plan-edge:coord-task:01kn3g2b58twgvqcb0ftsra75n:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2b58twgvqcb0ftsra75n` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`
- `plan-edge:coord-task:01kn3g2b6expme2z26e91r8n9a:depends-on:coord-task:01kn3g1ye0mc3v6rrthj04rfk5`: `coord-task:01kn3g2b6expme2z26e91r8n9a` depends on `coord-task:01kn3g1ye0mc3v6rrthj04rfk5`

