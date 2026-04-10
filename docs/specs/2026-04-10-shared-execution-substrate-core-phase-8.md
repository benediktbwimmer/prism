# Shared Execution Substrate Core Phase 8

Status: completed  
Audience: prism-core, prism-mcp, runtime, coordination, validation, and Actions maintainers  
Scope: land the shared execution substrate core beneath existing event-trigger execution without prematurely pulling validation or Actions into the same implementation slice

---

## 1. Goal

Phase 8 exists to make the shared execution substrate real as code, not just as design intent.

This phase should establish:

- shared substrate-native execution types
- shared substrate-native runner and result vocabulary
- one authority-facing substrate read and write seam
- one first real consumer of that seam: event-trigger execution

This phase should **not** try to finish warm-state validation or Actions. Those are the explicit
next phases in the roadmap.

---

## 2. Required outcomes

Phase 8 is complete only when all of the following are true:

- `prism-core` exposes substrate-native execution record, runner, capability, target, and result
  types
- those types do not leak SQLite or Postgres implementation details
- the event-trigger execution path uses the shared substrate vocabulary rather than bespoke
  event-only record interpretation
- authority-facing substrate reads and writes are available through explicit ports instead of
  forcing each caller to reinterpret raw event execution records independently
- validation and Actions can adopt the same substrate in later phases without redefining the
  execution model again

This phase does not require:

- validation execution migration
- first-class `Action`
- full service-to-runtime dispatch transport
- final materialization-policy support

---

## 3. Core implementation slice

This phase should land in one bounded implementation slice.

### Slice 1: Substrate-native execution core plus event-job adoption

Deliver:

- one `prism-core` execution-substrate module with:
  - execution family
  - runner category
  - runner reference
  - capability class reference
  - execution target reference
  - execution status
  - structured result envelope
  - substrate execution record view
- one authority-facing substrate store or adapter surface that returns those substrate records and
  accepts substrate transitions or writes where appropriate
- conversions between the current durable event execution records and the shared substrate-native
  view
- migration of the workspace event engine claim and execution planning code to the shared substrate
  vocabulary

Success condition:

- event-trigger execution is no longer the place where PRISM invents its own one-off execution
  model
- later validation and Action work can reuse the substrate types and read/write seams directly

---

## 4. Hard rules

The implementation must preserve these architectural constraints:

- the substrate must stay backend-neutral and SQL-safe
- the substrate must stay semantically below validation and Actions rather than flattening them
- event-trigger execution may remain the only active substrate consumer in this phase, but the
  types must not be event-specific
- structured result envelopes must be compact and JSON-friendly
- substrate execution reads must return exactly the shaped substrate DTOs the caller needs, not
  raw snapshots or backend-shaped blobs
- any current event-only metadata that is reused must be normalized behind substrate helpers

---

## 5. Recommended type shape

The exact naming may vary, but the phase should converge on concepts equivalent to:

- `ExecutionFamily`
  - `event_job`
  - `validation`
  - `action`
- `ExecutionRunnerCategory`
  - `event_runner`
  - `validation_runner`
  - `action_runner`
- `ExecutionRunnerRef`
  - category plus runner kind
- `ExecutionCapabilityClassRef`
  - optional named capability class
- `ExecutionTargetRef`
  - service-local or runtime-targeted execution identity
- `ExecutionStatus`
  - shared status vocabulary derived from the durable record layer
- `ExecutionResultEnvelope`
  - compact status, summary, duration, typed detail payload, and optional evidence or diagnostic
    refs
- `ExecutionRecordView`
  - the shared substrate read model for durable executions

These are the types later phases should build on instead of inventing validation-only or
Action-only equivalents.

---

## 6. Event-job migration requirement

The current workspace event engine is the required first adopter in this phase.

That means:

- recurring plan trigger claim logic should still behave the same semantically
- execution planning should still preserve current event claim and expiry behavior
- but the event engine should speak substrate execution records, runner identity, and target
  identity rather than bespoke event-only interpretation

Any remaining event-specific details should live in clear metadata or adapter helpers instead of
spreading through the engine logic.

---

## 7. Validation

Minimum validation for this phase:

- `cargo test -p prism-core`
- targeted `cargo test -p prism-mcp ...` coverage for the workspace event engine execution paths

Run the full workspace only if the final implementation crosses the repo’s Tier 3 threshold.

---

## 8. Exit note

Phase 8 should be judged by whether the next two phases can adopt the substrate directly:

- Phase 9 should be able to move warm-state validation onto the shared substrate without inventing
  a second execution core
- Phase 10 should be able to add `Action` as a first-class graph leaf on top of the same substrate

If those later phases still need to redefine runner, execution record, or result semantics, then
Phase 8 is not complete.

---

## 9. Implementation note

Phase 8 is landed with:

- a new `prism-core` shared execution substrate module and authority-backed adapter surface
- shared execution family, runner, target, status, and result-envelope types
- normalization helpers between durable event execution records and the substrate vocabulary
- migration of the workspace event engine claim and execution-pass code onto shared substrate
  records instead of bespoke event-only reads

Warm-state validation and `Action` adoption remain intentionally reserved for Phases 9 and 10.
