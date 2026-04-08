# PRISM Coordination Graph Rewrite

Status: normative implementation target  
Audience: PRISM core, coordination, query, MCP, UI, and shared-ref maintainers  
Scope: complete replacement of the current hybrid coordination task plus plan-node model

Supersedes for execution planning: [archived/PRISM_FIRST_CLASS_PLANS_SPEC.md](./archived/PRISM_FIRST_CLASS_PLANS_SPEC.md)

Concurrent-write replay, semantic merge, and rejection rules are defined in
[PRISM_COORDINATION_CONFLICT_HANDLING.md](./PRISM_COORDINATION_CONFLICT_HANDLING.md).

---

## 1. Summary

PRISM should replace the current hybrid coordination model with one executable graph model built
from exactly two entity types:

- `Plan`
- `Task`

No other graph entity type exists.

`Plan` is a container and strategic boundary.
`Task` is an atomic executable leaf.

Every non-root node has exactly one parent `Plan`.
Every dependency is an explicit edge between two existing nodes.
Only `Task` is claimable.
`Plan` status is derived.
`Task` actionability is computed by the daemon.

Human review, operator approval, test execution, deployment, and service work are all represented
as `Task`s with executor routing constraints. They are not distinct graph node types.

This rewrite removes the current three-layer model:

- coordination plans and coordination tasks
- first-class plan nodes and plan edges
- compatibility shims that try to keep both in sync

and replaces it with one canonical shared coordination graph.

The resulting architecture has these properties:

- one durable graph model
- one readiness engine
- one containment model
- one dependency model
- one claimability rule
- one shared-ref persistence format
- one MCP and UI surface

This document is normative.

If an older spec, a current implementation detail, or a compatibility shim disagrees with this
document, this document wins.

There are no intentionally unresolved model questions in this rewrite. Implementation work should
follow this document directly rather than reopening entity-shape or execution-semantics design.

---

## 2. Goals

Required goals:

1. Make `Plan` and `Task` the only graph entity types.
2. Make `Task` the only claimable leaf type.
3. Support nested plans with exact single-parent containment.
4. Support dependencies between any two nodes, including cross-plan dependencies.
5. Make the combined containment plus dependency graph a DAG enforced at mutation time.
6. Make plan status fully derived except for explicit operator overrides `abandoned` and
   `archived`.
7. Make task routing identity-aware without introducing new graph node types.
8. Make shared coordination refs and local SQLite read models speak one canonical schema.
9. Remove the durable distinction between coordination tasks and native plan nodes.
10. Preserve enough compatibility to migrate existing shared coordination state without losing
    meaning-bearing data.

Non-goals:

1. This rewrite does not change the shared-ref scaling, versioning, pruning, or semantic merge
   decisions already captured in [PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md](./PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md).
2. This rewrite does not remove task policies, git execution policy, claims, artifacts, or review
   surfaces. It rehomes them onto the new `Task`/`Plan` graph. The normative artifact, review, and
   review-task contract now lives in
   [contracts/coordination-artifact-review-model.md](./contracts/coordination-artifact-review-model.md).
3. This rewrite does not add a portfolio entity type. Root plans remain the portfolio surface.

---

## 3. Final Graph Model

### 3.1 Entity Types

There are exactly two graph entities.

#### Plan

Purpose:

- container
- strategic boundary
- grouping unit
- lifecycle aggregate

Properties:

- never directly claimable
- may contain child plans and tasks
- status is derived except for explicit operator override
- estimate is derived from descendant tasks

#### Task

Purpose:

- atomic execution unit
- claimable leaf
- executor-routed work item

Properties:

- always a leaf
- always belongs to exactly one parent plan
- status lifecycle is authored on the task itself
- estimate is stored directly on the task

### 3.2 Containment

Containment is explicit, not inferred from `root_tasks` or `child_of` edges.

Rules:

- every `Task` has exactly one `parent_plan_id`
- every non-root `Plan` has exactly one `parent_plan_id`
- every root `Plan` has `parent_plan_id = null`
- `Task` cannot contain children
- `Plan` may contain any mix of direct child plans and direct child tasks
- shallow hierarchies are preferred, but there is no hard nesting limit

Containment is stored as parent references, not as authored graph edges.

### 3.3 Dependencies

Dependencies are explicit precedence edges.

Each dependency edge means:

`source` cannot become graph-actionable until `target` is completed.

Allowed combinations:

- `Task -> Task`
- `Task -> Plan`
- `Plan -> Task`
- `Plan -> Plan`

No distinction exists between intra-plan and cross-plan dependency shape. Cross-plan dependencies
are allowed.

Containment and dependencies are distinct:

- containment answers ownership
- dependencies answer readiness

### 3.4 DAG Constraint

The combined graph made from:

- containment edges `parent_plan -> child`
- dependency edges `source -> target`

must be acyclic.

Mutation-time validation must reject:

- self-dependencies
- duplicate dependency edges
- dependency edges that introduce a cycle when combined with containment
- parent assignments that introduce a containment cycle

The validator must treat both plans and tasks as nodes in the same cycle check.

---

## 4. Durable Data Model

This section defines the canonical v2 durable model. Shared refs, SQLite cache, MCP, and query
surfaces must converge on these semantics.

### 4.1 Plan

Durable fields:

- `id: PlanId`
- `parent_plan_id: Option<PlanId>`
- `title: String`
- `goal: String`
- `scope: PlanScope`
- `kind: PlanKind`
- `policy: CoordinationPolicy`
- `scheduling: PlanScheduling`
- `tags: Vec<String>`
- `created_from: Option<String>`
- `metadata: JsonValue`
- `operator_state: PlanOperatorState`

`PlanOperatorState` values:

- `none`
- `abandoned`
- `archived`

Important rule:

- durable `Plan` does not store derived execution status
- durable `Plan` does not store child ids
- durable `Plan` does not store `root_tasks`
- durable `Plan` does not store authored dependency edges inline

Children are discovered by parent references.
Dependencies are stored in the dependency table.
Status is derived into the read model.

### 4.2 Task

Durable fields:

- `id: TaskId`
- `parent_plan_id: PlanId`
- `title: String`
- `summary: Option<String>`
- `lifecycle_status: TaskLifecycleStatus`
- `estimated_minutes: u32`
- `executor: TaskExecutorPolicy`
- `assignee: Option<AgentId>`
- `session: Option<SessionId>`
- `lease_holder: Option<LeaseHolder>`
- `lease_started_at: Option<Timestamp>`
- `lease_refreshed_at: Option<Timestamp>`
- `lease_stale_at: Option<Timestamp>`
- `lease_expires_at: Option<Timestamp>`
- `worktree_id: Option<String>`
- `branch_ref: Option<String>`
- `anchors: Vec<AnchorRef>`
- `bindings: PlanBinding`
- `acceptance: Vec<AcceptanceCriterion>`
- `validation_refs: Vec<ValidationRef>`
- `base_revision: WorkspaceRevision`
- `priority: Option<u8>`
- `tags: Vec<String>`
- `metadata: JsonValue`
- `git_execution: TaskGitExecution`

`TaskLifecycleStatus` values:

- `pending`
- `active`
- `completed`
- `failed`
- `abandoned`

Important rule:

- durable `Task` does not store `blocked`
- durable `Task` does not store `broken_dependency`

Those are derived read-model states.

### 4.3 Task Executor Policy

Every task carries executor routing constraints.

`TaskExecutorPolicy` is the durable struct:

- `executor_class: ExecutorClass`
- `target_label: Option<String>`
- `allowed_principals: Vec<PrincipalId>`

Durable fields:

- `executor_class: ExecutorClass`
- `target_label: Option<String>`
- `allowed_principals: Vec<PrincipalId>`

`ExecutorClass` values:

- `human`
- `worktree_executor`
- `service`

Matching rules:

1. caller principal must advertise the same `executor_class`
2. if `target_label` is set, caller principal must advertise that label
3. if `allowed_principals` is non-empty, caller principal id must appear in it

Interpretation:

- `executor_class` is coarse routing
- `target_label` is pool affinity
- `allowed_principals` is strict pinning

Defaults:

- if omitted during compatibility import, `executor_class = worktree_executor`
- `target_label = null`
- `allowed_principals = []`

Important rule:

- a human review, operator approval, deployment gate, CI step, or service action is still just a
  `Task`
- executor policy is the only routing mechanism for leaf execution ownership
- no additional leaf node type may be introduced for execution routing

### 4.4 Node References

All cross-entity references in the canonical v2 execution graph use:

- `NodeRef { kind: NodeKind, id: DurableId }`

`NodeKind` values:

- `plan`
- `task`

Rules:

- `PlanId` and `TaskId` remain distinct durable id domains
- query surfaces may return plan-specific and task-specific ids separately for convenience
- durable dependency storage and runtime traversal must normalize through `NodeRef`

### 4.5 Dependency Edge

Durable fields:

- `source: NodeRef`
- `target: NodeRef`

The only durable dependency edge kind is `depends_on`.

There are no durable graph edge kinds for:

- `blocks`
- `validates`
- `handoff_to`
- `informs`
- `related_to`
- `child_of`

Those are removed from the authoritative execution graph.

### 4.6 Estimates

`Task.estimated_minutes` is a required non-negative integer.

`Plan.estimated_minutes_total` is derived as:

- the sum of `estimated_minutes` for every descendant task in the plan subtree
- regardless of lifecycle state

`Plan.remaining_estimated_minutes` is derived as:

- the sum of `estimated_minutes` for descendant tasks whose effective status is one of:
  - `pending`
  - `active`
  - `blocked`
  - `broken_dependency`

These are read-model fields only.

---

## 5. Read-Model Status Semantics

The daemon computes effective status. Agents do not.

### 5.1 Effective Task Status

Query and UI surfaces expose:

- `pending`
- `active`
- `blocked`
- `broken_dependency`
- `completed`
- `failed`
- `abandoned`

Derivation for a task:

1. if `lifecycle_status = completed`, effective status is `completed`
2. if `lifecycle_status = failed`, effective status is `failed`
3. if `lifecycle_status = abandoned`, effective status is `abandoned`
4. if any direct dependency target has effective status `abandoned`, effective status is
   `broken_dependency`
5. if any direct dependency target has effective status `broken_dependency`, effective status is
   `blocked`
6. if any direct dependency target is not `completed`, effective status is `blocked`
7. if any ancestor plan has derived status `blocked`, effective status is `blocked`
8. if any ancestor plan has derived status `broken_dependency`, effective status is
   `broken_dependency`
9. if `lifecycle_status = active`, effective status is `active`
10. otherwise effective status is `pending`

Important rule:

- dependency target `failed` does not create `broken_dependency`
- it leaves direct dependents `blocked` with blocker cause `dependency_failed`

Reason:

- a failed prerequisite is operationally retryable or replaceable
- an abandoned prerequisite is an explicit structural break that cannot self-heal

### 5.2 Derived Plan Status

Query and UI surfaces expose:

- `pending`
- `active`
- `blocked`
- `broken_dependency`
- `completed`
- `failed`
- `abandoned`
- `archived`

Derivation order is exact and must be implemented in this precedence:

1. if `operator_state = archived`, status is `archived`
2. if `operator_state = abandoned`, status is `abandoned`
3. if any direct dependency target has effective status `abandoned`, status is
   `broken_dependency`
4. if any direct child has effective status `broken_dependency` or derived status
   `broken_dependency`, status is `broken_dependency`
5. if every descendant task in the subtree is `completed`, status is `completed`
6. if every descendant task in the subtree is terminal and at least one descendant task is
   `failed` or `abandoned`, status is `failed`
7. if any direct dependency target is not `completed`, status is `blocked`
8. if any descendant task is `active`, status is `active`
9. if every descendant task has effective status `pending`, status is `pending`
10. if there exists at least one graph-actionable descendant task, status is `active`
11. otherwise status is `blocked`

Notes:

- a plan with no children is invalid and must not be created by normal mutation flows
- migration may temporarily materialize an empty child plan, but migration must fill or remove it
  before the final persisted v2 state is written
- a plan with only child plans and no direct tasks is valid
- descendant-task aggregation always traverses through child plans recursively

### 5.3 Terminal Definitions

Task terminal statuses:

- `completed`
- `failed`
- `abandoned`

Plan terminal statuses:

- `completed`
- `failed`
- `abandoned`
- `archived`

`blocked` and `broken_dependency` are non-terminal.

---

## 6. Actionability and Routing

### 6.1 Graph-Actionable Tasks

A task is graph-actionable when all of the following are true:

1. effective task status is `pending`
2. all direct dependency targets are `completed`
3. no direct dependency target is `abandoned`
4. every ancestor plan status is either `pending` or `active`

This computation is identity-agnostic.

The daemon should expose this internal concept as:

- `graph_actionable_tasks()`

### 6.2 Runnable Tasks For A Caller

The externally surfaced actionable task query is identity-aware.

For caller principal `P`, a task is runnable when:

1. it is graph-actionable
2. `P` satisfies the task executor policy

The canonical user-facing query is:

- `actionable_tasks(principal)`

This query must filter after graph actionability is computed, not before.

The daemon must also expose the reason a graph-actionable task is not runnable for the caller when
requested through diagnostic surfaces. Canonical reasons are:

- `executor_class_mismatch`
- `target_label_mismatch`
- `principal_not_allowed`

### 6.3 Claim and Mutation Enforcement

Routing is not advisory only.

Mutation enforcement rules:

- only `Task` may be claimed
- caller must satisfy executor policy to:
  - claim a task
  - start a task
  - resume a task
  - complete a task
  - fail a task
  - abandon a task
- plan mutations are operator-only and non-claimable

If a caller does not satisfy executor policy:

- task discovery omits the task from runnable results
- direct mutation attempts are rejected with `ExecutorMismatch`

### 6.4 Human Tasks

Human tasks are ordinary tasks with:

- `executor_class = human`

They can be:

- discovered by the SSR console
- claimed and completed by a human principal
- blocked on the same dependency rules as any other task

No separate review node or approval node type exists.

### 6.5 Service Tasks

Service tasks are ordinary tasks with:

- `executor_class = service`

They can be:

- claimed by service principals
- driven by automation or daemon workers
- blocked and completed under the same graph rules

---

## 7. Mutation Rules

All structural coordination edits in this rewrite are transaction-based.

This is mandatory because shared coordination is published through compare-and-swap Git updates.
Any multi-step graph rewrite that is emitted as separate durable mutations can tear on mid-sequence
CAS failure and leave orphaned nodes, invalid containment, or partially rewired dependencies in the
authoritative branch.

Therefore:

- one logical graph rewrite must be represented as one transaction
- one successful transaction produces one durable state transition
- one successful transaction publishes through one authoritative shared-ref update
- partial transaction application is forbidden

The daemon may use SQLite transactions, in-memory staged state, or equivalent local machinery
internally, but the externally visible contract is atomic all-or-nothing mutation behavior.

### 7.1 Allowed Create Mutations

Allowed durable graph creates:

- `plan_create`
- `task_create`
- `dependency_create`

Allowed containment creates:

- root plan creation with `parent_plan_id = null`
- child plan creation with `parent_plan_id = existing plan`
- task creation with `parent_plan_id = existing plan`

Disallowed:

- task children
- free-floating tasks
- free-floating child plans without parent
- separate `plan_node_create`
- separate `plan_edge_create`

### 7.1A Transactional Mutation API

The canonical structural mutation surface is:

- `coordination_transaction`

The transaction payload contains:

- ordered primitive mutations
- optional caller-supplied intent metadata
- optional optimistic concurrency preconditions

Canonical shape:

```json
{
  "action": "coordination_transaction",
  "input": {
    "mutations": [
      { "action": "plan_create", "input": { "...": "..." } },
      { "action": "task_create", "input": { "...": "..." } },
      { "action": "dependency_create", "input": { "...": "..." } },
      { "action": "task_update", "input": { "...": "..." } }
    ]
  }
}
```

Normative execution rules:

1. the daemon applies the full mutation list against one staged coordination snapshot
2. validation runs against the final staged graph, not piecemeal durable intermediate states
3. if any mutation or invariant check fails, the entire transaction aborts
4. on abort, no durable shared-coordination state changes are written
5. on success, the daemon emits one durable revision and one authoritative publish

Transaction invariants checked against the final staged graph include:

- containment validity
- dependency validity
- global DAG validity across containment plus dependencies
- no orphan plans or tasks
- executor-policy invariants
- lifecycle-transition validity
- claim and lease validity
- archival and abandonment preconditions

The mutation list is ordered and later mutations may refer to entities created earlier in the same
transaction.

### 7.1B Primitive Mutations Versus Macros

Transactions are the only authoritative structural write unit.

High-level authoring helpers may still exist, but they are macros over transactions rather than
independent persistence paths.

Examples:

- `plan_bootstrap`
- `convert_task_to_plan`
- `split_task`
- `reparent_subtree`

Rule:

- each helper must compile into one `coordination_transaction`
- helper-specific validation may run before expansion, but the authoritative commit path is always
  the transaction engine

`plan_bootstrap` therefore remains allowed as convenience authoring sugar, but it is no longer a
foundational primitive. Its dedicated persistence logic must be removed, and its runtime behavior
must become exactly equivalent to a transaction that creates the initial plan subtree and
dependencies.

### 7.2 Allowed Update Mutations

Plan updates may change:

- title
- goal
- policy
- scheduling
- tags
- metadata
- parent assignment, subject to cycle validation
- operator state (`none`, `abandoned`, `archived`) under the rules below

Task updates may change:

- title
- summary
- `lifecycle_status`
- `estimated_minutes`
- executor policy
- assignee
- anchors
- bindings
- acceptance
- validation refs
- base revision
- priority
- tags
- metadata
- git execution state
- parent assignment, subject to cycle validation

Explicitly disallowed direct mutations:

- setting a plan's derived status directly
- setting a task's effective status directly
- creating or updating a task child
- storing dependency edges inline on a plan or task record
- creating authored executable graph edges other than `depends_on`

Any operation that performs more than one structural change to the coordination graph must be
exposed only through `coordination_transaction`, not through a sequence of separate top-level MCP
calls.

### 7.3 Plan Abandonment

Plan abandonment is an explicit operator action.

When a plan is abandoned:

1. every non-terminal descendant task is set to `abandoned`
2. every descendant plan receives `operator_state = abandoned`
3. active leases and claims rooted in the abandoned subtree are released or expired
4. direct dependents outside the subtree re-evaluate under normal broken-dependency rules

This is a cascading mutation, not a derived status only.

### 7.4 Plan Archival

Plan archival is allowed only when the plan subtree is terminal.

Archive preconditions:

1. every descendant task is terminal
2. there are no active claims in the subtree
3. there are no active task leases in the subtree

Archive effects:

- set `operator_state = archived`
- keep the subtree readable in history and archive surfaces
- remove the subtree from default active portfolio views

### 7.5 Dependency Mutation Semantics

Dependency mutations may target any existing plan or task.

Validation rules:

- source must exist
- target must exist
- source and target may be in different plans
- source and target may not be identical
- duplicate edges are rejected
- the combined graph must remain acyclic

### 7.6 Completion Contract

A dependency is satisfied only by `completed`.

This is absolute.

If downstream work needs branch publication, shared coordination publication, target integration,
review evidence, or validation evidence, that requirement must be expressed in one of these ways:

1. the upstream task is not marked `completed` until its completion policy is satisfied
2. the workflow is decomposed into multiple tasks with explicit dependencies

The rewrite removes special dependency lifecycle buckets such as:

- coordination-published dependency
- integrated-to-target dependency

Those are task completion semantics, not edge kinds.

### 7.7 Parent Reassignment Semantics

Parent reassignment is a first-class mutation for both plans and tasks.

Rules:

1. reassignment must preserve the global DAG constraint
2. reassignment must not orphan a non-root plan
3. reassignment must not move a task beneath another task
4. reassignment must not create a second parent for any node
5. reassignment immediately re-evaluates:
   - ancestor derived status
   - descendant effective status
   - graph-actionable task sets
   - affected plan estimate summaries

---

## 8. Blockers and Broken Dependencies

### 8.1 Blocker Causes

The read model must keep blocker causes explicit.

Canonical blocker causes include:

- dependency incomplete
- dependency failed
- dependency abandoned
- ancestor plan blocked
- ancestor plan broken_dependency
- executor mismatch
- stale revision
- claim conflict
- review required
- validation required
- artifact stale

### 8.2 Broken Dependency Rules

`broken_dependency` is reserved for explicit structural breakage.

A node enters `broken_dependency` when:

- one of its direct dependency targets is `abandoned`
- or one of its contained descendants is already `broken_dependency`

`broken_dependency` propagation rules:

- direct dependency propagation applies only one edge hop
- containment propagation applies recursively upward through ancestor plans
- downstream dependents of a `broken_dependency` node become `blocked`, not
  `broken_dependency`, unless they themselves directly depend on an `abandoned` target

This preserves the distinction between:

- structural breakage at the direct dependency
- ordinary downstream unavailability

### 8.3 Failed Dependency Rules

If a direct dependency target is `failed`:

- the dependent is `blocked`
- blocker cause is `dependency_failed`
- no automatic `broken_dependency` is set

Resolution is manual:

- reopen or replace the failed prerequisite
- remove or rewire the dependency
- or abandon the downstream work explicitly

---

## 9. Query Surface

### 9.1 Canonical Read APIs

The coordination query layer must converge on these concepts:

- `plan(id)` returns plan metadata plus derived status and direct children
- `task(id)` returns task metadata plus effective status and blockers
- `children(plan_id)` returns direct child plans and direct child tasks
- `dependencies(node_ref)` returns direct dependency refs
- `dependents(node_ref)` returns direct dependents
- `graph_actionable_tasks()` returns identity-agnostic actionable tasks
- `actionable_tasks(principal)` returns runnable tasks for caller identity
- `plan_summary(id)` derives from subtree status and estimates
- `portfolio()` returns root plans only by default

The canonical write API for structural graph edits is:

- `coordination_transaction(mutations)`

Single-step helpers may continue to exist, but only as thin wrappers around the same transaction
engine.

### 9.2 Query Payload Shape

Every surfaced plan and task view must include:

- durable authored fields
- derived status
- blocker summary
- estimate summary
- containment references
- dependency references

Task views must include both:

- `lifecycle_status`
- `effective_status`

Plan views must include:

- `operator_state`
- `derived_status`

The old flat `status` field may remain in compatibility views, but in v2 it means:

- task `status = effective_status`
- plan `status = derived_status`

### 9.3 Portfolio and Child Rendering

Portfolio view:

- show root plans only by default
- if a grouped view is requested, show at most one child-plan level inline and require explicit
  expansion for deeper nesting

Plan detail view:

- show direct child tasks
- show direct child plans
- show external dependency stubs for cross-plan edges

No query should require reconstructing containment from `root_tasks` or `child_of`.

---

## 10. UI Rendering Model

### 10.1 Plan Detail Graph

Each plan detail page renders direct children only.

Rendering rules:

- direct child tasks render as task nodes
- direct child plans render as plan nodes with distinct styling
- cross-plan dependency targets render as external stub nodes
- clicking a child plan navigates to that plan
- clicking an external stub navigates to the target plan or task detail

### 10.2 Task Presentation

Task cards and nodes should show:

- title
- effective status
- executor class
- target label when present
- estimate
- direct blockers
- dependency count

Human-only tasks must be visibly distinct from worktree-executor tasks.

### 10.3 Broken Dependency Presentation

Broken dependencies must be surfaced prominently in:

- plan detail
- portfolio summaries
- task detail
- inbox and actionable views

The operator must be able to see:

- which dependency was abandoned
- which node is now broken
- what rewiring or abandonment action is required

---

## 11. Persistence and Shared Refs

### 11.1 Schema Version

This rewrite introduces coordination schema version `2`.

Every authoritative shared-coordination payload in this rewrite must carry:

```json
{
  "schema_version": 2
}
```

Readers must:

- accept `schema_version <= current_supported`
- reject `schema_version > current_supported` with an explicit upgrade message

### 11.2 Authoritative Shared Representation

Authoritative shared coordination state must persist:

- plans
- tasks
- dependencies
- claims
- artifacts
- reviews
- runtime descriptors

The authoritative execution graph for plans and tasks must be reconstructible without any
plan-node compatibility layer.

The authoritative v2 shared payload for execution planning therefore consists of:

- plan records
- task records
- dependency records
- claims, reviews, artifacts, and task-local execution overlays keyed to plan or task ids

It must not contain:

- durable native plan nodes
- durable native plan edges
- `root_tasks`
- authored execution edge kinds other than `depends_on`

### 11.3 Local SQLite Role

Local SQLite remains:

- local cache
- read model
- checkpoint materialization
- restart accelerator

It is not authoritative for the coordination graph.

SQLite may store:

- normalized authored snapshots
- derived status tables
- blocker tables
- actionable-task indexes
- UI-oriented materializations

Every such table must be disposable and rebuildable from authoritative shared state plus local
runtime-only overlays.

---

## 12. Compatibility and Migration

### 12.1 Compatibility Window

The rewrite must ship with dual readers and single writers:

- readers accept old model and new model
- new mutations write only the new model

No new authoritative state should be written in the old hybrid plan-node format once v2 writing is
enabled.

### 12.2 Legacy Model Inputs

Legacy durable inputs may contain:

- plans with `root_tasks`
- tasks with dependency buckets
- task-backed plan graphs
- standalone native plan nodes
- authored edges with kinds:
  - `depends_on`
  - `blocks`
  - `validates`
  - `handoff_to`
  - `informs`
  - `related_to`
  - `child_of`

### 12.3 Deterministic Migration Rules

Migration to v2 is exact:

#### Plans

- every legacy plan becomes a v2 plan
- `parent_plan_id = null` unless created from an abstract legacy node
- `status` does not persist directly
- legacy `Archived` maps to `operator_state = archived`
- legacy `Abandoned` maps to `operator_state = abandoned`
- all other legacy plan statuses map to `operator_state = none`

#### Tasks

Every existing coordination task becomes a v2 task under its legacy plan.

Legacy status mapping:

- `Proposed -> pending`
- `Ready -> pending`
- `Blocked -> pending`
- `InProgress -> active`
- `InReview -> active`
- `Validating -> active`
- `Completed -> completed`
- `Abandoned -> abandoned`

Additional migration metadata:

- if legacy status was `InReview`, set `metadata.legacy_phase = "in_review"`
- if legacy status was `Validating`, set `metadata.legacy_phase = "validating"`
- if legacy status was `Blocked`, set `metadata.legacy_phase = "blocked"`
- if a legacy task lacked an estimate, set `estimated_minutes = 0`
- if a legacy task carried a claimable non-human executable node role, import it as
  `executor_class = worktree_executor` unless explicit principal routing metadata exists

#### Dependency Buckets

Legacy dependency buckets:

- `depends_on`
- `coordination_depends_on`
- `integrated_depends_on`

all become plain v2 dependency edges to the same targets.

Reason:

- in v2, dependency satisfaction is always `target completed`
- publication and integration semantics belong to upstream task completion policy, not edge kinds

#### Abstract Legacy Plan Nodes

Legacy abstract plan nodes are migrated to child plans.

Rules:

1. each abstract node becomes one child plan under the abstract node's containing plan
2. the new child plan title and goal both equal the abstract node title unless the legacy summary
   is present, in which case:
   - `title = legacy title`
   - `goal = legacy summary`
3. any node connected to the abstract node by `child_of` becomes a direct child of the new child
   plan
4. if a node has multiple `child_of` parents, migration must fail with a repair-required error
5. if an abstract node has no `child_of` children, migration still creates the child plan and
   immediately inserts one placeholder task:
   - `title = "Fill migrated empty child plan"`
   - `lifecycle_status = pending`
   - `executor_class = worktree_executor`
   - `estimated_minutes = 0`

#### Non-Abstract Standalone Legacy Plan Nodes

Legacy standalone leaf plan nodes that are not already task-backed become v2 tasks.

Mapping:

- `parent_plan_id = containing plan or migrated child plan`
- `estimated_minutes = 0`
- `executor_class = worktree_executor`
- `target_label = null`
- `allowed_principals = []`
- authored node fields map onto task fields where names overlap

#### Legacy Edge Kind Mapping

Legacy `depends_on` edges:

- become v2 dependencies

Legacy `blocks` edges:

- become v2 dependencies

Legacy `child_of` edges:

- become containment only
- do not survive as dependency edges

Legacy `validates`, `handoff_to`, `informs`, and `related_to` edges:

- do not survive as authoritative graph edges
- are copied into source node `metadata.legacy_edges`
- are emitted into the migration artifact report

This preserves the information without keeping extra executable edge kinds in the new model.

#### Review and Validation Constructs

Legacy review, validation, handoff, and artifact requirements do not become graph node types during
migration.

Migration rules:

- validation requirements attach to the owning task's acceptance or validation fields
- review requirements attach to the owning task's review or acceptance fields
- handoff annotations attach to task metadata
- none of these constructs create new executable graph entities

### 12.4 Canonical Mutation Surface

The compatibility window is closed.

- `task_create` creates canonical v2 tasks
- `plan_create` creates canonical v2 plans
- containment is expressed through parent plan ids
- dependencies are expressed through canonical task dependency fields

There is no separate `plan_node_create`, `plan_edge_create`, or `plan_edge_delete` mutation
surface anymore.

This compatibility surface is temporary and must be removed after the migration window.

Compatibility removal condition:

- once all authoritative shared coordination payloads in supported repos are on schema version `2`
  and all supported MCP/CLI mutation callers have been upgraded to native v2 mutations, the
  compatibility aliases must be deleted rather than retained indefinitely

Compatibility rule for `plan_bootstrap`:

- during the migration window, `plan_bootstrap` remains callable
- internally it must expand into `coordination_transaction`
- it must not retain a bespoke persistence, validation, or publish path

---

## 13. Implementation Plan

Implementation order is fixed.

### Phase 1: IR and Persistence

1. add v2 plan operator state
2. add v2 task lifecycle status and executor policy
3. add explicit parent references for plan and task
4. add dependency table keyed by typed refs
5. add shared-ref v2 payload envelopes
6. add SQLite cache schema for v2 read models and authoritative snapshots

### Phase 2: Runtime Derivation

1. implement combined containment plus dependency DAG validator
2. implement effective task status derivation
3. implement derived plan status derivation
4. implement graph-actionable task computation
5. implement identity-aware runnable task filtering
6. enforce executor policy in claim and lifecycle mutations

### Phase 3: Transaction Engine

1. implement `coordination_transaction`
2. stage primitive mutations against one in-memory or local-transaction snapshot
3. validate the final staged graph before any durable write
4. emit one durable revision and one authoritative shared-ref publish on success
5. rebase `plan_bootstrap` onto transaction expansion
6. add transaction-safe graph rewrite helpers for decomposition and reparenting

### Phase 4: Compatibility Layer

1. add v1 reader
2. add deterministic v1-to-v2 migrator
3. add compatibility mutation shims
4. emit migration artifact reports for discarded legacy edge semantics

### Phase 5: Query and MCP Rewrite

1. replace plan-node-first query surfaces with plan/task surfaces
2. retire plan-node-ready and plan-node-blocker APIs or rebase them as task views
3. update MCP schemas, vocabulary, and examples
4. update SSR console rendering to direct children plus external stubs

### Phase 6: Cleanup

1. remove durable native plan node storage for execution graphs
2. remove `root_tasks`
3. remove authored edge kinds from authoritative persistence
4. remove compatibility shims after the migration window

Phase boundaries are strict:

- Phase 2 may not ship without Phase 1
- Phase 3 may not ship without Phase 2
- Phase 5 may not ship as the only user-visible surface without Phase 4
- Phase 6 may not begin until shared-ref readers and MCP callers are confirmed v2-clean

---

## 14. Validation Requirements

The rewrite is not complete until all of the following pass.

### 14.1 Graph Rules

- nested plan creation
- task leaf enforcement
- cross-plan task-to-task dependency
- task-to-plan dependency
- plan-to-task dependency
- plan-to-plan dependency
- containment cycle rejection
- dependency cycle rejection
- mixed containment plus dependency cycle rejection

### 14.2 Status Derivation

- task `blocked` derivation
- task `broken_dependency` derivation
- plan `pending` derivation
- plan `active` derivation with actionable descendants
- plan `blocked` derivation from own dependencies
- plan `blocked` derivation when all remaining descendants are blocked
- plan `completed` derivation
- plan `failed` derivation
- plan `broken_dependency` propagation from abandoned dependency
- plan `broken_dependency` propagation from broken child

### 14.3 Executor Routing

- worktree executor sees only compatible tasks
- human sees only human tasks
- service sees only service tasks
- target label filtering works
- allowed principal pinning works
- incompatible claim/start/complete mutations are rejected

### 14.4 Migration

- v1 snapshot with only coordination tasks migrates losslessly
- v1 snapshot with task-backed native graph migrates losslessly
- abstract node to child-plan migration works
- multi-parent `child_of` legacy shape fails with repair-required error
- legacy non-executable edge kinds are preserved in migration artifact metadata

### 14.5 Shared Coordination

- cold hydration from shared refs into v2 graph
- local SQLite cache rebuild from shared refs
- authoritative mutations publish v2 payloads
- old readers reject newer schema with explicit upgrade message

### 14.6 Transactions

- multi-step graph decomposition succeeds or fails atomically
- multi-step reparenting succeeds or fails atomically
- convert-task-to-plan succeeds or fails atomically
- split-task-into-children succeeds or fails atomically
- final-graph validation runs against the staged post-transaction graph
- failed transactions leave authoritative shared coordination unchanged
- successful transactions emit exactly one authoritative publish
- `plan_bootstrap` and other graph macros execute through the same transaction engine

### 14.7 Canonical Routing

- canonical task and plan mutations are the only supported structural mutation surface
- human review migrates and executes as a human-routed task, not as a dedicated node kind
- service automation migrates and executes as a service-routed task, not as a dedicated node kind

---

## 15. Final Architectural Decision

PRISM execution planning should use one canonical graph model:

- `Plan` containers
- `Task` leaves
- explicit containment
- explicit dependencies
- derived plan status
- daemon-computed actionability
- identity-aware executor routing on tasks

The existing hybrid model of:

- coordination tasks
- native plan nodes
- authored edge kinds beyond executable dependencies

is transitional and should be removed.

This document is the implementation target.
