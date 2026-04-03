# PRISM

> This file is generated from repo-scoped PRISM knowledge. The concise summary lives here,
> while the full generated catalog lives under `docs/prism/`.

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
- Active repo memories: 0
- Published plans: 65
- Full concept catalog: `docs/prism/concepts.md`
- Full relation catalog: `docs/prism/relations.md`
- Full contract catalog: `docs/prism/contracts.md`
- Published memory catalog: `docs/prism/memory.md`
- Published plan catalog: `docs/prism/plans/index.md`

## How to Read This Repo

- Start with this file for the main architecture map and the most central repo concepts.
- Use `docs/prism/concepts.md` when you need the full generated concept encyclopedia.
- Use `docs/prism/relations.md` when you need the typed concept-to-concept graph.
- Use `docs/prism/contracts.md` when you need published guarantees, assumptions, validations, and compatibility guidance.
- Use `docs/prism/memory.md` when you need the current repo-published memory surface.
- Use `docs/prism/plans/index.md` when you need the current published plan catalog and per-plan markdown projections.
- Treat tracked `.prism/state/**` snapshot shards plus `.prism/state/manifest.json` as the current repo-published source of truth; the legacy tracked `.jsonl` streams are migration-era compatibility inputs, and these markdown files are derived artifacts.

## Key Concepts

- `tracked_snapshot_semantic_state_and_runtime_change_history` (`concept://tracked_snapshot_semantic_state_and_runtime_change_history`): Tracked `.prism/state` publishes only current semantic state, shared runtime owns append-only operational change history, and signed Git commit history is the coarse durable change timeline.

## Generated Docs

- `docs/prism/concepts.md`: full concept catalog with members, evidence, and risk hints.
- `docs/prism/relations.md`: full typed relation catalog with evidence and confidence.
- `docs/prism/contracts.md`: full contract catalog with guarantees, assumptions, validations, and compatibility guidance.
- `docs/prism/memory.md`: current repo-published memory entries with anchors, provenance, and trust.
- `docs/prism/plans/index.md`: published plan catalog plus per-plan markdown projections under `docs/prism/plans/`.
