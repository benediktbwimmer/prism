# Event Trigger Claim Loop Phase 15

Status: draft
Audience: service, coordination, event-engine, scheduling, MCP, and future automation maintainers
Scope: add the first explicit service-owned event trigger scan and authoritative claim loop on top of the settled event-record transition model

---

## 1. Summary

This spec is the next concrete Phase 15 target after authoritative event-execution transitions.

The goal is to move from "the service can store and transition event-execution records" to "the
service can scan due trigger candidates and claim them authoritatively through one owned event-loop
path."

This slice should:

- define one explicit event-engine claim-loop owner path inside the service
- read trigger candidates through the settled read and query seams
- deduplicate claim attempts against existing authoritative event-execution records
- claim new execution attempts through authoritative event-record transitions

This slice should not:

- execute hooks or external actions
- complete event attempts with success or failure mutations
- add recurring background daemon scheduling policy beyond the minimum owned loop
- add browser or CLI event management UI

## 2. Related roadmap

This spec continues:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/event-engine.md](../contracts/event-engine.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)

It follows:

- [2026-04-09-event-engine-foundation-phase-15.md](2026-04-09-event-engine-foundation-phase-15.md)
- [2026-04-09-event-execution-record-model-phase-15.md](2026-04-09-event-execution-record-model-phase-15.md)
- [2026-04-09-event-execution-authority-storage-phase-15.md](2026-04-09-event-execution-authority-storage-phase-15.md)
- [2026-04-09-event-execution-authority-transitions-phase-15.md](2026-04-09-event-execution-authority-transitions-phase-15.md)

## 4. Scope

This slice includes:

- one explicit event-engine method for scanning and claiming due trigger candidates
- deterministic candidate selection from settled coordination read/query inputs
- authoritative claim creation through `claim` transitions with optimistic preconditions
- deduplication against active existing execution records for the same trigger target

This slice does not include:

- hook execution runners
- completion transitions after real work runs
- retry backoff tuning beyond minimum claimed-versus-active checks
- hosted multi-instance scheduler coordination beyond optimistic claim correctness

## 5. Design constraints

- The event engine remains a client of the read, mutation, and authority seams; it does not bypass
  them.
- Claim-loop behavior must use explicit authority transitions, not generic record upserts.
- Candidate selection and deduplication must be deterministic enough that parallel service
  instances converge by conflict rather than by hidden local heuristics.
- The initial claim loop may be pull-driven or request-driven, but it must have one named owner in
  code.

## 6. Implementation slices

### Slice 1: Define candidate and claim-loop inputs

- define the event-engine-facing input and result types for one claim-loop pass
- keep the loop boundary explicit rather than returning loose tuples or raw records

Exit criteria:

- the service has one named event-engine claim-loop vocabulary

### Slice 2: Query due trigger candidates and active executions

- route candidate discovery through the settled read/query seams
- load existing active execution records needed for deduplication and optimistic claims

Exit criteria:

- one event-engine owner can explain which candidates are due and why they are claimable or
  skipped

### Slice 3: Apply authoritative claim transitions

- create new claimed execution records through the authority transition seam
- surface applied versus conflict versus skipped outcomes clearly

Exit criteria:

- one claim-loop pass can authoritatively claim due work without generic event-record writes

## 7. Validation

Minimum validation for this slice:

- targeted `prism-mcp` tests for event-engine claim-loop candidate selection and duplicate-skip
  behavior
- targeted `prism-core` tests for any new authority-facing event-engine inputs
- direct downstream compile or test coverage for changed event-engine owner APIs
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- the service has one named claim-loop entry point
- due trigger candidates are read through settled service seams
- new claims use authoritative event-record transitions
- the event engine can explain applied, conflicting, and skipped claim outcomes

## 9. Implementation checklist

- [ ] Define event-engine claim-loop input and result types
- [ ] Query due trigger candidates and active executions through settled seams
- [ ] Apply authoritative claim transitions through the event-engine owner
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap and spec status as slices land
