# CONCEPT_MAINTENANCE

Status: proposed operating policy
Audience: PRISM core, projection, memory, coordination, and agent-runtime maintainers
Scope: lifecycle, promotion, health, and scheduled maintenance for PRISM concepts

---

## 1. Summary

PRISM concepts are too valuable to remain ad hoc, but too dynamic to rely on manual curation alone.

This document defines the **concept maintenance loop**: a continuous process that lets PRISM discover, stabilize, promote, repair, and retire concepts with minimal human effort and without allowing the concept layer to decay into stale or noisy ontology.

The goal is not to build a giant taxonomy. The goal is to maintain a **small, high-signal, repo-quality semantic layer** that materially improves cold-start navigation, architectural reasoning, impact analysis, and execution continuity for agents.

The maintenance loop should ensure that:

* useful local discoveries are not lost
* repo-quality concepts remain trustworthy
* stale concepts are surfaced and repaired instead of silently rotting
* promotion and maintenance happen as part of normal repo work
* the concept graph grows through evidence, not theory

---

## 2. Why this exists

A concept layer only helps if it stays:

* grounded
* current
* compact
* reusable
* trusted by agents

Without an explicit maintenance loop, concepts will eventually fail in one of two ways:

1. **underproduction** — useful concepts are repeatedly rediscovered but never promoted
2. **decay** — once-useful concepts drift away from the live repo and become stale or misleading

PRISM already has the ingredients needed to avoid both failures:

* local/session/repo quality levels
* concept hydration
* lineage and rebinding
* plans and tasks
* outcome memory
* concept usage telemetry
* risk/impact and validation surfaces

The missing piece is a formal loop that turns these ingredients into an operating system for concept upkeep.

---

## 3. Core philosophy

### 3.1 Concepts are evidence-backed semantic packets

A concept is not a loose summary. It is a grounded semantic object that binds repo-native meaning to live members, likely tests, risks, outcomes, and other runtime surfaces.

### 3.2 Repo-quality concepts are precious

Repo-quality concepts should remain high-signal and relatively sparse. They are part of the repo's published knowledge and should feel trustworthy.

### 3.3 Most concepts should begin small

New concepts should usually begin as local or session-scoped concepts. Promotion to repo quality is earned through repeated usefulness and structural stability.

### 3.4 Maintenance must be lazy but real

Humans should not have to constantly groom the concept graph by hand. Instead, PRISM should schedule concept-review work and surface narrowly scoped maintenance tasks as part of ordinary agent execution.

### 3.5 Concepts should be maintained through use

The concept layer should improve because agents actually use it while doing real work, not because someone decided to write architecture taxonomy in advance.

### 3.6 Staleness must be surfaced explicitly

A decayed concept must not quietly continue to look healthy. Drift, dead members, stale validations, and ambiguous retrieval should lower confidence and trigger repair work.

---

## 4. Quality levels

```rust
pub enum ConceptQuality {
    Local,
    Session,
    Repo,
}
```

### Local

Short-lived concept created inside a narrow task or investigation.

Properties:

* may be rough
* may have incomplete aliases or summaries
* may include tentative members
* should not be exported to `.prism`

### Session

Concept that has proven useful within a session and may survive handoff or later review.

Properties:

* should have clearer naming
* should have more stable core members
* may be promoted to repo quality
* may be retained in runtime state across session operations

### Repo

Published concept committed into `.prism` and hydrated on startup.

Properties:

* stable enough to matter across cold starts
* grounded enough to trust
* small enough to remain legible
* maintained through concept-review work

---

## 5. Lifecycle

Concepts should move through this lifecycle.

### 5.1 Create

A concept is first created when an agent identifies a meaningful repo-native cluster during real work.

Typical triggers:

* a broad term resolves to the same cluster repeatedly
* a task workset stabilizes around a recurring semantic unit
* several files/symbols/tests/outcomes clearly belong to one meaning
* a handoff repeatedly needs the same multi-artifact explanation

### 5.2 Stabilize

The concept remains local or session-scoped while it proves useful.

Stabilization activities:

* improve canonical name
* add aliases
* refine summary
* remove weak members
* mark likely tests and risk hints
* observe whether future retrieval resolves to the same packet

### 5.3 Promote

A concept is promoted to repo quality when evidence shows it is reusable and stable enough for cold-start use.

Promotion should be explicit or policy-driven, never accidental.

### 5.4 Maintain

Repo-quality concepts are periodically reviewed for drift, cohesion, and usefulness.

Maintenance actions may include:

* rebind dead members
* refresh aliases
* update likely tests
* drop stale warnings
* split an overgrown concept
* merge duplicates
* supersede an old concept with a newer one

### 5.5 Retire

A concept may be retired or superseded when it no longer represents a useful living unit of meaning.

Retirement should remain inspectable and queryable through history.

---

## 6. Creation and proposal sources

PRISM should support both **agent-authored concepts** and **system-suggested concept proposals**.

### 6.1 Agent-authored creation

Agents should create local or session concepts when they discover a repo-native concept that would save future rediscovery.

Good creation moments:

* after resolving a broad noun into a stable multi-artifact cluster
* after a successful task workset becomes clearly reusable
* after a bug hunt reveals a stable semantic boundary
* after a handoff requires a compact architectural explanation

### 6.2 System-suggested proposals

PRISM may propose candidate concepts based on evidence such as:

* repeated co-change clusters
* repeated co-usage in worksets
* repeated broad-query convergence to the same members
* repeated shared validations
* repeated outcome clustering
* repeated handoff bundles
* repeated impact/risk clustering

System-suggested proposals should not silently become repo-quality concepts. They should enter the maintenance loop as **promotion candidates**.

---

## 7. Promotion policy

A session/local concept should be eligible for repo-quality promotion when it satisfies most of the following.

### 7.1 Reuse

The concept has been used successfully across multiple tasks, sessions, or distinct retrievals.

### 7.2 Stability

Its core members remain structurally stable enough that hydration and rebinding are likely to hold.

### 7.3 Cohesion

The concept feels like a real semantic unit, not just a bag of files opened together once.

### 7.4 Compression value

The concept materially reduces discovery effort, ambiguity, or repeated tool calling.

### 7.5 Grounding

The concept is well anchored in live members, concepts, or durable structural anchors.

### 7.6 Clarity

It has a canonical name, a small alias set, and a summary that another agent could use correctly.

### 7.7 Safety

It does not encode transient or misleading local context that will quickly rot.

### 7.8 Evidence threshold

Promotion should generally require at least one of:

* repeated successful use
* repeated successful retrieval from ambiguous broad terms
* attachment to successful worksets or outcomes
* explicit curator/policy approval

---

## 8. What should not be promoted

Do not promote concepts that are:

* one-off debugging clusters
* unstable hypotheses
* temporary local decompositions
* arbitrary file groups with no repo-native meaning
* concepts whose summaries are too vague to guide action
* clusters whose core members are already obviously captured by a stronger existing concept

Rule of thumb:

**A repo-quality concept should feel like something future agents would naturally think or speak in.**

---

## 9. Concept health

Repo-quality concepts should carry a health model.

```rust
pub enum ConceptHealth {
    Healthy,
    Watch,
    Degraded,
    Stale,
    Superseded,
    Retired,
}
```

Health should be derived from several signals.

### 9.1 Binding health

* percentage of live core members
* lineage rebind success rate
* unresolved members after hydration
* degree of concept-centered recovery required

### 9.2 Cohesion health

* whether core members still co-occur in worksets
* whether shared validations still make sense
* whether linked risk and outcome surfaces still cluster around the concept

### 9.3 Retrieval health

* alias match confidence
* ambiguity frequency
* repeated concept misselection
* fallback to raw search or `prism_query` when the concept should have sufficed

### 9.4 Usage health

* whether agents still use the concept productively
* whether it helps real tasks complete faster or more safely
* whether it has effectively gone dormant

### 9.5 Staleness health

* whether core members were deleted or heavily refactored
* whether likely tests are no longer recognized
* whether risk hints or warnings contradict current reality

### 9.6 Relationship health

For concept-to-concept edges:

* whether linked concepts still meaningfully interact
* whether dependency or part-of edges appear outdated
* whether supersession or split is needed

---

## 10. Maintenance actions

When a concept enters review, the maintenance loop should choose one or more of these actions.

### 10.1 Promote

Upgrade a local/session concept to repo quality.

### 10.2 Refresh

Update aliases, summary, tests, risks, or related memories without changing core meaning.

### 10.3 Rebind

Repair dead or drifted member bindings using anchors, lineage, and nearby semantic context.

### 10.4 Split

Break an overgrown concept into two or more cleaner concepts when one packet has become semantically overloaded.

### 10.5 Merge

Combine two near-duplicate concepts where one stronger concept would better serve retrieval and navigation.

### 10.6 Supersede

Mark a concept as replaced by a newer concept when architectural evolution invalidates the older packet.

### 10.7 Retire

Move a concept out of active use while preserving inspectable historical provenance.

### 10.8 Reject

Decline promotion of a local/session proposal that lacks stability, cohesion, or value.

---

## 11. Scheduled maintenance loop

PRISM should periodically schedule concept-maintenance work as normal plan/task nodes.

This work should be explicit, inspectable, and narrow.

### 11.1 Maintenance plan categories

Typical recurring maintenance tasks:

* review promotion candidates
* inspect degraded repo concepts
* rebind stale core members
* split or merge overloaded concepts
* review weak aliases and retrieval failures
* validate concept-to-concept edges

### 11.2 Example recurring tasks

Examples of scheduled work:

* "Review local/session concepts for repo promotion"
* "Inspect repo concepts with degraded health"
* "Repair concept bindings after recent refactor cluster"
* "Review ambiguous broad-term resolution failures"
* "Check concept graph edges for stale dependencies"

### 11.3 How scheduled work should be generated

Scheduled maintenance should be driven by signals such as:

* concept health falling below threshold
* new promotion candidates passing evidence bar
* repeated retrieval ambiguity
* repeated failed rebinds
* concept graph drift after major refactors
* dormant concepts with no productive use over time

### 11.4 Maintenance work must be grounded

Maintenance nodes should attach directly to:

* the concept under review
* its core members
* relevant outcomes or memories
* any affected plan nodes or handoffs

This keeps maintenance work concrete and cheap to execute.

---

## 12. Integration with plans

Concept maintenance should be expressed through first-class plans and plan nodes.

### 12.1 Why plans matter here

Using plans for concept maintenance gives PRISM:

* visibility into pending upkeep
* explicit execution ordering
* handoff support
* review and validation gates
* durable shared intent about knowledge upkeep

### 12.2 Recommended plan node kinds

Common node kinds for concept upkeep:

* `Investigate` — inspect drift or ambiguous retrieval
* `Decide` — determine whether to promote, split, merge, or retire
* `Edit` — update concept packet or relationships
* `Validate` — verify improved retrieval and healthy bindings
* `Review` — human or senior-agent review for repo-quality promotion

### 12.3 Maintenance should be small and local

A maintenance node should usually address one concept or one tightly related concept cluster, not the entire knowledge graph.

---

## 13. Integration with hydration and rebinding

Concept maintenance depends on runtime hydration.

On startup and reload, PRISM should:

* hydrate repo-quality concepts
* attempt binding refresh
* compute concept health signals
* mark unresolved or degraded concepts explicitly
* enqueue or surface maintenance work when thresholds are crossed

Hydration should not silently pretend a concept is healthy when core members cannot be rebound.

---

## 14. Integration with retrieval

Concept maintenance should directly improve retrieval quality.

### 14.1 Alias maintenance

Promotion and maintenance should review:

* canonical name quality
* observed synonyms
* successful broad-term retrievals
* common misspellings or local shorthand

### 14.2 Retrieval failure loop

If agents repeatedly fail to resolve a broad term to the right concept, PRISM should:

* lower concept retrieval confidence
* record the ambiguity
* suggest alias or summary refresh
* optionally schedule review work

### 14.3 Concept usage as maintenance evidence

Successful concept retrieval followed by productive task execution should count as positive maintenance evidence.

---

## 15. Integration with concept-to-concept edges

If PRISM supports concept-to-concept relationships, these edges must be maintained with the same discipline as concept packets.

### 15.1 Edge principles

Concept edges should be:

* typed
* inspectable
* grounded in real use or architecture
* minimal rather than encyclopedic

### 15.2 Candidate edge types

Good initial edge types:

* `depends_on`
* `part_of`
* `validated_by`
* `used_with`
* `supersedes`
* `confused_with`

### 15.3 Edge maintenance triggers

Review edges when:

* linked concepts drift apart in usage
* supersession occurs
* a split or merge changes semantic boundaries
* retrieval pathfinding would materially improve from a new relationship

---

## 16. Automation boundaries

PRISM should automate suggestions and routine review, but it should be conservative about irreversible semantic changes.

### 16.1 Safe to automate

* health scoring
* promotion candidate identification
* rebind attempts
* weak alias suggestions
* stale concept detection
* concept review task creation

### 16.2 Requires explicit approval or strong policy

* repo-quality promotion
* merge of two repo concepts
* split of a repo concept
* retirement or supersession of a repo concept
* adding high-impact concept-to-concept edges

This policy can loosen later if the system proves highly reliable.

---

## 17. Suggested maintenance metrics

The maintenance loop should track at least:

* number of local/session concepts created
* number of promotion candidates surfaced
* promotion acceptance rate
* repo-quality concept count
* healthy / degraded / stale concept counts
* average live core-member ratio
* lineage rebind success rate
* ambiguous retrieval rate
* concept-assisted task success rate
* concept maintenance backlog
* time from proposal to promotion or rejection
* concepts superseded or retired per period

The point is not vanity metrics. The point is to keep the concept layer trustworthy and economically useful.

---

## 18. Operator and agent guidance

Agents should treat concept upkeep as normal repo work, not as optional hygiene.

Agents should create or refine concepts when:

* a reusable semantic unit clearly emerges during real work
* a broad term repeatedly resolves to the same cluster
* a handoff would be much clearer with a concept packet
* a task workset would obviously help future tasks

Agents should promote concepts when:

* the concept has proven useful beyond one narrow moment
* the members are stable and grounded
* the packet is clear enough for another agent to use correctly

Agents should request maintenance when:

* a repo-quality concept is degraded or stale
* a concept repeatedly misfires in retrieval
* a concept has clearly split or been superseded
* concept edges appear misleading or out of date

---

## 19. Non-goals

This maintenance loop is not meant to:

* produce a giant hand-authored ontology
* promote every local cluster into repo knowledge
* replace code reading with concept graphs
* hide drift under generic summaries
* encourage agents to invent concepts with no evidence

The goal is narrower:

**maintain a compact, trusted, living semantic layer that compounds repo understanding over time.**

---

## 20. Final principle

PRISM concepts should evolve the same way good engineering knowledge evolves:

* discovered during real work
* stabilized through repeated use
* promoted when clearly reusable
* repaired when drift appears
* retired when no longer true

The concept layer should feel less like documentation and more like a living semantic substrate that the repository earns and maintains over time.

That maintenance loop is what keeps concepts from becoming stale abstractions and turns them into durable infrastructure for agent reasoning.
