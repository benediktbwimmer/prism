# PRISM Identity Model

Status: normative contract  
Audience: auth, coordination, runtime, query, MCP, CLI, UI, knowledge, and future service maintainers  
Scope: durable and ephemeral identity primitives, actor classes, trust roots, and identity relationships across PRISM

---

## 1. Goal

PRISM must define one explicit **identity model** because identity is part of PRISM's truth model,
not just transport plumbing.

This contract exists so that:

- provenance and authorization rely on one shared actor model
- runtimes, worktrees, principals, and future services do not invent conflicting identities
- signing and verification can attribute durable state transitions meaningfully

This contract refines the broader shared vocabulary in
[shared-scope-and-identity.md](./shared-scope-and-identity.md).

Canonical ownership:

- this document defines actor classes, trust roots, authorship versus execution identity, and
  stable-versus-ephemeral identity rules
- [shared-scope-and-identity.md](./shared-scope-and-identity.md) defines only the shared cross-layer
  vocabulary
- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md) specializes runtime
  publication and discovery semantics

This contract captures the normative identity boundary distilled from:

- `docs/designs/PRISM_AUTH_AND_IDENTITY_MODEL.md`

## 2. Core invariants

The identity model must preserve these rules:

1. Principal identity is the durable trust root in the default model.
2. Agents are execution lanes, not durable principals.
3. Service sessions and runtime sessions are not durable trust roots.
4. Services may later manage identities, but that does not eliminate principal-rooted trust
   semantics.
5. Worktree execution identity is the durable local execution identity for agent work.
6. Bridge sessions are ephemeral carriers, not durable owners of work.
7. Identity classes and relationships must be explicit enough to support provenance, authorization,
   and verification.

## 3. Minimum identity kinds

The identity model must distinguish at least:

- bootstrap issuer
- human principal
- local human credential
- human session
- worktree record
- worktree execution identity
- bridge session
- service session
- runtime identity
- runtime session
- future service-managed identity or account

Not all of these are durable in the same way, but they must not be conflated.

## 4. Stable versus ephemeral identities

The contract must distinguish:

- durable portable identities
  - for example principal identities
- durable local execution identities
  - for example worktree execution identity
- ephemeral live carriers
  - for example bridge sessions, service sessions, runtime sessions, and current runtime processes

This distinction matters for:

- provenance
- authorization
- lease and claim continuity
- restart behavior

## 5. Trust roots

The identity model must also identify the minimum trust anchors:

- external bootstrap issuers
- durable principal authority namespace
- local credential material
- service attestation authority when applicable
- runtime identities used for delegated publication or execution when applicable

Private secret material and mutable trust-root state remain outside repo-published truth.

## 6. Authorship versus execution

The identity model must let PRISM answer all of these separately when relevant:

- which principal authorized this
- which local credential or authenticator was used
- which runtime or worktree executed it
- which bridge or session carried it

PRISM must not collapse those into one undifferentiated "actor" field.

For policy and provenance purposes it must also be possible to distinguish the effective authority
class:

- delegated machine
- human attested
- service attested

## 7. Relationship to worktree exclusivity

For ordinary local agent work, the enforceable local coordination boundary is one mutating bridge
per worktree.

The identity contract must therefore treat worktree execution identity as a first-class durable
execution identity rather than trying to revive durable local agent principals.

## 8. Relationship to other contracts

This contract is foundational for:

- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [provenance.md](./provenance.md)
- [signing-and-verification.md](./signing-and-verification.md)
- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md)

## 9. Minimum implementation bar

This contract is considered implemented only when:

- actor classes are explicit
- stable versus ephemeral identities are explicit
- provenance and authorization can distinguish principal, credential, runtime, and execution lane
