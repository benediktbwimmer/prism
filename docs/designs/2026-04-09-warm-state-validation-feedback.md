# Warm-State Validation Feedback

Status: proposed design  
Audience: coordination, runtime, query, service, MCP, CLI, UI, and policy maintainers  
Scope: warm-state validation as a first-class task-correctness mechanism, with shared execution substrate beneath its runtime execution

---

## 1. Summary

PRISM should support warm-state validation feedback as a native coordination feature.

The semantic model remains:

- tasks and required artifacts may declare validation policy up front
- the runtime that claims the task is the preferred validation execution site
- validation runs against an already-warm worktree and toolchain state
- task completion can be gated by structured validation results when policy requires it
- validation uses named validation runners plus structured arguments, not ad hoc shell conventions
- validation capability classes are declared in repo or service policy and may be seeded from bundled defaults
- runtime capability posture is established through explicit proof commands on capability classes
- validation produces durable provenance tied to the task, runtime, capability class, runner kind, and revision

The architectural correction is:

- validation should **not** keep a bespoke standalone execution stack
- validation execution should use the shared substrate defined in
  [2026-04-09-shared-execution-substrate.md](./2026-04-09-shared-execution-substrate.md)

Validation remains semantically special even though its execution substrate is shared with:

- Actions
- event jobs

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
- share execution mechanics with Actions and event jobs without flattening validation semantics

Required non-goals:

- no generic arbitrary remote shell execution feature in v1
- no attempt to replace cold CI for every trust boundary
- no reduction of validation into “just another action”
- no treatment of validation success as a free-form comment or soft signal

This design is complemented by
[2026-04-09-repo-init-and-validation-ejection.md](./2026-04-09-repo-init-and-validation-ejection.md),
which defines how validation runners and capability classes become explicit repo-local
configuration.

## 3. Semantic model

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

### 3.2 Validation remains part of task correctness

Validation remains special because it is tied to:

- task completion gating
- artifact correctness
- evidence and review boundaries
- warm runtime ownership

That is why validation should remain semantically distinct from both Actions and event jobs.

### 3.3 Warm validation is owned by the claiming runtime

The default execution model remains:

- the runtime that claims the task is also the runtime that runs required validation

Reasons:

- it already has the relevant worktree
- it already has warm cache and build state
- it already knows the local execution context
- the same agent is best positioned to repair failures immediately

## 4. Shared execution correction

### 4.1 What changes

What changes is the execution architecture beneath validation.

Validation execution should now share with Actions and event jobs:

- capability classes
- runtime capability posture
- runner contracts
- execution records
- timeout and retry machinery
- structured result envelopes
- provenance model

### 4.2 What does not change

This correction does **not** change the higher-level semantics that matter to task lifecycle.

Validation remains:

- part of task correctness
- completion-gating
- warm-state aware
- runtime-local by default
- distinct from generic orchestration work

## 5. Capability classes and posture

Validation capability classes should be declared in repo or service policy rather than compiled into
PRISM.

Examples:

- `cargo:test`
- `cargo:workspace-test`
- `npm:test`
- `pytest`
- `playwright`
- `integration:db`

A capability class is a small policy object, not just a label. It should define:

- `name`
- optional `description`
- `proof_commands`
- `proof_success_policy`
- optional `proof_ttl`
- intended validation runner kinds where appropriate

Runtime capability posture should distinguish at least:

- `declared`
- `provisional`
- `verified`

An optional later `stale` posture can capture expired proof or obvious environment drift.

## 6. Validation runners

Validation should use named runners with structured arguments instead of raw command strings as the
semantic layer.

Examples:

- `cargo_test`
- `pytest`
- `vitest`
- `playwright`

Tasks or artifacts then declare:

- runner kind
- structured args

Validation runners should execute on the shared substrate, but they remain a distinct runner family
within it.

## 7. Repo-local and bundled runners

PRISM should support repo-local validation adapters under `.prism/validation/runners/`,
implemented through the existing JS or TS runtime.

PRISM should also ship bundled default runners such as:

- `cargo_test`
- `pytest`
- `vitest`
- `playwright`

Repos may use the built-ins directly or eject them into `.prism/` for customization.

## 8. Relationship to cold CI

Warm-state validation remains the primary fast-feedback validation layer during active work.

Cold CI remains the selective final gate for higher trust boundaries such as:

- merge to main
- release branch promotion
- production deployment

PRISM should model both layers explicitly rather than forcing one to replace the other.

## 9. Relationship to Actions and event jobs

Validation should share the execution substrate with:

- Actions
- event jobs

But it should remain semantically distinct from both:

- Actions are explicit machine-work graph leaves
- event jobs are orchestration-triggered work
- validation is task-correctness work

That distinction is important for policy, UX, and audit clarity.

## 10. Query and UI expectations

PRISM surfaces should make it easy to answer:

- which tasks require validation
- which validations are pending, passed, or failed
- which runtime ran the validation
- which capability class and runner kind were used
- whether completion is still blocked on validation

## 11. Recommendation

PRISM should keep warm-state validation as a first-class task-correctness mechanism while moving its
execution beneath the shared execution substrate.

That gives PRISM:

- one execution architecture
- one capability and provenance model
- one runner and result model

without losing the special lifecycle role that validation still plays.
