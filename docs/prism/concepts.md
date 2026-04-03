# PRISM Concepts

> Generated from repo-scoped PRISM concept and relation knowledge.
> Return to the concise entrypoint in `../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:c16a125a8af58eb0b4e611004b010e0deddf0b34ba81ac166c8fab1285bd9b7b`
- Source logical timestamp: `1775229116`
- Source snapshot: `1` concepts, `0` relations, `1` contracts

## Overview

- Active repo concepts: 1
- Active repo relations: 0

- Active repo contracts: 1

## Published Concepts

- `tracked_snapshot_semantic_state_and_runtime_change_history` (`concept://tracked_snapshot_semantic_state_and_runtime_change_history`): Tracked `.prism/state` publishes only current semantic state, shared runtime owns append-only operational change history, and signed Git commit history is the coarse durable change timeline.

## tracked_snapshot_semantic_state_and_runtime_change_history

Handle: `concept://tracked_snapshot_semantic_state_and_runtime_change_history`

Tracked `.prism/state` publishes only current semantic state, shared runtime owns append-only operational change history, and signed Git commit history is the coarse durable change timeline.

Aliases: `tracked changes removal`, `snapshot semantic state`, `runtime change journal boundary`

### Core Members

- `prism_core::tracked_snapshot`
- `prism_core::repo_patch_events`
- `prism_core::memory_events`

### Supporting Members

- `prism_core::published_plans`
- `prism_core::prism_doc::repo_state`

### Evidence

- The tracked changes removal rewrite makes shared runtime the sole append-log owner and removes tracked `.prism/state/changes` from repo authority.
- Signed Git commit history already provides the durable coarse-grained change timeline at commit boundaries.
- Cold clones should load current semantic state from tracked snapshot shards without replaying tracked operational change history.

