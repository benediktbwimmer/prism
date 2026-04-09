# PRISM Event Engine Architecture

Status: proposed orchestration architecture
Audience: PRISM core, coordination, runtime, bridge, MCP, and orchestration maintainers
Scope: event-trigger evaluation, recurring orchestration, event ownership, and the event engine’s role relative to the shared execution substrate

---

Normative event-engine semantics live in
[contracts/event-engine.md](../contracts/event-engine.md).

This document is now the higher-level orchestration companion. Execution mechanics are centered in:

- [2026-04-09-shared-execution-substrate.md](./2026-04-09-shared-execution-substrate.md)
- [2026-04-09-event-job-runners.md](./2026-04-09-event-job-runners.md)
- [2026-04-09-actions-and-machine-work.md](./2026-04-09-actions-and-machine-work.md)

## 1. Summary

PRISM still needs a temporal and event-driven orchestration layer above the coordination graph.

That layer should provide:

- recurring execution without unbounded graph growth
- trigger or hook evaluation
- decentralized event-ownership correctness
- creation and routing of event-driven executions

The important refinement is:

- the event engine is **not** the primary machine-execution architecture
- it is an orchestration role that creates and routes event-driven work onto the shared execution
  substrate

## 2. Design goals

Required goals:

- support push-based orchestration on top of the coordination graph
- keep graph growth bounded for recurring work
- make hook logic ergonomic and type-safe
- preserve correctness even when multiple services may exist
- avoid polluting hot coordination state with transient execution bookkeeping
- ensure trigger evaluation is based on verified coordination state
- make event execution ownership durable and attributable

Required non-goals:

- no second authority plane
- no event-engine-specific bespoke execution stack
- no assumption that all important machine work should be hidden behind event reactions

## 3. Event engine role

The event engine should own:

- trigger evaluation
- recurring schedules
- deterministic event identity
- event execution ownership
- creation of event jobs
- creation of follow-up Actions when policy says that graph-visible machine work should happen

The event engine should not own:

- a separate machine execution substrate
- hidden workflow semantics that bypass the coordination graph

## 4. Continue-as-new

Recurring or scheduled work cannot keep appending new tasks into one permanent live plan without
causing boundedness and retention problems.

Recurrence should therefore be modeled at the plan boundary.

The target model is:

- a reusable plan definition or template defines recurring structure
- each execution cycle is a separate plan instance
- when the instance completes, the event engine performs the coordination transition that archives
  the old instance and creates the new one

The event engine should think in terms of:

- recurrence policy
- plan definition
- plan instance lineage
- carried state where appropriate

not “reopen old tasks in place.”

## 5. Hook and trigger model

PRISM should support repository-local trigger logic such as:

- state-transition hooks
- periodic or cron hooks
- explicit orchestration actions

Hooks should run against the verified coordination view exposed by the service, not against random
local cache state.

Typical trigger examples:

- task became actionable
- claim expired
- recurring plan tick fired
- runtime became stale

## 6. Event identity

Event identity must be derived from authoritative transitions, not level predicates alone.

Bad:

- “task 123 is actionable”

Good:

- “task 123 entered actionable at authoritative coordination revision X”

The deterministic event id should be based on:

- trigger kind
- target identity
- authoritative revision or transition marker
- hook identity
- hook version or content digest when appropriate

## 7. Event execution ownership

PRISM should not model event deduplication as a permanent one-bit tombstone only.

Instead it should use authoritative event execution records with lifecycle states such as:

- `claimed`
- `running`
- `succeeded`
- `failed`
- `expired`
- `abandoned`

These records support:

- winner selection
- crash visibility
- retry rules
- operator inspection
- bounded cleanup and compaction

## 8. Relationship to event jobs and Actions

The event engine should usually create event jobs for:

- notifications
- webhooks
- lightweight recurring work
- secondary reactions

It may create or route explicit Actions when policy says that the resulting machine work should be
graph-visible and first-class.

This keeps the event engine focused on orchestration rather than making it the only machine-work
path.

## 9. Recommendation

PRISM should keep the event engine as a service-side orchestration role responsible for:

- deciding when event-driven work should happen
- claiming authoritative ownership of that work
- routing the resulting work onto the shared execution substrate

It should not grow into a second bespoke execution architecture or hide all meaningful machine work
behind event side effects.
