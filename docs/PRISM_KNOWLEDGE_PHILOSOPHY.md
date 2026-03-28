# PRISM Published Knowledge Philosophy

## Why this exists

PRISM is not only a runtime for exploring a repository. It is a system for helping a repository accumulate **earned self-knowledge** over time.

A codebase already contains a large amount of information in its files, structure, and history. PRISM should not duplicate that information blindly. Instead, it should help preserve the things that are **learned while working on the repository** and that would otherwise be lost between sessions, machines, branches, and agents.

This document defines the boundary between:

1. **The repository itself**: files, symbols, configuration, tests, commits, and other raw artifacts.
2. **PRISM runtime state**: local, session, and ephemeral working state used to make the current agent loop efficient.
3. **Published repo knowledge**: durable, reviewable knowledge the repository has earned about itself and should keep.

That third category is the one this philosophy is about.

## Core principle

**Commit learned knowledge, not computed state.**

PRISM should publish back into the repository only the knowledge that clears a high quality bar and is not merely a projection of the raw repository artifacts.

The repository should remember things that were *discovered*, *validated*, or *earned* through work, not things that can be cheaply recomputed from files and history.

## The three layers of truth

### 1. Repository substrate
This is the primary factual substrate:
- source files
- documentation
- tests
- configs
- generated artifacts that are already part of the repo
- git history
- review history where available

These are the raw materials from which PRISM perceives the repository.

### 2. Runtime substrate
This is PRISM's working state:
- session-local memory
- local hypotheses
- temporary worksets
- transient rankings
- handles and caches
- in-progress coordination state
- rebuildable indexes and projections

This state exists to make the current workflow efficient. It is allowed to be fast, mutable, and disposable.

### 3. Published repo knowledge
This is durable learned knowledge that the repository should carry forward:
- high-quality memories
- high-quality concept packs
- durable lessons
- recurring risk notes
- workflow truths that are not obvious from any single file
- validated negative knowledge such as "do not do this" patterns

This knowledge is exported from the state database into versioned JSONL files inside the workspace repository and hydrated back into state on startup.

## Why publish durable knowledge into the repo

Publishing durable repo knowledge into the repository has several benefits.

### Portability
Knowledge no longer lives only inside one local database. It travels with the repository.

### Reviewability
Knowledge becomes visible, diffable, and discussable. It can be reviewed like code.

### Branch-awareness
Knowledge can evolve with feature branches and merge when appropriate.

### Cold-start continuity
A fresh clone or a new agent can start with the repository's accumulated self-knowledge instead of learning everything again from scratch.

### Shared situational awareness
Different agents and future sessions can inherit the same durable understanding of the repository.

## What belongs in published repo knowledge

A piece of knowledge belongs in the repository only if it is all of the following:

- **Durable**: likely to remain useful beyond the current session or task
- **Reusable**: likely to help a future agent or contributor
- **Non-trivial**: not a cheap projection of raw files and history
- **Evidence-backed**: grounded in actual work, outcomes, or repeated use
- **Stable enough**: not likely to rot immediately
- **Safe to publish**: appropriate to commit into the repository

Examples:
- a durable memory about a recurring validation pitfall
- a concept pack describing a repo-native subsystem boundary
- a negative lesson from repeated failed attempts
- a recurring risk note that is not obvious from code structure alone
- a stable handoff-worthy understanding of how a broad repo concept actually resolves in practice

## What does not belong in published repo knowledge

The repository should **not** commit projections or caches that PRISM can reconstruct.

Examples of things that should remain runtime-only:
- symbol graphs
- dependency expansions
- text search indexes
- lineage tables
- temporary worksets
- raw blast-radius outputs
- file excerpts and windows
- session chatter
- local hypotheses
- volatile coordination state
- implementation details of ranking or caching

These may be important for runtime performance, but they are not part of the repository's durable self-knowledge.

## Promotion model

Not all knowledge should immediately become repo knowledge. PRISM should use a promotion ladder.

### Local
Cheap, tentative, local to the current workflow.

### Session
Useful during one session or task, but not yet trusted enough to publish.

### Repo
Durable enough to become part of the repository's published knowledge layer.

Only the **repo** level should be exported into committed JSONL.

## Memories and concepts

### Memories
Memories capture what happened:
- successful fixes
- failures
- validations that mattered
- misleading paths
- durable lessons

### Concept packs
Concept packs capture what belongs together:
- repo-native concepts
- recurring clusters of files, symbols, tests, risks, and outcomes
- durable subsystem meanings that agents repeatedly rediscover

Together, memories and concepts form a powerful knowledge layer:
- memories preserve outcomes
- concepts preserve meaning

## Hydration model

On startup, PRISM should hydrate the repository's published knowledge back into runtime state.

This means PRISM starts from:
- raw repository facts
- repository history
- published durable knowledge
- local/session state as it is rebuilt

This is closer to how an experienced human approaches a codebase: not as a blank slate, but as a system that already remembers some of what working on it has taught us.

## Maintenance philosophy

Published repo knowledge should be maintained **lazily and opportunistically**, not through heavy manual curation.

The preferred pattern is:
1. agents discover useful knowledge while doing real work
2. high-quality knowledge is promoted when it clears the bar
3. PRISM exports repo-level knowledge into committed JSONL
4. PRISM hydrates it on startup
5. stale or superseded knowledge is revised, downgraded, or retired

This keeps the knowledge layer grounded in actual task flow.

## Quality bar

A memory or concept should only become published repo knowledge when it would save future agents from rediscovering something important.

A good test is:

> Would a future agent be meaningfully better off if this knowledge were already present when work begins?

If the answer is no, it likely belongs in runtime state, not in the repository.

## Staleness and supersession

Published knowledge is durable, not immutable.

PRISM should support:
- confidence and quality levels
- staleness signals
- supersession
- retirement or tombstoning
- lineage-aware refresh where appropriate

A stale concept or memory should not silently remain authoritative forever.

## Trust model

Published repo knowledge must remain a trusted layer. To protect that trust:
- the quality bar should stay high
- low-value noise should not be promoted
- published knowledge should be concise and reviewable
- unsafe or overly sensitive content should not be committed
- derived projections should not pollute the layer

The committed knowledge layer should feel special, not like a junk drawer.

## Long-term vision

Over time, this pattern can extend beyond memories and concepts to any form of durable repo self-knowledge that clears the same bar.

The rule remains the same:

- do not commit what PRISM can cheaply recompute
- do commit what the repository has genuinely learned about itself

If this boundary is preserved, PRISM becomes more than a runtime. It becomes a system that helps a repository remember what working on it has taught us.

## Final principle

**A repository should carry its earned self-knowledge with it.**

PRISM exists to help make that possible.
