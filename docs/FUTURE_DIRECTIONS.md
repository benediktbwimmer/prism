# PRISM — Perceptual Representation & Intelligence System for Machines

## Future Directions

## Thesis

PRISM should not stop at being a smarter code index. The higher-upside direction is a code cognition engine: a persistent, evidence-backed world model of a software system.

The target stack is:

* structure
* time
* intent
* memory
* reasoning

That combination matters because most coding agents are weak at understanding change. They can describe code and sometimes patch it, but they are much worse at answering:

* what changed here before?
* what tends to break when this changes?
* what was this supposed to do?
* what did we learn last time?
* what is the safest way to modify this now?

PRISM can differentiate by answering those questions well.

## Highest-Upside Directions

### 1. Temporal Graph, Not Just Structural Graph

The graph should eventually exist through time, not only as a snapshot.

Useful capabilities:

* symbol lineage across renames and moves
* when a node appeared and changed
* which neighbors tend to co-change
* what bugs, regressions, or migrations followed those changes

This is the core move from static understanding to change understanding.

### 2. Intent Graph

PRISM should model the relationship between:

* specs
* docs
* ADRs
* TODOs
* invariants
* tests
* code symbols

That enables intent-grounded reasoning such as:

* requirement X is described in spec section Y
* symbols A, B, and C appear to implement it
* test T validates it
* current code now drifts from the spec

### 3. Outcome Memory

The memory layer gets much more valuable when it records consequences, not just notes.

Examples:

* this refactor caused a regression
* this patch required updating integration tests
* this area repeatedly triggers allocation review comments
* this schema change required a migration note

This is how PRISM learns from what actually happened in a repo.

### 4. Blast Radius and Change Simulation

Given a proposed change, PRISM should be able to estimate:

* directly affected symbols
* historically co-changing areas
* likely tests and builds to run
* docs or specs that may drift
* migrations or config changes that are commonly required
* warnings attached by prior outcomes

The goal is a change graph, not just a call graph.

### 5. Evidence-Backed Agent Actions

Every recommendation or patch should be explainable using evidence such as:

* graph evidence
* temporal evidence
* intent evidence
* memory evidence
* runtime or test evidence

That makes the system auditable, not just impressive.

### 6. Runtime Grounding

Static structure is necessary, but debugging and validation also need runtime truth:

* test coverage
* stack traces
* logs
* profiler samples
* failing inputs
* execution traces

The long-term value is the join between static structure, runtime behavior, and memory.

### 7. First-Class Uncertainty

PRISM should model uncertainty explicitly:

* unresolved calls
* ambiguous lineage mappings
* low-confidence inferred edges
* stale memories
* suspected spec-code drift

That supports honest behavior such as running a test, asking for clarification, or surfacing a known unknown.

### 8. Tasks as Durable Objects

Tasks should eventually be resumable, anchored artifacts with:

* goal
* hypotheses
* attempted edits
* failures
* validations
* next likely moves

This is the path from one-shot assistance to real continuity.

### 9. Policy and Invariant Layer

Examples:

* changes to this schema require a migration note
* this API must remain backward-compatible
* this module should not allocate on hot paths
* changes here require updating docs and tests

This is repo-specific engineering knowledge, not generic retrieval.

### 10. PRISM as the Agent’s Learning Substrate

PRISM should eventually retain:

* what the agent saw
* what it changed
* which evidence mattered
* what worked
* what failed

That creates repo-specific compounding rather than generic "AI memory."

## Three Bets To Push Hardest

If prioritization has to stay narrow, the three strongest bets are:

1. temporal graph and lineage
2. intent graph
3. outcome memory plus change simulation

That trio makes PRISM a change-intelligence system rather than a retrieval system.

## Trap To Avoid

The main trap is becoming "RAG for repos with extra steps."

Signals of the wrong center of gravity:

* over-investing in embeddings early
* chasing generic semantic search before repo depth
* broad language support before strong temporal and intent models
* agent cleverness before verification
* chat polish before grounded change intelligence

The win condition is not "PRISM knows more text." It is "PRISM knows what exists, what changed, why it matters, and what happened last time."

## End-State Vision

The dream version of PRISM is:

* faster repo understanding than a human cold start
* retention of lessons across weeks or months
* prediction of risky edits before they happen
* detection of drift between implementation and intent
* debugging grounded in runtime evidence and history
* patch generation with explanations humans can trust

Short version:

**PRISM’s best future is memory-backed, intent-aware, temporally grounded change intelligence.**

## Sibling Product Family

PRISM's architecture is domain-specific, but the pattern it establishes is domain-agnostic. The same layered model of structure, time, memory, coordination, and code-as-query applies to other domains where agents struggle with spatial awareness, change understanding, and outcome learning.

### The Reusable Pattern

The following PRISM concepts transfer directly to sibling products:

* `AnchorRef` and the anchoring model
* `EventMeta`, `EventId`, `TaskId` and the event envelope
* `MemoryModule` trait and memory composition
* `OutcomeEvent` and outcome recording
* `ObservedChangeSet` and the change stream model
* `LineageEvent` and the temporal identity pattern
* code-as-query via embedded TypeScript
* MCP server model and session lifecycle
* coordination framework for multi-agent work

What changes per domain is only the node kinds, edge kinds, fingerprint semantics, and adapter implementations.

### Candidate 1: Database Perception

A database sibling would model:

* **Node identity** — `schema.table.column`, `schema.index`, `schema.constraint`, `schema.view`
* **Lineage** — column identity through renames, type changes, table splits, migrations
* **Fingerprint** — column type plus constraints plus index participation
* **Outcome memory** — "migration X took 47 minutes on prod", "this ALTER requires a backfill", "last schema change caused a 3-hour lock"
* **Blast radius** — queries that reference a column, views that depend on a table, FK cascade chains, downstream application code

The killer capability agents lack today: "what queries will break if I change this column type?" That requires joining schema structure with query analysis and historical migration outcomes — exactly the PRISM pattern.

### Candidate 2: Infrastructure Perception

An infrastructure sibling would model:

* **Node identity** — `cluster.namespace.deployment.container`, `vpc.subnet.security-group`, `dns.zone.record`
* **Lineage** — resource identity through Terraform applies, Helm upgrades, blue-green switches
* **Outcome memory** — "last time we scaled this service down, latency spiked", "this config change caused a certificate rotation failure"
* **Blast radius** — dependent services, routing rules, DNS propagation, certificate chains, cross-boundary dependency chains (Terraform → Kubernetes → application → database)

The pattern agents struggle with most in infrastructure is dependency chains that cross abstraction boundaries. An infra perception layer would surface the full chain.

### Cross-Domain Composition

The products are independent systems, not modules inside PRISM. Each one is expert in its domain, exposes code-as-query, and contributes outcomes to its own memory layer. An agent connects to multiple MCP servers simultaneously and composes queries across them:

```ts
// PRISM: find code that uses this table
const refs = prism.search("users_table", { kind: "Function" });

// SCHEMA: find recent column changes on that table
const changes = schema.lineageHistory(schema.lineageOf("public.users.email"));

// Cross-domain reasoning happens in the agent, grounded
// by structured evidence from both systems
```

The vision is not one monolith but a family of domain-specific perception layers that agents compose over. PRISM is the proving ground for the pattern. The siblings will be faster to build because the architecture is already validated.

### Naming and Delivery

* **PRISM** — code perception and memory
* **VAULT** — database perception and memory
* **HARBOR** — infrastructure perception and memory

PRISM ships first as the standalone product. Once VAULT is ready (likely first sibling, since schema graphs are closer to code graphs than infrastructure is), the three products move to an umbrella monorepo with shared substrate code (event envelope, memory traits, anchoring model, code-as-query runtime, MCP primitives) but independent domain models, independent adapters, and independent MCP server surfaces.

## Near-Term Design Priority

The next concrete architecture step is the event model for:

* temporal lineage
* outcome memory

Those two primitives unlock a large share of the rest of the roadmap.
