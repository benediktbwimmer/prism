# Warm-State Validation Feedback

Status: proposed design  
Audience: coordination, runtime, query, service, MCP, CLI, UI, and policy maintainers  
Scope: warm-state validation as a first-class coordination mechanism, including task lifecycle, runtime capability posture, validation runners, seeded capability classes, and relationship to cold CI

---

## 1. Summary

PRISM should support warm-state validation feedback as a native coordination feature.

The core model is:

- tasks and required artifacts may declare validation policy up front
- the runtime that claims the task is the preferred validation execution site
- validation runs against an already-warm worktree and toolchain state
- task completion can be gated by structured validation results when policy requires it
- validation uses named validation runners plus structured arguments, not ad hoc shell conventions
- validation capability classes are declared in repo or service policy and may be seeded from bundled defaults
- runtime capability posture is established through explicit proof commands on capability classes
- validation produces durable provenance tied to the task, runtime, capability class, runner kind, and revision

This is not a generic remote-command feature.

The intended v1 posture is:

- runtime-local validation execution
- coordination-aware validation lifecycle
- warm feedback during active development
- cold CI retained as the selective final integration gate

This design is complemented by [2026-04-09-repo-init-and-validation-ejection.md](./2026-04-09-repo-init-and-validation-ejection.md), which defines how validation runners and capability classes become explicit repo-local configuration.

## 2. Goals

Required goals:

- make validation a first-class coordination concern
- let tasks or artifacts require explicit validation before completion
- run validation in the claiming runtime when possible so build and toolchain state stay warm
- preserve explicit provenance for validation results
- keep capability requirements deterministic and queryable
- keep validation capability vocabulary configurable at runtime rather than compiled into PRISM
- support bundled default validation runners and seeded capability classes
- allow repo-local customization of runners and capability classes
- let query surfaces answer which work is validated, pending validation, or failed validation
- let policy distinguish warm validation from final cold CI integration gates

Required non-goals:

- no generic arbitrary remote shell execution feature in v1
- no attempt to replace cold CI for every trust boundary
- no requirement that the event engine dispatch generic commands to runtimes in v1
- no treatment of validation success as a free-form comment or soft signal

## 3. Core Model

### 3.1 Validation policy is declared up front

Tasks or required artifacts may declare one or more validation policies before work begins.

A validation policy should define at least:

- validation capability class
- validation runner kind
- structured runner args
- timeout or budget
- whether validation is required for completion
- whether failure returns the task to an actionable repair state
- whether bounded recovery is allowed
- whether `passed_with_recovery` is acceptable

PRISM should not let an agent invent the validation rule only at completion time.

### 3.2 Capability classes are policy-defined vocabulary

Validation capability classes should be declared in repo or service policy rather than compiled into PRISM.

Examples:

- `cargo`
- `cargo:test`
- `cargo:workspace-test`
- `npm`
- `npm:test`
- `pytest`
- `playwright`
- `integration:db`

These are explicit, queryable strings that tasks and artifacts can reference directly.

If a mutation attempts to reference an unknown capability class, PRISM should fail closed and require the user or agent to define the class first.

### 3.3 Capability classes define proof policy

A capability class is a small policy object, not just a label.

At minimum it should define:

- `name`
- optional `description`
- `proof_commands`
- `proof_success_policy`
- optional `proof_ttl`
- optional intended validation runner kinds

Example:

```yaml
capability: cargo:test
description: Runtime can execute Cargo-based test validations
proof_commands:
  - cargo -V
  - rustc -V
proof_success_policy: all
proof_ttl: 7d
intended_runner_kinds:
  - cargo_test
```

This gives runtimes a deterministic way to establish capability posture before they attempt required validation.

### 3.4 Runtime capability posture is distinct from capability vocabulary

PRISM should separate:

1. capability class definition
2. runtime capability posture for that class

A runtime posture should distinguish at least:

- `declared`
- `provisional`
- `verified`

An optional later `stale` posture can capture expired proof or obvious environment drift.

### 3.5 Warm validation is owned by the claiming runtime

The default execution model is:

- the runtime that claims the task is also the runtime that runs required validation

Reasons:

- it already has the relevant worktree
- it already has warm cache and build state
- it already knows the local execution context
- the same agent is best positioned to repair failures immediately

This is a better v1 model than making the service or event engine act as a generic remote-command dispatcher.

## 4. Capability Posture Lifecycle

### 4.1 Declared

A runtime may advertise that it expects to support capability class `X`.

This is useful for scheduling, filtering, and ranking, but it is not yet strong proof.

### 4.2 Provisional

A runtime may claim a task that requires a known capability class it has not yet verified if policy allows provisional claim.

In that case:

- the task is claimable only under a short provisional lease
- the runtime must establish proof quickly
- successful proof upgrades posture and extends the lease normally
- failed proof releases the lease and returns the task to an actionable state

### 4.3 Verified

A runtime becomes verified for a capability class when it produces acceptable proof according to the class definition.

Proof should be:

- explicit
- recorded
- attributable
- queryable

Capability proof demonstrates class availability, not completion of the task-specific validation itself.

### 4.4 Optional stale posture

A later version may mark posture stale when:

- proof TTL expires
- environment or toolchain changes
- repeated failures suggest drift

This is optional for v1.

## 5. Validation Runners

### 5.1 Named runner kinds with structured args

Validation should use named runners with structured arguments instead of raw command strings as the semantic layer.

Examples:

- `cargo_test`
- `pytest`
- `vitest`
- `playwright`

Tasks or artifacts then declare something like:

- runner kind: `cargo_test`
- args: `{ scope: "workspace" }`

### 5.2 PRISM orchestrates, runners interpret

PRISM core should own:

- validation job lifecycle
- timeout and budget enforcement
- command execution
- stdout and stderr capture
- provenance recording
- attempt accounting
- task state transitions
- final result persistence

Validation runners should own:

- translation from runner args to execution plan
- result parsing
- failed-unit identification
- bounded recovery plan generation
- rerun aggregation into a structured outcome

This keeps PRISM in the Mode A posture where the platform owns orchestration and runners own runner-specific logic.

### 5.3 Repo-local JS or TS adapters plus bundled defaults

PRISM should support repo-local validation adapters under `.prism/validation/runners/`, implemented through the existing JS or TS runtime.

Reasons:

- structured input and output
- easier parsing and bounded recovery planning
- versioned, reviewable runner logic in repo
- less hardcoded runner behavior in Rust

PRISM should also ship bundled default runners such as:

- `cargo_test`
- `pytest`
- `vitest`
- `playwright`

Repos may use the built-ins directly or eject them into `.prism/` for customization.

### 5.4 Recovery is partly generic and partly runner-specific

The generic platform loop is:

- run
- parse
- optionally recover
- rerun
- aggregate
- record

The runner-specific logic is:

- identifying failed units
- generating isolated rerun plans
- deciding which failures are safely flake-recoverable

Recovery therefore belongs in the runner adapter contract, not in a fully generic shell abstraction.

## 6. Claimability and Scheduling

A task is fully actionable for a runtime when:

- normal coordination preconditions hold
- required validation capability classes are already verified for that runtime

A task is conditionally actionable when:

- normal coordination preconditions hold
- required capability classes are known in policy
- the runtime is not yet verified
- policy allows provisional claim and rapid proof establishment

A task is not actionable for a runtime when:

- a required capability class is unknown in policy
- provisional claim is forbidden
- the runtime cannot establish the required capability
- or other coordination eligibility checks fail

The query layer should surface these distinctions explicitly.

## 7. Validation Jobs and Task Lifecycle

Warm validation should use first-class validation jobs.

A validation job should include at least:

- `job_id`
- `repo_id`
- `runtime_id`
- `task_id`
- optional `artifact_id`
- triggering transition or event
- capability class
- runner kind
- runner args
- timeout or budget
- lifecycle state
- started and completed timestamps
- result reference
- provenance

Validation job states should include:

- `pending`
- `running`
- `succeeded`
- `failed`
- `timed_out`
- `cancelled`

The task lifecycle should support explicit validation-gated completion states such as:

- `CompletionRequested`
- `ValidationPending`
- `ValidationRunning`
- `ValidationFailed`
- `Completed`

The claiming runtime should normally keep the lease during:

- validation pending
- validation running
- a short grace period after validation failure

That preserves continuity and avoids claim thrash.

## 8. Completion Flow

Preferred flow:

1. agent requests task completion
2. PRISM verifies ownership, required artifacts, known capability classes, and runtime eligibility
3. PRISM transitions the task to `CompletionRequested` and creates a validation job
4. PRISM returns the `job_id` quickly
5. the runtime starts validation locally
6. the task moves through `ValidationPending` and `ValidationRunning`
7. the agent polls or subscribes on `job_id`
8. success records validation truth and finalizes completion
9. failure records validation truth, enters `ValidationFailed`, and preserves ownership briefly for repair

The completion mutation should not block synchronously on long-running validation.

## 9. Validation Result Envelope

The coordination layer should store validation truth rather than runner implementation detail.

The compact result envelope should include at least:

- `status`
- `runner_kind`
- `capability_class`
- `runtime_id`
- `repo_id`
- `validated_revision`
- `started_at`
- `completed_at`
- `duration_ms`
- `attempt_count`
- optional unit counts
- optional recovery metadata
- optional `failure_class`
- optional `summary`
- optional `result_artifact_ref` or `diagnostic_ref`

It should not store giant logs, raw command strings, or runner-internal parsing structures as authoritative coordination truth.

## 10. Relationship to Artifacts, Reviews, and Cold CI

Warm validation should fit naturally with artifact and review models.

Completion may depend on:

- required artifact exists
- artifact validation succeeded
- review requirements consume validation as evidence

Review surfaces should be able to answer:

- this artifact was validated
- by runtime `R`
- at revision `C`
- under validation policy `P`

Warm validation is best for:

- task completion gating
- review gating
- fast iterative feedback
- using already-warm toolchain and build state

Cold CI remains best for:

- final integration to `main` or another target branch
- environment-neutral confirmation
- organization-wide merge policy
- catching “works only on this machine” issues

PRISM should model both rather than forcing a false choice between them.

## 11. Repo Policy, Query, and Diagnostics Expectations

The query layer and UI should surface at least:

- which tasks require validation
- which capability classes are required
- whether a runtime is fully or conditionally eligible to claim a task
- validation job status
- validation provenance
- tasks blocked or delayed by validation
- runtimes with verified or provisional posture
- unknown or missing capability references

Sanity checks should cover:

- tasks referencing unknown capability classes
- capability classes with no viable runtime
- repeated capability-proof failures
- tasks stuck in validation-pending or validation-failed posture
- validation policies that reference missing runner implementations

These checks should be available through CLI, query surfaces, and later UI diagnostics.

## 12. Recommended Rollout

Phase 1:

- capability class vocabulary in repo or service policy
- runtime capability posture
- capability proof records
- capability-aware claimability
- validation-gated completion with validation jobs
- explicit task lifecycle states
- compact result and provenance recording

Phase 2:

- bundled validation runners
- seeded capability classes
- repo init and eject UX
- repo-local JS or TS adapters
- richer UI surfaces
- retry, grace, and staleness policy controls

Phase 3:

- broader distributed validation fabric if still needed
- service-assisted scheduling across runtimes where policy allows
- partitioned validation jobs and other execution-fabric growth

V1 should stay validation-specific rather than becoming a general execution substrate.

## 13. Recommendation

PRISM should implement warm-state validation feedback as a first-class completion subprotocol owned by the claiming runtime.

The recommended posture is:

- tasks and artifacts declare validation policy up front
- capability classes are policy-defined vocabulary with proof commands
- runtimes establish posture through declaration and proof
- tasks become fully or conditionally claimable based on posture
- completion creates an asynchronous validation job
- the claiming runtime executes validation locally in warm state
- named runners with structured args define how validation is performed
- PRISM ships bundled default runners and seeded capability classes
- repos may explicitly eject and customize those defaults under `.prism/`
- success finalizes completion
- failure stays visible, attributable, and locally repairable
- cold CI remains the selective final integration gate

That gives PRISM a strong answer to one of the main bottlenecks in multi-agent development: centralized cold CI is too slow and too queue-bound to act as the main inner-loop validation mechanism, while warm runtime-local validation is faster, better attributed, and closer to the real execution context that produced the change.
