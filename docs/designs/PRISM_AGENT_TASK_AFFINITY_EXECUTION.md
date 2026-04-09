# PRISM Agent Task Affinity Execution

Status: proposed target design
Audience: PRISM coordination, runtime, query, MCP, and execution-fabric maintainers
Scope: decentralized task selection, execution-chain continuity, and agent-local task fitness

---

## 1. Summary

PRISM should stop assuming that the optimal execution model is "cold-start one agent per actionable
task."

That model was reasonable when context windows were small and agents paid a steep penalty whenever
they carried too much prior state. It is no longer the right default.

Modern agents can:

- carry larger working context
- compact their own prior work effectively
- preserve useful local understanding across sequential tasks

PRISM should take advantage of that by letting agents work through coherent execution chains when
that increases throughput and correctness.

The core design is:

- agents continue to **pull** work rather than receive assignments from a central manager
- each agent directly assigns a **task fitness score** to actionable tasks from its own current
  context state
- agents prefer high-fitness continuation tasks over unrelated cold-start tasks
- PRISM exposes **soft continuation reservations** so agents can preserve warm context across short
  dependency chains without monopolizing a plan indefinitely
- shared coordination refs remain the authoritative shared fact plane, but not a central scheduler

This keeps PRISM decentralized while materially improving token efficiency, latency, and execution
quality.

---

## 2. Problem

PRISM today is structurally biased toward a cold-start task model:

- an agent claims one actionable node
- executes it
- completes it
- goes idle or chooses another task from a mostly flat actionable pool

That model has two major inefficiencies:

1. it throws away warm task context too early
2. it treats all actionable tasks as if they have equal ramp-up cost for every agent

In practice, that is false.

When an agent has just completed work on:

- the same plan
- the same files
- the same semantic anchors
- the same concept or contract cluster
- the immediate prerequisite of a follow-up node

that agent is often dramatically more efficient on the next related task than a different cold
agent would be.

Today humans compensate manually by:

- keeping one Codex thread focused on one plan
- letting that thread execute a sequence of related tasks
- using multiple threads in parallel across unrelated work

That pattern works because **context continuity matters**.

PRISM should make that behavior first-class instead of depending on human manual scheduling.

---

## 3. Design Goals

Required goals:

- preserve PRISM's decentralized pull-based execution model
- let agents directly score how good each actionable task is for them right now
- increase reuse of warm local context across related tasks
- support short execution chains without hard central assignment
- avoid starvation of urgent unrelated work
- avoid long-lived task hoarding
- keep coordination truth in shared refs, not in one manager process
- make task selection legible and explainable through shared facts and query surfaces

Required non-goals:

- PRISM should not introduce a mandatory central scheduler or manager agent
- PRISM should not require perfect global optimization before an agent can start work
- PRISM should not create hard reservations that block takeover when an agent stalls
- PRISM should not require an agent to publish its full prompt or private local context

---

## 4. Core Principle

The right execution unit is no longer always a single isolated task.

Instead, PRISM should optimize for:

- **parallelism across unrelated chains**
- **continuity within a chain**

That means:

- use many agents in parallel where work is semantically disjoint
- let one agent continue across neighboring tasks when its local context is already warm

This is not central planning.
It is local, context-aware pull selection.

---

## 5. No Manager Agent by Default

PRISM should not start with a central manager agent that assigns work.

A manager agent creates several avoidable problems:

- it becomes a bottleneck
- it can hallucinate a bad global schedule
- workers become dependent on one coordinator for liveness
- the system becomes less peer-to-peer and less robust under partial failure

Instead, PRISM should keep the authority split clean:

- shared coordination refs publish durable shared execution facts
- each execution agent decides what to pull next

Later, PRISM may add an **advisory planner** that publishes recommended opportunities, but that
planner must not be required for progress and must not become the source of truth.

---

## 6. Task Fitness

### 6.1 Definition

Each agent should directly assign a **task fitness score** to candidate actionable tasks.

This score is:

- local to the evaluating agent
- based on the agent's current context and recent work
- a direct agent estimate, not a separate handcrafted scoring engine

The central question is:

> "How good of a next task is this for me right now?"

That estimate should be represented as a bounded scalar, for example:

- `0` to `100`

where:

- `0` means "cold and low-value for me right now"
- `100` means "best possible continuation for my current warm state"

### 6.2 What the score means

`task_fitness` is not global importance.

It is the agent's direct estimate of:

- context reuse value
- ramp-up cost
- likelihood of fast, correct completion

An urgent task may still deserve attention even if its local fitness is low.
So PRISM should keep global plan/task priority separate from local agent fitness.

### 6.3 Inputs to the agent's score

When directly assigning a score, the agent should consider:

- whether it just completed an immediate dependency of the task
- whether it is already working in the same plan
- overlap with recently touched files
- overlap with recently used anchors
- overlap with recent concepts, contracts, or memories
- whether the same repo branch already contains the relevant local code context
- whether the task extends the same implementation thread or validation thread
- expected cold-start cost to understand the task correctly
- confidence that it can complete the task cleanly without large context rebuild

### 6.4 Priority is still separate

PRISM should keep global execution priority distinct from local fitness.

The effective selection signal should be a composition of:

- global task priority
- local task fitness
- current claimability / readiness

In other words:

- high-priority urgent work can still interrupt a warm chain
- warm continuation can still dominate among otherwise similar tasks

---

## 7. Actionable Pull Model

When an agent becomes ready for more work, it should:

1. query the actionable task set
2. filter to tasks it is actually allowed to claim
3. assign a direct `task_fitness` score to a bounded shortlist
4. choose the best candidate according to:
   - global priority
   - local fitness
   - reservation state
   - current blockers and claimability
5. claim the selected task

This remains a pull model.

The only change is that agents stop acting like all claimable tasks are equally cheap to begin.

---

## 8. Execution Chains

### 8.1 Definition

An execution chain is a short sequence of related tasks where one agent retaining local context is
materially beneficial.

Typical examples:

- implement node A, then validate node B
- complete parent task A, then continue into immediate child task B
- finish a refactor task, then execute the adjacent compile-fix task
- implement a feature task, then take the plan's follow-up validation node

### 8.2 What belongs in a chain

Tasks belong in the same chain when they strongly overlap in:

- files
- anchors
- concepts/contracts
- branch-local implementation state
- recent semantic reasoning

### 8.3 What does not belong in a chain

A chain should not justify:

- holding unrelated urgent work
- monopolizing a whole large plan
- bypassing readiness or claim rules
- skipping review or validation gates

PRISM should optimize for **short useful continuity**, not ownership empires.

---

## 9. Soft Continuation Reservations

### 9.1 Purpose

PRISM should let an agent preserve likely short-term continuation rights without creating a hard
exclusive lock.

The mechanism is a **soft continuation reservation**.

It means:

> "I am executing the prerequisite or current node, and I am likely the best next worker for this
> follow-up node because my context is already warming around it."

### 9.2 Reservation properties

A continuation reservation must be:

- explicit
- time-bounded
- non-exclusive
- revocable by staleness or stronger competing need

It must not be treated as a hard claim.

### 9.3 Reservation fields

PRISM should model continuation reservations in shared coordination with fields like:

- `task_id`
- `reserved_by_principal`
- `reserved_by_runtime`
- `reservation_reason`
  - for example `dependency_continuation`, `same_file_followup`, `validation_followup`
- `reservation_created_at`
- `reservation_expires_at`
- `reservation_source_task_id`
- `reservation_score`

### 9.4 Reservation duration

The reservation TTL should be short.

Recommended default:

- 15 minutes

This is long enough to preserve warm continuation and short enough to avoid starvation.

### 9.5 Reservation semantics

Reservation means:

- other agents should discount the task unless they have a materially stronger reason to take it
- the reserving agent should prefer it strongly when it becomes actionable
- the task remains claimable if the reserving agent stalls or times out

### 9.6 Reservation creation rule

An agent may create a continuation reservation only when:

- it currently owns or just completed the upstream task
- the downstream task is in the same semantic chain
- the downstream task is not blocked by unrelated missing prerequisites

### 9.7 Reservation depth

PRISM should bound continuation reservations to avoid long speculative chains.

Recommended default:

- at most 2 reserved successor tasks beyond the active one

That is enough to preserve useful continuity without allowing one agent to reserve half a plan.

---

## 10. Claim Selection with Reservations

When an agent evaluates a task with an active continuation reservation:

- if the reservation belongs to itself, the task gets a strong positive boost
- if the reservation belongs to another agent, the task gets a negative discount
- if the reserving agent is stale or the reservation is expired, the discount disappears

This is not a hard rule.
It is part of the selection score.

Another agent should still take the task when:

- the reservation expired
- the reserving agent is stale
- the task has materially higher global priority
- the evaluating agent has an overwhelmingly better local fit

---

## 11. Shared Coordination Facts

The shared coordination ref should publish facts that make affinity-aware pull selection possible.

It does not need to publish private prompts or full local context.

It should publish:

- active task claims
- claim holder identity
- runtime identity
- branch identity
- recent task completion lineage
- continuation reservations
- reservation TTL and reason
- durable task priority
- explicit readiness / blockers
- high-level touched anchor or binding summaries already visible in coordination state

This is enough for agents to make good local choices.

---

## 12. What Stays Local

The agent should not publish its whole context window or full private reasoning.

Local only:

- exact prompt contents
- intermediate chain-of-thought
- detailed internal weighting logic
- private scratch summaries

Shared:

- the resulting direct score or reservation signal
- the factual execution metadata that justifies coordination

This keeps the system legible without over-sharing internal reasoning state.

---

## 13. Recommended Selection Procedure

The default PRISM selection loop should become:

1. fetch actionable tasks from shared coordination
2. remove tasks blocked by hard policy or hard claim conflicts
3. build a bounded candidate shortlist
4. directly score each candidate from the current agent context
5. prefer:
   - urgent high-priority tasks
   - strong same-chain continuation tasks
   - tasks with high local fit and low ramp-up cost
6. claim one task
7. optionally place or refresh continuation reservations on immediate follow-up nodes

This is simple enough to implement early and robust enough to ship.

---

## 14. Starvation and Hoarding Guards

PRISM should explicitly prevent warm-context optimization from turning into unfair task capture.

Required guards:

- short reservation TTL
- maximum reservation depth
- priority override for urgent tasks
- reservation decay when the reserving agent stops heartbeating
- no hard exclusivity from reservations
- no reservation on blocked or speculative distant work

This keeps the system responsive to real repo needs.

---

## 15. Interaction with Git Execution

Continuation reservations should complement, not replace, Git execution policy.

Good examples:

- an agent completes an implementation task and reserves the immediate validation task on the same
  branch because branch-local code state is warm
- an agent finishes the first half of a publish chain and reserves the integration-follow-up task
  because it already knows the branch and evidence state

Reservations should not override:

- clean-branch requirements
- publish requirements
- review requirements
- completion policy

Git execution remains a policy gate.
Affinity only influences who should take the next task.

---

## 16. Interaction with Federated Runtime

This design composes naturally with the federated runtime model.

An agent can incorporate runtime-local facts into its direct score, such as:

- whether its current runtime already has the repo hot
- whether the relevant files were recently indexed locally
- whether it already has warm build or validation context
- whether another runtime currently has stronger local affinity

This still does not require a central scheduler.

Each runtime-local agent simply becomes better at evaluating:

> "Should I continue this chain here, or should another runtime likely take it?"

---

## 17. Data Model Additions

PRISM should add explicit coordination fields for:

- `continuation_reservations`
- `reservation_source_task_id`
- `reservation_score`
- `reservation_reason`
- `reservation_expires_at`
- `recent_execution_lineage`

And should expose query surfaces for:

- actionable tasks plus reservation state
- reservation ownership and expiry
- "why this task is a good continuation candidate"
- recent execution lineage per plan

These should be compact read surfaces, not raw verbose logs.

---

## 18. Direct Scoring Contract

PRISM should not require a rigid universal formula.

Instead, the contract should be:

- the agent must produce a bounded numeric score
- the score must be explainable in plain language if queried
- the score must reflect current local context, not static repo metadata alone

Recommended explanation shape:

- `same plan continuation`
- `same files warm`
- `immediate dependency just completed`
- `low ramp-up cost`
- `high confidence to finish cleanly`

This keeps the scoring model flexible while still inspectable.

---

## 19. Default Policy

PRISM should ship with the following default behavior:

- all agents use decentralized pull-based task selection
- agents directly assign `task_fitness` scores to bounded actionable shortlists
- agents may place soft continuation reservations
- reservations expire after 15 minutes by default
- reservations may cover at most 2 immediate successors
- urgent higher-priority work may preempt continuation
- stale reservations are ignored automatically

This should be the standard repo-execution mode.

No manager agent is required.

---

## 20. Why This Is Better

This design keeps the best parts of PRISM's philosophy:

- decentralized coordination
- shared durable facts
- no fragile central scheduler

while fixing a major execution inefficiency:

- agents no longer behave as though every new task is a cold start

The result should be:

- fewer unnecessary context rebuilds
- higher-quality sequential work within a plan
- better throughput across multiple agents
- less manual human steering of "which Codex thread should keep going"

In short:

> PRISM should let agents pull work based on direct local fit, not just global actionability.
> Shared refs publish the facts. Agents score their own best next move.

