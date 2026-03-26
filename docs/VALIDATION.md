# VALIDATION.md

Canonical reference for validating **PRISM** now, while planning ahead for **LEDGER** and **HARBOR**.

This document defines how the family of products should earn trust as **measured world models**, not just useful retrieval systems.

---

## 1. Purpose

PRISM, and later LEDGER and HARBOR, aim to become **authoritative or near-authoritative world models** over important technical domains:

- **PRISM** — code and adjacent docs/config
- **LEDGER** — databases, schema, migrations, and operational migration history
- **HARBOR** — infrastructure, desired/live topology, rollout history, and operational state

These systems become transformative only if users and agents can trust that the world model is **correct enough**, **honest about uncertainty**, and **validated continuously**.

This document exists to define:

- what “correct enough” means
- how correctness is measured
- how each validation layer is tested
- how trust is communicated in product behavior
- what PRISM should implement now
- what LEDGER and HARBOR will need later

---

## 2. Core Thesis

The key product risk is not just incomplete retrieval.

The key risk is **false confidence**.

A world model that is incomplete but honest can still help.  
A world model that is wrong but sounds authoritative can hurt more than it helps.

Therefore, these products must be validated as **measurement systems**.

That means:

- validation is **continuous**, not one-time
- validation is **layered**, not a single aggregate number
- validation uses **authoritative external sources** where possible
- validation includes **historical replay**
- validation distinguishes **authoritative**, **derived**, **inferred**, and **uncertain** outputs
- risky operations require stricter validation gates than exploratory reads

---

## 3. Validation Principles

### 3.1 Layered Validation

Do not ask one vague question like:

> “Is the world model correct?”

Instead, ask:

- Is the **current structure** correct?
- Is **identity through time** correct?
- Are **memories attached to the right things**?
- Are **derived projections** useful and calibrated?
- Are **inferred overlays** precise and clearly labeled?
- Is the system **fresh**, not stale?
- Does the system surface **unknowns** rather than hiding them?

Each layer must be validated separately.

### 3.2 Authoritative Sources First

Whenever possible, validate against lower-level or external truth sources:

- compiler/language tooling
- live DB introspection
- cloud/platform APIs
- Git history
- migration history
- test/build/deploy outcomes

### 3.3 Honesty Over Apparent Coverage

If the system is unsure, ambiguity should be surfaced explicitly.

A graceful “not sure” is better than an incorrect definitive claim.

### 3.4 Conservative Promotion

Inferred overlays, structural memories, and risk summaries must not silently graduate into authoritative truth.

Promotion requires evidence, validation, and often repetition across real tasks.

### 3.5 Replayability

The validation system must be able to replay:

- historical revisions
- historical changes
- historical outcomes
- previous corrections

Replay is the difference between a toy and a trustworthy system.

### 3.6 Continuous Feedback

Humans and agents should be able to report:

- wrong lineage
- missing dependencies
- incorrect memory re-anchoring
- misleading risk summaries
- bogus conflicts or claims

Those reports must feed back into:

- fixtures
- replay tests
- thresholds
- heuristics
- calibration

### 3.7 Dogfooding Feedback First

Before a formal replay harness exists, PRISM should still collect replay-worthy cases during normal use.

For every notable PRISM-assisted task, capture:

- the query or task context
- the anchors involved
- what PRISM said
- what was actually true
- whether the case was `wrong`, `stale`, `noisy`, `helpful`, or `mixed`
- whether the issue belonged to `structural`, `lineage`, `memory`, `projection`, `coordination`, `freshness`, or `other`
- whether the user or agent corrected it manually

This repository now keeps that material in an append-only JSONL log at `.prism/validation_feedback.jsonl`.

Current write paths:

- CLI: `prism feedback record --context ... --prism-said ... --actually-true ... --category projection --verdict wrong --symbol alpha`
- MCP: `prism_mutate { action: "validation_feedback", input: { ... } }`

The first replay and evaluation harness should ingest these real dogfooding entries instead of relying on imagined fixtures alone.

---

## 4. Truth Classes

All outputs in the family should conceptually belong to one of these truth classes.

### 4.1 Authoritative

Definition:
- directly grounded in deterministic source-of-truth inputs
- no probabilistic inference required

Examples:
- parsed node inventory
- compiler-discoverable symbol existence
- live schema introspection
- Kubernetes object returned from cluster API
- explicit outcome event recorded from an executed test or deploy

### 4.2 Derived

Definition:
- computed from authoritative inputs
- deterministic, but not a direct source-of-truth object

Examples:
- call graph assembled from static edges
- co-change summaries from event history
- validation recipes derived from past outcomes
- blast-radius ranking from structure + outcomes

### 4.3 Inferred

Definition:
- agent- or heuristic-generated conclusions not directly provable from deterministic structure
- should carry confidence and clear provenance

Examples:
- likely unresolved callee
- likely related module
- likely hidden dependency
- candidate structural memory

### 4.4 Uncertain

Definition:
- system knows it lacks enough confidence or enough evidence

Examples:
- ambiguous lineage match
- anchor unresolved
- stale revision
- dependency unknown
- insufficient data for risk summary

### 4.5 Product Requirement

The system should expose these distinctions in behavior, diagnostics, and later UI/API surfaces.

A wrong answer is less dangerous if it is clearly labeled **Derived**, **Inferred**, or **Uncertain** rather than masquerading as **Authoritative**.

---

## 5. Validation Layers Across the Product Family

All three products share the same validation stack.

### 5.1 Structural Truth

Question:
- Does the current-snapshot world model correctly represent what exists now?

Examples:
- code symbols, edges, files, docs/config nodes
- schema entities and dependencies
- infra resources and relationships

### 5.2 Temporal Identity / Lineage

Question:
- Does the system correctly identify the same conceptual thing across change?

Examples:
- function rename
- column rename
- deployment replacement
- resource move or reparenting

### 5.3 Memory Anchoring

Question:
- Does memory stay attached to the right anchor as the world changes?

Examples:
- note survives rename
- outcome follows lineage
- memory is dropped when identity truly dies
- no memory “bleeds” into the wrong current node

### 5.4 Derived Projections

Question:
- Are higher-level summaries useful, relevant, and properly calibrated?

Examples:
- blast radius
- co-change neighborhoods
- validation recipes
- hotspots
- blocker summaries

### 5.5 Inferred Overlays

Question:
- Are non-authoritative guesses precise enough to help and clearly marked as non-authoritative?

Examples:
- inferred edges
- inferred risks
- candidate structural memories
- suggested hidden dependencies

### 5.6 Freshness / Staleness

Question:
- Is the system operating on current enough inputs for the action being taken?

Examples:
- graph not rebuilt after edits
- schema drift unobserved
- infra live state out of date
- coordination claims based on stale revision

### 5.7 Coordination Correctness

Question:
- Are claims, blockers, reviews, stale revisions, and handoffs correct and policy-consistent?

Examples:
- edit conflict detected
- stale artifact blocked
- review required before completion
- lease expired correctly

---

## 6. Validation Methods

The family should use four major validation methods.

### 6.1 Golden Fixtures

Small, hand-crafted test worlds with explicitly labeled expected behavior.

Use for:
- parser correctness
- lineage edge cases
- re-anchoring
- coordination policy behavior
- projection basics
- diagnostics and truncation behavior

Benefits:
- easy to understand
- fast in CI
- catches regressions quickly

Limits:
- can be overfit
- may miss real-world messiness

### 6.2 Historical Replay

Replay real changes over time.

Use for:
- lineage correctness
- memory re-anchoring
- co-change projections
- validation recipes
- coordination state under realistic churn

Benefits:
- tests behavior against reality
- essential for time-aware products
- reveals fragility not visible in toy fixtures

Limits:
- slower
- needs curated datasets
- requires expected labels or measurable comparisons

#### Label Generation Strategy

Labeling historical replay datasets should use **agent-based labeling via Codex CLI** (with a configurable model), not manual human labeling.

Approach:

1. for each consecutive revision pair, give the agent both full source snapshots, the diff, and any git rename hints
2. the agent labels each disappeared/appeared symbol pair as rename, move, split, merge, genuinely new, or genuinely dead
3. the agent assigns a self-confidence score to each label
4. high-confidence labels enter the dataset directly
5. low-confidence labels are dropped, not forced into the dataset

Why agent labeling is preferred:

- agents are better than humans at comparing large source files and noticing structural similarity
- agents do not fatigue after labeling hundreds of renames
- agents can process thousands of revision pairs across repos
- dropping low-confidence labels produces a smaller but higher-precision evaluation set, which is more useful than a larger set contaminated by uncertain labels

Monitoring the drop rate:

- the rate of low-confidence labels is itself a signal about the inherent ambiguity of a repo's change patterns
- track the drop rate as a meta-metric
- periodically sample dropped cases to verify PRISM at least flags them as `Ambiguous` rather than overcommitting; this ensures the eval set does not systematically exclude all hard cases

Critical rule:
- do not use PRISM itself to generate labels for its own evaluation; the labeling agent and the system under test must have independent failure modes
- use a general-purpose LLM with access to raw source and diffs, not the PRISM query surface

### 6.3 Differential Validation

Compare the product’s extracted world model to another authoritative or lower-level source.

Use for:
- code graph vs compiler/tooling
- schema graph vs DB introspection
- infra graph vs provider/cluster APIs

Benefits:
- catches systematic extraction errors
- useful for both offline and periodic runtime audits

Limits:
- external source may itself be incomplete for some edges
- requires careful source selection

### 6.4 Shadow Mode / Online Validation

Run the system in observation mode before trusting it to guide important changes.

Use for:
- blast radius
- validation recipes
- coordination blockers
- inferred overlays
- deployment/migration recommendations

Benefits:
- compares predictions to real outcomes
- calibrates trust before enabling stronger automation

Limits:
- slower to gather evidence
- needs instrumentation around real tasks

---

## 7. Validation Pipeline

Every product should eventually implement a dedicated evaluation subsystem.

Suggested conceptual name:

- `world-model-eval`

It does not need to be user-facing at first.

It should support:

- fixture execution
- historical replay
- differential checks
- metric aggregation
- confidence calibration
- error categorization
- regression comparison across commits

---

## 8. PRISM Validation Plan (Implement First)

PRISM is the proving ground. Validation should start here.

### 8.1 Structural Truth for PRISM

Goal:
- verify that PRISM correctly models the current code/document/config structure

Validate:
- node inventory
- containment edges
- calls/imports/implements where statically supported
- language adapter correctness
- unresolved reference capture

Authoritative sources:
- source files
- parser outputs
- compiler/tooling where applicable
- known fixture expectations
- rust-analyzer / cargo metadata / language-aware inventory where available

Suggested checks:
- expected nodes exist
- expected edges exist
- unexpected nodes/edges are absent
- unresolved refs are deterministic and stable

Metrics:
- node precision / recall
- edge precision / recall
- parse failure rate
- unresolved reference rate
- stale graph rate

### 8.2 PRISM Lineage Validation

Goal:
- verify that current nodes are matched correctly across history

Validate:
- rename continuity
- move continuity
- module reshaping
- signature changes
- extractions / inlining where applicable
- ambiguity surfacing

Primary method:
Primary replay dataset:
- the PRISM repo itself; it is sizeable enough to exercise real lineage scenarios, has a clean commit history, and is manageable in size
- additional external repos may be added later for diversity

Label generation:
- use Codex CLI with a configurable model to label lineage events across consecutive revisions
- compare PRISM's deterministic resolver output to the agent-generated labels
- disagreements are the interesting cases: either PRISM got it wrong, the labeling agent got it wrong, or the case is genuinely ambiguous
- PRISM must never be used as its own labeling oracle

Metrics:
- rename continuity accuracy
- move continuity accuracy
- false continuity rate
- ambiguity rate
- incorrect "Born/Died" rate
- incorrect evidence labeling rate

Important rule:
- ambiguous cases should remain ambiguous rather than being overcommitted

### 8.3 PRISM Memory Re-Anchoring Validation

Goal:
- verify that notes and outcomes stay attached to the right code entities over change

Validate:
- node-anchored memory surviving rename/move
- lineage-anchored memory resolving back to correct current node
- dead-node memory invalidation
- no accidental memory transfer to unrelated nodes

Methods:
- synthetic rename/move fixtures
- historical replay with injected memory entries
- mutation / replay tests

Metrics:
- correct re-anchor rate
- wrong re-anchor rate
- dropped-but-should-have-survived rate
- survived-but-should-have-dropped rate

### 8.4 PRISM Projection Validation

Goal:
- verify that derived projections are useful and calibrated

Validate:
- blast radius
- co-change neighbors
- validation recipes
- hotspot summaries

Methods:
- historical replay of real changes and incidents
- top-k relevance judgment
- human evaluation for high-value tasks

Metrics:
- top-k blast-radius recall
- top-k blast-radius precision
- validation recipe usefulness
- hotspot precision
- missed critical dependency rate

### 8.5 PRISM Inferred Overlay Validation

Goal:
- keep inferred edges helpful but bounded

Validate:
- precision of inferred edges
- confidence calibration
- promotion thresholds
- expiry / session scoping
- disagreement with authoritative graph

Metrics:
- inferred-edge precision@k
- calibration error
- false-authority rate
- expired overlay cleanup rate

Product rule:
- inferred overlays should never silently rewrite authoritative structure

### 8.6 PRISM Coordination Validation

Goal:
- verify claims, blockers, stale revisions, review requirements, and artifact state

Validate:
- claim acquisition conflict behavior
- release / renew / expiry
- stale revision detection
- task blockers
- review gating
- artifact status transitions

Methods:
- policy fixtures
- event-sourced replay
- simulated concurrent sessions

Metrics:
- conflict precision
- false-block rate
- missed-block rate
- stale-revision detection rate
- illegal transition rate

### 8.7 PRISM MCP / Query Surface Validation

Goal:
- verify that the programmable query interface remains safe, stable, and repairable

Validate:
- diagnostics correctness
- truncation behavior
- depth limits
- read-only guarantees
- session/task attribution
- resource/API reference consistency

Methods:
- API examples as executable tests
- snippet goldens
- negative tests
- mutation attribution tests

Metrics:
- diagnostic correctness
- example snippet success rate
- serialization failure rate
- read-only boundary violations
- query limit enforcement rate

---

## 9. PRISM Evaluation Harnesses To Build

Suggested test/eval buckets:

```text
tests/
  fixtures/
    prism/
      structure/
      lineage/
      memory/
      coordination/
      query/
  replay/
    prism/
      repos/
      labeled_cases/
  differential/
    prism/
      rust_inventory/
      metadata_comparisons/
```

Suggested categories:

### A. Structure Fixtures
Small repos with expected:
- nodes
- edges
- unresolved refs

### B. Lineage Fixtures
Tiny commit-style sequences that test:
- rename
- move
- split candidate
- merge candidate
- ambiguity

### C. Memory Fixtures
Inject notes/outcomes at revision A and verify anchor behavior at revision B

### D. Coordination Fixtures
Simulate:
- conflicting claims
- stale revision
- review-required task completion
- handoffs and blockers

### E. Query Fixtures
Run representative `prism_query` snippets and verify:
- results
- diagnostics
- truncation
- depth limiting

### F. Real-Repo Replay
Curated internal/external repos where history can be replayed repeatedly

---

## 10. Metrics Dashboard

The validation system should eventually produce a machine-readable scorecard.

Suggested metric groups:

### 10.1 Structural Metrics
- `node_precision`
- `node_recall`
- `edge_precision`
- `edge_recall`
- `parse_failure_rate`
- `unresolved_ref_rate`

### 10.2 Lineage Metrics
- `rename_continuity_accuracy`
- `move_continuity_accuracy`
- `false_continuity_rate`
- `ambiguity_rate`
- `wrong_evidence_label_rate`

### 10.3 Memory Metrics
- `memory_reanchor_accuracy`
- `wrong_memory_reanchor_rate`
- `stale_memory_surface_rate`

### 10.4 Projection Metrics
- `blast_radius_topk_recall`
- `blast_radius_topk_precision`
- `validation_recipe_success_rate`
- `hotspot_precision`

### 10.5 Inference Metrics
- `inferred_edge_precision`
- `inferred_overlay_calibration_error`
- `false_authority_rate`

### 10.6 Coordination Metrics
- `claim_conflict_precision`
- `false_block_rate`
- `missed_block_rate`
- `stale_revision_detection_rate`

### 10.7 Query Runtime Metrics
- `snippet_success_rate`
- `diagnostic_accuracy`
- `limit_enforcement_rate`
- `result_truncation_clarity`

---

## 11. Release Gates and Trust Gates

Not all features need the same accuracy threshold.

### 11.1 Exploratory Read Path

Examples:
- browsing symbols
- reading lineage history
- inspecting notes
- querying plans/claims

Requirement:
- strong usefulness
- moderate tolerance for ambiguity if clearly surfaced

### 11.2 Planning Path

Examples:
- blast radius
- validation recipes
- coordination conflict simulation

Requirement:
- better calibration
- clearer uncertainty
- lower false-confidence tolerance

### 11.3 High-Risk Mutation Guidance

Examples:
- migration suggestions
- production rollout guidance
- promotion of inferred knowledge
- review/blocker decisions

Requirement:
- strongest validation
- stricter evidence requirements
- often human approval
- derived/inferred distinctions must be explicit

Rule:
- the higher the operational risk, the more the system must rely on **Authoritative** and well-validated **Derived** layers rather than weak inference

---

## 12. Human Feedback Loop

Humans should be able to report corrections against the world model.

Required correction categories:

- wrong lineage match
- missing dependency
- wrong memory re-anchor
- misleading blast radius
- bogus blocker/conflict
- stale or outdated graph state

Every correction should ideally produce one or more of:
- new fixture
- replay case
- threshold adjustment
- confidence recalibration
- bug fix
- better uncertainty handling

This loop should be treated as part of the product, not an afterthought.

---

## 13. Prism-First Implementation Roadmap

### Phase 1 — Structural Confidence

Implement first:
- parser fixtures
- graph fixtures
- unresolved-ref determinism checks
- basic metric output

Goal:
- know whether PRISM’s base graph is right

### Phase 2 — Lineage Replay

Implement:
- Git-history replay harness
- rename/move fixture suite
- lineage scorecard
- incorrect evidence-label checks

Goal:
- know whether PRISM’s time model is trustworthy

### Phase 3 — Memory Re-Anchoring

Implement:
- memory anchor replay tests
- synthetic rename/move survival tests
- stale-anchor invalidation checks

Goal:
- know whether memories stay attached to the right things

### Phase 4 — Projection Validation

Implement:
- blast-radius replay evaluation
- validation recipe usefulness checks
- hotspot/co-change evaluation

Goal:
- know whether derived summaries are useful enough to trust in planning

### Phase 5 — Coordination and Query Validation

Implement:
- claim conflict fixtures
- stale revision tests
- API example tests
- diagnostics goldens
- mutation attribution tests

Goal:
- know whether collaboration and programmable queries remain reliable

### Phase 6 — Shadow Mode and Calibration

Implement:
- compare predictions to real tasks
- track false confidence
- tighten promotion rules for inferred overlays

Goal:
- move from “helpful” to “trustworthy”

---

## 14. Planning Ahead for LEDGER

LEDGER does not exist yet, but its validation needs should already shape family design.

### 14.1 What Will Transfer From PRISM

The following validation patterns should transfer directly:
- layered validation
- fixtures
- historical replay
- differential validation
- truth classes
- shadow mode
- release gates
- human correction loops

### 14.2 LEDGER-Specific Validation Needs

LEDGER must validate:

#### Structural Truth
- schema entity extraction
- dependency extraction
- migration state reconstruction

Authoritative sources:
- live schema introspection
- migration tool metadata
- information schema / catalogs
- explicit DDL

#### Lineage
- table rename
- column rename
- type evolution
- constraint/index recreation
- split/merge patterns

#### Memory
- migration notes and outcomes follow schema lineage correctly

#### Projections
- query dependency impact
- migration risk
- likely validations/checks
- downstream breakage

#### Crucial domain-specific pillar
LEDGER must validate not only schema structure, but **workload-aware migration risk**.

That means future evaluation will need to account for:
- lock duration
- query plans
- cardinality
- backfill behavior
- replication lag
- operational timing and environment

Without this, LEDGER may be structurally correct but operationally unsafe.

---

## 15. Planning Ahead for HARBOR

HARBOR also does not exist yet, but its validation needs should already shape family design.

### 15.1 What Will Transfer From PRISM

The following validation patterns should transfer directly:
- layered validation
- historical replay
- differential validation
- coordination tests
- truth classes
- shadow mode
- confidence calibration

### 15.2 HARBOR-Specific Validation Needs

HARBOR must validate:

#### Structural Truth
- declared resource extraction
- topology relationships
- dependency graphs
- environment partitioning

Authoritative sources:
- IaC source
- cluster APIs
- cloud provider APIs
- deployment system APIs
- runtime service discovery where applicable

#### Lineage
- rollout replacements
- apply cycles
- renamed resources
- environment transitions
- state reconciliation

#### Memory
- deploy outcomes, incidents, rollbacks, and drift notes remain attached correctly

#### Projections
- rollout blast radius
- policy impact
- dependency reachability
- validation recipes
- blocker correctness

#### Crucial domain-specific pillar
HARBOR must validate **desired state vs live state reconciliation**.

That means future evaluation will need to account for:
- drift
- hidden live mutations
- rollout replacement
- provider-specific behavior
- partial rollout or stale health state

Without this, HARBOR may be topologically elegant but operationally misleading.

---

## 16. Family-Level “Do Not Cross” Lines

The following mistakes would undermine validation integrity across the family.

### 16.1 Do Not Collapse Truth Classes
Never present inferred or uncertain data as authoritative.

### 16.2 Do Not Use Aggregate Scores Alone
A single “quality score” is not enough.  
Layer-specific metrics must remain visible.

### 16.3 Do Not Overfit to Fixtures
Fixtures are necessary but insufficient.  
Historical replay is mandatory.

### 16.4 Do Not Promote Inference Without Evidence
A repeated inferred pattern may become useful, but promotion must be deliberate and audited.

### 16.5 Do Not Hide Staleness
A stale graph, stale schema, or stale live state must be surfaced clearly.

---

## 17. Minimum Validation Deliverables for PRISM

Before PRISM should be trusted as more than an interesting prototype, it should have:

- structure fixtures
- lineage fixtures
- memory re-anchor fixtures
- at least one real-repo historical replay harness
- machine-readable validation score output
- diagnostics tests for the query runtime
- coordination policy fixtures
- a clear distinction between authoritative, derived, inferred, and uncertain outputs in code/docs

This is the minimum credible validation baseline.

---

## 18. Definition of Done (Validation)

Validation is not “done” permanently, but a capability can be considered validated enough for broader use when:

- its relevant layer metrics are tracked continuously
- it is covered by fixtures and replay where appropriate
- its confidence or truth class is explicit
- its failure modes are known
- regressions are caught automatically
- risky surfaces are gated more strictly than exploratory ones

---

## 19. Final Principle

PRISM, and later LEDGER and HARBOR, should be treated as **measured world models with auditable error bars**, not clever black boxes.

The family only earns trust if it can answer:

- what it knows
- what it derived
- what it inferred
- what it is unsure about
- how often each layer is actually right

That is the standard this document establishes.
