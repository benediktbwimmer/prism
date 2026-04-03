# Implement strict declared-work provenance, remove implicit undeclared mutation fallback, and cleanly separate work attribution from coordination tasks before PRISM release.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:ee810bec4d3485964c691e4e835256ec440260e60597cd44c6d0627d6b4b4101`
- Source logical timestamp: `unknown`
- Source snapshot: `13 nodes, 27 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn6rkat0t02ncv5pj0756yyz`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `13`
- Edges: `27`

## Goal

Implement strict declared-work provenance, remove implicit undeclared mutation fallback, and cleanly separate work attribution from coordination tasks before PRISM release.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn6rkat0t02ncv5pj0756yyz.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn6rkat0t02ncv5pj0756yyz.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn6rm7bf7k1qnmhcegvypt86`

## Nodes

### Lock the provenance model, terminology, and invariants

- Node id: `coord-task:01kn6rm7bf7k1qnmhcegvypt86`
- Kind: `edit`
- Status: `completed`
- Summary: Locked the target declared-work provenance contract in the design docs: strict declare_work bootstrap, work-versus-coordination terminology, and the rule that repo-published .prism state must stay semantically self-contained while runtime ids remain correlation only.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Defines the target `workContext` payload, undeclared-mutation rejection rule, and migration stance for legacy session task fallback. [any]
- Defines the target vocabulary: principal, work, coordination task, plan, and runtime correlation ids. [any]
- Defines which event fields are semantic versus correlation and forbids repo-published semantic dependency on shared-runtime-only state. [any]

### Introduce first-class work context types and durable event snapshots

- Node id: `coord-task:01kn6rmjszn0az4kypvh97yq1d`
- Kind: `edit`
- Status: `completed`
- Summary: Added typed work-context snapshots to mutation execution context and stamped authenticated outcome, validation-feedback, concept, contract, and concept-relation events with durable work meaning resolved from task, coordination task, and plan context.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Adds first-class work-context types and event fields in the shared IR / schema layer. [any]
- Correlation-only identifiers remain optional diagnostics and are not required for semantic interpretation. [any]
- Repo-published events carry enough inline work context to be understood without shared runtime DB resolution. [any]

### Add authenticated declare-work bootstrap and current-work runtime state

- Node id: `coord-task:01kn6rn36wwvrz8ezq8p0e02d6`
- Kind: `edit`
- Status: `completed`
- Summary: Implementing explicit authenticated declare-work bootstrap and runtime current-work state so authoritative mutations can require declared intent instead of falling back to implicit session tasks.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Adds an authenticated mutation for declaring work intent before other authoritative mutations. [any]
- Session/runtime state tracks current declared work separately from coordination task bindings. [any]
- Spawned or delegated work can carry an optional parent-work relationship without making bootstrap tokens semantically authoritative. [any]

### Reject undeclared mutations and remove implicit session-task fallback

- Node id: `coord-task:01kn6rnm8sffvrtsv6km1btd98`
- Kind: `edit`
- Status: `completed`
- Summary: Authenticated mutations now require declared work, the implicit session-task bootstrap no longer applies on the MCP mutation surface, and the updated guidance/tests validate the live server behavior.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Agents learn the required bootstrap order through instructions and error messages instead of trial and error. [any]
- Any authoritative mutation without current or explicit work context is rejected with clear repair guidance. [any]
- The implicit session task fallback is removed from mutation resolution paths. [any]

### Thread work attribution through all mutation provenance and read surfaces

- Node id: `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`
- Kind: `edit`
- Status: `completed`
- Summary: Authenticated coordination, claim, and artifact mutations now preserve the active declared-work chain in their execution context, and the authenticated curator apply path no longer drops inferred-edge promotions onto the non-authenticated branch.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- All authoritative mutation families stamp work context in a consistent schema alongside principal provenance. [any]
- Plan creation, task creation, and later mutations all preserve the originating declared work chain. [any]
- Session/task/read surfaces expose current work state and repair guidance using the new model. [any]

### Unify coordination binding, plan attribution, and delegated-work semantics

- Node id: `coord-task:01kn6rp7gt1mjkkz13ev37dc76`
- Kind: `edit`
- Status: `completed`
- Summary: Delegated work now inherits parent coordination context by default, coordination mutations rebind active work/task state after plan and task transitions, and the session seed persists those bindings.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Coordination tasks bind into work context without being the only provenance carrier. [any]
- Delegated child work can reference parent work cleanly without depending on runtime-only bootstrap history. [any]
- Plan and coordination creation events are attributable to the declared originating work. [any]

### Update instructions, schemas, examples, specs, and concept docs for strict declared work

- Node id: `coord-task:01kn6rppndba3zmwxf2fhcjr24`
- Kind: `edit`
- Status: `completed`
- Summary: Updated runtime docs, schema examples, agent guidance, and specs so the exposed PRISM workflow now describes declare_work-first mutations, automatic checkpoint publication, current work/task session context, and the non-durable status of detached session-task leftovers.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- PRISM concept and relation docs are updated to the cleaned-up work versus coordination model. [any]
- Tool schemas, examples, JS docs, and repo specs reflect the new work terminology and rejection behavior. [any]
- `prism://instructions` teaches adopt -> inspect -> declare work or bind coordination -> mutate. [any]

### Migrate and repair legacy session-task and detached-context behavior

- Node id: `coord-task:01kn6rq1n2ggm0m47a37ewascn`
- Kind: `edit`
- Status: `completed`
- Summary: Legacy bare session-task context no longer survives effective read surfaces or restart persistence without declared work, while coordination-bound task focus still survives when anchored by current work.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Detached or stale legacy task context cannot silently satisfy the new declared-work requirement. [any]
- Legacy session current-task seeds are either upgraded, cleared, or surfaced with explicit repair guidance. [any]
- Migration behavior is bounded and does not reintroduce undeclared fallback semantics. [any]

### Validate cold-clone self-containment and end-to-end declared-work enforcement

- Node id: `coord-task:01kn6rqgdfassyvep8q89sd0mk`
- Kind: `edit`
- Status: `completed`
- Summary: Validated strict declared-work enforcement, restart persistence boundaries, automatic checkpoint publication, and cold-clone self-containment with a repo-memory regression that still reloads inline work context after deleting the shared runtime database; workspace validation is green aside from the known parallel query-history/runtime-refresh flakes that passed in isolated reruns.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Cold-clone tests pass with empty shared runtime state while repo-published provenance remains interpretable. [any]
- End-to-end mutation tests verify declared-work bootstrap, plan/task creation attribution, and undeclared-mutation rejection. [any]
- Workspace test suite, release builds, and MCP restart/health checks pass after the rollout. [any]

### Audit and enforce `.prism` self-containment boundaries

- Node id: `coord-task:01kn6s12m8w9qgs766n8453aa4`
- Kind: `edit`
- Status: `completed`
- Summary: Audited the live .prism publication paths, codified the semantic-versus-correlation boundary, and tightened plan-stream export/tests so runtime session/worktree/branch scope cannot leak into repo-published plan logs.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Defines the concrete enforcement points so repo-published writes cannot introduce unresolved shared-runtime-only references. [any]
- Enumerates all repo-published event and artifact families that can carry references into provenance or coordination state. [any]
- For each reference shape, classifies semantic versus correlation usage and forbids runtime-only semantic dependencies in `.prism`. [any]

### Enforce exclusive principal binding per worktree and define pre-declare edit policy

- Node id: `coord-task:01kn6t3shx6ks37m500hme2gpj`
- Kind: `edit`
- Status: `completed`
- Summary: Enforced one-principal-per-worktree on authenticated mutations by binding the first verified principal in WorkspaceSession and rejecting later mutation attempts from different principals with a structured conflict response.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Bridge and runtime state enforce at most one actively bound principal per worktree at a time, with explicit rejection on ambiguous or conflicting binds. [any]
- The design and runtime contract distinguish hot observed edits from repo-published authored change provenance. [any]
- Watcher-observed edits that occur before declared work exists are handled explicitly and do not silently become durable authored history. [any]

### Accumulate watcher-observed file changes by worktree, principal, and work with automatic flush boundaries

- Node id: `coord-task:01kn6t41ekhqwqedjg2p7pctt7`
- Kind: `edit`
- Status: `completed`
- Summary: Added a core observed-change tracker, mirrored declared work into the workspace session, recorded watcher-observed changes only when principal and work attribution are unambiguous, and flushed accumulated batches automatically on mutation boundaries, work transitions, and disconnect.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Automatic flushes remain unpublished or reject publication when attribution is ambiguous, missing, or no declared work is active. [any]
- The daemon maintains a per-worktree change accumulator keyed by the exclusively bound principal and the active declared work context. [any]
- Watcher-observed file changes flush automatically at authoritative mutation boundaries, work transitions, and disconnect/shutdown boundaries without agent bookkeeping. [any]

### Publish auto-flushed change checkpoints and add an explicit checkpoint escape hatch

- Node id: `coord-task:01kn6t48enhz3xvyer1j7r8tck`
- Kind: `edit`
- Status: `completed`
- Summary: Published durable observed-change checkpoints for mutation boundaries, work transitions, disconnect shutdown, and explicit checkpoint requests, with self-contained work-bound metadata recorded in outcome events and surface tests updated for the new checkpoint behavior.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- An optional explicit checkpoint mutation remains available for meaningful manual milestones without being required for normal agent workflows. [any]
- Auto-flushed change sets attach to the correct work-bound mutation or standalone boundary event with principal, work, and flush-trigger provenance. [any]
- Repo-published change provenance persists self-contained change-set snapshots that resolve from .prism alone and do not require runtime DB state. [any]

## Edges

- `plan-edge:coord-task:01kn6rmjszn0az4kypvh97yq1d:depends-on:coord-task:01kn6rm7bf7k1qnmhcegvypt86`: `coord-task:01kn6rmjszn0az4kypvh97yq1d` depends on `coord-task:01kn6rm7bf7k1qnmhcegvypt86`
- `plan-edge:coord-task:01kn6rmjszn0az4kypvh97yq1d:depends-on:coord-task:01kn6s12m8w9qgs766n8453aa4`: `coord-task:01kn6rmjszn0az4kypvh97yq1d` depends on `coord-task:01kn6s12m8w9qgs766n8453aa4`
- `plan-edge:coord-task:01kn6rn36wwvrz8ezq8p0e02d6:depends-on:coord-task:01kn6rm7bf7k1qnmhcegvypt86`: `coord-task:01kn6rn36wwvrz8ezq8p0e02d6` depends on `coord-task:01kn6rm7bf7k1qnmhcegvypt86`
- `plan-edge:coord-task:01kn6rnm8sffvrtsv6km1btd98:depends-on:coord-task:01kn6rmjszn0az4kypvh97yq1d`: `coord-task:01kn6rnm8sffvrtsv6km1btd98` depends on `coord-task:01kn6rmjszn0az4kypvh97yq1d`
- `plan-edge:coord-task:01kn6rnm8sffvrtsv6km1btd98:depends-on:coord-task:01kn6rn36wwvrz8ezq8p0e02d6`: `coord-task:01kn6rnm8sffvrtsv6km1btd98` depends on `coord-task:01kn6rn36wwvrz8ezq8p0e02d6`
- `plan-edge:coord-task:01kn6rnxxzfcwhtfmty9j1hdvr:depends-on:coord-task:01kn6rmjszn0az4kypvh97yq1d`: `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr` depends on `coord-task:01kn6rmjszn0az4kypvh97yq1d`
- `plan-edge:coord-task:01kn6rnxxzfcwhtfmty9j1hdvr:depends-on:coord-task:01kn6rn36wwvrz8ezq8p0e02d6`: `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr` depends on `coord-task:01kn6rn36wwvrz8ezq8p0e02d6`
- `plan-edge:coord-task:01kn6rnxxzfcwhtfmty9j1hdvr:depends-on:coord-task:01kn6rnm8sffvrtsv6km1btd98`: `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr` depends on `coord-task:01kn6rnm8sffvrtsv6km1btd98`
- `plan-edge:coord-task:01kn6rp7gt1mjkkz13ev37dc76:depends-on:coord-task:01kn6rmjszn0az4kypvh97yq1d`: `coord-task:01kn6rp7gt1mjkkz13ev37dc76` depends on `coord-task:01kn6rmjszn0az4kypvh97yq1d`
- `plan-edge:coord-task:01kn6rp7gt1mjkkz13ev37dc76:depends-on:coord-task:01kn6rn36wwvrz8ezq8p0e02d6`: `coord-task:01kn6rp7gt1mjkkz13ev37dc76` depends on `coord-task:01kn6rn36wwvrz8ezq8p0e02d6`
- `plan-edge:coord-task:01kn6rppndba3zmwxf2fhcjr24:depends-on:coord-task:01kn6rnm8sffvrtsv6km1btd98`: `coord-task:01kn6rppndba3zmwxf2fhcjr24` depends on `coord-task:01kn6rnm8sffvrtsv6km1btd98`
- `plan-edge:coord-task:01kn6rppndba3zmwxf2fhcjr24:depends-on:coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`: `coord-task:01kn6rppndba3zmwxf2fhcjr24` depends on `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`
- `plan-edge:coord-task:01kn6rppndba3zmwxf2fhcjr24:depends-on:coord-task:01kn6rp7gt1mjkkz13ev37dc76`: `coord-task:01kn6rppndba3zmwxf2fhcjr24` depends on `coord-task:01kn6rp7gt1mjkkz13ev37dc76`
- `plan-edge:coord-task:01kn6rppndba3zmwxf2fhcjr24:depends-on:coord-task:01kn6t48enhz3xvyer1j7r8tck`: `coord-task:01kn6rppndba3zmwxf2fhcjr24` depends on `coord-task:01kn6t48enhz3xvyer1j7r8tck`
- `plan-edge:coord-task:01kn6rq1n2ggm0m47a37ewascn:depends-on:coord-task:01kn6rnm8sffvrtsv6km1btd98`: `coord-task:01kn6rq1n2ggm0m47a37ewascn` depends on `coord-task:01kn6rnm8sffvrtsv6km1btd98`
- `plan-edge:coord-task:01kn6rq1n2ggm0m47a37ewascn:depends-on:coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`: `coord-task:01kn6rq1n2ggm0m47a37ewascn` depends on `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`
- `plan-edge:coord-task:01kn6rqgdfassyvep8q89sd0mk:depends-on:coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`: `coord-task:01kn6rqgdfassyvep8q89sd0mk` depends on `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`
- `plan-edge:coord-task:01kn6rqgdfassyvep8q89sd0mk:depends-on:coord-task:01kn6rp7gt1mjkkz13ev37dc76`: `coord-task:01kn6rqgdfassyvep8q89sd0mk` depends on `coord-task:01kn6rp7gt1mjkkz13ev37dc76`
- `plan-edge:coord-task:01kn6rqgdfassyvep8q89sd0mk:depends-on:coord-task:01kn6rppndba3zmwxf2fhcjr24`: `coord-task:01kn6rqgdfassyvep8q89sd0mk` depends on `coord-task:01kn6rppndba3zmwxf2fhcjr24`
- `plan-edge:coord-task:01kn6rqgdfassyvep8q89sd0mk:depends-on:coord-task:01kn6t48enhz3xvyer1j7r8tck`: `coord-task:01kn6rqgdfassyvep8q89sd0mk` depends on `coord-task:01kn6t48enhz3xvyer1j7r8tck`
- `plan-edge:coord-task:01kn6s12m8w9qgs766n8453aa4:depends-on:coord-task:01kn6rm7bf7k1qnmhcegvypt86`: `coord-task:01kn6s12m8w9qgs766n8453aa4` depends on `coord-task:01kn6rm7bf7k1qnmhcegvypt86`
- `plan-edge:coord-task:01kn6t3shx6ks37m500hme2gpj:depends-on:coord-task:01kn6s12m8w9qgs766n8453aa4`: `coord-task:01kn6t3shx6ks37m500hme2gpj` depends on `coord-task:01kn6s12m8w9qgs766n8453aa4`
- `plan-edge:coord-task:01kn6t41ekhqwqedjg2p7pctt7:depends-on:coord-task:01kn6rn36wwvrz8ezq8p0e02d6`: `coord-task:01kn6t41ekhqwqedjg2p7pctt7` depends on `coord-task:01kn6rn36wwvrz8ezq8p0e02d6`
- `plan-edge:coord-task:01kn6t41ekhqwqedjg2p7pctt7:depends-on:coord-task:01kn6rnm8sffvrtsv6km1btd98`: `coord-task:01kn6t41ekhqwqedjg2p7pctt7` depends on `coord-task:01kn6rnm8sffvrtsv6km1btd98`
- `plan-edge:coord-task:01kn6t41ekhqwqedjg2p7pctt7:depends-on:coord-task:01kn6t3shx6ks37m500hme2gpj`: `coord-task:01kn6t41ekhqwqedjg2p7pctt7` depends on `coord-task:01kn6t3shx6ks37m500hme2gpj`
- `plan-edge:coord-task:01kn6t48enhz3xvyer1j7r8tck:depends-on:coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`: `coord-task:01kn6t48enhz3xvyer1j7r8tck` depends on `coord-task:01kn6rnxxzfcwhtfmty9j1hdvr`
- `plan-edge:coord-task:01kn6t48enhz3xvyer1j7r8tck:depends-on:coord-task:01kn6t41ekhqwqedjg2p7pctt7`: `coord-task:01kn6t48enhz3xvyer1j7r8tck` depends on `coord-task:01kn6t41ekhqwqedjg2p7pctt7`

