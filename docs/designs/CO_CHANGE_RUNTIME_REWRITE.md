# Co-Change Runtime Rewrite

Status: proposed migration baseline
Audience: PRISM runtime, storage, projections, and MCP maintainers
Scope: architectural rewrite of co-change persistence and hydration boundaries

---

## 1. Goal

Rewrite PRISM co-change handling so:

* authoritative runtime state stays small, current, and cheap to hydrate
* bounded serving projections stay fast for MCP queries
* cold analytical evidence can exist without poisoning daemon startup, refresh, or memory use

This rewrite is motivated by a live daemon health failure pattern, not by speculative scaling work.

---

## 2. Current Failure Mode

Today PRISM treats co-change like hot runtime history:

* `crates/prism-projections/src/projections.rs` emits pairwise co-change deltas for every distinct lineage touched in a change set
* `crates/prism-store/src/sqlite/history_io.rs` persists those pairwise rows into `history_co_change`
* `crates/prism-store/src/sqlite/history_io.rs` also reloads the full `history_co_change` table during history hydration
* `crates/prism-history/src/store.rs` stores the hydrated rows in an in-memory `HashMap<(LineageId, LineageId), u32>`

This means the daemon pays for the full historical co-change corpus:

* on refresh write
* on startup reload
* in steady-state memory footprint

That coupling is the architectural bug.

---

## 3. Baseline Evidence

The current live workspace on 2026-03-30 shows the following baseline:

### Runtime

* daemon RSS: about 3297 MB
* daemon elapsed time: about 40 minutes
* daemon health: intermittently unhealthy, but currently healthy

### SQLite size

* `.prism/cache.db`: 2,250,551,296 bytes
* page size: 4096
* page count: 549451
* freelist pages: 213454

The freelist alone accounts for about 874 MB of dead space, so the file is both logically too large and physically bloated.

### Dominant tables

Largest current relations by on-disk bytes:

* `sqlite_autoindex_history_co_change_1`: about 558 MB
* `history_co_change`: about 522 MB
* `history_events`: about 61 MB
* `outcome_event_log`: about 54 MB
* `projection_co_change`: about 28 MB

### Row counts

* `history_co_change`: 6,327,442 rows
* `projection_co_change`: 289,746 rows
* `history_events`: 88,079 rows
* `nodes`: 14,714 rows
* `edges`: 27,765 rows

This asymmetry matters:

* the bounded serving projection is moderate
* the historical pair table is huge

### Pathological live refresh

The daemon log captured a targeted filesystem refresh that touched only 2 files but produced:

* 1,891 lineage events
* 3,491,754 co-change deltas
* 52.9 seconds of SQLite persist time

During that persist window, the background runtime repeatedly failed with `database is locked`.

This is sufficient to explain:

* daemon health degradation
* long response stalls
* cache growth over time
* restart-time slowness after the cache accumulates

---

## 4. Root Cause

The current design mixes three different state classes:

1. authoritative runtime state
2. bounded serving projections
3. cold analytical evidence

Co-change is currently spread across all three without a clean boundary:

* generated eagerly in the hot refresh path
* persisted unbounded as pairwise history
* loaded back into hot runtime memory at startup

That creates a quadratic write pattern for sufficiently broad change sets and an unbounded startup hydration cost.

---

## 5. Required Invariants

The migration must preserve these invariants.

### 5.1 Authoritative state stays authoritative

The following remain hot authoritative runtime state:

* graph
* lineage assignments
* lineage event log
* tombstones
* outcomes
* workspace tree

They must stay transactionally coherent after refresh.

### 5.2 Co-change is derived, not authoritative

Co-change is useful for ranking, impact, and discovery, but it is not required to define the live workspace truth.

Therefore:

* daemon correctness must not depend on hydrating full historical co-change pairs
* refresh correctness must not require persisting full symbol-pair cliques synchronously

### 5.3 Serving state must be bounded

Any co-change state used directly by MCP request paths must remain bounded and cheap to hydrate.

The existing top-K projection behavior is closer to the right serving model than the historical pair table.

### 5.4 Cold evidence must not be loaded wholesale on startup

If PRISM keeps analytical evidence for later derivation, that evidence must stay cold:

* not fully materialized into daemon memory at startup
* not required in full for normal request handling

### 5.5 Large edits must degrade gracefully

Large refreshes, generated files, parser churn, or broad refactors must not explode into quadratic hot-path writes.

When PRISM encounters a bulk change, it should:

* cap work
* coarsen work
* defer work
* or mark derived state as catching up

It should not stall the daemon.

### 5.6 Freshness must be explicit

If derived projections lag authoritative workspace state, PRISM should surface that as freshness state rather than collapsing daemon health into an apparent hard failure.

---

## 6. Target Architecture

The rewrite should separate co-change into three layers.

### 6.1 Authoritative runtime layer

Owns:

* graph
* lineage
* event history
* tombstones
* outcomes
* workspace revisions

This layer is eagerly hydrated and participates in refresh transactions.

### 6.2 Serving projection layer

Owns:

* bounded co-change neighbors used by MCP
* bounded validation and related derived lookup surfaces

This layer is:

* explicitly bounded
* cheap to hydrate
* allowed to be refreshed independently of authoritative history

### 6.3 Cold analytical evidence layer

Owns:

* historical evidence sufficient to derive co-change later

This layer must not be an unbounded hot pair table hydrated into memory on startup.

Possible representations include:

* change-set membership
* file-level or package-level aggregates
* decayed or windowed evidence
* deferred recomputation inputs

The exact representation is a later design task. The invariant is more important than the specific schema.

---

## 7. Immediate Implications For Implementation

The migration should drive these concrete code directions:

* remove `history_co_change` from hot runtime hydration
* stop treating pairwise co-change history as part of `HistoryStore` hot state
* preserve only bounded co-change projection data in request-facing runtime state
* add containment guardrails before the full migration lands so one large refresh cannot poison the daemon
* add migration and compaction support for existing oversized cache DBs

---

## 8. Non-Goals

This rewrite is not trying to:

* delete co-change as a product signal
* rebuild the full refresh architecture from scratch
* make the database the conceptual center of PRISM
* optimize every analytical projection at once

The goal is narrower:

* restore daemon health
* repair state boundaries
* keep PRISM fast and grounded under repeated real-world edits

---

## 9. Planned Follow-Ons

The next implementation phases should proceed in this order:

1. add containment for pathological co-change batches
2. remove cold co-change history from startup hydration
3. introduce the new cold-evidence plus bounded-projection split
4. migrate and compact legacy cache state
5. harden freshness and lock handling around long-running derived work
6. validate the full migration against large existing caches

This ordering matches the active PRISM coordination plan for the rewrite.
