# PRISM Auth And Identity Model

Status: canonical proposed design
Audience: PRISM auth, coordination, MCP, runtime, storage, and operator UX maintainers
Scope: one canonical local and shared identity model for bootstrap, login, human mutation, agent execution, worktree continuity, and mutation provenance

This document is the canonical design for PRISM auth and identity.

It supersedes the following historical design docs:

- `PRINCIPAL_IDENTITY_AND_COORDINATION.md`
- `PRISM_EXTERNAL_BOOTSTRAP_ATTESTATION_AUTH.md`
- `SHARED_IDENTITY_FUTURE_STATE.md`
- `MACHINE_OWNER_AND_WORKTREE_IDENTITY.md`

---

## 1. Summary

PRISM should use a simplified identity model:

- humans are the only durable authenticated principals
- human principals are portable across machines
- each machine keeps its own protected local credential material for acting as that human
- agents do not have durable local principals or durable local credentials
- services are a distinct future durable principal class for hosted or remote automation
- a worktree is the durable execution identity for agent work
- a bridge session is only the live carrier for one worktree execution slot
- only one live mutating bridge may own one worktree at a time
- multiple read-only bridges may coexist

This model is intentionally honest about local trust:

- human versus agent is a real boundary and must be protected
- same-user agent versus same-user agent is not a real security boundary
- worktree exclusivity is the enforceable local coordination boundary that matters for correctness

The result should be:

- strong protection against agent impersonation of humans
- simple bridge restart continuity
- cleaner lease and ownership semantics
- less local credential sprawl
- clearer audit trails

## 2. Goals

- Make it impossible for ordinary agents to act as a human by reading local files.
- Preserve one stable human principal across multiple machines.
- Keep the local implementation cross-platform and not dependent on OS keychain APIs.
- Eliminate durable local agent credential management.
- Reserve a clean future path for durable service principals without reintroducing durable local agent identity.
- Make bridge restart continuity automatic within one worktree.
- Enforce one active mutator per worktree.
- Allow direct human mutation through an explicit interactive human session in a registered worktree.
- Keep operator UX legible through unique human-readable worktree labels.
- Preserve a clean path to shared-runtime identity later.

## 3. Non-Goals

- Strong cryptographic isolation between same-user agents on one machine.
- Durable local agent principals that survive independently of worktrees.
- Portable cross-machine agent identities.
- Multiple active human owners on one machine in v1.
- Treating MCP session ids as durable identity.
- Requiring PRISM itself to build browser login, password manager, biometric, or MFA UX.

## 4. Core Entities

### 4.1 Bootstrap issuer

An external trust authority that can attest that a human completed a bootstrap or recovery ceremony.

Examples:

- GitHub Device Flow
- SSH signature workflow
- GPG signature workflow
- enterprise SSO or OIDC issuer
- future hosted or self-hosted attestation issuer

### 4.2 Human principal

A durable PRISM principal representing one human.

Properties:

- portable across machines
- globally stable within its authority namespace
- the only durable authenticated actor class in PRISM

The preferred high-assurance default bootstrap source is GitHub-backed attestation.
The canonical human-readable name should come from the GitHub identity when that path is used.
SSH-signature bootstrap may remain available as a lower-assurance fallback.

### 4.3 Local human credential

Machine-local secret material that lets one machine act as a portable human principal.

Properties:

- local to one machine
- encrypted at rest
- protected by a human-entered password or passphrase
- never directly adoptable by agent bridges

Recommended form:

- encrypted private key or equivalent signing secret
- shared runtime stores verifier or public-key metadata
- local machine stores the usable private side
- avoid long-lived plaintext bearer tokens as the steady-state model

Losing one machine credential must not require minting a new human principal.

### 4.4 Human session

A short-lived unlocked local session created after explicit human authentication.

Properties:

- interactive
- time-bounded
- usable for direct human mutations
- not reusable by agent bridges

Recommended policy:

- ordinary unlock creates a reusable short-lived human session
- idle timeout: 15 minutes
- absolute lifetime: 8 hours
- fresh re-authentication required within 5 minutes for high-risk operations

High-risk operations include:

- bootstrap
- recovery
- credential rotation
- worktree takeover
- future admin overrides

### 4.5 Worktree record

A durable local record for one discovered worktree.

Properties:

- keyed by `repo_id + worktree_id`
- persists across bridge restarts
- stores operator metadata such as label and mode
- uses a registration-time generated opaque `worktree_id`, not a path-derived identifier

### 4.6 Worktree execution identity

The durable execution identity for agent work in one worktree.

Properties:

- derived from the worktree, not from a secret credential
- owns task continuity, leases, and execution attribution for agent work
- replaces the notion of a durable local agent principal

### 4.7 Bridge session

An ephemeral live process or connection attached to one worktree.

Properties:

- current live carrier for the worktree execution identity
- used for liveness, heartbeat, and diagnostics
- never treated as the durable owner of work by itself

### 4.8 Service principal

A future durable PRISM principal representing hosted or remote non-human automation.

Properties:

- portable across machines and runtimes
- distinct from local agent worktree execution identity
- intended for future hosted services, remote workers, or server-side automation
- not required for ordinary local MCP bridge execution

## 5. Trust Model

PRISM should enforce three different boundaries:

### 5.1 Real boundary: human versus agent

Human credentials must be protected from agents.

That means:

- no plaintext reusable human bearer token on disk
- no bridge adoption of human credentials
- no normal agent flow may silently reuse a human secret

### 5.2 Best-effort boundary: agent versus agent

PRISM should not pretend that same-user agents on the same machine are strongly isolated.

Therefore:

- per-agent durable local principals should be removed
- per-agent local secret material should be avoided
- audit and worktree exclusivity should do the real coordination work

### 5.3 Enforceable local coordination boundary: one mutator per worktree

This is the boundary PRISM can actually uphold.

PRISM should enforce:

- any number of read-only bridges
- at most one active mutating bridge per worktree

### 5.4 Durable actor classes

PRISM should recognize three durable actor classes over time:

- `human`
- `service`
- no durable local `agent` class

Local agents are execution lanes, not principals.

## 6. Bootstrap And Recovery

### 6.1 Bootstrap rule

Brand-new human principals must be created only via verified external bootstrap attestation.

PRISM must not use "empty local registry" as permission to bootstrap.

### 6.2 Recovery rule

If local owner state is lost or corrupted, PRISM must require a recovery attestation rather than silently treating the machine as fresh.

### 6.3 Assurance levels

Bootstrap and recovery should record explicit assurance levels:

- `high`
  GitHub-backed or similarly strong interactive attestation
- `moderate`
  SSH/GPG or equivalent cryptographic fallback without strong human-presence proof
- `legacy`
  migrated from the old local bootstrap model

### 6.4 Machine behavior

On a second trusted machine for the same human:

- PRISM should discover the portable human principal through shared identity state
- the machine still requires explicit local credential acquisition
- the machine receives distinct local secret material for that same human principal

## 7. Local Human Auth Model

### 7.1 One active local human credential per machine

Each machine has exactly one active local human-owner credential in v1.

This is intentionally simple:

- one human owner
- one login state
- one local protected credential

### 7.2 Storage rule

Local human credential material must be encrypted at rest.

PRISM should not rely on OS keychain APIs as the architectural foundation. Optional OS integrations may exist later, but the baseline model must work portably through encrypted local storage.

### 7.3 Unlock rule

Human-only operations require explicit interactive unlock.

Unlock should produce a short-lived human session with an explicit expiration.

The human session is the reusable local authorization envelope for direct human work.

### 7.4 Direct human mutation

Humans must be allowed to perform mutations directly.

That path is legitimate for:

- ordinary interactive repo work
- planning and review adjustments
- debugging and operator workflows
- manual coordination changes

Direct human mutation should occur through a human-unlocked session, not through an agent bridge identity.

All authoritative mutations must still occur in a registered worktree context.

There is no worktree-less authoritative mutation path in v1.

## 8. Agent Model

### 8.1 No durable local agent credentials

Agents should not receive durable local credentials.

### 8.2 No durable local agent principals

PRISM should stop modeling local agents as durable principals for ordinary worktree execution.

### 8.3 No portable cross-machine agent identity

Agent continuity must not be modeled as a portable identity.

Portable continuity belongs to:

- humans
- future service principals

Ordinary local agents stay worktree-scoped.

### 8.4 Agent mutations are worktree-executor mutations

For ordinary local agent work:

- the bridge attaches to a worktree execution slot
- the daemon authorizes mutations as that worktree executor
- continuity belongs to the worktree, not to a synthetic agent principal

This keeps provenance honest:

- human actions are human
- local agent actions are worktree-executor actions

### 8.5 Future service role

PRISM should reserve a first-class `service` actor class for future hosted or remote automation.

That role is intentionally separate from local MCP agent execution:

- local MCP agents use worktree execution identity
- future hosted or remote automation may use durable service principals
- introducing service principals must not reintroduce durable local agent credentials

## 9. Worktree Registration And Labels

### 9.1 Discovery

PRISM may discover worktrees automatically.

Discovered worktrees initially exist in an unregistered state.

Unregistered worktrees are read-only from PRISM's perspective.

### 9.2 Registration

A worktree must be explicitly registered before it can perform authoritative mutation.

Registration is a human-owned local operation.

At registration time, PRISM generates a new opaque `worktree_id`.

Recommendation:

- use a registration-time ULID to match the rest of PRISM's identifier style
- do not derive the identity from the absolute path
- persist the `worktree_id` in worktree-local PRISM metadata and mirror it in the local machine registry
- treat the filesystem path as mutable locator metadata, not as identity

Registration assigns:

- `worktree_id`
- unique `agent_label`
- worktree mode: `agent` or `human`

Registration should support both:

- one-shot command mode when all information is supplied
- interactive guided mode when information is missing

### 9.3 Registration rule

Unregistered worktrees may be used for reads.

Unregistered worktrees may not acquire the mutating slot.

If a mutation is attempted from an unregistered worktree, PRISM should reject it with a clear human next action to register that worktree.

If a worktree is moved and its persisted metadata comes with it, the `worktree_id` remains the same.

If PRISM encounters a checkout with no persisted `worktree_id`, it should treat it as unregistered until a human registers or explicitly relinks it.

### 9.4 Labels

Worktree labels must be unique per machine.

Labels are:

- human-readable
- mutable
- descriptive only
- not security identities

Changing a label must not change the durable worktree identity.

### 9.5 Recommended operator pattern

Humans should keep a dedicated human worktree for direct human mutations.

This is a strong recommendation, not a separate identity system. It keeps the local model clean:

- human worktrees for direct human work
- agent worktrees for agent execution

This is the clean default operator pattern for avoiding accidental slot conflicts.

## 10. Worktree Mutation Slot

### 10.1 Slot key

The authoritative mutation slot is keyed by:

- `repo_id + worktree_id`

### 10.2 Exclusivity

Only one live mutating bridge may hold a worktree slot at a time.

If another bridge already holds the slot:

- read-only operations may continue
- authoritative mutations must be rejected

### 10.3 Human direct mutation and the slot

Direct human mutation must respect the same slot boundary.

If an agent bridge already owns the worktree mutating slot, a human attempting direct mutation in that same worktree must either:

- wait
- operate from a different worktree
- or explicitly take over the worktree slot

This keeps the one-mutator rule real.

All authoritative mutations, whether human or agent initiated, are worktree-bound mutations.

## 11. Liveness And Takeover

### 11.1 Liveness

The active mutating bridge should be considered live through explicit daemon or bridge heartbeat with a short grace window, on the order of 30 to 60 seconds.

### 11.2 Automatic same-worktree reacquire

If the active mutating bridge disappears and the liveness window expires, a bridge in the same worktree may reacquire the slot automatically.

This gives ordinary bridge-restart continuity with no human intervention.

### 11.3 Stuck live bridge

A bridge may remain technically alive while being operationally unusable.

In that case PRISM must support explicit human-authorized takeover of the worktree slot without waiting indefinitely for liveness expiry.

### 11.4 Takeover UX

Takeover should support both:

- one-shot command mode when all arguments are supplied
- interactive guided mode when information is missing

Representative command:

- `prism worktree takeover --reason <text>`

The audit trail should record:

- approving human principal
- local credential id
- previous live bridge session
- target worktree
- reason
- resulting new live session if known

## 12. Lease And Claim Continuity

### 12.1 Ownership model

Task leases and claim continuity for agent work should attach primarily to:

- `worktree_id`

and secondarily record:

- `session_id`
- refresh timestamps

### 12.2 Consequences

This means:

- a restarted bridge in the same worktree continues the same execution lane
- moving work to another worktree is a real reassignment
- stale or expired work remains reclaimable explicitly

### 12.3 Refresh rule

Ambient reads must not refresh leases.

Lease refresh happens only through explicit authenticated mutation activity or explicit heartbeat mutation.

## 13. Provenance And Audit

Every authoritative mutation should record:

### 13.1 Actor layer

- actor class: `human`, `worktree_executor`, or future `service`
- portable human principal id when applicable
- local human credential id when applicable

### 13.2 Execution layer

- `repo_id`
- `worktree_id`
- `agent_label`
- `session_id`
- daemon or bridge instance metadata when useful

### 13.3 Work layer

- declared work id
- coordination task id when present
- plan id when present

This gives clean interpretation such as:

- human: `principal:github:bene`
- local credential: `credential:machine-a:...`
- worktree: `worktree-5820c89d186523db`
- label: `codex-d`
- session: `session:01...`

## 14. Storage Model

### 14.1 Local machine state

The local machine should keep:

- encrypted local human credential material
- local owner metadata and active login state
- worktree registration records
- worktree labels and modes
- worktree mutation-slot state
- consumed bootstrap or recovery authority state

### 14.2 Shared runtime state

The shared runtime should keep:

- portable principal registry state
- credential metadata and verifiers or public keys
- lifecycle state such as issued, rotated, revoked, and last-used
- bootstrap and recovery provenance
- audit records for issuance and use

### 14.3 Important split

Shared runtime must not become the wallet for local secret material.

Local machines hold usable secrets.
Shared runtime holds portable identity metadata and verification state.

## 15. CLI And UX Shape

Representative commands should include:

- `prism auth bootstrap`
- `prism auth recover`
- `prism auth login`
- `prism auth whoami`
- `prism worktree list`
- `prism worktree register --label <label> --mode human|agent`
- `prism worktree relabel <new-label>`
- `prism worktree takeover --reason <text>`

Bridge UX should be:

- attach automatically to a registered worktree
- expose whether the current bridge owns the mutating slot
- reject authoritative mutation on unregistered worktrees with a clear human next action

Registration UX should be:

- automatic worktree discovery for read-only use
- explicit registration before first authoritative use
- a non-interactive command path when args are supplied
- an interactive guided path when args are missing
- the same first-use registration prompt shape in future UI surfaces

## 16. Migration

1. Introduce this canonical model in docs and operator messaging.
2. Mark durable local agent principals and agent credential profiles as deprecated.
3. Add explicit worktree registration and mutation-slot state.
4. Reattach lease continuity to `worktree_id`.
5. Replace normal bridge credential adoption with worktree-slot attachment.
6. Reduce the local credential store to human-owner login state plus encrypted local human secret material.
7. Keep old profile-driven auth flows only as temporary compatibility shims.
8. Add shared-runtime credential metadata and revocation behavior without storing local secrets there.
9. Reserve the future `service` principal path for hosted or remote automation instead of reintroducing durable local agent principals.

## 17. Acceptance Criteria

This design is successful when:

- deleting ordinary local runtime state is not enough to re-bootstrap a human root
- human principals are portable across machines
- each machine still requires explicit local credential acquisition to act as that human
- human local credential material is encrypted at rest using encrypted private-key or equivalent signing-secret storage rather than long-lived plaintext bearer secrets
- agents cannot adopt or read a human credential
- local agents do not require durable local credentials
- authoritative mutations always require a registered worktree context
- worktree identity is the continuity key for ordinary local agent execution
- PRISM enforces at most one active mutating bridge per worktree
- PRISM allows multiple read-only bridges per worktree
- restarting a bridge in the same worktree preserves ordinary task continuity
- PRISM does not model portable cross-machine agent identities
- future hosted or remote automation fits under a separate `service` principal class rather than local agent identity
- direct human mutation is supported through interactive human-authenticated sessions
- direct human mutation still respects the one-mutator-per-worktree rule
- worktree labels are unique per machine and required before authoritative use
- operator-authorized takeover exists for stuck live bridges
- audit trails distinguish human actions from worktree-executor actions
