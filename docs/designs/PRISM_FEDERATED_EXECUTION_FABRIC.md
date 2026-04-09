# PRISM Federated Execution Fabric

Status: proposed target design
Audience: PRISM core, coordination, auth, runtime, and MCP maintainers
Scope: repo-scoped distributed compute, delegated execution, and optional remote agent spawning

---

## 1. Summary

PRISM should evolve beyond a repo-aware MCP server into a repo-scoped distributed execution fabric.

The execution fabric builds on:

- shared coordination refs from [PRISM_SHARED_COORDINATION_REFS.md](../PRISM_SHARED_COORDINATION_REFS.md)
- federated runtime discovery and peer transport from
  [PRISM_FEDERATED_RUNTIME_ARCHITECTURE.md](./PRISM_FEDERATED_RUNTIME_ARCHITECTURE.md)

The core idea is:

- shared refs advertise which runtimes are alive, what compute they can donate, and what execution
  capabilities they expose
- runtimes may accept bounded remote jobs such as tests, builds, indexing, or analysis
- runtimes may optionally expose a stronger capability to spawn local delegated agents on demand
- execution usage is metered locally but settled durably through compact signed ledger records
- Git remains the durable coordination and settlement substrate, while rich operational exchange
  stays runtime-local or peer-to-peer

This gives PRISM two new kinds of value:

- live multi-agent collaboration before anything is committed
- repo-scoped distributed compute sharing across humans and machines already participating in the
  repo

---

## 2. Problem

Shared coordination refs solve durable cross-branch truth, but they do not yet solve distributed
execution.

Today PRISM still lacks a first-class model for:

- discovering idle or lightly loaded runtimes that could help with work
- donating machine-local compute to another human or agent working on the same repo
- sending bounded validation or indexing jobs to another runtime
- asking another runtime to host a fresh local agent for interactive collaboration
- recording who donated resources and who consumed them

Without that layer:

- idle machines are wasted even when they already have the repo, daemon, and indexes warm
- long validations remain serial when they could be partitioned cheaply across peers
- sophisticated peer-to-peer workflows require ad hoc manual coordination
- agents cannot explicitly ask for remote help from a daemon that is alive and available

PRISM therefore needs an execution plane above the coordination plane.

---

## 3. Design Goals

Required goals:

- let runtimes advertise execution capacity and execution capabilities through shared coordination
  state
- support bounded remote job execution without requiring a central scheduler
- support optional remote agent spawning as an explicit advertised capability
- keep humans and agents as first-class clients of the same execution fabric
- preserve strict capability gating and explicit budgets
- keep Git and shared refs as durable coordination and settlement truth
- keep high-frequency metering local, not in Git
- make remote execution legible, inspectable, and attributable per principal and per runtime

Required non-goals:

- PRISM should not require every runtime to accept remote jobs
- PRISM should not require every runtime to allow remote agent spawning
- the shared ref should not carry live per-second resource counters
- this design does not require central orchestration or a global queue service
- settlement records should not become a chatty append log in live HEAD

---

## 4. Two Execution Capabilities

The execution fabric should treat remote jobs and remote agent spawning as related but distinct
capabilities.

### 4.1 Remote job execution

Remote jobs are bounded execution requests with explicit inputs and explicit result contracts.

Illustrative job classes:

- `tests`
- `build`
- `index`
- `analysis`
- `benchmark`
- `lint`
- `validation`

These are best for:

- parallel test partitioning
- expensive builds
- semantic indexing
- profiling
- bounded validation work

### 4.2 Remote agent spawning

Remote agent spawning is a stronger capability.

It allows a runtime to host a fresh local agent that can:

- inspect the repo with warm local state
- participate in interactive peer-to-peer collaboration
- negotiate draft APIs or speculative approaches
- assist another stuck agent
- perform shift-change handoffs
- pair with a human through the same runtime fabric

This is not just "run a shell command somewhere else." It is delegated interactive intelligence.

### 4.3 Capability split

Runtimes should advertise these capabilities separately:

- `can_accept_remote_jobs`
- `can_spawn_remote_agents`

Many runtimes may safely expose the first without the second.

---

## 5. Execution Descriptor Model

The runtime descriptor in shared coordination refs should grow an execution section.

Illustrative fields:

- `execution_enabled`
- `available_job_slots`
- `available_agent_slots`
- `max_concurrent_jobs`
- `max_concurrent_agents`
- `supported_job_classes`
- `supported_models`
- `can_accept_remote_jobs`
- `can_spawn_remote_agents`
- `current_load`
- `queue_depth`
- `repo_warmness`
  - for example `cold`, `warm`, `hot`
- `platform`
- `arch`
- `toolchain_hints`
- `budget_policy_ref`
- `settlement_identity`

This should answer:

- who is currently available?
- who can help with test validation?
- who can host a spawned agent?
- who already has the repo or indexes warm?
- who is overloaded and should not receive more work?

### 5.1 Availability semantics

Execution availability should be explicit, not inferred.

Useful states:

- `disabled`
- `idle`
- `available`
- `busy`
- `draining`

This is more useful than merely knowing that a daemon exists.

### 5.2 Budget advertisement

A runtime should be able to advertise bounded budgets such as:

- maximum wall-clock minutes it is willing to donate
- maximum token budget for spawned agents
- maximum concurrent remote jobs
- allowed job classes
- whether remote agent sessions may allocate tools that mutate repo state

This turns execution into an explicit contract instead of an implicit courtesy.

---

## 6. Motivating Workflows

The execution fabric enables several important workflows that shared refs alone cannot solve.

### 6.1 Distributed validation

A controlling runtime can partition a large validation suite across multiple idle runtimes that
advertise `can_accept_remote_jobs`.

Examples:

- split `cargo test` by crate group
- run platform-specific validation on the right machine
- dispatch indexing or analysis slices to already-warm peers

The results return as bounded signed job outcomes and are aggregated into one validation conclusion.

### 6.2 Overnight compute donation

A developer can leave a machine running and expose execution capacity to trusted collaborators on
the same repo.

Examples:

- another developer across the world borrows that idle capacity for a long test suite
- a remote peer reuses a warm index instead of rebuilding it
- a large benchmark job runs on a machine that would otherwise sit idle

This is likely one of the most common practical use cases.

### 6.3 Speculative parallel implementation

Two runtimes can each host a bounded speculative pass on different implementation directions.

After a fixed interval they can exchange:

- uncommitted diffs
- local validation results
- summary diagnostics

Only the winning direction needs to touch Git.

### 6.4 Live API contract negotiation

One runtime can host a producer-side delegated agent while another hosts a consumer-side delegated
agent.

They can exchange bounded draft contract packets such as:

- type signatures
- schema fragments
- sample payload shapes
- local validation failures

This lets interfaces converge before any durable publish step.

### 6.5 Human-agent live pairing

Humans should be able to act as execution-fabric peers too.

A human using PRISM CLI or a future UI should be able to:

- inspect a runtime's current execution packet
- request bounded remote jobs
- attach to a delegated remote agent session
- send corrective or prioritizing input into that session

This keeps human intervention inside the same coordination and capability model.

---

## 7. Job Model

Remote jobs should be first-class execution objects.

### 7.1 Job shape

Illustrative job fields:

- `job_id`
- `repo_id`
- `requesting_principal`
- `hosting_runtime_id`
- `job_class`
- `title`
- `input_ref`
- `expected_outputs`
- `budget`
- `lease_expires_at`
- `created_at`
- `accepted_at`
- `completed_at`
- `failed_at`
- `result_ref`
- `settlement_ref`

### 7.2 Job lifecycle

Illustrative states:

- `proposed`
- `queued`
- `accepted`
- `running`
- `completed`
- `failed`
- `expired`
- `cancelled`

### 7.3 Result model

Jobs should return structured results, not only raw logs.

Illustrative result classes:

- pass/fail summary
- structured validation outcome
- produced artifact pointer
- log bundle pointer
- timing data
- settlement summary

Large payloads should be exported as bundles or artifacts, not embedded into shared refs.

---

## 8. Remote Agent Model

Spawned remote agents should also be first-class execution objects, not hidden implementation
details.

### 8.1 Agent session shape

Illustrative fields:

- `agent_session_id`
- `requesting_principal`
- `hosting_runtime_id`
- `spawned_agent_kind`
- `model`
- `budget`
- `task_binding`
- `capabilities`
- `started_at`
- `last_seen_at`
- `handoff_ref`
- `result_ref`
- `settlement_ref`

### 8.2 Spawn contract

A runtime that advertises `can_spawn_remote_agents` should define:

- what models it can host
- how many spawned agents it can run concurrently
- which tools those agents may use
- whether those agents may mutate repo state
- which principals are allowed to request them

### 8.3 Why spawning matters

Many high-value workflows need more than a bounded shell job:

- unsticking another agent interactively
- pair-debugging a weird integration bug
- speculative parallel solution design
- live contract negotiation
- shift-change handoffs
- human-agent pairing

These are exactly the cases where a spawned local agent is useful.

---

## 9. Capability and Trust Model

Execution capabilities must be explicit and repo-scoped.

Illustrative capabilities:

- `can_discover_runtime`
- `can_request_remote_jobs`
- `can_accept_remote_jobs`
- `can_spawn_remote_agents`
- `can_attach_to_remote_agent`
- `can_read_remote_diagnostics`
- `can_read_remote_handoff_packets`
- `can_receive_settlement_records`

### 9.1 Trust rules

Trust should be grounded in:

- PRISM principal identity
- explicit capability policy
- shared-ref runtime descriptors
- end-to-end authenticated transport

The main trust anchor should be repo participation plus explicit capability policy.

Concretely:

- a principal should generally need to be recognized as an allowed participant for the repo
- that alone should not automatically grant all execution capabilities
- sensitive actions such as remote jobs, remote agent spawning, diagnostics reads, or handoff reads
  should still be gated by explicit repo-scoped capability policy

No runtime should infer permission merely from "this principal has touched the repo before."

### 9.2 Public internet transport does not require a hosted relay

When execution or agent traffic crosses the public internet:

- a runtime may simply expose a public endpoint through an external tunnel or forwarding service
  such as `ngrok`, Tailscale Funnel, Cloudflare Tunnel, or a directly managed public URL
- a hosted PRISM relay is optional, not required
- the important contract is that the remote runtime advertises a reachable endpoint in its runtime
  descriptor and that the endpoint enforces PRISM's signed request protocol

If a relay exists later, it should still be transport only, not a new authority plane.

### 9.3 Authentication should stay simple but explicit

PRISM does not need a heavyweight new auth stack for this layer.

A strong baseline is:

- each runtime has a local signing key tied to its PRISM principal identity
- every execution request is signed by the requesting principal
- the hosting runtime verifies the signature locally
- the hosting runtime checks whether that principal is trusted for the repo and whether the
  requested capability is allowed
- unknown or unauthorized callers are dropped immediately

This keeps the model simple, local-first, and repo-native.

### 9.4 What actually needs hardening

The hard part is mostly protocol discipline, not inventing stronger cryptography.

Requests should be bound to:

- the exact request body
- the target repo id
- the target runtime id
- a nonce
- an expiry timestamp

This gives protection against:

- replay attacks
- cross-repo confused-deputy mistakes
- cross-runtime misrouting
- capability ambiguity

So "hardening" in this design should mean:

- request expiry and nonce validation
- exact body signing
- repo/runtime binding in the signed envelope
- explicit capability checks
- bounded budgets and scopes for jobs and spawned agents

not a new centralized auth service.

---

## 10. Budget Model

Budgets are the gating mechanism for both jobs and spawned agents.

### 10.1 Local runtime budget

Each runtime should enforce its own local limits, for example:

- max concurrent job slots
- max concurrent agent slots
- max wall-clock donation per period
- max token donation per period
- allowed job classes
- allowed principals

### 10.2 Requested execution budget

Every request should carry an explicit requested budget, for example:

- max wall-clock time
- max CPU time
- max token budget
- max artifact size
- max fan-out

This makes execution refusal understandable and auditable.

### 10.3 Settlement budget vs admission budget

Admission budgets and settlement records should stay separate.

- admission budget controls what the runtime is willing to start
- settlement records describe what was actually consumed

That keeps policy separate from accounting.

---

## 11. Settlement Ledger

The execution fabric should include a durable signed settlement ledger.

### 11.1 Why a ledger exists

The ledger answers:

- which principal donated compute to the repo?
- which principal consumed it?
- how much CPU time, wall time, and token budget was used?
- which runtime hosted the work?
- which jobs or agent sessions were responsible?

This makes the fabric legible and fair without requiring a central billing service.

### 11.2 What should be recorded

Settlement records should be compact signed summaries, not live meter ticks.

Illustrative fields:

- `settlement_id`
- `repo_id`
- `principal_id`
- `runtime_id`
- `counterparty_principal_id`
- `job_or_agent_ref`
- `job_class`
- `wall_time_ms`
- `cpu_time_ms`
- `tokens_in`
- `tokens_out`
- `normalized_compute_units`
- `recorded_at`
- `signature`

### 11.3 What should not be recorded live

Do not publish:

- per-second token counters
- streaming CPU deltas
- every shell event
- every prompt/response fragment

That detail stays in local metering logs.

### 11.4 Normalized compute units

The ledger should likely treat `normalized_compute_units` as the main comparable accounting metric,
with raw CPU and token fields as supporting evidence.

Reason:

- raw tokens are not comparable across providers or models
- CPU time alone ignores inference cost
- one normalized unit makes later scheduling and fairness policies easier

### 11.5 Ledger publication

Settlement records should be published through compact signed summaries on shared refs or
repo-published state, depending on which ledger scope PRISM ultimately chooses.

The important rule is:

- detailed meter logs stay local
- compact signed settlements become durable shared truth

---

## 12. Artifact and Result Transfer

Not all execution results are tiny.

### 12.1 Small results

Small results may travel directly through peer transport:

- pass/fail summaries
- bounded diagnostics
- structured validation outcomes

### 12.2 Large results

Large outputs should use artifact pointers or export bundles:

- build artifacts
- large logs
- big profile outputs
- large index shards

This avoids overloading shared refs or peer-control messages with bulky payloads.

### 12.3 Provenance

Artifacts and results should remain attributable to:

- the requesting principal
- the hosting runtime
- the exact job or agent session
- the git/base context used

That is necessary if PRISM later wants to trust or reuse remote outputs.

---

## 13. Failure Model

Execution failures must be explicit and non-corrupting.

### 13.1 Runtime refusal

A runtime may refuse a request because:

- it is overloaded
- budget would be exceeded
- the capability is not allowed
- the repo state is not acceptable

Refusal is normal behavior, not a protocol failure.

### 13.2 Mid-execution loss

If a hosting runtime disappears:

- the job or agent session should become expired or failed
- any already-exported partial results remain attributable
- shared coordination should show the loss honestly

### 13.3 Settlement disagreement

If settlement publication fails:

- local metering still exists
- the job result may still be valid
- the accounting record is missing and should be retried explicitly

Settlement is important, but it should not silently rewrite job outcomes.

---

## 14. Relationship to the Existing Federated Runtime Design

The execution fabric depends on the federated runtime design, but it is not the same thing.

- the federated runtime architecture defines storage, authority, and peer transport
- the execution fabric defines how runtimes donate compute, accept work, and host delegated agents

Execution should therefore be treated as a higher layer on top of:

- shared coordination refs
- runtime descriptors
- peer transport
- export bundles

not as a replacement for them.

---

## 15. Migration Plan

### Phase 1: execution descriptors

Extend runtime descriptors with explicit execution capabilities, capacity, and local budget
advertisement.

### Phase 2: bounded remote jobs

Implement remote job request, acceptance, execution, and result return for classes like tests,
builds, indexing, and analysis.

### Phase 3: settlement ledger

Add local metering plus signed compact settlement publication.

### Phase 4: remote agent spawning

Implement optional hosted remote agents as an explicit capability with bounded budgets and attach
semantics.

### Phase 5: richer collaborative flows

Add speculative execution sessions, live contract negotiation packets, shift-change handoff packets,
and human-agent pairing on top of the remote agent substrate.

---

## 16. Testing Requirements

Implementation should add coverage for:

- runtime descriptor capacity advertisement
- job admission and refusal
- budget enforcement for jobs and agents
- multi-runtime test partitioning
- signed settlement publication
- settlement retry after transient failure
- remote agent spawn success and denial
- human attach or inspect flows
- artifact provenance and result attribution
- graceful degradation when only some runtimes expose execution capability

---

## 17. Recommendation

PRISM should adopt a federated execution fabric as the next layer above shared coordination refs and
federated runtime discovery.

The recommended shape is:

- shared refs for execution discovery and durable settlement
- local runtimes for actual compute donation and metering
- bounded remote jobs as the default execution primitive
- optional remote agent spawning as a stronger explicit capability
- signed settlement summaries for fairness and accountability

That would make PRISM useful not only as a coordination system, but as a repo-scoped distributed
compute and collaboration fabric.
