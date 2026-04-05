# PRISM First-Class Plans Specification

Superseded for execution-planning rewrite by [PRISM_COORDINATION_GRAPH_REWRITE.md](./PRISM_COORDINATION_GRAPH_REWRITE.md).

Status: proposed target design  
Audience: PRISM core, coordination, MCP, memory, and projection maintainers  
Scope: full first-class plan implementation, not an MVP or transitional shim

---

## 1. Summary

PRISM should treat a **plan** as a first-class, repo-published, runtime-hydrated object.

A plan is not just a private agent checklist and not just a generic project-management artifact. In PRISM, a plan is a **grounded DAG of intended work** whose nodes bind to live repo entities, concepts, validations, risks, outcomes, and multi-agent coordination state.

This design makes plans part of the repo's **published active intent**.

That means a cold-start agent should be able to enter a repository and recover all three of these layers:

1. **Repo facts** — files, symbols, history, graph structure
2. **Repo knowledge** — memories, concepts, durable learned knowledge
3. **Repo intent** — what is currently being worked on, in what order, under what constraints

The runtime job of PRISM is to hydrate these layers into one queryable surface for agents.

The persistence job of `.prism` is to let the repository carry its own active plans and earned knowledge in a reviewable, branchable, commit-friendly form.

---

## 2. Why plans must become first-class

PRISM already models spatial and temporal perception well:

- where things are
- what belongs together
- what changed
- what happened before
- what is risky
- what a change touches

But real coding work also requires a third dimension:

- what should happen next
- what depends on what
- what can proceed in parallel
- what is blocked
- what must be validated before completion
- what another agent can safely pick up

Without first-class plans, agents repeatedly reconstruct execution structure from scratch.

A first-class plan system gives PRISM **execution perception**:

- a live DAG of work
- grounded in repo structure
- enriched by memory and history
- coordinated across agents
- persisted in the repo when it is part of shared active intent

This is not workflow theater. This is the minimal structure needed so that a repository can remember not only what it **is** and what it **knows**, but also what it is **currently trying to become**.

---

## 3. Core design principles

### 3.1 Plans are shared DAGs of work
A plan is fundamentally a directed acyclic graph. The DAG is the structural skeleton.

### 3.2 A plan is more than a DAG
The graph is overlaid with runtime state:

- status
- blockers
- claims
- handoffs
- validations
- risks
- outcomes
- provenance
- workspace revision assumptions

### 3.3 Plans are grounded, not prose-only
Every meaningful node must be able to bind to one or more of:

- structural anchors
- live handles
- concept handles
- artifacts
- validations
- outcomes

### 3.4 Plans are published active intent
Plans that represent shared current work belong in `.prism`, not only in runtime state.

### 3.5 Plans are branch-aware
Plan state is part of repo intent. Intent may differ by branch and should travel with branches when committed.

### 3.6 Runtime state and published state are distinct
Committed plan state should capture shared active intent. Ultra-ephemeral execution details should remain in the state DB.

Published plan state must also remain semantically self-contained. A cold clone with only `.prism`
state should be able to understand every published plan, node, edge, and provenance snapshot
without resolving runtime-only rows from a shared state database.

### 3.7 Plans are handle-native
The runtime surface should carry opaque handles wherever possible. Agents should not have to re-specify the same repo objects by text.

### 3.8 Plans must survive repo motion
When possible, plan bindings should reattach through anchors and lineage rather than becoming dead text after a rename or refactor.

### 3.9 Plans must stay inspectable
The persisted representation in `.prism` must be reviewable, diffable, mergeable, and understandable without PRISM internals.

Runtime-only ids may appear in published plan events as optional correlation handles, but they must
never be the only meaning-bearing reference in repo-published state.

---

## 4. Terminology

### Plan
A published DAG of intended work in the repository.

### Plan node
A typed unit of work or gate within a plan.

### Plan edge
A typed relationship between two plan nodes.

### Plan overlay
Derived or runtime-attached state that enriches a node or plan, such as blockers, claims, risks, outcomes, stale bindings, or validation state.

### Published plan
A plan stored in `.prism/plans/...` and intended to be visible to future agents and collaborators.

### Scratch plan
A local/session plan that has not yet been promoted into shared repo intent.

### Active intent
The currently shared execution structure for the repo or branch.

### Plan hydration
The process of loading committed plan events and snapshots into live coordination state, rebinding nodes and edges to the current graph and runtime.

---

## 5. The plan object model

## 5.1 Identity types

```rust
pub struct PlanId(pub SmolStr);
pub struct PlanNodeId(pub SmolStr);
pub struct PlanEdgeId(pub SmolStr);
pub struct PlanRevision(pub u64);
```

Rules:

- `PlanId` is stable for the life of the plan
- `PlanNodeId` is stable across node updates
- `PlanEdgeId` is stable across edge updates
- `PlanRevision` increments on every material graph mutation
- overlays do not necessarily increment graph revision unless they change published semantics
- when migrating existing coordination data, `PlanNodeId` should preserve the existing `CoordinationTaskId` byte-for-byte whenever possible
- the system should not require a second parallel node identity scheme for migrated tasks unless a collision or format repair makes it unavoidable

## 5.2 Plan status

```rust
pub enum PlanStatus {
    Draft,
    Active,
    Blocked,
    Completed,
    Abandoned,
    Archived,
}
```

`Archived` means the plan is complete or no longer active but still intentionally retained in published repo history.

## 5.3 Plan publication scope

```rust
pub enum PlanScope {
    Local,
    Session,
    Repo,
}
```

Rules:

- `Local` and `Session` plans may exist only in runtime state
- `Repo` plans are publishable and hydratable from `.prism`
- `Repo` is the default for shared active work
- promotion from `Session` to `Repo` must be explicit or policy-driven

## 5.4 Plan kind

```rust
pub enum PlanKind {
    TaskExecution,
    Investigation,
    Refactor,
    Migration,
    Release,
    IncidentResponse,
    Maintenance,
    Custom,
}
```

This is descriptive, not behavioral. Runtime behavior should depend on node kinds, edges, policies, and overlays.

## 5.5 Plan node kinds

```rust
pub enum PlanNodeKind {
    Investigate,
    Decide,
    Edit,
    Validate,
    Review,
    Handoff,
    Merge,
    Release,
    Note,
}
```

Guidance:

- `Investigate` identifies discovery or diagnosis work
- `Decide` captures branch points or design choices
- `Edit` is a code/config/docs change task
- `Validate` is a gate or check node
- `Review` is human or agent review
- `Handoff` is a controlled ownership transfer node
- `Merge` and `Release` are explicit late-stage gates when needed
- `Note` is for published plan-local context that should be attached to the graph but is not itself executable work

## 5.6 Plan node status

```rust
pub enum PlanNodeStatus {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    Waiting,
    InReview,
    Validating,
    Completed,
    Abandoned,
}
```

`Waiting` is distinct from `Blocked` and means the node is intentionally paused pending another process, review, deployment window, or external signal.

## 5.7 Plan edge kinds

```rust
pub enum PlanEdgeKind {
    DependsOn,
    Blocks,
    Informs,
    Validates,
    HandoffTo,
    ChildOf,
    RelatedTo,
}
```

Semantics:

- `DependsOn`: destination must complete before source can become ready
- `Blocks`: authored only when the blocking relationship is itself durable shared intent; most blockers should remain runtime overlays rather than authored graph edges
- `Informs`: destination should consult source output before completion but is not structurally blocked
- `Validates`: destination is a validation gate for source
- `HandoffTo`: explicit ownership-transfer path
- `ChildOf`: hierarchical grouping edge for nested plan structure without being an execution dependency
- `RelatedTo`: semantic association only

DAG rule:

- `DependsOn`, `Blocks`, `Validates`, `HandoffTo`, and `ChildOf` must remain acyclic within a single plan
- `Informs` and `RelatedTo` may be excluded from cycle enforcement if treated as non-scheduling edges

Authoring guidance:

- prefer `DependsOn` for execution ordering
- prefer derived blocker overlays for transient claim, review, validation, freshness, or policy failures
- use authored `Blocks` edges only when the blocking relationship is part of the intended shared workflow and should survive replay and handoff

## 5.8 Binding model

Each plan node may bind to repo-published semantic surfaces and may receive runtime-attached conveniences during hydration.

```rust
pub struct PlanBinding {
    pub anchors: Vec<AnchorRef>,
    pub concept_handles: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub memory_refs: Vec<String>,
    pub outcome_refs: Vec<String>,
}

pub struct HydratedPlanBindingOverlay {
    pub handles: Vec<String>,
}
```

Rules:

- anchors are the durable structural substrate
- concept handles are first-class semantic bindings
- committed plan state should persist anchors, concept handles, and stable published refs only
- session-local handles are hydration conveniences, not authored binding material, and must be refreshable on hydration
- runtime transport handles such as `handle:*` must be rejected from authored plan bindings rather than normalized into repo truth
- `artifact_refs`, `memory_refs`, and `outcome_refs` are valid in repo-published plans only when they resolve to durable published or branch-stable references
- implementations should reject authored `concept_handles`, `artifact_refs`, and `outcome_refs` that do not resolve in the currently loaded published/runtime knowledge stores instead of preserving unresolved published identifiers as if they were valid authored intent
- if an artifact, memory, or outcome reference is only meaningful inside one runtime session, it belongs in a runtime overlay rather than the authored plan object

## 5.9 Acceptance model

```rust
pub struct ValidationRef {
    pub id: String,
}

pub struct PlanAcceptanceCriterion {
    pub label: String,
    pub anchors: Vec<AnchorRef>,
    pub required_checks: Vec<ValidationRef>,
    pub evidence_policy: AcceptanceEvidencePolicy,
}

pub enum AcceptanceEvidencePolicy {
    Any,
    All,
    ReviewOnly,
    ValidationOnly,
    ReviewAndValidation,
}
```

Acceptance must be structured enough to survive handoffs and allow runtime gating.

Acceptance and policy precedence:

- plan policy defines the baseline completion gates for all executable nodes in the plan
- node acceptance criteria may strengthen those gates for a specific node but should not silently weaken plan-wide policy
- artifact review state, recorded validations, and acceptance evidence must compose into one replayable completion decision
- completion queries should explain whether a node is blocked by plan policy, node acceptance, artifact state, or derived risk/review thresholds
- `ValidationRef` should be a stable validation identity, recipe ref, or check id, not an ad hoc display string

## 5.10 Plan node

```rust
pub struct PlanNode {
    pub id: PlanNodeId,
    pub plan_id: PlanId,
    pub kind: PlanNodeKind,
    pub title: String,
    pub summary: Option<String>,
    pub status: PlanNodeStatus,
    pub bindings: PlanBinding,
    pub acceptance: Vec<PlanAcceptanceCriterion>,
    pub is_abstract: bool,
    pub assignee: Option<AgentId>,
    pub base_revision: WorkspaceRevision,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
}
```

Node authorship rules:

- `assignee` is authored shared intent when collaborators should see the intended owner
- pending handoff state is execution state derived from handoff events and overlays, not canonical node structure
- `session` is runtime-only transport state and should never be part of the authored node object
- `is_abstract` must be explicit so intentionally unbound structure does not disappear into free-form metadata

## 5.11 Plan edge

```rust
pub struct PlanEdge {
    pub id: PlanEdgeId,
    pub plan_id: PlanId,
    pub from: PlanNodeId,
    pub to: PlanNodeId,
    pub kind: PlanEdgeKind,
    pub summary: Option<String>,
    pub metadata: serde_json::Value,
}
```

## 5.12 Plan root object

```rust
pub struct Plan {
    pub id: PlanId,
    pub scope: PlanScope,
    pub kind: PlanKind,
    pub title: String,
    pub goal: String,
    pub status: PlanStatus,
    pub policy: CoordinationPolicy,
    pub revision: PlanRevision,
    pub root_nodes: Vec<PlanNodeId>,
    pub tags: Vec<String>,
    pub created_from: Option<String>,
    pub metadata: serde_json::Value,
}
```

---

## 6. Derived overlays and runtime views

These are not the authored graph itself. They are computed, hydrated, or runtime-attached views over the graph.

## 6.1 Plan blockers

```rust
pub struct PlanNodeBlocker {
    pub kind: BlockerKind,
    pub summary: String,
    pub related_node_id: Option<PlanNodeId>,
    pub related_artifact_id: Option<ArtifactId>,
    pub risk_score: Option<f32>,
    pub validation_checks: Vec<String>,
}
```

## 6.2 Execution overlay
A node may surface execution-local state such as:

- effective assignee after handoff resolution
- pending handoff target
- runtime session currently executing the node
- execution lease / heartbeat state when available

These are overlays even when they materially affect readiness. They are not automatically part of the authored node object.

## 6.3 Claims and contention
Plan nodes do not directly own claims, but claims should be resolvable from node bindings.

A node should surface:

- active claims on its anchors
- conflict severity
- contending sessions/agents
- safe-to-proceed signal

## 6.4 Risk overlay
A node may surface:

- compact blast radius summary
- likely tests
- recent relevant failures
- review requirement threshold

## 6.5 Outcome overlay
A node may surface:

- last successful execution outcome
- related failures
- recent patches
- previous attempted resolutions

## 6.6 Freshness overlay
A node or entire plan may surface:

- graph drift since `base_revision`
- moved bindings reattached by lineage
- unresolved bindings requiring repair
- stale risk requiring re-validation

## 6.7 Critical-path overlay
The runtime should be able to derive:

- ready nodes
- blocked nodes
- current critical path
- parallelizable nodes
- validation frontier

For PRISM, the current critical path means the longest remaining blocking chain under the current dependency graph and blocker state, not an implicit duration-weighted project-management schedule unless explicit weights are later introduced.

These are runtime projections and should not be committed unless explicitly published as annotations.

---

## 7. Persistence model in `.prism`

## 7.1 Directory layout

```text
.prism/
  state/
    plans/
      <plan-id>.json
    coordination/
      tasks/<task-id>.json
      artifacts/<artifact-id>.json
    indexes/
      plans.json
      coordination_tasks.json
    manifest.json
```

Optional non-committed local materializations may live elsewhere, but the committed source of truth
for published plans is the shard-oriented tracked snapshot plus the signed publish manifest.

## 7.2 Why shard-oriented snapshots
Plans are living objects. They need:

- stable narrow conflict surfaces
- deterministic publication
- inspectable current state
- branch-aware divergence
- cheap hydration into runtime state

The published layout should prefer stable per-object snapshot shards over monolithic domain files so
that:

- active plans merge independently
- branch divergence is naturally localized to the plans or coordination objects that changed
- hydration can load current plan state directly without replaying repo history
- Git carries the durable history and branch semantics for the tracked snapshot set

## 7.3 Snapshot publication policy
`.prism/state/plans/<plan-id>.json`, `.prism/state/coordination/**/*.json`, the derived indexes,
and `.prism/state/manifest.json` are the committed source of truth for published plan intent.

This plan-specific persistence policy follows the repo-wide classification in [`docs/PERSISTENCE_STATE_CLASSIFICATION.md`](PERSISTENCE_STATE_CLASSIFICATION.md): authored plan intent and durable workflow continuity are authoritative, while hydrated graph materializations, compatibility task projections, summaries, recommendations, and snapshots remain derived.

The indexes should remain intentionally small and hold only compact plan-level metadata needed for
discovery and navigation.

The runtime state DB may keep:

- materialized plan indices
- per-session leases
- heartbeats
- derived blockers
- derived readiness
- ephemeral draft edits not yet published

But these are not the published plan truth.

Git history is the canonical history substrate for published plans. The tracked snapshot is the
current repo-published authority. The index is a discovery and routing layer, not a second semantic
source of truth.

## 7.4 Publish manifest and continuity

Each publish boundary should update one stable `.prism/state/manifest.json` that:

- digests the exact authoritative snapshot shards
- records publisher identity and work context
- chains to the previous manifest digest
- may record a `migrationSourceDigest` on the first snapshot-format publish

The tracked snapshot should not accumulate one manifest file per publish in HEAD. Git already
preserves prior manifest versions.

## 7.5 Publication rules

- `Repo`-scoped plans must be exportable to deterministic snapshot shards plus a signed manifest
- plan publication should be deterministic
- Git checkout across commits must reconstruct the correct historical authored graph and status state
- derived overlays must not be persisted unless explicitly promoted as authored plan annotations

## 7.6 Archived plans
Completed or abandoned plans may remain in the current snapshot set with `status: Archived`, but
they should not require separate append-log movement semantics in tracked `.prism`.

## 7.7 Runtime journals and compaction

- fine-grained plan/task mutation history belongs in runtime/shared journals, not in tracked repo state
- deterministic runtime compaction is allowed for those journals
- tracked repo publication should summarize current authored plan state, not preserve every intermediate mutation forever

---

## 8. Hydration model

## 8.1 Startup
On startup PRISM must:

1. verify `.prism/state/manifest.json`
2. load current tracked plan and coordination snapshot shards
3. load any supporting tracked snapshot indexes needed for efficient plan lookups
4. rebuild authored plan graph objects
5. bind node anchors to the current graph
6. refresh concept bindings and attach fresh runtime handles
7. derive blockers, claims, risk summaries, and readiness projections
8. attach execution-local overlays such as pending handoffs and runtime sessions
9. mark stale or unresolved bindings where rebinding fails

## 8.2 Rebinding behavior
Bindings should reattach in this order:

1. exact authored anchor
2. lineage re-anchoring
3. concept-centered recovery through concept core members
4. explicit unresolved marker

A hydrated plan must never silently pretend a binding succeeded when it did not.

## 8.3 Staleness semantics
A plan or node becomes stale when:

- `base_revision.graph_version` is behind current graph revision and authored bindings drift materially
- a required anchor cannot be rebound
- a validation gate references checks no longer recognized
- an edge references a removed node

Staleness should not destroy the plan. It should surface as a first-class overlay requiring repair.

## 8.4 Branch semantics
Published plans are branch-aware because they live in the repo tree.

Rules:

- plans may diverge across branches naturally
- merge conflicts in plan shards and indexes are expected and must be manageable
- Git remains the durable history layer for published plan state
- the persisted layout should minimize unrelated conflicts, which is why plan and coordination objects should remain narrowly sharded

## 8.5 Local scratch plans
Session or local plans may remain in the DB until promoted.

Promotion to `Repo` scope should:

- allocate stable repo-owned IDs if needed
- materialize deterministic plan/task shards for the first repo publication
- optionally preserve provenance to the scratch origin in the publish manifest or runtime journal

---

## 9. Relationship to existing coordination state

The current coordination model already includes:

- plans
- tasks
- claims
- artifacts
- reviews
- handoffs
- policy checks

The full first-class plan implementation should **subsume and generalize** this into a richer graph model rather than creating a separate competing subsystem.

### Mapping

- existing `Plan` maps to first-class `Plan`
- existing `CoordinationTask` becomes a specialized `PlanNode`
- existing `depends_on` becomes `PlanEdgeKind::DependsOn`
- existing artifacts, reviews, and claims become overlays or linked objects attached to nodes
- existing policy and blocker machinery remains valid and should become plan-node aware

Compatibility rules:

- migration should preserve existing `PlanId` values
- migration should preserve existing `CoordinationTaskId` values as `PlanNodeId` values whenever possible
- existing coordination reads should remain valid even if they are implemented as views over the richer plan graph
- existing coordination mutations may continue to exist as compatibility shims, but they should write the richer plan model rather than a separate legacy store
- the implementation should avoid a long-lived dual model where tasks and plan nodes can drift apart semantically

### Concrete ownership contract

For `PlanKind::TaskExecution`, a node id that is also a `CoordinationTaskId` is task-backed.

Task-backed ids follow these rules:

- the tracked plan snapshot in `.prism/state/**` is the authoritative owner of authored node fields such as `kind`, `title`, `summary`, `status`, `bindings`, `acceptance`, `validation_refs`, `is_abstract`, `priority`, `tags`, and `base_revision`
- coordination runtime remains authoritative for live workflow continuity that is not published plan authorship, such as leases, claims, handoffs, reviews, artifacts, and execution overlays
- hydrated plan graphs and coordination task views may project authored node fields from the tracked snapshot into hot memory, but those projections are serving state, not a second mutable authority
- compatibility mutations that target a task-backed id must still route through coordination-task update semantics, but those semantics must publish authored field changes back into the tracked snapshot instead of inventing a separate authoritative store for the same fields
- external edits to published plan files must be observed and rehydrated so hot memory and coordination-facing task views converge back to repo-published truth
- blocker, validation, risk, and task-brief surfaces should resolve authored task-backed fields from the hydrated published plan authority, while continuing to layer runtime continuity state from coordination overlays

Standalone native plan nodes remain first-class authored state when they are not backed by a coordination task.

This is the intended split:

- `TaskExecution` plans use tracked snapshot state as the source of truth for task-backed authored node fields, while coordination owns live continuity overlays and uses compatibility mutations to write authored changes back into the tracked snapshot
- non-`TaskExecution` plans may continue to use standalone native plan nodes as the authored node authority

The new system is not a replacement of coordination. It is coordination elevated into a first-class graph substrate.

---

## 10. Integration with concepts, memories, outcomes, impact, and validation

## 10.1 Concepts
A plan node may bind directly to one or more concept handles.

This allows plans to refer not only to files or symbols but to repo-native semantic units such as:

- session persistence flow
- validation pipeline
- compact tool surface
- runtime status path

On hydration, concept bindings should refresh through concept hydration rather than becoming dead strings.

## 10.2 Memories
Plans should be able to attach:

- related structural memories
- related episodic memories
- warnings and footguns
- prior successful patterns

Memory is not the plan, but it should enrich node interpretation and handoffs.

## 10.3 Outcomes
Plans should consume and emit outcomes.

Typical uses:

- resuming a plan node after prior failed attempts
- attaching successful validations to completion
- using recent outcomes to derive blockers or next actions

## 10.4 Blast radius and risk
Plans should integrate directly with impact queries.

A node should be able to surface:

- likely coupled handles
- likely tests
- risk score
- review thresholds
- change neighbors

## 10.5 Validation recipes
Validation recipes should be attachable as:

- authored acceptance criteria
- derived recommendations
- node overlays
- completion gates

---

## 11. MCP and JS runtime surface

The full plan system should expose a compact default read path and a richer query/runtime path.

## 11.1 Resource and query discovery surface

Plans should not add a separate family of dedicated MCP tools. The compact browse and discovery path is:

- `prism://plans`
- `prism.plans({ status?, scope?, contains?, limit? })`
- `prism.plan(planId)`
- `prism.planSummary(planId)`
- `prism.planNext(planId, limit?)`

Design guidance:

- prefer a browseable plans resource plus the existing JS/query surface over adding more top-level MCP tools
- discovery is `list/filter -> inspect by id`, not fuzzy single-item lookup
- `prism.plan(planId)` is an exact-id read, and broader lookup should start from `prism://plans` or `prism.plans(...)`
- plan discovery payloads should expose plan-native identifiers such as `root_node_ids`, not compatibility task ids
- related resources may point back to the filtered plans resource, schemas, and session context; they should not imply that plans are fundamentally task-addressed

`prism://plans` should return compact list entries with:

- `plan_id`
- `title`
- `goal`
- `status`
- `scope`
- `kind`
- `root_node_ids`
- `summary`
  - compact human-readable discovery summary text
- `plan_summary`
  - richer numeric counters for actionable, blocked, gated, stale, and completed state when a UI
    or runtime consumer needs more than the compact text

`prism.plan(planId)` should return exact plan metadata with at least:

- `id`
- `title`
- `goal`
- `status`
- `scope`
- `kind`
- `revision`
- `tags`
- `created_from`
- `root_node_ids`

## 11.2 Runtime query surface
The JS/query runtime should expose richer programmatic access:

```ts
prism.plans(options?)
prism.plan(planId)
prism.planGraph(planId)
prism.planExecution(planId)
prism.planReadyNodes(planId)
prism.planNodeBlockers(planId, nodeId)
prism.planSummary(planId)
prism.planNext(planId, limit?)
```

## 11.3 Mutation surface
Mutations remain explicit through MCP mutation tools.

Add actions for:

- `plan_create`
- `plan_update`
- `plan_archive`
- `plan_node_add`
- `update`
- `plan_node_remove`
- `plan_node_status`
- `plan_edge_add`
- `plan_edge_remove`
- `plan_acceptance_update`
- `plan_publish`
- `plan_promote_scope`

These mutations should append per-plan events, maintain the plan index, validate DAG invariants, and update runtime materializations.

---

## 12. Agent ergonomics

Plans should become a natural shared object for agents.

Desired workflows:

### 12.1 Cold-start resume
Agent enters repo, asks for active plan, sees:

- what is in progress
- what is blocked
- what is ready
- what validations matter
- what concept or handle each node touches

### 12.2 Safe parallelism
Different agents can pick ready nodes that are structurally independent and low-conflict.

### 12.3 Grounded handoff
A handoff is not prose only. It can target a plan node with:

- current status
- attached memories
- outcomes
- blockers
- acceptance state

### 12.4 Better self-review
Before completion an agent can ask whether the node:

- still has blockers
- lacks required validations
- exceeds review threshold
- became stale under graph change

---

## 13. What should and should not be committed

## 13.1 Commit to `.prism`
Commit plan data that represents shared repo intent:

- plan creation and updates
- plan nodes and edges
- status changes meaningful to collaborators and future agents
- acceptance criteria
- authored assignee changes when they are part of shared intent
- durable blockers or plan annotations worth preserving
- handoff requests and acceptances
- review and validation events when they materially change shared understanding of plan state

Publication rubric for status-like events:

- commit status transitions that change what another agent should do next, such as `Proposed -> Ready`, `InProgress -> Blocked`, `InReview -> Completed`, or explicit reopen events
- commit status transitions that change branch-visible ownership or responsibility, such as handoff request, handoff acceptance, archive, abandon, or reopen
- keep local retries, temporary decomposition experiments, and short-lived personal reordering in runtime state until they become shared intent
- if a decomposition of a published node changes the execution structure another agent should see, promote it into authored plan nodes and edges; otherwise keep it local

## 13.2 Keep in runtime state only
Do not commit ultra-ephemeral execution details such as:

- heartbeats
- momentary cursor/caret-style activity
- transient lease renewals
- runtime session ids
- hydrated opaque handles
- speculative scratch nodes not yet part of shared intent
- temporary local decompositions of a published node that no one else needs

Principle:

**Commit shared intent, not momentary twitch.**

---

## 14. Invariants

A valid first-class plan implementation must enforce the following.

### Graph invariants
- plan scheduling edges remain acyclic
- every edge references existing nodes in the same plan
- every root node belongs to the plan
- orphan nodes are allowed only if explicitly marked detached or draft

### Status invariants
- completed nodes cannot move back to proposed without explicit reopen event
- completed plans require all required completion gates satisfied
- abandoned nodes cannot block readiness calculations

### Binding invariants
- every executable node must have at least one structural or conceptual binding, unless `is_abstract` is explicitly true
- missing bindings must be surfaced, not hidden
- runtime-attached handles must not be treated as committed authored bindings

### Validation invariants
- completion must honor plan policy for review and validation
- node acceptance criteria must be normalized and replayable
- repo-published required checks should resolve through stable validation identifiers or recipe refs

### Publication invariants
- replay of committed events must be deterministic
- plan IDs, node IDs, and edge IDs must remain stable
- published plan scope must be explicit
- the plan index and per-plan logs must agree on active versus archived location

### Multi-agent invariants
- claim conflicts must remain queryable per node
- handoffs must target valid agents or sessions when required by policy
- stale revision checks must remain plan-node aware

---

## 15. Suggested file-level schema layout

The persisted event payloads should be intentionally boring.

Example node-added event payload:

```json
{
  "node": {
    "id": "node:session-refresh-edit",
    "kind": "Edit",
    "title": "Patch session refresh logic",
    "summary": "Unify refresh path and stale-token fallback",
    "status": "Ready",
    "bindings": {
      "anchors": [
        { "kind": "symbol", "id": "sym:session_refresh" }
      ],
      "concept_handles": ["concept://session_persistence_flow"],
      "handles": [],
      "artifact_ids": [],
      "memory_refs": [],
      "outcome_refs": []
    },
    "acceptance": [
      {
        "label": "refresh path passes integration checks",
        "anchors": [],
        "required_checks": ["integration/session_refresh"],
        "evidence_policy": "ReviewAndValidation"
      }
    ],
    "assignee": null,
    "pending_handoff_to": null,
    "session": null,
    "base_revision": {
      "graph_version": 412,
      "git_commit": "abc123"
    },
    "priority": 2,
    "tags": ["session", "runtime"],
    "metadata": {}
  }
}
```

---

## 16. Query and derived behavior requirements

A full implementation must support at least the following derived behaviors.

### 16.1 Ready node calculation
A node is ready when:

- it is not terminal
- all blocking dependency edges are satisfied
- it is not blocked by policy, stale revision, claim conflict, missing review, or missing validation
- its plan is not terminal

### 16.2 Critical path
The runtime should derive the current critical path over executable nodes using dependency structure and blocker state.

### 16.3 Parallelism hints
The runtime should surface nodes that can safely proceed in parallel based on:

- no dependency conflict
- low claim overlap
- low blast-radius conflict
- plan policy editor limits

### 16.4 Node-local next actions
For a given node, the runtime should be able to suggest:

- read this concept or handle first
- inspect this blocker
- run these validations
- request this handoff
- review this artifact

### 16.5 Plan health
A plan brief should surface:

- number of ready nodes
- number of blocked nodes
- stale bindings count
- unresolved handoffs count
- review debt
- validation debt

---

## 17. Testing requirements

A full plan implementation should have coverage for:

### Contract tests
- event replay reconstructs the same graph
- committed and hydrated plans preserve identity and status
- node and edge serialization are deterministic

### DAG tests
- cycle creation is rejected
- edge kind semantics are enforced
- ready-node calculation matches graph reality

### Hydration tests
- anchor rebinding after rename works through lineage
- concept-bound nodes remain usable after concept hydration
- unresolved bindings are surfaced explicitly

### Coordination tests
- claim conflicts attach to the correct plan nodes
- handoffs preserve node state and provenance
- review and validation gates block completion correctly

### Branching tests
- plan index and per-plan logs merge without corruption
- duplicate events are de-duplicated by event ID
- archived plans remain hydratable

### Agent-ergonomics tests
- cold-start active plan retrieval is compact
- node brief surfaces enough information for a next action without forcing full graph dump
- plan-next results prefer truly executable nodes

---

## 18. Migration from current coordination model

The implementation should migrate current shared plan/task coordination state into this richer graph model without breaking existing concepts.

Migration rules:

- every existing `CoordinationTask` becomes a `PlanNode`
- current `depends_on` relations become `PlanEdgeKind::DependsOn`
- current plan root tasks become `root_nodes`
- current policy, claims, artifacts, reviews, and handoffs remain valid overlays
- existing MCP coordination queries should continue to work, internally reading from the richer model where possible
- existing `CoordinationTaskId` values should become `PlanNodeId` values directly unless an explicit repair step is required
- compatibility layers should be intentionally thin and temporary; the richer plan graph should become the single semantic source of truth as quickly as feasible

This is an additive semantic upgrade, not a forked replacement.

---

## 19. Non-goals

The full plan implementation is **not**:

- a generic enterprise project-management tool
- a substitute for GitHub issues or PR review systems
- a requirement that every tiny task in a repo be plan-authored
- an excuse to persist transient noise into `.prism`
- a prose-only planning notebook

The goal is narrower and more powerful:

**give repositories a grounded, published, hydratable representation of current work intent that agents can execute against safely and coherently.**

---

## 20. Final philosophy

A repository should be able to remember:

- what it is
- what it has learned
- what it is currently trying to do

First-class plans are the third pillar.

If memories and concepts give PRISM durable self-knowledge, plans give it durable shared intent.

That is what turns PRISM from a smart repo runtime into a system that can provide cold-start situational awareness, execution continuity, and multi-agent coherence at the same time.
