# PRISM Coordination Mutation Protocol

Status: normative contract  
Audience: coordination, runtime, MCP, CLI, UI, and future service maintainers  
Scope: transactional mutation intent, validation, retry, commit, and rejection semantics for
authoritative coordination writes

---

## 1. Goal

PRISM must define one backend-neutral **Coordination Mutation Protocol** for authoritative
coordination writes.

The protocol exists so that:

- all authoritative coordination writes share one transaction model
- the mutation broker orchestrates transactions instead of inventing their meaning
- backend choice changes commit mechanics, not workflow semantics
- callers receive one consistent authoritative commit result shape

This protocol is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [coordination-artifact-review-model.md](./coordination-artifact-review-model.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [provenance.md](./provenance.md)
- [identity-model.md](./identity-model.md)

## 2. Core invariants

The protocol must preserve these rules:

1. One logical coordination transaction either commits atomically or fails atomically.
2. Validation must be deterministic for a given staged input and authority base.
3. Conflict detection must happen against the authoritative base, not only local materialization.
4. Retries may restage and replay, but may not silently weaken validation rules.
5. The commit result must identify the authoritative post-commit version or stamp.
6. Local materialization and checkpoint updates are downstream effects, not the definition of
   commit success.

## 3. Mutation intent shape

Every authoritative write must be expressed as a coordination transaction intent.

A transaction intent must include:

- coordination root identity
- caller identity and authorization context
- base authority metadata or conflict preconditions
- one or more staged mutations
- deterministic client ids for newly created objects when applicable

The caller identity and authorization context should be rich enough to distinguish principal,
credential, runtime, and execution lane when relevant.

The protocol should make staged mutations explicit rather than relying on side effects hidden
behind many unrelated entrypoints.

## 4. Transaction boundaries

The mutation protocol must support one transaction containing multiple related staged mutations.

Examples:

- create a plan and several tasks atomically
- record a review verdict and reopen an existing task atomically
- record a rejected review plus create follow-up tasks and dependency wiring atomically
- yield a task and persist its checkpoint metadata atomically

The transaction is the only authoritative structural write unit.

## 5. Validation ordering

Validation must occur in this order:

1. input shape validation
2. authorization and capability validation
3. object existence and identity validation
4. domain-rule validation against the staged graph or staged snapshot
5. conflict detection against the authoritative base
6. commit

Domain validation must evaluate the staged transaction as a whole, not mutation-by-mutation in a
way that can observe impossible intermediate states.

## 6. Deterministic rejection

A transaction must be rejected when:

- the caller is not authorized
- the input shape is invalid
- referenced objects do not exist or are out of scope
- the staged transaction violates coordination domain rules
- the authoritative base has advanced in a way that makes the staged transaction invalid and replay
  cannot preserve the same semantics

Rejection must be explicit and structured.

The caller must receive:

- rejection category
- affected objects or references when known
- a stable reason code
- enough metadata to distinguish retryable conflict from deterministic invalid input

## 7. Replay and retry

When the authoritative base has advanced, the protocol may support replay and retry.

Replay means:

- reload current authoritative state
- restage the logical transaction against the new base
- re-run deterministic validation

Replay is allowed only if the transaction's semantic meaning is preserved.

Replay must reject rather than silently alter meaning when:

- the affected write set changed incompatibly
- the reopen owner is no longer valid
- review scope or evidence lineage changed in a way that changes the logical action
- dependency or lifecycle rules make the staged action invalid on the new base

## 8. Commit result shape

Successful commit results must include:

- authoritative outcome: committed
- post-commit authority stamp, version, or equivalent identifier
- committed coordination root identity
- created or changed object ids
- any transaction metadata needed for later provenance queries

That metadata should be sufficient to attribute the commit to principal, execution context, and
authority base according to [provenance.md](./provenance.md).

The result may also include backend-specific metadata, but the common semantic result must be
backend-neutral.

## 9. Local pending versus authoritative state

The protocol must keep these states distinct:

- staged local intent
- authoritative committed state
- eventual local materialized state derived from the authoritative commit

Authoritative success means the coordination backend committed the transaction.
It does not mean every local read model, checkpoint, or UI cache has already advanced.

## 10. Artifact and review outcomes

The protocol must support the artifact and review rules from
[coordination-artifact-review-model.md](./coordination-artifact-review-model.md).

That includes:

- artifact emission against declared requirements
- primitive review records on one artifact
- review-task pass resolution
- `approved`
- `changes_requested` with atomic reopen of exactly one existing task
- `rejected` with atomic creation and wiring of explicit follow-up tasks
- `yield_task` with checkpoint requirements

## 11. Surface relationship

Different mutation surfaces may exist temporarily, but they must all route into this protocol.

Examples:

- `prism_code`
- CLI mutation commands
- future PRISM Service mutation broker

The target public surface is already converged:

- `prism_code` is the canonical programmable write surface

The underlying protocol must remain one even when multiple transports front it.

## 12. Minimum implementation bar

This contract is considered implemented only when:

- authoritative coordination writes no longer bypass the transaction protocol
- replay and conflict behavior is defined through one mutation path
- commit results carry authoritative version metadata
- local materialization is clearly downstream of authoritative commit
