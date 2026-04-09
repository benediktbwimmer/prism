# PRISM Actions And Machine Work

Status: proposed design  
Audience: coordination, service, runtime, query, MCP, CLI, UI, and extension maintainers  
Scope: introduction of `Action` as a first-class coordination-graph leaf for bounded machine-executed work

---

## 1. Summary

PRISM should add `Action` as a new explicit executable leaf node type in the coordination graph.

The graph should evolve toward:

- `Plan`
- `Task`
- `Action`

Where:

- `Plan` is structural and non-claimable
- `Task` is human or agent executed work
- `Action` is machine-executed bounded work

Actions should execute on top of the shared execution substrate defined in
[2026-04-09-shared-execution-substrate.md](./2026-04-09-shared-execution-substrate.md).

## 2. Why `Action` should exist

PRISM already has a rich workflow DAG, but meaningful machine work is still at risk of being hidden
behind:

- event triggers
- implicit CI glue
- ad hoc side effects
- runner configuration detached from the graph itself

Making `Action` explicit allows PRISM to model machine work as visible workflow structure.

Examples:

- build artifact
- package artifact
- publish artifact
- deploy artifact
- rollout check
- rollback
- maintenance step

## 3. Core stance

### 3.1 Actions are explicit graph nodes

Actions should be explicit coordination-graph leaves rather than only implicit event-triggered side
effects.

### 3.2 Tasks should not be overloaded

Tasks should remain reserved for human or agent work.

PRISM should not blur the meaning of `Task` by using it for both:

- claimable human or agent work
- machine-executed workflow steps

### 3.3 Actions are bounded

Actions should represent bounded, inspectable machine work.

They should not be:

- arbitrary remote execution
- opaque script blobs
- hidden service side effects
- uncontrolled shell hooks

## 4. Coordination model

### 4.1 Plan

Plan remains:

- structural
- aggregating
- non-claimable
- graph-derived in state

### 4.2 Task

Task remains:

- claimable
- leased
- artifact and review aware
- the unit of human or agent work

### 4.3 Action

Action should be:

- an explicit graph leaf
- machine-executed
- typed
- policy-controlled
- runner-based
- provenance-rich
- retryable by policy
- capable of producing structured outputs or evidence

## 5. Execution model

Actions should use the shared execution substrate rather than inventing a special action-only
execution mechanism.

The expected split is:

- coordination graph owns Action semantics
- shared execution substrate owns machine execution mechanics

This means Actions share with validation and event jobs:

- capability classes
- runner kinds
- target selection
- execution records
- retries and timeout policy
- result envelopes
- provenance

## 6. Service and runtime posture

### 6.1 Service routes

The service should:

- evaluate Action readiness
- apply policy
- pick a target
- create durable execution state
- record result and provenance

### 6.2 Runtime usually executes

Most substantial Actions should be runtime-executed.

Examples:

- build
- package
- publish
- deploy
- rollout check
- rollback
- repo-local maintenance

Service-executed Actions should remain a small built-in minority.

## 7. Action lifecycle

Action should have its own lifecycle, distinct from Task.

A reasonable v1 lifecycle is:

- `Ready`
- `Queued`
- `Running`
- `Succeeded`
- `Failed`
- `TimedOut`
- `Cancelled`

An optional later `RetryScheduled` can be added if policy needs it.

## 8. Failure and retry

Actions should support:

- explicit retry policy
- timeout and budget
- failure policy
- compact structured result
- durable provenance

Later, failures may also trigger:

- investigation tasks
- fix tasks
- downstream blocking
- remediation hints

## 9. Relationship to validation

Validation should not be collapsed into `Action`.

Validation remains semantically special because it is tied to:

- task correctness
- completion gating
- artifact evidence

However, validation execution should share the same substrate used by Actions.

## 10. Relationship to event jobs

Event jobs remain useful for:

- notifications
- webhooks
- recurring triggers
- lightweight orchestration reactions

But important machine-executed workflow steps should often be explicit Actions rather than hidden
event side effects.

## 11. Recommendation

PRISM should add `Action` as a first-class coordination-graph leaf for bounded machine work.

The core semantic split should become:

- `Plan` = structural
- `Task` = human or agent work
- `Action` = machine work

Actions should execute on the shared substrate, remain policy-controlled, and make meaningful
machine workflow steps explicit in the graph instead of hiding them behind secondary automation.
