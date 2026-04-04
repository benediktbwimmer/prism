# PRISM Memory

> Generated from repo-scoped PRISM memory events.
> Return to the concise entrypoint in `../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:3aec247c4714030ec95add58a55956e31f90405971205987854898708412c4dd`
- Source logical timestamp: `1775265372`
- Source snapshot: `1 active memories, 1 memory events`

## Overview

- Active repo memories: 1
- Repo memory events logged: 1

## Published Memories

- `memory:01knb0zp726c9zjw7fdhcq1mg3`: Shared coordination startup is currently structurally coupled to authority import: `load_repo_protected_plan_state(...)` builds a coordination snapshot from the journal and immediately calls `load_hydrated_coordination_plan_state(...)`, which prioritizes `load_shared_coordination_ref_state(...)`. That shared-ref loader refreshes the ref, verifies every manifest file digest, and hydrates records file-by-file, and the same path is reused in indexer bootstrap, session recovery, and `workspace_runtime::reload_coordination_snapshot_if_needed`. The correct boundary is shared ref as authority plus replication source, but one local materialized coordination snapshot/checkpoint as the daemon startup artifact keyed by verified shared-ref identity such as head commit or manifest digest.

## memory:01knb0zp726c9zjw7fdhcq1mg3

Kind: episodic  
Source: agent  
Trust: 0.93  
Created at: `1775265372`

Shared coordination startup is currently structurally coupled to authority import: `load_repo_protected_plan_state(...)` builds a coordination snapshot from the journal and immediately calls `load_hydrated_coordination_plan_state(...)`, which prioritizes `load_shared_coordination_ref_state(...)`. That shared-ref loader refreshes the ref, verifies every manifest file digest, and hydrates records file-by-file, and the same path is reused in indexer bootstrap, session recovery, and `workspace_runtime::reload_coordination_snapshot_if_needed`. The correct boundary is shared ref as authority plus replication source, but one local materialized coordination snapshot/checkpoint as the daemon startup artifact keyed by verified shared-ref identity such as head commit or manifest digest.

### Anchors

- `file:177`
- `file:180`
- `file:330`
- `file:421`
- `file:424`
- `file:487`

### Publication

- lastReviewedAt: 1775265372
- publishedAt: 1775265372
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:01knb0zp72wvnr7w2pw9j2h8k5`
- Latest recorded at: `1775265372`
- Event count: `1`

