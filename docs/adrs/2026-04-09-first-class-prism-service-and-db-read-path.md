# ADR: First-Class PRISM Service And DB-Backed Read Path

Status: accepted  
Date: 2026-04-09  
Scope: PRISM Service process model, service versus MCP ownership, service auth and session shape,
DB-backed coordination read path defaults, and deployment topology rules

---

## Context

PRISM has already crossed the threshold where coordination participation is service-backed in
practice, but the product and contract story is still partially shaped by older assumptions:

- the MCP daemon still acts like the implicit service host
- the browser UI is still described too close to the MCP daemon
- service lifecycle is not yet a first-class CLI concept
- DB-backed coordination authority still carries older materialization assumptions that were useful
  for the Git-backed path but are not the right default for SQLite or Postgres authority
- the service auth story has not yet been frozen tightly enough for local dogfooding, hosted
  deployments, and future service-managed identities to share one coherent path

At the same time, PRISM now has a clearer target:

- a first-class PRISM Service
- one local machine service in local mode
- direct hosted-service connectivity in team mode
- MCP daemon as the worktree-local runtime and MCP surface only
- UI served by the PRISM Service
- DB-backed authority as the release-oriented path
- no extra coordination SQLite materialization by default for DB-backed authority

## Decision

PRISM adopts the following architecture.

### 1. PRISM Service is first-class and separate from the MCP daemon

PRISM Service is a first-class host with its own lifecycle surface:

- `prism service up`
- `prism service stop`
- `prism service restart`
- `prism service status`
- `prism service health`
- additional service lifecycle or diagnostics commands later

The MCP daemon is not the PRISM Service.

The MCP daemon is:

- the worktree-local runtime process
- the MCP server surface for that worktree

The browser UI is served by the PRISM Service, not by the MCP daemon.

### 2. Service startup and login remain explicit

PRISM must not implicitly:

- boot the PRISM Service
- log a user in
- unlock a principal identity

If a configured or discovered service endpoint is unavailable, PRISM must fail clearly.

If an explicit service endpoint is configured, PRISM must not silently fall back to a local
machine service.

### 3. MCP daemon launch and restart may be bridge-assisted

The stdio bridge may:

- launch the worktree-local MCP daemon when it is absent
- restart the worktree-local MCP daemon when it is temporarily unavailable
- expose restart or warmup state through `prism://startup`

This is allowed because the MCP daemon is a local runtime surface, not the service trust root.

The bridge must not:

- log the user in implicitly
- unlock a principal identity implicitly
- boot the PRISM Service implicitly

### 4. Service connectivity model

PRISM supports two primary coordination participation modes:

- local mode
  - clients connect to one machine-local PRISM Service
- hosted mode
  - clients connect directly to a hosted PRISM Service

PRISM does not require a local per-machine service proxy in front of a hosted service.

Local edge or proxy service modes may be added later, but they are not required for the primary
hosted topology.

### 5. Service endpoint selection

Service endpoint selection follows this rule:

1. explicit configured endpoint
2. otherwise machine-local service discovery
3. otherwise fail clearly

If an explicit endpoint is configured, it wins and must fail loudly if unavailable.
PRISM must not silently fall back to a local service in that case.

### 6. Local service state and backend selection

Machine-local PRISM Service state lives under:

- `PRISM_HOME` when set
- otherwise `~/.prism`

Authority backend selection is:

- if `PRISM_POSTGRES_DSN` is set, use Postgres
- otherwise use SQLite

SQLite-backed PRISM Service is supported only for a single-instance topology.
PRISM Service must warn loudly on startup when running with SQLite authority that:

- this mode is single-instance only
- multi-instance deployments must use Postgres

### 7. DB-backed coordination read path default

When the active coordination authority backend is DB-backed:

- PRISM must not use a separate coordination SQLite materialization by default
- current authority reads are the standard coordination read path
- many eventual reads may collapse to the same authority-backed read path as strong reads

PRISM retains the semantic distinction between:

- `strong`
- `eventual`

but for DB-backed authority those modes may map to the same current-authority path until a real
lagging projection exists.

Optional coordination materialization remains allowed only as an explicit optimization for the
Postgres backend.
It is disabled by default and is not part of the default DB-backed read path.

### 8. Service auth model

PRISM adopts a layered auth model:

- principal identity is the durable trust root
- service sessions are the authenticated service-side actor context
- runtime sessions are short-lived delegated execution sessions under a principal-backed service
  session

The service stores:

- trusted principal public records
- memberships
- capability grants
- active sessions
- revocations
- audit and provenance metadata

Private principal keys remain local by default.

### 9. Human and service authority are attested, not borrowed

PRISM must not rely on reusable human or service elevation bearer tokens.

Instead:

- ordinary agent-visible work uses delegated machine or runtime sessions
- human-required actions use one-shot human attestation bound to a canonical action digest
- service-required actions use internal service attestation for the exact action

This applies not only to dangerous administrative actions, but also to coordination mutations whose
policy explicitly requires human or service identity.

### 10. Identity continuity

For v1, principal identity continuity across machines is achieved by:

- explicit export and import of the encrypted identity bundle

This is intentionally similar to password-protected SSH key continuity.

PRISM should also support local password change for that identity bundle.

Future service-managed identities may add:

- service-side account registry
- service-issued onboarding
- new-device enrollment under one account

But those are follow-on capabilities, not the default trust-root model for v1.

### 11. Repo registration and enrollment

Runtime connection may automatically propose repo presence to the service.

Repo enrollment must still be capability-gated.
The service may only accept automatic repo registration when the authenticated principal has the
required registration capability.

## Consequences

### Positive

- service lifecycle becomes explicit and understandable
- MCP daemon and UI ownership become much cleaner
- hosted deployments avoid an unnecessary local proxy layer
- DB-backed authority gets a simpler default read path
- the strong versus eventual semantic contract is preserved without forcing an unnecessary cache
- local dogfooding and hosted deployments share one coherent auth model
- the design now has a clean path toward future service-managed identities

### Tradeoffs

- older docs that still treat MCP as the implicit service host must change
- local and hosted service discovery rules must be explicit in code and docs
- auth flows must be modeled more carefully than “runtime just talks to service”
- SQLite local mode must be documented as single-instance only

## Security posture note

PRISM can provide a strong application-level distinction between:

- delegated machine activity
- human-attested actions
- service-attested actions

It cannot provide an absolute guarantee against an agent with unrestricted control inside the same
OS trust boundary as the human or service signer.

Therefore:

- human-attested guarantees assume the local signing helper environment is trusted relative to the
  agent
- service-attested guarantees assume the service signing environment is trusted and isolated

## Follow-through requirements

The following docs must align with this ADR:

- `docs/contracts/service-architecture.md`
- `docs/contracts/service-auth-and-session-model.md`
- `docs/contracts/service-capability-and-authz.md`
- `docs/contracts/authorization-and-capabilities.md`
- `docs/contracts/identity-model.md`
- `docs/contracts/signing-and-verification.md`
- `docs/contracts/runtime-identity-and-descriptors.md`
- `docs/contracts/service-runtime-gateway.md`
- `docs/contracts/coordination-authority-store.md`
- `docs/contracts/coordination-materialized-store.md`
- `docs/contracts/local-materialization.md`
- `docs/contracts/consistency-and-freshness.md`
- `docs/roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md`
- `docs/roadmaps/2026-04-09-prism-service-extraction-and-db-read-simplification.md`
