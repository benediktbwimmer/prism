# PRISM Runtime Identity And Descriptors

Status: normative contract  
Audience: coordination, runtime, MCP, CLI, UI, and future service maintainers  
Scope: runtime identity, runtime descriptor publication, discovery, and authority boundaries

---

## 1. Goal

PRISM must define one stable contract for runtime identity and runtime descriptors.

This contract exists so that:

- runtimes can be identified consistently across local runtime, authority backend, MCP, UI, and a
  future service
- runtime publication and discovery use one authority-backed shape
- runtime-local diagnostics and peer hints do not leak into authority as ad hoc blobs

Canonical ownership:

- this document defines runtime-specific identity fields plus descriptor publication, clearing, and
  discovery behavior
- [identity-model.md](./identity-model.md) defines the broader actor and trust model
- [shared-scope-and-identity.md](./shared-scope-and-identity.md) defines the shared vocabulary

This contract relies on:

- [identity-model.md](./identity-model.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [provenance.md](./provenance.md)
- [signing-and-verification.md](./signing-and-verification.md)

## 2. Core identities

The minimum identity model must include:

- `runtime_id`
  - stable identity for one running runtime instance
- `principal_id`
  - authenticated principal acting through that runtime
- `repo_id`
  - logical repository identity
- `project_id`
  - optional higher-level coordination scope
- `worktree_id`
  - local checkout or worktree identity when applicable

Implementations may add more fields, but these are the minimum stable identities.

## 3. Runtime descriptor purpose

A runtime descriptor is the authoritative published summary that says:

- this runtime exists
- it is bound to this repo or project scope
- it has these declared capabilities
- it was last observed at this time

A runtime descriptor is not:

- a full telemetry stream
- a local diagnostics dump
- a substitute for local runtime state

## 4. Required descriptor fields

The authoritative runtime descriptor contract must include at least:

- `runtime_id`
- `principal_id`
- `repo_id`
- optional `project_id`
- optional `worktree_id`
- runtime kind or role
- capability summary
- liveness or last-seen timestamp
- publication authority metadata

Publication authority metadata should be sufficient to explain who published the descriptor, under
which authority context, and with what verification posture.

Optional fields may include:

- binary or protocol version
- declared feature flags
- local binding hints that are safe to publish

When a descriptor family is designated as signed or verified authority data, the signing boundary is
governed by [signing-and-verification.md](./signing-and-verification.md).

## 5. Capability summary

The runtime descriptor must publish only bounded capability metadata.

Examples:

- may serve runtime-targeted reads
- may host MCP surfaces
- may execute coordination mutations
- may host event execution

The descriptor should not publish large opaque state blobs as "capabilities."

## 6. Authority boundary

Runtime descriptors are coordination authority data.

That means:

- publishing a runtime descriptor is an authoritative coordination write
- clearing a runtime descriptor is an authoritative coordination write
- descriptor discovery reads go through the coordination authority store

It does not mean:

- every runtime-local packet or diagnostic becomes authority data

Local diagnostics, hot telemetry, command traces, and intervention packets remain runtime-local
unless another explicit contract says otherwise.

## 7. Discovery rules

Discovery surfaces must be able to answer:

- what runtimes are currently visible for this coordination root
- what repo or project scope each runtime is bound to
- what capabilities each runtime declares
- how fresh that discovery information is

Discovery answers must carry the shared consistency envelope from
[consistency-and-freshness.md](./consistency-and-freshness.md).

## 8. Relationship to local bindings

Runtime descriptors may include publishable binding hints, but machine-local path bindings remain
local operational state.

Examples:

- `repo_id -> local checkout path` is local
- `runtime_id is bound to repo_id X and worktree_id Y` may be publishable

This distinction matters especially for future cross-repo coordination.

## 9. Relationship to the authority backend

Runtime descriptor semantics must not depend on whether the active authority backend is Git shared
refs or a future PostgreSQL backend.

The backend may store descriptors differently.
The meaning of publication, clearing, discovery, and freshness must remain the same.

## 10. Minimum implementation bar

This contract is considered implemented only when:

- runtimes have stable ids and scope-qualified descriptors
- authoritative runtime publication and clearing go through the authority store
- discovery surfaces do not bypass the authority store for canonical runtime visibility
