# PRISM Auth And Identity Audit

Status: historical baseline audit captured before the auth-and-identity cutover
Audience: PRISM auth, coordination, MCP, runtime, storage, and operator UX maintainers
Scope: current codebase audit against the canonical target model in [PRISM_AUTH_AND_IDENTITY_MODEL.md](PRISM_AUTH_AND_IDENTITY_MODEL.md)

This document maps the pre-cutover implementation to the target model and identifies the concrete gaps that had to be closed for the end-to-end auth and identity redesign.

It is retained as baseline context. The current target and operator-facing contract live in [PRISM_AUTH_AND_IDENTITY_MODEL.md](PRISM_AUTH_AND_IDENTITY_MODEL.md).

## 1. Executive Summary

The current implementation is still built around durable local principals and reusable local bearer-style credentials.

The largest mismatches with the canonical model are:

- local human credentials are stored in plaintext in the repo-local credentials file
- local agent principals and local agent credentials are first-class supported
- bridge adoption works by reading a stored local credential and injecting it into `prism_mutate`
- worktree identity is path-derived, not registration-backed
- there is no registered worktree record with unique label and `human|agent` mode
- there is no enforced one-mutating-bridge slot per worktree
- direct human mutation uses the same local credential path as agent mutation
- runtime descriptors and audit still center durable principals more than worktree execution identity

In other words, the current system implements the old model well, but it is structurally misaligned with the new one.

## 2. Canonical Target

The target model is defined in [PRISM_AUTH_AND_IDENTITY_MODEL.md](PRISM_AUTH_AND_IDENTITY_MODEL.md).

The key target rules that drive this audit are:

- `human` is the only durable authenticated local actor class in v1
- `service` is reserved as a future durable remote actor class
- local `agent` identity is not durable and is replaced by worktree execution identity
- all authoritative mutations require a registered worktree
- worktree records have unique labels and `human|agent` mode
- one active mutating bridge slot is enforced per worktree
- human credentials are protected from agent adoption and plaintext reuse
- direct human mutation is allowed only through a short-lived human session in a registered worktree

## 3. Current Implementation

### 3.1 Principal registry

The principal registry is implemented in [principal_registry.rs](../crates/prism-core/src/principal_registry.rs).

Current behavior:

- `bootstrap_owner_principal(...)` creates a local `Human` principal directly in the local registry
- owner bootstrap defaults to local authority id `local-daemon`
- `mint_principal_credential(...)` explicitly supports minting child principals
- `MintChildPrincipal` is scoped to minting `Agent` principals
- authentication is based on `credential_id + principal_token`

Implication:

- the code assumes durable local human and agent principals already exist
- the trust root is local bootstrap, not external attestation-backed portable human identity

### 3.2 Local credentials file

The local credential store is implemented in [local_credentials.rs](../crates/prism-core/src/local_credentials.rs).

Current behavior:

- `CredentialsFile` is version `1`
- profiles are stored in plaintext TOML
- each `CredentialProfile` stores:
  - `authority_id`
  - `principal_id`
  - `credential_id`
  - `principal_token`
- `find_by_selector(...)` resolves the active or requested stored credential directly

Implication:

- the local credential file is not metadata-only
- bearer-style reusable secret material is readable by any local process with file access
- there is no separation between human-stored credentials and agent-usable credentials

### 3.3 CLI auth flow

The CLI flow is implemented in [auth_commands.rs](../crates/prism-cli/src/auth_commands.rs).

Current behavior:

- `prism auth init` bootstraps a local owner principal and stores the issued token locally
- `prism auth login` just selects an already stored local credential and verifies it
- `prism principal mint` mints durable child principals, including agent principals, and stores them locally
- CLI output prints `principal_token`

Implication:

- bootstrap is local, not attested
- login is profile selection, not interactive human authentication
- agent credential minting and storage are part of the normal user flow

### 3.4 Worktree identity

Worktree identity is implemented in [workspace_identity.rs](../crates/prism-core/src/workspace_identity.rs).

Current behavior:

- `repo_id` is derived from git common dir or canonical root
- `worktree_id` is derived from the canonical root path via `scoped_id("worktree", ...)`
- `instance_id` is process-local and ephemeral
- `branch_ref` is discovered from `.git/HEAD`

Implication:

- `worktree_id` is path-derived, not registration-backed
- moving a worktree changes its identity
- there is no explicit registration ceremony or durable worktree record

### 3.5 Worktree principal binding

The current per-worktree identity gate is implemented in [worktree_principal.rs](../crates/prism-core/src/worktree_principal.rs).

Current behavior:

- the first authenticated principal to mutate a workspace binds that worktree session in memory
- later authenticated mutations must use the same principal or they fail with `mutation_worktree_principal_conflict`
- the binding is stored as `authority_id + principal_id + principal_name`
- the binding is attached to `WorkspaceSession`, not to a registered durable worktree record

Implication:

- the current guard enforces "one principal per loaded workspace session"
- it does not implement the target model of "one mutating bridge slot per worktree"
- it is principal-centric, not worktree-execution-centric
- it is in-memory and not an explicit operator-managed worktree registration model

### 3.6 Bridge adoption

The bridge adoption path is implemented in [bridge_auth.rs](../crates/prism-mcp/src/bridge_auth.rs).

Current behavior:

- `prism_bridge_adopt` loads the local credentials file
- it selects a stored profile by `profile`, `principalId`, or `credentialId`
- it binds the bridge to that stored credential
- later `prism_mutate` calls may omit `credential`
- the bridge injects the stored `credentialId + principalToken`

Implication:

- bridges can directly adopt durable local credentials
- the local bridge identity model is still "principal continuity via stored credential"
- this is the exact behavior the canonical model wants to remove for human credentials

### 3.7 UI operator identity and mutation

The UI/operator console identity flow is implemented in [ui_identity.rs](../crates/prism-mcp/src/ui_identity.rs) and [ui_mutations.rs](../crates/prism-mcp/src/ui_mutations.rs).

Current behavior:

- the UI resolves the active local stored profile from `CredentialsFile`
- the mutate endpoint injects that profile's `credentialId + principalToken`
- the UI reports conflicts against the current bound worktree principal
- operator console mutation is still just local credential reuse

Implication:

- direct human mutation is not a distinct short-lived human session
- the UI path reuses the same bearer-style local credential model as agents
- there is no human-only unlock barrier

### 3.8 Provenance and event execution context

Mutation and observed-change provenance currently include worktree and session context in [session.rs](../crates/prism-core/src/session.rs) and [watch.rs](../crates/prism-core/src/watch.rs).

Current behavior:

- `EventExecutionContext` includes:
  - `repo_id`
  - `worktree_id`
  - `branch_ref`
  - `session_id`
  - `instance_id`
  - `credential_id`
- observed file changes use the currently bound worktree principal as actor when available

Implication:

- provenance already has useful worktree/session context
- but actor identity is still fundamentally principal-centric
- the model does not yet distinguish:
  - human session in a human worktree
  - worktree execution identity for local agent work
  - future service principals

### 3.9 Runtime descriptors

Runtime descriptors currently live in coordination state in [types.rs](../crates/prism-coordination/src/types.rs).

Current behavior:

- `RuntimeDescriptor` stores:
  - `runtime_id`
  - `repo_id`
  - `worktree_id`
  - `principal_id`
  - `branch_ref`
  - capability and endpoint fields

Implication:

- live runtime identity is still modeled as principal-bearing
- there is no worktree registration label or mode
- there is no explicit mutating-slot ownership model in runtime descriptors

### 3.10 Leases, claims, and handoffs

Coordination task and claim continuity lives primarily in [mutations.rs](../crates/prism-coordination/src/mutations.rs).

Current behavior:

- tasks and claims carry `assignee`, `session`, `worktree_id`, `branch_ref`, and lease metadata
- reclaim/resume/handoff logic uses lease-holder and stale/expired semantics
- continuity is partly session-based and partly task-assignee-based

Implication:

- the coordination layer already has the right raw fields to move toward worktree continuity
- but the ownership model is still mixed:
  - principal/agent identity
  - session continuity
  - worktree context
- there is no explicit worktree-level mutator slot or human-approved takeover flow

## 4. Gap Analysis Against The Target Model

### 4.1 Portable human identity is missing

Target:

- durable human principal should be portable across machines
- bootstrap and recovery should be backed by an external attestation issuer

Current gap:

- bootstrap is local-only
- authority defaults to `local-daemon`
- there is no external attestation flow or portable recovery model

### 4.2 Local human credential protection is missing

Target:

- human credential material must be encrypted at rest
- agents must not be able to adopt human credentials

Current gap:

- `principal_token` is stored in plaintext in `credentials.toml`
- CLI and MCP both read that token directly
- bridges and UI can use stored credentials without any human unlock flow

### 4.3 Durable local agent principals still exist

Target:

- local agent identity should not be durable
- worktree execution identity should replace local agent credentials

Current gap:

- agent principals are first-class mintable principals
- agent credentials are persisted locally
- bridge adoption is built around them

### 4.4 Worktree registration does not exist

Target:

- worktrees must be registered before authoritative mutation
- registration assigns:
  - opaque registration-time `worktree_id`
  - unique human-readable label
  - `human|agent` mode

Current gap:

- no worktree record exists
- no worktree registration command exists
- `worktree_id` is path-derived
- there is no unique label or mode

### 4.5 One mutating bridge slot per worktree is not enforced

Target:

- multiple read-only bridges allowed
- exactly one mutating bridge slot per worktree

Current gap:

- current enforcement is an in-memory principal binding on one `WorkspaceSession`
- there is no durable worktree slot record
- there is no explicit mutating-bridge acquisition or takeover protocol

### 4.6 Human direct mutation is not a distinct path

Target:

- humans can mutate directly only through a short-lived human session in a registered worktree

Current gap:

- UI and CLI mutate by directly reusing stored local credentials
- there is no explicit human unlock/session lifecycle
- there is no distinction between:
  - stored human credential material
  - unlocked short-lived human session

### 4.7 Runtime and audit semantics are still principal-centric

Target:

- local agent execution should be attributed primarily to worktree execution identity
- human and future service principals should remain durable principal actors

Current gap:

- runtime descriptors store `principal_id`
- worktree labels and modes do not exist
- bridge continuity is tied to stored principal credentials rather than worktree slot ownership

## 5. Recommended Implementation Tracks

### 5.1 Track A: Introduce new auth model primitives without cutover

Add:

- portable human principal metadata model
- encrypted local human credential format
- short-lived human session abstraction
- worktree registration records
- registration-time generated opaque `worktree_id`
- unique worktree label and `human|agent` mode

Do not cut over old bridge adoption yet.

### 5.2 Track B: Introduce worktree mutator slots

Add:

- explicit acquire/release/takeover model for one mutating bridge slot per worktree
- multiple read-only bridge allowance
- human-authorized takeover flow for stuck live bridges

This is the core correctness boundary for the new model.

### 5.3 Track C: Cut human mutation to human sessions

Change:

- UI mutate flow
- CLI direct human mutation flow

So that:

- authoritative mutation requires a registered worktree
- human mutation requires an unlocked human session
- stored human credential material is no longer directly reusable by agent-style code paths

### 5.4 Track D: Cut bridge mutation to worktree execution identity

Replace:

- `prism_bridge_adopt` binding to stored credential profiles

With:

- bridge attachment to a registered worktree execution slot
- slot-scoped session identity
- optional human-readable worktree label in UI and diagnostics

### 5.5 Track E: Remove durable local agent credentials

Deprecate and remove:

- local agent principal minting as the ordinary local execution path
- bridge credential injection from `CredentialsFile`
- principal-token-based local agent continuity assumptions

Reserve durable non-human principals for future `service` actors only.

### 5.6 Track F: Rekey provenance and runtime descriptors

Update:

- event actor attribution
- event execution context
- runtime descriptors
- UI/session bootstrap identity views
- coordination lease ownership semantics

So that local agent work is centered on:

- machine human owner
- registered worktree
- worktree label
- live session id

## 6. Suggested Task Breakdown For The Existing Plan

The current coordination plan should treat this audit as the source of truth for follow-on implementation tasks.

Recommended order:

1. Add worktree registration records, labels, and modes.
2. Switch `worktree_id` from path-derived identity to registration-backed identity.
3. Introduce human session abstraction and encrypted local human credential storage.
4. Refactor UI and CLI human mutation flows to require human sessions.
5. Add worktree mutator-slot acquisition and takeover.
6. Replace bridge credential adoption with worktree-slot attachment.
7. Deprecate durable local agent credentials and minting flows.
8. Rekey runtime descriptors, provenance, and lease ownership semantics.

## 7. Immediate Code Hotspots

The highest-leverage code hotspots for the redesign are:

- [principal_registry.rs](../crates/prism-core/src/principal_registry.rs)
- [local_credentials.rs](../crates/prism-core/src/local_credentials.rs)
- [auth_commands.rs](../crates/prism-cli/src/auth_commands.rs)
- [workspace_identity.rs](../crates/prism-core/src/workspace_identity.rs)
- [worktree_principal.rs](../crates/prism-core/src/worktree_principal.rs)
- [bridge_auth.rs](../crates/prism-mcp/src/bridge_auth.rs)
- [ui_identity.rs](../crates/prism-mcp/src/ui_identity.rs)
- [ui_mutations.rs](../crates/prism-mcp/src/ui_mutations.rs)
- [server_surface.rs](../crates/prism-mcp/src/server_surface.rs)
- [types.rs](../crates/prism-coordination/src/types.rs)
- [mutations.rs](../crates/prism-coordination/src/mutations.rs)

## 8. Conclusion

The target model is implementable without rewriting the entire coordination stack, but it does require replacing the identity assumptions that currently sit underneath CLI auth, bridge auth, UI mutation, and worktree binding.

The coordination layer already carries enough worktree and lease metadata to support the new design. The largest required shifts are:

- move from plaintext stored reusable tokens to protected human credential material plus short-lived human sessions
- move from durable local agent principals to registered worktree execution identity
- move from in-memory principal binding to enforced worktree mutator slots
- move from path-derived worktree identity to registration-backed worktree records
