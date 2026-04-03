# prism-general-improvements: improve PRISM MCP and repo-awareness dogfooding quality by fixing first-hop doc discovery noise, strengthening concept-to-concrete follow-through, making session/task context clearer, improving compact-tool ergonomics, and adding better governing-section or cross-doc navigation.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:bc128bd412e8b8fc4931fec58ef6cffd69541487f0a77250177fa52bdfeac26d`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn384cseg90m3a2gpf3802ca`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `6`

## Goal

prism-general-improvements: improve PRISM MCP and repo-awareness dogfooding quality by fixing first-hop doc discovery noise, strengthening concept-to-concrete follow-through, making session/task context clearer, improving compact-tool ergonomics, and adding better governing-section or cross-doc navigation.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn384cseg90m3a2gpf3802ca.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn384cseg90m3a2gpf3802ca.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn384q9273c7ak2baermst1v`

## Nodes

### Triage and prioritize PRISM dogfooding improvements

- Node id: `coord-task:01kn384q9273c7ak2baermst1v`
- Kind: `investigate`
- Status: `completed`
- Summary: Prioritized order: (1) improve heading-aware doc discovery and first-hop ranking, (2) add governing-section and cross-doc navigation affordances, (3) make compact tools more forgiving and self-correcting, (4) strengthen concept packets with concrete follow-through targets, (5) clarify session and task-context UX. Rationale: the first two items share the same compact text/doc ranking surface and directly address the highest-friction failure seen in live use; compact-tool normalization is a bounded high-leverage ergonomics fix; concept follow-through is valuable but likely reaches deeper into concept publication and decode paths; session/task-context clarity matters but is lower-frequency and touches broader host/session UX decisions.

#### Acceptance

- The plan captures the concrete PRISM issues surfaced during live use and orders them by impact and implementation cost. [any]

#### Tags

- `dogfooding`
- `mcp`
- `planning`

### Improve heading-aware doc discovery and first-hop ranking

- Node id: `coord-task:01kn385db3vr1skpmntyn34vcq`
- Kind: `edit`
- Status: `completed`
- Summary: Updated compact gather ranking for docs so Markdown heading hits outrank incidental earlier mentions, requested a larger candidate pool for doc gathers before truncation, and added a regression test covering the exact failure mode where a bullet mention previously beat the defining section heading.
- Priority: `9`

#### Acceptance

- Heading and section queries find the governing doc section before incidental earlier mentions. [any]

#### Tags

- `compact-tools`
- `docs`
- `ranking`

### Strengthen concept packets with concrete follow-through targets

- Node id: `coord-task:01kn385dcetbhcjm4hpse9y4jw`
- Kind: `edit`
- Status: `completed`
- Summary: Strengthened concept packets with concrete follow-through targets by synthesizing fallback inspect/support/test candidates when hydrated concept members are empty, wiring those fallbacks into concept curation hints plus compact concept/open/workset flows, validating with focused concept regressions, full cargo test, and release rebuild/restart.
- Priority: `6`

#### Acceptance

- Common concept reads yield actionable concrete members or a clearly preferred next open target. [any]

#### Tags

- `concepts`
- `follow-through`
- `navigation`

### Clarify session and task-context UX

- Node id: `coord-task:01kn385ddnwb09zzn36a69f4mm`
- Kind: `edit`
- Status: `completed`
- Summary: Clarified session and task-context UX by surfacing current-task contextStatus/contextSummary/nextAction in session views, reusing the same logic in dashboard session snapshots, adding detached and stale session regressions plus an active coordination-binding assertion, validating targeted prism-mcp tests and the full workspace suite, and fixing a prism-core PRISM_HOME env race in recovery tests so the workspace suite stays green under parallel execution.
- Priority: `5`

#### Acceptance

- Agents can tell when current session context is unrelated and have a clear supported path to re-scope. [any]

#### Tags

- `session`
- `task-context`
- `ux`

### Make compact tools more forgiving and self-correcting

- Node id: `coord-task:01kn385dezjaxe8thgz70v74b0`
- Kind: `edit`
- Status: `completed`
- Summary: Made compact tools more forgiving and self-correcting by broadening taskIntent aliases, accepting common snake_case field aliases, adding direct deserialization coverage plus MCP round-trip coverage, and validating with targeted tests, full cargo test, and release rebuild/restart.
- Priority: `7`

#### Acceptance

- Obvious synonym or variant mistakes produce successful normalization or highly targeted recovery guidance. [any]

#### Tags

- `compact-tools`
- `diagnostics`
- `ergonomics`

### Add governing-section and cross-doc navigation affordances

- Node id: `coord-task:01kn385dg7g1pgcxnnntbjzhef`
- Kind: `edit`
- Status: `completed`
- Summary: Added cross-doc followups for doc/spec targets so compact workset/open can surface sibling governing sections before code-owner hops. Updated guidance and suggested actions to reflect governing-section follow-through, added a regression fixture/test, validated focused tests, ran the full workspace suite green, and rebuilt/restarted the release MCP daemon.
- Priority: `8`

#### Acceptance

- PRISM can take agents to the governing section or best related design/spec document without manual multi-tool stitching. [any]

#### Tags

- `docs`
- `navigation`
- `ranking`

## Edges

- `plan-edge:coord-task:01kn385db3vr1skpmntyn34vcq:depends-on:coord-task:01kn384q9273c7ak2baermst1v`: `coord-task:01kn385db3vr1skpmntyn34vcq` depends on `coord-task:01kn384q9273c7ak2baermst1v`
- `plan-edge:coord-task:01kn385dcetbhcjm4hpse9y4jw:depends-on:coord-task:01kn384q9273c7ak2baermst1v`: `coord-task:01kn385dcetbhcjm4hpse9y4jw` depends on `coord-task:01kn384q9273c7ak2baermst1v`
- `plan-edge:coord-task:01kn385ddnwb09zzn36a69f4mm:depends-on:coord-task:01kn384q9273c7ak2baermst1v`: `coord-task:01kn385ddnwb09zzn36a69f4mm` depends on `coord-task:01kn384q9273c7ak2baermst1v`
- `plan-edge:coord-task:01kn385dezjaxe8thgz70v74b0:depends-on:coord-task:01kn384q9273c7ak2baermst1v`: `coord-task:01kn385dezjaxe8thgz70v74b0` depends on `coord-task:01kn384q9273c7ak2baermst1v`
- `plan-edge:coord-task:01kn385dg7g1pgcxnnntbjzhef:depends-on:coord-task:01kn384q9273c7ak2baermst1v`: `coord-task:01kn385dg7g1pgcxnnntbjzhef` depends on `coord-task:01kn384q9273c7ak2baermst1v`
- `plan-edge:coord-task:01kn385dg7g1pgcxnnntbjzhef:depends-on:coord-task:01kn385db3vr1skpmntyn34vcq`: `coord-task:01kn385dg7g1pgcxnnntbjzhef` depends on `coord-task:01kn385db3vr1skpmntyn34vcq`

