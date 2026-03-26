# PRISM · LEDGER · HARBOR

Canonical reference for the future product family, shared substrate, and eventual transition into an umbrella monorepo.

---

## 1. Purpose

This document defines the high-level shape of the product family currently envisioned as:

- **PRISM** — code cognition
- **LEDGER** — database cognition
- **HARBOR** — infrastructure cognition

It exists to guide:

- the continued evolution of **PRISM**
- the inception of **LEDGER** and **HARBOR**
- the eventual transition from a PRISM-only repository into a family-level umbrella monorepo

This is intentionally a **high-level architecture and product reference**, not a low-level implementation spec.

---

## 2. Executive Summary

The family thesis is:

> Agents do not just need better retrieval.  
> They need a persistent, evidence-backed world model of the systems they work on.

Across code, databases, and infrastructure, the same failures recur:

- weak structural understanding
- weak temporal continuity across change
- poor recall of prior outcomes
- weak blast-radius reasoning
- poor coordination across parallel workers or sessions

PRISM is the proving ground for a pattern that can generalize:

1. **Structure** tells the system what exists now
2. **History** tells it what persisted through change
3. **Memory** tells it what happened when it changed
4. **Coordination** tells it who is doing what, on which anchors, under which policy
5. **Programmable query surfaces** let agents compose all of the above in one round-trip

That pattern appears reusable across at least three domains:

- code
- relational / analytical databases
- infrastructure / deployment environments

The family strategy should be:

- **ship PRISM first**
- let the architecture settle
- use PRISM to help build the second product
- extract shared substrate only when a second domain creates real pressure
- create an umbrella monorepo only when **LEDGER** starts for real

---

## 3. Product Family Vision

### 3.1 PRISM

**PRISM** is the code cognition system.

It builds an authoritative structural graph over code and adjacent docs/config, tracks identity through change, remembers outcomes attached to code anchors, and exposes a programmable query surface for agents and humans.

Core value:

- "What exists here?"
- "What changed?"
- "What usually breaks when this changes?"
- "What happened last time we touched this?"
- "Who is already working in this area?"

### 3.2 LEDGER

**LEDGER** is the database cognition system.

It models schema, migrations, query dependencies, workload-sensitive risk, and operational outcomes associated with schema evolution.

Core value:

- "What depends on this table / column / constraint?"
- "What will break if I change this column type?"
- "What migrations touched this lineage before?"
- "What past changes caused long locks, failed backfills, or replication issues?"
- "Who is already changing this table or migration path?"

### 3.3 HARBOR

**HARBOR** is the infrastructure cognition system.

It models desired state, deployed state, resource topology, cross-system dependencies, rollout history, operational incidents, and coordination around risky changes.

Core value:

- "What resources exist and how are they connected?"
- "What is the blast radius of changing this deployment, subnet, DNS record, or policy?"
- "What happened last time we touched this service or route?"
- "Which environments are drifting from declared state?"
- "Who is already modifying this operational surface?"

---

## 4. Why These Three Belong Together

They belong together because they share the same deep problem shape.

| Dimension | PRISM | LEDGER | HARBOR |
|---|---|---|---|
| Current structure | code graph | schema graph + query graph | resource graph + topology graph |
| Identity through change | symbol lineage | schema/table/column lineage | resource/service/config lineage |
| Outcome memory | code changes, tests, failures, fixes | migrations, locks, backfills, regressions | deploys, incidents, rollout failures, drift, latency |
| Coordination | edit claims, reviews, handoffs | migration ownership, rollout sequencing | environment claims, rollout ownership, review gates |
| Query interface | code-oriented TS query surface | database-oriented TS query surface | infra-oriented TS query surface |

The common pattern is not "everything is a graph."

The real common pattern is:

- authoritative structure
- time-aware identity
- outcome-bearing memory
- auditable coordination
- programmable read surface
- explicit mutation boundary

---

## 5. Shared Conceptual Model

All three products are expected to share the same **conceptual stack**.

### 5.1 Structure

Each product has an authoritative, deterministic model of the domain as it exists now.

- PRISM: modules, functions, structs, traits, documents, imports, calls
- LEDGER: schemas, tables, columns, indexes, constraints, views, procedures, queries
- HARBOR: services, deployments, clusters, namespaces, routes, policies, networks, stateful resources

### 5.2 History

Each product tracks identity through change without pretending the current snapshot ID is eternal.

- PRISM: `NodeId` vs `LineageId`
- LEDGER: current schema identity vs entity lineage through renames, type changes, table splits, index recreation
- HARBOR: current resource identity vs lineage through redeploys, rollout replacements, rename-like state transitions, infra apply cycles

### 5.3 Memory

Each product maintains memory attached to domain anchors.

- episodic / outcome memory first
- structural memory later
- semantic memory as an escape hatch, not authoritative truth

### 5.4 Coordination

Each product supports multi-agent / multi-session coordination through:

- plans
- shared tasks
- claims / leases
- artifacts
- reviews
- handoffs
- revision-aware policy

This layer should ship behind explicit feature flags.

- a `--no-coordination` mode should disable the coordination layer end to end
- when all coordination features are off for a session, the coordination layer should not be loaded or persisted for that session
- workflow, claim, and artifact capabilities should be independently enableable as pressure proves they are worth the complexity
- the exposed MCP surface should reflect those flags rather than advertising coordination universally

### 5.5 Programmable Query

Each product exposes a programmable, read-only query surface over a live in-memory world model.

The language can remain TypeScript-first across the family, even though each product exposes a different domain API.

### 5.6 Explicit Mutations

Writes stay explicit and auditable.

- query runtime remains read-only
- mutations happen through explicit tools or API calls
- outcomes, notes, inferred edges, claims, and artifacts are recorded with event metadata and attribution

For **LEDGER** and **HARBOR**, the long-term operating model should be:

> **propose -> plan -> approve -> execute -> verify -> record outcomes**

They should not become blind mutation engines. They should become change-control systems with durable execution.

The family-level split should be:

- **cognition plane**: understand structure, history, blast radius, and prior outcomes
- **control plane**: coordinate tasks, claims, approvals, policy, and maintenance windows
- **execution plane**: trigger the real migration/apply/deploy adapters
- **observation plane**: watch outcomes and attach them back to anchors and lineage

That separation keeps the products trustworthy. The lowest-level actuation should usually happen through explicit executors such as migration runners, CI jobs, Terraform, ArgoCD, Kubernetes APIs, or cloud/provider APIs rather than an agent directly mutating production systems.

Human-in-the-loop approval is not a bolt-on for these domains. It is a first-class transition in the workflow state machine for risky changes.

---

## 6. Concepts Shared Across the Family

The following concepts are expected to exist in all three products.

### 6.1 Directly Shared Concepts

These concepts are likely reusable with very small domain changes.

| Concept | Why it generalizes |
|---|---|
| `EventId` | single event identity is domain-neutral |
| `TaskId` | local task correlation is domain-neutral |
| session attribution | all domains need actor/session-aware mutations |
| event envelope / metadata | timestamp, actor, causation, correlation are universal |
| explicit mutation audit pattern | shared write discipline across all products |
| memory module pattern | store / recall / re-anchor works everywhere |
| memory composite | weighted composition is domain-neutral |
| coordination event log | plans, tasks, claims, reviews are domain-neutral enough |
| structured diagnostics | query repair matters in all domains |
| MCP runtime/session model | one programmable read surface + explicit mutations works everywhere |
| TypeScript query runtime pattern | agents write short programs well across domains |

### 6.2 Shared Concepts With Domain-Specific Implementations

These concepts should stay family-wide conceptually, but each sibling will likely implement them differently.

| Concept | Shared idea | Domain-specific implementation |
|---|---|---|
| `AnchorRef` | durable attachments to meaningful entities | code nodes vs schema nodes vs infra resources |
| `LineageId` | stable identity across change | symbol lineage vs schema lineage vs resource lineage |
| observed change stream | raw change capture before interpretation | file changes vs schema diffs vs infra state / desired-state changes |
| fingerprints | deterministic matching across change | symbol shape vs schema structure vs resource identity/topology |
| blast radius | likely downstream effect surface | code callers/importers/tests vs views/queries/migrations vs routes/services/policies/dependencies |
| validation recipe | what to check when this changes | tests/builds vs migration checks/query plans/backfills vs rollout checks/health probes/alerts |
| inferred overlay | non-authoritative augmentation | inferred code relations vs inferred query dependency vs inferred service dependency |
| artifacts/reviews | deliverables and approvals | patches vs migrations vs infra change bundles/runbooks |

### 6.3 Concepts That Must Stay Domain-Specific

These should not be prematurely generalized.

| PRISM-specific | LEDGER-specific | HARBOR-specific |
|---|---|---|
| `NodeKind` for code | schema entity taxonomy | infra resource taxonomy |
| code `EdgeKind` | schema/query/dependency edge taxonomy | topology/dependency/policy edge taxonomy |
| Rust / doc / config adapters | schema / migration / query adapters | IaC / platform / runtime adapters |
| symbol fingerprints | column/table/index/query fingerprints | service/resource/topology fingerprints |
| code blast radius logic | migration/workload blast radius logic | deployment/network/ops blast radius logic |
| code validation recipes | migration validation recipes | rollout / environment validation recipes |

---

## 7. How Each Product Works

## 7.1 PRISM: Code Cognition

PRISM's core loop is:

1. parse and index code, docs, and config into an authoritative graph
2. emit raw observed changes when files change
3. resolve cross-time lineage for current nodes
4. re-anchor memory through lineage
5. record outcomes and notes
6. expose graph/history/memory/coordination via programmable query

Typical anchors:

- workspace
- package
- module
- function
- method
- struct
- enum
- trait
- impl
- document / heading
- config key

Typical outcomes:

- tests run
- failures observed
- fixes validated
- review feedback
- migration-sensitive notes
- risky co-change behavior

Typical blast radius questions:

- who calls this?
- what imports or references this?
- what historically changes with it?
- what failed here before?
- who is already editing nearby?

## 7.2 LEDGER: Database Cognition

LEDGER's core loop is likely to be:

1. ingest schema definition, migration state, and possibly live schema introspection
2. build an authoritative schema/resource graph
3. ingest query/view/procedure dependencies where available
4. emit raw observed change sets for schema and migration change
5. resolve lineage across renames, type changes, table reshaping, index recreation
6. attach operational outcomes and migration history
7. expose structure/history/memory/coordination via query

Typical anchors:

- database
- schema
- table
- column
- index
- constraint
- foreign key
- view
- materialized view
- trigger
- stored procedure
- migration unit

Typical outcomes:

- migration applied
- migration blocked
- lock / long-running DDL
- backfill success/failure
- query regression
- replication lag
- constraint violation spike
- rollback required

Typical blast radius questions:

- what queries depend on this column?
- what views or procedures break if this changes?
- what foreign keys or indexes are affected?
- what migrations touched this lineage before?
- what happened the last time this table changed in production?

The domain-specific pillar LEDGER needs beyond PRISM is:

> **workload-aware migration risk**

Structure alone is not enough. LEDGER ultimately needs to understand query patterns, cardinality, lock implications, backfill behavior, and environment-specific operational risk.

Over time, LEDGER should grow from cognition into controlled schema-change orchestration. That means:

- analyze a proposed migration
- estimate blast radius and operational risk
- detect whether staged rollout, backfill handling, or manual confirmation is needed
- request approval when policy requires it
- trigger the actual migration runner
- watch results and require verification
- record outcomes against schema lineage

## 7.3 HARBOR: Infrastructure Cognition

HARBOR's core loop is likely to be:

1. ingest declared state (IaC, manifests, config)
2. optionally ingest live state from platforms/providers
3. build an authoritative resource/topology graph
4. emit raw observed change sets from declared or live state deltas
5. resolve lineage across replacement, rollout, renaming, environment divergence, and state transitions
6. attach outcomes from deploys, incidents, reviews, and rollbacks
7. expose structure/history/memory/coordination via query

Typical anchors:

- environment
- account / project / subscription
- cluster
- namespace
- service
- deployment
- pod template / workload
- route / ingress / gateway rule
- DNS record
- certificate
- network policy
- IAM / security policy
- queue / topic / cache / datastore binding
- Terraform resource / module

Typical outcomes:

- deploy started / succeeded / failed
- rollout stalled
- health check failures
- certificate rotation issues
- DNS propagation incidents
- policy misconfiguration
- latency or saturation spike
- rollback performed
- manual hotfix required
- drift detected

Typical blast radius questions:

- what services depend on this route or policy?
- what environments diverge from declared state?
- what systems are downstream of this resource?
- what happened the last time this service was rolled out?
- who is currently modifying adjacent operational surface?

The domain-specific pillar HARBOR needs beyond PRISM is:

> **desired state vs live state reconciliation**

Structure alone is not enough. HARBOR needs a principled model for declared topology, live topology, and drift between them.

Over time, HARBOR should grow from cognition into controlled infrastructure/deployment orchestration. That means:

- analyze an IaC or deployment change
- estimate blast radius and affected environments/services
- gate on policy, claims, and approval requirements
- trigger the actual apply/deploy system
- watch rollout health and pause when intervention is required
- record incidents, rollbacks, and successful validations against resource lineage

---

## 8. What Code Is Likely Shared

This section is about **likely shared substrate code**, not just shared ideas.

## 8.1 Likely Shared Later

These are good candidates for eventual extraction into family-level crates once a second product exists.

### A. Event Core

Likely shared responsibilities:

- event IDs
- task IDs
- timestamps
- attribution envelope
- causation / correlation model
- common mutation result envelopes

Why it shares well:

- the event model is nearly domain-neutral
- all products need append-only event streams
- all products need actor/session-aware writes

### B. Memory Core

Likely shared responsibilities:

- memory trait(s)
- memory composite
- scored memory
- recall query scaffolding
- ranking/normalization helpers
- lineage application hooks

Why it shares well:

- the shape of memory composition is common
- scoring and composition are generic
- only memory content and anchor semantics differ meaningfully

### C. Coordination Core

Likely shared responsibilities:

- plans
- shared tasks
- claims / leases
- artifacts
- reviews
- blockers
- coordination event log
- revision-aware policy helpers

Why it shares well:

- coordination is mostly domain-neutral
- only anchor types, conflict semantics, and validation policies vary

### D. MCP / Runtime Core

Likely shared responsibilities:

- session lifecycle
- read-only query execution scaffolding
- mutation attribution context
- task/session defaults
- structured diagnostics handling
- limits enforcement
- runtime reuse model

Why it shares well:

- the "programmable read surface + explicit mutation tools" pattern is common
- the host/runtime/session model should generalize well

### E. TypeScript Query Runtime Core

Likely shared responsibilities:

- TypeScript transpilation
- JS engine hosting
- serialization contract
- resource publishing
- diagnostics plumbing
- query result shaping

Why it shares well:

- agents will benefit from the same mental model across all siblings
- the runtime pattern is more reusable than the domain API itself

### F. Diagnostics Core

Likely shared responsibilities:

- result truncation diagnostics
- depth-limit diagnostics
- ambiguity diagnostics
- stale-revision diagnostics
- structured machine-readable repair hints

Why it shares well:

- agents need this everywhere
- the diagnostic framework is likely generic even when the specific codes differ

---

## 8.2 Promising But Unproven Shared Substrate

These are plausible, but should not be extracted until at least two products create strong pressure.

### A. Lineage Framework

Possible shared parts:

- lineage event storage
- matching framework / staged resolver machinery
- confidence/evidence structures
- current-ID to lineage-ID mapping machinery

Why it is not obviously safe to share yet:

- symbol lineage, schema lineage, and infra lineage may differ more than they first appear
- the staged resolver structure may generalize
- the actual evidence and matching logic likely remain domain-specific

### B. Anchor Model

Possible shared parts:

- generalized anchor ref enum pattern
- anchor serialization / query views
- overlap helpers
- claim conflict primitives

Why it is not obviously safe to share yet:

- code/node anchors, schema anchors, and infra anchors may need distinct richness
- keeping anchor models too generic too early could destroy useful domain semantics

### C. Projection Framework

Possible shared parts:

- derived-view/event-driven projection infrastructure
- append-only update and checkpoint model
- projection invalidation/update hooks

Why it is not obviously safe to share yet:

- hotspot, validation, and co-change logic will be domain-sensitive
- only the projection mechanics may be shareable

### D. Store / Persistence Patterns

Possible shared parts:

- append-only event storage patterns
- checkpointing
- projection persistence
- query snapshots

Why it is not obviously safe to share yet:

- current-graph persistence, schema persistence, and infra-resource persistence may diverge significantly

---

## 8.3 Explicitly Premature To Generalize

Do not force these into shared substrate early.

- code `NodeId` semantics
- code `NodeKind` / `EdgeKind`
- schema ontology
- infra ontology
- all domain adapters
- all domain-specific fingerprints
- domain-specific blast radius logic
- validation recipe logic
- risk summarization semantics
- query result semantics tightly coupled to one domain
- domain-specific projections
- anything that sounds like "entity", "relation", or "asset" only because the real name was removed

---

## 9. Shared Concepts, Different Implementations

The following concepts should probably exist across the family, but each product will likely implement them differently.

## 9.1 Identity

### Shared idea
Every system needs:

- a current-snapshot identifier
- a stable identity across change
- an attachable anchor model

### Different implementations

**PRISM**
- current: code node identity
- stable: symbol lineage

**LEDGER**
- current: schema/table/column/etc. identity
- stable: lineage through rename, type evolution, move, split, migration chain

**HARBOR**
- current: resource or declared object identity
- stable: lineage through rollout replacement, apply cycles, renames, environment transitions

## 9.2 Observed Change

### Shared idea
Each system should capture raw change facts before trying to interpret them historically.

### Different implementations

**PRISM**
- file changes
- parsed graph deltas

**LEDGER**
- migration deltas
- schema diffs
- introspection changes

**HARBOR**
- declarative config changes
- apply plans
- live-state deltas
- drift observations

## 9.3 Fingerprints

### Shared idea
Every domain needs deterministic fingerprints for continuity matching.

### Different implementations

**PRISM**
- signature/body/skeleton/child-shape

**LEDGER**
- type/constraints/index participation/default/nullability/query usage profile

**HARBOR**
- resource shape, declared intent, topology position, dependency profile, rollout identity

## 9.4 Blast Radius

### Shared idea
Predict likely downstream effect of changing an anchor.

### Different implementations

**PRISM**
- callers, references, imports, co-change history, test relevance

**LEDGER**
- dependent queries/views/procedures, migrations, lock risk, FK/index effects, backfill burden

**HARBOR**
- routing, dependent services, policy implications, environment scope, rollout and incident history

## 9.5 Validation Recipes

### Shared idea
Suggest what validations historically matter when a thing changes.

### Different implementations

**PRISM**
- tests, builds, lint, integration boundaries

**LEDGER**
- dry-run migration, lock estimation, query plan checks, canary migration, replication checks

**HARBOR**
- staged rollout, health checks, synthetic traffic, dependency probes, DNS/cert verification

---

## 10. Query Surface Pattern Across the Family

A key family decision is to preserve the same **query model** across siblings:

- read-only programmable query tool
- TypeScript-first snippets
- a product-specific `*.://api-reference` resource
- structured diagnostics
- explicit mutation tools
- long-lived in-memory session state

This is likely the single strongest family-level product decision.

### Why keep this consistent?

Because agents learn patterns. An agent that has used `prism.symbol()` → `prism.blastRadius()` → `prism.relatedOutcomes()` can immediately use `ledger.column()` → `ledger.blastRadius()` → `ledger.relatedOutcomes()` with the same mental model. It does not need to relearn the interaction pattern for each sibling.

This creates a cross-product network effect: the more an agent uses any one sibling, the better it gets at using all of them. That compounds over time and across tasks. It also means that system prompt instructions, recipes, and agent habits transfer across domains without retraining.

No competing approach offers this. Most agent tooling treats each domain as a bespoke integration. A consistent query surface across code, database, and infrastructure is a genuine moat.

### What changes?

Only the domain API.

Examples:

#### PRISM
```ts
const sym = prism.symbol("handle_request");
return {
  lineage: sym?.lineage(),
  impact: prism.blastRadius(sym!.id),
};
```

#### LEDGER
```ts
const col = ledger.column("public.users.email");
return {
  lineage: col?.lineage(),
  impact: ledger.blastRadius(col!.id),
  migrations: ledger.relatedOutcomes(col!.id),
};
```

#### HARBOR
```ts
const svc = harbor.service("prod/api");
return {
  rolloutRisk: harbor.blastRadius(svc!.id),
  recentIncidents: harbor.relatedOutcomes(svc!.id),
  claims: harbor.claims(svc!.anchor()),
};
```

The shape is consistent even though the content is not.

---

## 11. Coordination Across the Family

Coordination is a likely strong shared pattern.

Each product will support:

- plan creation
- shared task graph
- claims/leases
- artifacts
- reviews
- handoffs
- revision-aware blockers

### What remains shared

- lease semantics
- conflict reporting
- review lifecycle
- handoff model
- task/plan/event sourcing
- revision awareness
- read-via-query, write-via-explicit-tools pattern

### What changes by product

**PRISM**
- claims are about code anchors and edit surfaces

**LEDGER**
- claims are about migration paths, schema anchors, operational sequencing

**HARBOR**
- claims are about infra surfaces, rollout surfaces, environments, or high-risk operational domains

### Family principle

> Coordination should remain anchored, revision-aware, and auditable.  
> It should never degrade into vague chat-only workflow.

### Calibration note

Coordination is the layer most at risk of over-engineering before real demand exists. The core — claims and conflict detection — is essential for multi-agent safety. The surrounding ceremony — reviews, artifacts, handoffs, plan DAGs — may turn out to be unnecessary if agents naturally coordinate through shared memory and outcome visibility alone. Build claims first. Add the rest only when real multi-agent usage creates pressure.

---

## 12. Recommended Future Umbrella Monorepo Shape

Do **not** create this yet.

Create it only when **LEDGER** starts for real.

A likely shape:

```text
family/
  Cargo.toml
  crates/
    shared/
      event-core/
      memory-core/
      coordination-core/
      mcp-runtime/
      ts-query-runtime/
      diagnostics-core/
      maybe-lineage-core/
      maybe-projection-core/

    prism/
      prism-core/
      prism-ir/
      prism-parser/
      prism-lang-rust/
      prism-lang-markdown/
      prism-lang-json/
      prism-lang-yaml/
      prism-store/
      prism-history/
      prism-memory/
      prism-query/
      prism-js/
      prism-mcp/
      prism-cli/
      prism-projections/
      prism-coordination/
      prism-curator/

    ledger/
      ledger-core/
      ledger-ir/
      ledger-adapters/
      ledger-store/
      ledger-history/
      ledger-memory/
      ledger-query/
      ledger-js/
      ledger-mcp/
      ledger-cli/
      ledger-projections/
      ledger-coordination/

    harbor/
      harbor-core/
      harbor-ir/
      harbor-adapters/
      harbor-store/
      harbor-history/
      harbor-memory/
      harbor-query/
      harbor-js/
      harbor-mcp/
      harbor-cli/
      harbor-projections/
      harbor-coordination/
```

This is intentionally a **family monorepo**, not a giant generic platform repo with three thin adapters.

The products remain first-class.

---

## 13. Transition Strategy From the Current PRISM Repo

## 13.1 Phase 0 — PRISM Only

Current state:

- keep building PRISM where it is
- let architecture settle
- improve boundaries and docs
- identify likely shared substrate, but do not extract aggressively

Goal:

- ship a genuinely usable PRISM
- prove the architecture under real usage

## 13.2 Phase 1 — Prep For Future Extraction

Before LEDGER starts, shape PRISM so extraction is easier.

Do now:

- document crate ownership and future disposition
- classify crates into:
  - Prism-specific
  - likely shared later
  - maybe shared later
- ensure generic-ish code is documented as generic-ish
- avoid hard-coding "code-only" assumptions into session/runtime/event/memory/coordination machinery where they are not necessary

Do not do now:

- create generic mush abstractions
- rename specific concepts into vague platform terms
- create shared crates without second-domain pressure

## 13.3 Phase 2 — LEDGER Spike

When PRISM is usable, begin a real LEDGER spike.

Purpose:

- pressure-test which abstractions actually generalize
- discover where schema/workload/migration semantics differ materially from code
- validate the query/runtime/memory/coordination pattern in a second domain

At this point, maintain LEDGER as either:
- a spike branch
- or a temporary sibling experimental repo

The goal is learning, not immediate repo consolidation.

## 13.4 Phase 3 — Create Umbrella Monorepo

Create the family monorepo only when:

- LEDGER has become real, not hypothetical
- at least some shared substrate candidates have proven themselves
- moving PRISM into the new repo simplifies future work more than it disrupts it

Move PRISM into the umbrella monorepo as the first product.
Only then begin extracting shared crates.

## 13.5 Phase 4 — Extract Shared Substrate Conservatively

Extraction order should likely be:

1. event / attribution core
2. diagnostics core
3. TS runtime / MCP scaffolding
4. memory core
5. coordination core
6. possibly lineage framework
7. possibly projection framework

Do **not** start by extracting domain model or ontology crates.

## 13.6 Phase 5 — HARBOR Follows

Only after PRISM and LEDGER have created enough shared pressure should HARBOR enter the umbrella repo.

HARBOR will likely reuse more family substrate than LEDGER did, because the extraction work will already have happened.

---

## 14. Family-Level Product Principles

These principles should remain true across PRISM, LEDGER, and HARBOR.

### 14.1 Structure Is Authoritative

The base domain model is deterministic and authoritative.

Agents do not rewrite truth.

### 14.2 Time Is Layered On Top Of Structure

Current identity and cross-time identity are different things.

Do not overload one to fake the other.

### 14.3 Memory Is Durable, Anchored, and Evidence-Backed

Memory must attach to meaningful anchors and survive change through lineage when possible.

### 14.4 Coordination Is Explicit

Claims, reviews, blockers, and handoffs should be first-class and auditable.

### 14.5 Query Surfaces Stay Programmable, Mutations Stay Explicit

Read queries compose.
Writes remain explicit and attributable.

For LEDGER and HARBOR specifically, this extends to execution:

- they may eventually drive real migrations, applies, and deploys
- they should do so as orchestrators over explicit executors, not as direct autonomous operators
- approval and pause/resume semantics should be first-class for risky changes

### 14.6 Shared Later Is A Hypothesis, Not A Mandate

A concept being similar is not enough.
A second real product must create real extraction pressure.

### 14.7 Durable Workflow Runtime Is Optional Until It Is Not

Do not start with a heavyweight workflow engine by default.

Early versions can persist event history, approvals, waits, and execution state directly in the family event/coordination model while delegating execution to existing runners.

A dedicated durable workflow runtime becomes attractive when the system must reliably handle:

- long-running migrations or staged rollouts
- approval waits or maintenance windows
- retries across flaky external systems
- pause/resume after crashes or outages
- compensating actions or rollback workflows
- many adapters in a single change path

At that point, a Temporal-class workflow engine can make sense as part of the execution plane, not as the core product thesis.

### 14.8 Keep Domain Ontology Sharp

Do not erase domain semantics in pursuit of reuse.

The family wins by sharing substrate while preserving domain truth.

---

## 15. Anti-Patterns To Avoid

### 15.1 Premature Platformization

Bad move:
- turning PRISM into a generic "entity graph platform" before LEDGER exists

Why it is bad:
- destroys clarity
- invites vague abstractions
- slows real product progress

### 15.2 Three Completely Separate Repos Too Early

Bad move:
- creating isolated repos for PRISM, LEDGER, and HARBOR from day one

Why it is bad:
- duplicates substrate
- creates version skew
- makes extraction and reuse painful

### 15.3 Over-Sharing Ontology

Bad move:
- forcing code, database, and infra domains into one shared `NodeKind`

Why it is bad:
- erases useful domain detail
- produces lowest-common-denominator models

### 15.4 Letting Coordination Become Workflow Theater

Bad move:
- coordination becomes vague state machines detached from anchors/revisions

Why it is bad:
- less trustworthy
- less useful to agents
- drifts toward generic workflow software

### 15.5 Letting Semantic Memory Become Authority

Bad move:
- vector recall starts driving truth instead of assisting recall

Why it is bad:
- undermines determinism
- lowers trust
- contaminates the world model

### 15.6 Treating Ledger Or Harbor As Blind Auto-Operators

Bad move:
- giving an agent direct production mutation power without explicit approval, execution, and verification boundaries

Why it is bad:
- collapses cognition, control, and execution into one unsafe surface
- makes failures harder to reason about and audit
- reduces operator trust precisely where blast radius is highest

---

## 16. Practical Recommendation For Now

Right now:

- keep building PRISM in the current repo
- finish making PRISM genuinely usable
- shape the repo so future extraction is easy
- do not create the family monorepo yet
- do not add LEDGER or HARBOR code yet
- document likely shared substrate and likely product-specific areas clearly

The immediate strategic goal is:

> make PRISM strong enough that it can help build LEDGER.

That is the first real proof that the architecture generalizes.

---

## 17. Canonical Decisions

These are the working decisions this document recommends.

1. **PRISM ships first**
2. **LEDGER is the likely second sibling**
3. **HARBOR follows later**
4. **Do not move to three separate repos**
5. **Do not permanently keep all siblings inside the PRISM repo**
6. **Create an umbrella monorepo only when LEDGER becomes real**
7. **Extract shared substrate only after second-domain pressure**
8. **Keep product ontologies domain-specific**
9. **Keep the query-runtime pattern consistent across the family**
10. **Keep structure authoritative and inference additive across all siblings**

---

## 18. Suggested Short Descriptions

These are concise product descriptions that can be reused internally.

### PRISM
Persistent code cognition: structure, history, memory, and coordination for codebases.

### LEDGER
Persistent database cognition: schema, migrations, workload-aware risk, memory, and coordination for databases.

### HARBOR
Persistent infrastructure cognition: topology, drift, rollout history, operational memory, and coordination for infrastructure.

---

## 19. Final Note

PRISM, LEDGER, and HARBOR should not become three skins over one generic engine.

They should become three domain-native cognition systems built on a shared family substrate.

That is the right balance:

- share the deep machinery
- preserve the domain truth
- let agents compose them together
- and keep every durable claim anchored, attributable, and evidence-backed
