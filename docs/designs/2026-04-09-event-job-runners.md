# PRISM Event Jobs

Status: proposed design  
Audience: service, runtime, coordination, query, MCP, CLI, UI, and extension maintainers  
Scope: event-triggered work as one semantic consumer of the shared execution substrate

---

## 1. Summary

PRISM should support event-triggered automation through typed event jobs.

Event jobs should:

- be created by event-trigger evaluation or recurring policy
- execute on the shared substrate defined in
  [2026-04-09-shared-execution-substrate.md](./2026-04-09-shared-execution-substrate.md)
- remain semantically distinct from Actions and validation

The key point is:

- event jobs are a semantic family
- not a separate execution stack

## 2. Why event jobs should exist

Event jobs are still useful for:

- notifications
- webhooks
- recurring maintenance triggers
- lightweight orchestration reactions
- service-driven follow-up work

They are especially appropriate when the work is:

- reactive
- secondary
- not itself an important graph-visible workflow node

## 3. Architectural stance

PRISM should use:

- one event model
- one event-orchestration layer
- one shared execution substrate beneath event jobs

This is better than:

- arbitrary shell hooks
- one-off service scripts
- event-specific bespoke execution logic

## 4. Event engine role

The event engine should be understood primarily as an orchestration role.

It should own:

- trigger evaluation
- recurring scheduling
- deterministic event identity
- durable event execution ownership
- creation and routing of event jobs

It should not be understood as a separate general-purpose machine-execution plane.

## 5. Event jobs on the shared substrate

Event jobs should share with Actions and validation:

- capability classes
- runner families
- runtime routing
- execution records
- retries and timeout policy
- structured result envelopes
- provenance

But they remain semantically distinct:

- Action = explicit graph node
- validation = task-correctness execution
- event job = orchestration-triggered execution

## 6. Job categories

Event jobs should define typed categories rather than one generic runner bucket.

Initial categories that make sense:

- `webhook_job`
- `notification_job`
- `runtime_action_job`

Possible future categories:

- `delivery_job`
- `rollout_check_job`
- `maintenance_job`
- `artifact_publish_job`

## 7. Job records and lifecycle

Every event-triggered job should be a first-class durable object.

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
- terminal timestamps
- `status`
- `attempt_count`
- timeout or budget
- compact result ref or inline result
- provenance

A reasonable v1 state model is:

- `proposed`
- `queued`
- `running`
- `succeeded`
- `failed`
- `timed_out`
- `cancelled`
- optional later `expired`

## 8. Target scopes

Event jobs should declare where they are supposed to execute.

At minimum:

- `service`
- `runtime:<runtime-id>`

Service-scoped jobs should stay limited to small built-in cases such as:

- webhook dispatch
- notifications
- tiny internal maintenance work

Runtime-scoped jobs should remain opt-in and policy-controlled.

## 9. Runner model

Event jobs should use typed JS or TS runners with:

- structured input
- structured output
- bundled defaults
- optional repo-local customization under `.prism/`

Runners own typed behavior. PRISM owns lifecycle, routing, retries, timeout policy, provenance, and
durable state.

## 10. Relationship to Actions

Important machine-executed workflow steps should often be modeled as explicit Actions rather than
as hidden event side effects.

Event jobs are most appropriate for:

- notifications
- webhooks
- recurring service reactions
- secondary orchestration work

They are less appropriate for major delivery-critical workflow steps that should appear directly in
the coordination graph.

## 11. Relationship to validation

Validation and event jobs now share one execution substrate, but remain distinct semantic families.

Validation remains tied to task correctness and completion gating.
Event jobs remain orchestration-triggered work.

## 12. Recommendation

PRISM should keep event jobs as a first-class orchestration concept while moving their execution
beneath the shared execution substrate.

That lets the event engine stay focused on:

- when work should happen
- why it should happen
- who should execute it

instead of becoming a second bespoke execution plane.
