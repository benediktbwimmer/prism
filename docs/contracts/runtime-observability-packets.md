# PRISM Runtime Observability Packets

Status: normative contract  
Audience: runtime, coordination, MCP, CLI, UI, and future service maintainers  
Scope: bounded runtime-local observability packets for live status, interventions, handoffs, and activity summaries

---

## 1. Goal

PRISM must define one explicit contract for **runtime observability packets**.

This contract exists so that:

- runtime-local live status does not become an ad hoc diagnostics dumping ground
- future federated runtime reads and service runtime gateway behavior have stable packet shapes
- observability stays distinct from authority data unless explicitly promoted

## 2. Core invariants

The runtime observability layer must preserve these rules:

1. Observability packets are runtime-local by default.
2. Observability packets are not authority data unless another contract explicitly promotes a
   bounded summary into authority.
3. Packet families must be bounded and typed.
4. Packet identity and freshness must be explicit.

## 3. Packet families

The contract should support at least:

- live task status packets
- intervention packets
- handoff packets
- hint packets
- command-activity summaries
- file-activity summaries

These packet families may evolve, but they must remain explicit rather than being hidden in
miscellaneous diagnostics blobs.

## 4. Required packet metadata

Every packet must include at least:

- packet type
- runtime id
- coordination root identity when relevant
- repo or project scope when relevant
- created-at timestamp
- freshness or expiry metadata when relevant

## 5. Relationship to authority

Some runtime-local observations may later be summarized into authority-backed coordination records.

Examples:

- bounded lease-activity summaries
- handoff checkpoints
- compact runtime descriptors

The promotion boundary must be explicit.

The default rule is:

- packet stays local
- summaries may be promoted when another contract says so

## 6. Relationship to capability and authorization

Runtime observability packet access is governed by
[service-capability-and-authz.md](./service-capability-and-authz.md) and
[authorization-and-capabilities.md](./authorization-and-capabilities.md).

Not every caller allowed to read authoritative coordination state is automatically allowed to read
runtime-local packets.

## 7. Minimum implementation bar

This contract is considered implemented only when:

- runtime-local live packets are named and typed explicitly
- packet freshness is explicit
- packet access is treated as a distinct capability surface
