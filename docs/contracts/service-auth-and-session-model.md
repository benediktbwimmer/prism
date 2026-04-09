# PRISM Service Auth And Session Model

Status: normative contract  
Audience: auth, service, runtime, MCP, CLI, UI, and future hosted deployment maintainers  
Scope: principal trust roots, service sessions, runtime sessions, human and service attestation, repo enrollment, and auth state ownership for PRISM Service

---

## 1. Goal

PRISM must define one explicit **service auth and session model** contract.

This contract exists so that:

- principal identity remains the trust root
- service auth does not drift into ad hoc transport-specific rules
- runtimes participate under delegated service sessions instead of acting as roots of trust
- human-required and service-required actions use explicit attestation rather than reusable bearer
  elevation
- future service-managed identities can extend the same model instead of replacing it

This contract relies on:

- [identity-model.md](./identity-model.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [service-capability-and-authz.md](./service-capability-and-authz.md)
- [signing-and-verification.md](./signing-and-verification.md)
- [provenance.md](./provenance.md)

## 2. Core invariants

The service auth model must preserve these rules:

1. Principal identity is the durable trust root.
2. Runtimes and MCP bridges are not durable trust roots.
3. Service sessions are authenticated transport and authorization context, not replacement
   identities.
4. Human-required and service-required actions must use explicit action-bound attestation.
5. Private principal key material remains local by default.
6. The PRISM Service stores authz state and sessions; it does not become the default holder of user
   private keys.

## 3. Identity and session artifacts

PRISM must distinguish at least these artifacts:

- encrypted principal identity bundle
- principal public record
- machine-wide service session
- runtime session
- browser session
- one-shot human attestation proof
- one-shot internal service attestation proof

These artifacts must not be conflated.

## 4. Principal identity

The principal identity is the durable trust root.

In the local and bring-your-own-principal v1 model:

- the user holds an encrypted local identity bundle
- the bundle is protected by a password or passphrase
- the service stores public trust material and policy state only

The service may later support service-managed identities, but that does not change the trust-model
requirement that service sessions and runtime sessions remain distinct from durable trust roots.

## 5. Service session model

A service session is the machine- or client-scoped authenticated context established after a
principal proves control of a trusted identity.

At minimum a service session should be bound to:

- `principal_id`
- service audience or endpoint identity
- issued-at and expiry
- repo or project scope when relevant
- capability envelope or resolvable capability bindings
- revocation and renewal metadata

In local mode, a machine-wide service session may be stored under `PRISM_HOME` and reused by local
MCP daemons and local tooling on that machine.

## 6. Runtime session model

Runtime sessions are delegated sessions issued by the PRISM Service under an authenticated
principal-backed service session.

A runtime session must be bound to at least:

- `principal_id`
- `runtime_id`
- `repo_id`
- optional `worktree_id`
- runtime capability envelope
- issued-at and expiry

Runtime sessions must be:

- short-lived
- renewable
- revocable
- narrower than the underlying principal authority

## 7. Browser session model

Browser sessions are transport state for the service UI.

The browser UI must resolve to an effective `principal_id`.
Browser sessions must not become a second independent identity root.

## 8. Login and authentication flows

### 8.1 Bring-your-own principal login

The baseline login flow is signed-challenge authentication:

1. client requests challenge
2. service returns nonce, audience, and expiry
3. local helper unlocks identity briefly and signs challenge
4. service verifies the signature
5. service issues a service session

The service must not require the local private key to be sent to the service.

### 8.2 Future service-managed identities

PRISM may later add service-managed account login such as:

- username and password
- SSO
- social login

Those modes may map the authenticated user to a PRISM principal or principal-equivalent service
account, but they must still feed into the same service-session and attestation model.

## 9. Identity continuity across machines

For v1, identity continuity across machines is achieved by explicit export and import of the
encrypted identity bundle.

PRISM should also support:

- local password change for that bundle

Future service-managed identity continuity should prefer:

- new-device enrollment under the same account or principal

rather than requiring raw private-key download as the primary UX.

## 10. Attestation classes

PRISM must distinguish these authority classes:

- `delegated_machine`
- `human_attested`
- `service_attested`

### 10.1 Delegated machine

Delegated machine authority is the normal session-backed authority used by local tooling, MCP, and
runtime clients for ordinary permitted actions.

### 10.2 Human attested

Human-attested actions require a one-shot approval proof bound to a canonical action digest.

The proof must be:

- scoped to one exact action
- nonce-bound
- audience-bound
- expiry-bound
- non-replayable

PRISM must not rely on reusable human elevation bearer tokens.

### 10.3 Service attested

Service-attested actions require internal service-side attestation for one exact action.

Client-side agents must not receive reusable service-elevation credentials.

## 11. Sensitive and policy-required actions

Human or service attestation is not only for dangerous administration.

Any mutation or operation whose policy explicitly requires human or service identity must use the
required attestation class, including when the operation is an ordinary coordination mutation.

## 12. Repo registration and enrollment

Runtime connection may automatically propose repo presence or registration to the service.

The service may only accept repo enrollment when the authenticated principal or session has the
required registration capability.

Automatic detection does not imply automatic authorization.

## 13. Auth state ownership

The PRISM Service stores:

- trusted principal public material
- membership and capability grants
- runtime registration records
- active sessions
- revocation state
- audit and provenance metadata

Storage backend:

- SQLite in single-instance local mode
- Postgres in hosted or multi-instance mode

Private principal keys remain local by default.

## 14. Service secret posture

Service-attestation secrets or signing keys must not rely on plain environment variables in
production-grade deployments.

Allowed postures are:

- encrypted service key material under service control
- OS keystore or keychain
- KMS or workload identity

Environment-variable secrets are development-only and must be documented and surfaced as such.

## 15. Machine-wide sessions and agent isolation

Machine-wide service sessions may be visible to local runtimes and agents.

That is acceptable only because:

- those sessions represent delegated machine authority
- they are not reusable human or service elevation credentials

Human attestation and service attestation must use separate approval or signing paths so agents
cannot silently escalate from delegated machine authority to human or service authority.

## 16. Security boundary note

This model provides a strong application-level distinction between:

- delegated machine activity
- human-attested actions
- service-attested actions

It does not provide an absolute guarantee against an agent with unrestricted control inside the
same OS trust boundary as the human or service signer.

For stronger guarantees:

- human signing should occur through a trusted helper path or isolated environment
- service signing should occur in an isolated and trusted service environment

## 17. Minimum implementation bar

This contract is considered implemented only when:

- principal identity remains the durable trust root
- runtimes operate under delegated runtime sessions rather than as roots of trust
- human and service authority use action-bound attestation rather than reusable elevation bearer
  tokens
- repo enrollment is capability-gated
- service authz and session state have an explicit storage owner
