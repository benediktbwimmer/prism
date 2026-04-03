# PRISM Contracts

> Generated from repo-scoped PRISM contract knowledge.
> Return to the concise entrypoint in `../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:c16a125a8af58eb0b4e611004b010e0deddf0b34ba81ac166c8fab1285bd9b7b`
- Source logical timestamp: `1775229116`
- Source snapshot: `1` concepts, `0` relations, `1` contracts

## Overview

- Active repo contracts: 1
- Active repo concepts: 1
- Active repo relations: 0

## Published Contracts

- `tracked state excludes operational change history` (`contract://tracked_state_excludes_operational_change_history`): Repo-owned tracked `.prism/state` contains durable semantic state and publish metadata, not append-only operational change history.

## tracked state excludes operational change history

Handle: `contract://tracked_state_excludes_operational_change_history`

Repo-owned tracked `.prism/state` contains durable semantic state and publish metadata, not append-only operational change history.

Kind: operational  
Status: active  
Stability: internal

### Subject

Anchors:
- `file:500`
- `file:498`
Concept Handles:
- `concept://tracked_snapshot_semantic_state_and_runtime_change_history`

### Guarantees

- `tracked_prism_state_excludes_durable_changes_shards_and_other_append_only_operational_change_logs`: Tracked `.prism/state` excludes durable `changes` shards and other append-only operational change logs. (scope: repo snapshot authority) [hard]
- `shared_runtime_is_the_architectural_owner_of_append_only_operational_change_history_including_fine_grained_change_journals_between_commits`: Shared runtime is the architectural owner of append-only operational change history, including fine-grained change journals between commits. (scope: runtime authority) [hard]
- `signed_git_commit_history_supplies_the_durable_coarse_grained_repo_change_timeline_at_commit_boundaries_while_the_prism_manifest_adds_structured_publish_metadata_such_as_principal_work_context_continuity_digests_and_publishsummary`: Signed Git commit history supplies the durable coarse-grained repo change timeline at commit boundaries, while the PRISM manifest adds structured publish metadata such as principal, work context, continuity digests, and publishSummary. (scope: publish trust boundary) [hard]

### Consumers

#### Target 1

Concept Handles:
- `concept://tracked_snapshot_semantic_state_and_runtime_change_history`

### Validations

- `docs/PRISM_TRACKED_CHANGES_REMOVAL.md`: Design source for the tracked changes removal boundary.

### Evidence

- The snapshot rewrite already moved tracked `.prism` toward stable snapshot shards and manifest-based publication.
- The tracked changes removal design locks the remaining boundary: current semantic state stays tracked, operational history moves to shared runtime, and commit history replaces tracked change shards.

