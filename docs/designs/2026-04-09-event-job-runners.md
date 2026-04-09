# PRISM Event Job Runners

Status: proposed design  
Audience: service, runtime, coordination, query, MCP, CLI, UI, and extension maintainers  
Scope: typed event-triggered job execution through structured JS/TS runners, including categories, lifecycle, boundaries, and relationship to validation runners

---

## Summary

PRISM should support typed event job runners as the execution mechanism for event-triggered
automation.

The key rules are:

- event-triggered work should go through typed job categories
- job behavior should be implemented through structured JS/TS runners
- PRISM core should own orchestration, lifecycle, policy, provenance, retries, and durable job
  state
- runners should own typed action logic, input interpretation, output parsing, and structured
  result production

This is not arbitrary command execution.

The intended shape is:

- one event model
- one service-level orchestration layer
- typed job categories
- bundled default runners in the binary
- repo-local ejected or customized runners under `.prism/` when needed

This keeps event-triggered automation:

- inspectable
- policyable
- testable
- bounded
- much less spooky than arbitrary shell hooks

## Goals

Required goals:

- provide a structured execution surface for event-triggered automation
- avoid arbitrary shell execution as the primary event automation model
- keep event behavior typed, queryable, and reviewable
- reuse the existing PRISM JS engine as the extension surface
- allow bundled default event job runners
- allow repo-local customization and extension through `.prism/`
- preserve clear boundaries between:
  - event trigger evaluation
  - job orchestration
  - local/runtime execution
  - external system calls

Required non-goals:

- this design does not create a second hidden workflow engine beside coordination
- this design does not turn runtimes into uncontrolled remote command hosts
- this design does not make job runners authoritative over coordination state
- this design does not replace the validation subprotocol, which remains a task-lifecycle concern

## Architectural stance

PRISM should use:

- one event model
- one authoritative service-side event orchestration layer
- typed job runners for event-triggered work

This is better than:

- arbitrary webhooks everywhere
- ad hoc shell hooks
- two competing event engines
- repo-specific scripting without structured envelopes

The service should own:

- trigger evaluation
- event or job creation
- retries, backoff, and expiry
- durable job records
- capability and policy checks
- result normalization
- provenance

Job runners should own:

- category-specific execution logic
- typed interpretation of args
- local or external action logic
- structured success or failure output

## Core boundary

### PRISM core owns orchestration

PRISM core or service should own:

- event subscriptions
- schedule or trigger evaluation
- durable job creation
- timeout and budget enforcement
- durable job lifecycle
- retries and backoff
- provenance recording
- result persistence
- policy enforcement
- capability checks
- query and UI surfaces over job state

### Job runners own typed behavior

Job runners should own:

- how to interpret structured args
- how to construct external payloads or local execution plans
- how to perform category-specific action logic
- how to parse typed responses
- how to produce a structured result envelope

### Job runners do not own lifecycle

PRISM should not support a mode where event runners own the entire execution lifecycle.

In particular, runners must not independently define:

- durable job state transitions
- retry policy
- timeout semantics
- provenance model
- authority mutations outside normal mutation paths

This mirrors the validation-runner philosophy: the host owns lifecycle, the runner owns typed
execution logic.

## Job categories

PRISM should define typed job categories rather than one universal generic runner.

Initial categories that make sense:

- `webhook_job`
- `notification_job`
- `runtime_action_job`

Possible future categories:

- `delivery_job`
- `rollout_check_job`
- `maintenance_job`
- `artifact_publish_job`

Every job record and runner definition should carry an explicit category.

This gives PRISM:

- policy handles
- UI clarity
- safer capability gating
- cleaner result normalization

## Job model

Every event-triggered job should be a first-class object.

A job record should include at least:

- `job_id`
- `repo_id`
- `event_id`
- `job_category`
- `runner_kind`
- `args`
- `target_scope`
- `requested_by`
- `created_at`
- `started_at`
- `completed_at`
- `failed_at`
- `timed_out_at`
- `status`
- `attempt_count`
- `timeout_ms`
- `budget`
- `result_ref`
- `provenance`

### Job states

At minimum:

- `proposed`
- `queued`
- `running`
- `succeeded`
- `failed`
- `timed_out`
- `cancelled`

An `expired` state may be added later if the event-engine lifecycle needs it, but the state model
must stay explicit.

## Target scopes

Event jobs should declare where they are supposed to execute.

At minimum:

- `service`
- `runtime:<runtime-id>`

An explicit runtime-selector family can come later.

### Service scope

Service-scoped jobs are executed by the PRISM Service itself.

Typical examples:

- webhook dispatch
- outbound notifications
- summary generation
- maintenance triggers
- archive or export initiation

### Runtime scope

Runtime-scoped jobs are executed by a runtime, but only through typed job categories and explicit
capability checks.

Typical examples:

- local maintenance actions
- repo-local script execution through a named runner
- local refresh of derived state
- future bounded worktree-local automation

Runtime-scoped jobs should remain opt-in and policy-controlled.

## Bundled and repo-local runners

### Bundled default runners

PRISM should ship a useful bundled set of event job runners in the binary.

Initial examples:

- generic webhook sender
- notification formatter or sender
- safe runtime-local maintenance job handlers

### Repo-local JS or TS runners

Repos should be able to provide or customize runners under `.prism/`, using the PRISM JS engine.

Illustrative layout:

```text
.prism/
  event-jobs/
    runners/
      webhook_task_lifecycle.ts
      runtime_refresh_index.ts
      notify_release_state.ts
```

This allows:

- repo-specific automation
- reviewable behavior
- versioned runners
- extension without touching Rust core

### Ejection model

Bundled runners should be ejectable into `.prism/` via explicit CLI commands, not implicitly
written on runtime boot.

## Inputs and outputs

### Structured inputs

Every runner should receive a structured input envelope that includes at least:

- job id
- job category
- runner kind
- args
- repo id
- principal and provenance context
- triggering event summary
- target scope
- timeout and budget metadata

### Structured outputs

Every runner should return a structured result envelope including at least:

- `status`
  - `succeeded`
  - `failed`
  - `timed_out`
  - `cancelled`
  - optional `inconclusive`
- `summary`
- `duration_ms`
- optional normalized detail fields relevant to the category
- optional `artifact_ref`
- optional `diagnostic_ref`

PRISM should store the compact, coordination-relevant result. Detailed logs or raw payloads should
remain runtime-local or artifact-backed where appropriate.

## Relationship to validation runners

Validation runners and event job runners should share the same design philosophy but remain distinct
subsystems.

Validation runners are used for:

- task-lifecycle validation
- capability-aware completion gating
- warm-state validation in the claiming runtime

Event job runners are used for:

- event-triggered automation
- notifications
- webhooks
- runtime-local maintenance actions
- future typed delivery actions

Validation should remain outside the generic event engine in the primary architecture. The event
engine may react to validation outcomes, but it should not own the primary completion-validation
loop.

## Capability and policy model

Event job execution must be policy-controlled.

At minimum, policy should be able to govern:

- which job categories are allowed
- which runner kinds are allowed
- which repos can use which runners
- which principals may create or enable specific event jobs
- which runtimes may accept runtime-scoped jobs
- whether bundled-only or repo-local runners are permitted

This is important because event jobs can have side effects.

## Safety rules

The following rules should be strict:

- no arbitrary shell as the default event mechanism
- no hidden coordination mutation
- runtime-scoped jobs must stay opt-in
- logs and detailed output remain out of coordination state

More concretely:

- event automation should not be modeled as run-this-shell-command by default
- if a runner wants to mutate coordination, it must go through the normal mutation protocol
- no runtime should silently become a generic event execution worker without explicit permission
- detailed logs, raw stdout or stderr, and large payloads should remain runtime-local or
  artifact-backed

## Query and UI expectations

The query layer and UI should surface:

- event subscriptions
- queued, running, succeeded, and failed jobs
- job category and runner kind
- target scope
- attempt count and timing
- compact summaries
- links to richer diagnostics or artifacts when available
- whether a job used a bundled or repo-local runner

This is essential for trust.

## Relationship to other PRISM docs

This design should eventually narrow into or update:

- `docs/contracts/event-engine.md`
- `docs/contracts/service-architecture.md`
- `docs/contracts/service-runtime-gateway.md`
- `docs/contracts/service-capability-and-authz.md`

It is intentionally a design doc first because the category model, runner boundary, and lifecycle
shape are still being settled.

## Recommended rollout

### Phase 1

Implement:

- service-owned event job lifecycle
- typed job categories
- structured job records
- bundled `webhook_job` and `notification_job` runners
- query and UI surfaces for job status

### Phase 2

Add:

- repo-local JS or TS event runners
- explicit eject commands
- `runtime_action_job`
- richer policy controls

### Phase 3

Later:

- delivery-oriented categories
- rollout checks
- more advanced runtime-targeted jobs
- deeper orchestration integrations

## Recommendation

PRISM should implement event-triggered automation through typed JS or TS event job runners, not
arbitrary commands.

The recommended architecture is:

- one event model
- one service-side orchestration layer
- typed job categories
- bundled default runners in the binary
- repo-local custom runners under `.prism/`
- PRISM core owns lifecycle and policy
- runners own typed action logic and structured outputs

This gives PRISM a powerful but disciplined automation surface that fits naturally with the rest of
the system’s design.
