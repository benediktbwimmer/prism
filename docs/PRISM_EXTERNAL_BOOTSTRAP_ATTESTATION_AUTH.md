# PRISM External Bootstrap Attestation Auth

Status: planning design note  
Audience: PRISM auth, MCP, runtime, and deployment maintainers  
Scope: human-root bootstrap, delegated identity issuance, and recovery without local interactive auth

See also:
- [PRINCIPAL_IDENTITY_AND_COORDINATION.md](./PRINCIPAL_IDENTITY_AND_COORDINATION.md)
- [SHARED_IDENTITY_FUTURE_STATE.md](./SHARED_IDENTITY_FUTURE_STATE.md)

---

## 1. Summary

PRISM should stop treating "the principal registry is empty" as the root bootstrap
invariant.

Instead, PRISM should allow human-root identity creation only when the caller
presents a valid **bootstrap attestation** signed by a configured external
issuer.

This keeps PRISM out of the business of implementing its own password, biometric,
or browser login UX while still giving PRISM a stronger trust boundary than the
current local-file bootstrap.

The target model is:

- human-root creation requires an externally issued bootstrap attestation
- PRISM verifies the attestation and mints a local human root principal
- the human principal identity is portable across machines
- each machine receives its own local credential material to act as that human principal
- agents never mint new human roots
- agents may mint child agent principals when delegated that capability
- headless/server runtimes never bootstrap roots directly
- servers import delegated identities or credentials derived from a human root

This makes human bootstrap an explicit trust ceremony without requiring PRISM to
ship its own interactive authentication system.

---

## 2. Problem

Today, the first `prism auth init` on a machine can mint a human owner principal
because the local principal registry is empty.

That is too weak for the long-term model.

If an agent has sufficient local authority to destroy or replace the registry
state, it can potentially make PRISM believe the machine is "fresh" and repeat
bootstrap.

The real requirement is:

- a human, and only a human, should be able to establish the first trust root on
  a machine
- all later agent lineage should trace back to one or more human roots
- deleting local runtime state must not silently enable re-bootstrap

PRISM should therefore stop using local emptiness as the trust root and instead
verify a signed bootstrap proof from outside the normal agent/runtime trust
boundary.

---

## 3. Design Goals

- Do not build password, biometric, or browser login UX into PRISM itself.
- Support strong human-root bootstrap on macOS, Linux, and Windows.
- Allow offline or self-hosted issuers later, but do not require them for v1.
- Make server and CI deployments work without server-side root bootstrap.
- Preserve the existing principal and child-principal model where possible.
- Ensure every agent lineage can terminate at a human root principal.
- Keep bootstrap attestations explicit, inspectable, and auditable.

---

## 4. Non-Goals

- Proving humanness in a philosophical or adversarially perfect sense.
- Preventing compromise when an attacker already has unrestricted machine-level
  control over the user's full environment.
- Making PRISM itself into a general identity provider.
- Requiring a hosted PRISM auth service for every deployment.

---

## 5. Core Model

PRISM should distinguish four layers:

- **Bootstrap issuer**
  - external trust authority that can authenticate a human and sign a bootstrap
    attestation
- **Human root principal**
  - first-class PRISM principal representing a human trust root for a machine or
    authority namespace
- **Delegated principal**
  - agent or other non-human principal minted under an existing principal chain
- **Credential**
  - usable bearer token or key material that lets a process act as a principal

The key rule is:

- only a bootstrap issuer may authorize minting a brand-new human root
- later agent identity minting stays inside PRISM's normal authenticated mutation
  model

Human identity should be split from local credential material:

- the human principal id is durable and portable across machines
- each machine acquires its own credential material to act as that principal
- losing one machine credential must not imply losing or recreating the human principal itself

---

## 6. Bootstrap Attestation

### 6.1 What it is

A bootstrap attestation is a signed statement saying, in effect:

> a trusted issuer authenticated a human and authorizes this PRISM machine or
> authority to mint a specific human root principal

PRISM does not care how the issuer authenticated the human.

That could be:

- GitHub Device Flow
- passkey / WebAuthn
- OIDC / SSO login
- an SSH or GPG signing workflow
- an enterprise signing service
- a future self-hosted issuer

PRISM only needs to verify the signed attestation against configured trusted
issuers.

### 6.2 Minimum attestation contents

At minimum, a bootstrap attestation should include:

- `issuer_id`
- `issuer_key_id`
- `subject_type = human_root_bootstrap`
- `authority_id`
- `machine_id` or equivalent machine binding
- `principal_name`
- optional human profile metadata
- `assurance_level`
- `issued_at`
- optional `expires_at`
- optional nonce or one-time token
- signature over the full payload

Optional but useful fields:

- `allowed_root_kinds`
- `repo_scope` for repo-bound bootstrap policies
- `require_interactive_recovery = true`
- human-readable issuer identity details for audit

### 6.3 Assurance levels

Bootstrap attestations should carry an explicit assurance label so PRISM can be
honest about how strongly it believes a human, rather than an agent, performed
the bootstrap ceremony.

Recommended v1 levels:

- `high`
  - strong evidence of human-interactive approval outside ordinary agent reach
  - intended default for normal developer bootstrap
  - example: GitHub Device Flow completed through the user's authenticated web
    session with GitHub's own MFA/2FA policies
- `moderate`
  - cryptographically attributable, but without a strong human-presence
    guarantee
  - acceptable as an offline or self-hosted fallback
  - example: SSH signature or GPG signature where PRISM can verify the key but
    cannot know whether the signing action was performed by a human or by a
    sufficiently privileged local agent
- `legacy`
  - migrated from the old local bootstrap model
  - retained for honesty and provenance continuity, not as a preferred mode

PRISM should record the bootstrap assurance level durably on the human root
principal or its bootstrap provenance so later audit, policy, and delegation
logic can take it into account.

### 6.4 Verification rules

PRISM should accept bootstrap only if:

- the issuer is trusted locally
- the signature is valid
- the attestation has not expired
- the attestation is bound to the expected machine or authority
- the attestation has not already been consumed, if single-use semantics apply
- no incompatible existing bootstrap authority is already present
- the declared `assurance_level` is permitted by local policy for the requested
  bootstrap action

---

## 7. Trust Boundary

This design intentionally moves the sensitive "prove a human was present" step
outside PRISM.

PRISM is responsible for:

- attestation format
- issuer trust policy
- verification
- local minting and lineage rules
- recovery and revocation semantics

PRISM is not responsible for:

- password prompts
- biometric UX
- browser login pages
- MFA flows
- third-party account management

This keeps PRISM focused on repo/runtime trust semantics instead of reimplementing
interactive auth poorly.

---

## 8. Bootstrap Flow

### 8.1 Interactive machine bootstrap

The preferred human-root bootstrap flow is:

1. User runs `prism auth bootstrap` or equivalent.
2. PRISM starts an external attestation flow.
3. The human completes interactive authentication with the external issuer.
4. PRISM receives a signed bootstrap attestation.
5. PRISM verifies it.
6. PRISM mints a new local human root principal and local credential.
7. PRISM records bootstrap provenance in the principal registry and audit history.

If the attested human principal already exists, bootstrap on a second machine
should mint or import a new machine-local credential for that same principal
rather than inventing a second human root principal.

For v1, the preferred default external flow should be GitHub Device Flow. It
gives PRISM a strong human-approval boundary without requiring PRISM to build
its own interactive auth stack.

### 8.2 Important invariant

The machine should no longer treat "empty principal registry" as permission to
bootstrap.

Instead:

- bootstrap is allowed only when no prior bootstrap authority exists
- if the registry is missing but bootstrap authority history already exists,
  PRISM must enter **recovery**, not fresh bootstrap

That distinction is the core hardening improvement over the current model.

---

## 9. Recovery

If local principal registry state is lost, corrupted, or partially wiped, PRISM
must not silently allow `auth init` again.

Instead, PRISM should require a recovery ceremony.

Recovery should work like this:

1. PRISM detects that bootstrap authority metadata exists but the registry is not
   usable.
2. PRISM refuses fresh bootstrap.
3. User obtains a recovery attestation from a trusted external issuer.
4. PRISM verifies the attestation.
5. PRISM reconstructs or reauthorizes the local human root state.

Recovery attestations should be distinct from bootstrap attestations so the audit
trail remains clear.

---

## 10. Principal Minting Policy

### 10.1 Human roots

Brand-new parentless human roots should be created only via verified bootstrap or
verified recovery.

Normal agent credentials must never be able to mint a new human root.

Human/admin principals may optionally be allowed to mint additional human roots
later, but only if policy explicitly permits it.

### 10.2 Agents

Agents should mint only through the normal authenticated mutation path.

The intended rule set is:

- human roots may mint child agents
- agents with delegated capability may mint child agents recursively
- every child chain must preserve provenance back to a human root
- parent-child relationships are immutable once minted

### 10.3 Portable human identity

Human principals should be portable across machines.

That means:

- the same human should keep one durable principal identity across laptop,
  desktop, and other trusted machines
- each machine should hold distinct local credential material for acting as that
  principal
- credential rotation or loss on one machine should not require minting a new
  human principal
- audit and coordination history should therefore converge on one stable human
  principal id instead of fragmenting by machine

### 10.4 Current-code alignment target

This implies tightening current behavior:

- `admin_principals` or `all` should not automatically imply unrestricted
  parentless principal minting forever
- parentless minting should become an explicit human/admin ceremony or disappear
  outside bootstrap/recovery
- `mint_child_principal` should remain the normal agent delegation capability

---

## 11. Server And CI Model

Servers and headless environments should not bootstrap human roots at all.

The preferred model is:

- interactive machine performs human-root bootstrap
- that trusted machine issues a delegated identity or credential for the server
- server imports the delegated identity
- server may act only within delegated capabilities
- server may mint child agents if explicitly allowed
- server may not create a new human root or re-bootstrap itself

If server state is lost:

- the server does not run fresh bootstrap
- a trusted interactive machine must re-delegate credentials or identities

This is simpler and stronger than trying to make headless root bootstrap safe.

---

## 12. Local Storage

PRISM should keep the current split conceptually, even if the exact file layout
changes later:

- shared/local runtime store holds principal registry metadata
- local machine store holds usable secret credential material for acting as a
  portable principal
- trusted issuer config and consumed bootstrap records live in a local trust store

The important invariant is:

- destroying the ordinary principal registry must not be enough to re-bootstrap

That means bootstrap/recovery authority state must be stored and checked
separately from the ordinary mutable registry snapshot.

---

## 13. Why a Plain Local Helper CLI Is Not Enough

A local helper CLI that simply reads a signing key from disk does not create a
real human-only boundary.

If an agent can execute arbitrary local commands and access the helper's key
material, it can invoke the helper too.

Therefore:

- a helper CLI is acceptable only as a transport or convenience layer
- the real trust boundary must come from an external signer or external
  interactive ceremony

This is why the preferred model uses external bootstrap attestations rather than
"a second local binary that signs for me."

---

## 14. Issuer Models

PRISM should support multiple issuer models over time, all through the same
attestation-verification abstraction.

Possible issuers include:

- GitHub Device Flow or equivalent GitHub-backed attestation
- hosted PRISM-compatible identity service
- enterprise SSO / OIDC issuer
- WebAuthn/passkey-backed browser signer
- SSH/GPG-backed signer
- self-hosted signer for offline or controlled deployments

The critical requirement is not the specific issuer type.

The critical requirement is that:

- the issuer is trusted
- the attestation is signed
- the ceremony is outside ordinary agent execution context

Recommended v1 posture:

- prefer GitHub Device Flow as the default `high` assurance bootstrap path
- allow SSH/GPG signing as a `moderate` assurance fallback
- surface that assurance level visibly in auth inspection and provenance reads

---

## 15. Proposed CLI Shape

PRISM should eventually expose auth flows in terms of attestations, not raw local
bootstrap state.

Representative commands could look like:

- `prism auth bootstrap`
- `prism auth bootstrap --issuer <issuer>`
- `prism auth recover --attestation <path>`
- `prism auth import-delegation <path>`
- `prism principal mint --kind agent --name <name> --parent <principal>`

And possibly inspection helpers:

- `prism auth show-bootstrap`
- `prism auth whoami`
- `prism auth trust list-issuers`

The exact transport details may vary by issuer:

- browser callback
- pasted token
- local file bundle
- device code flow

PRISM should normalize all of them into the same verified-attestation step.

---

## 16. Migration From Current Bootstrap

The current local-only bootstrap behavior should be treated as legacy.

Migration should likely proceed in phases:

1. Add explicit bootstrap authority metadata and refuse repeat bootstrap when it
   exists.
2. Introduce attestation verification alongside current local bootstrap.
3. Mark local-empty-registry bootstrap as deprecated.
4. Require attestation-based bootstrap for new installs.
5. Add recovery flow and server delegation flow.

Legacy roots may need a one-time migration that records:

- machine authority id
- migrated root principal id
- migration timestamp
- migration assurance level

So later code can distinguish:

- externally attested roots
- migrated legacy roots

---

## 17. Open Questions

- Should bootstrap attestations be strictly single-use?
- Should recovery require the same issuer class as bootstrap, or may policy allow
  a different trusted issuer?
- Should additional human roots after bootstrap require a new external
  attestation, or is an existing human root sufficient?
- How much issuer identity detail should be surfaced in provenance and MCP reads?

---

## 18. Acceptance Criteria

This design is successful when:

- deleting ordinary local runtime state is not enough to enable fresh bootstrap
- agents cannot silently mint new human roots through ordinary local execution
- human-root bootstrap occurs only through verified external attestations
- bootstrap provenance records an explicit assurance level such as `high`,
  `moderate`, or `legacy`
- servers never bootstrap their own human roots
- delegated agent lineages remain traceable back to one or more human roots
- PRISM does not need to own password, biometric, or browser auth UX itself
