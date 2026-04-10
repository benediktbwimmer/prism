# PRISM Lease Heartbeat V2

Status: proposed design
Audience: PRISM core, coordination, runtime, SQLite, bridge, and MCP maintainers
Scope: automatic lease renewal, compact shared-ref activity summaries, and the narrow `prism run`
flight-recorder slice needed to support them

---

## 1. Summary

PRISM should stop relying on the agent model to remember explicit `heartbeat_lease` mutations.

Instead:

- the write-enabled bridge becomes the authoritative local liveness sensor
- the local runtime persists lease activity in the worktree SQLite store
- the runtime decides when an authoritative shared-ref renewal is actually due
- the shared coordination ref receives only low-frequency compact lease-renewal summaries
- the richer agent activity timeline stays local to the runtime and is exposed through local or
  federated query

This design fixes two current problems at once:

- lease renewal should keep working even when an agent is heads-down in the shell and makes no
  PRISM tool calls
- PRISM should preserve a compact, useful picture of what the agent has been doing without flooding
  the shared ref with high-volume telemetry

The narrow flight-recorder slice in this document is intentionally smaller than the full
[`docs/PRISM_RUN_FLIGHT_RECORDER.md`](PRISM_RUN_FLIGHT_RECORDER.md) vision. It implements only the
pieces needed to make automatic lease renewal reliable and inspectable.

---

## 2. Problem

The current heartbeat model depends on explicit authenticated mutation calls from the agent:

- the lease is renewed through `heartbeat_lease`
- the agent is supposed to be reminded to send that mutation on later PRISM calls
- coordination-only work or shell-heavy work may produce no PRISM reads for long stretches

That is the wrong abstraction boundary.

The bridge already owns the one write-enabled worktree slot. It has a much better view of whether
the agent is still active:

- file writes in the worktree
- explicit wrapped shell commands
- bridge lifetime itself

The runtime already owns the durable local state and the policy for when shared coordination should
change. That makes it the correct place to debounce, rate-limit, and summarize renewal activity.

---

## 3. Design Goals

Required goals:

- eliminate the need for model-driven lease heartbeat reminders
- keep leases alive automatically while the write-enabled bridge is clearly active
- preserve safe expiry when the bridge dies or activity goes cold
- work for both PRISM-heavy and coordination-only coding sessions
- persist local activity in the worktree SQLite DB so restart and crash recovery remain legible
- avoid flooding shared coordination refs with tiny heartbeat writes
- publish compact activity summaries that help other runtimes understand ongoing work
- keep rich detailed telemetry queryable through local and federated runtime queries only

Required non-goals:

- no OS-level spying on arbitrary shell processes
- no requirement to publish raw command lines or detailed file lists into shared refs
- no attempt to preserve an unbounded full activity timeline in shared git-backed coordination state
- no dependence on harness-specific command-execution hooks that PRISM does not control

---

## 4. Core Decision

### 4.1 The bridge should sense activity, not decide policy

The write-enabled bridge should observe trustworthy local activity and write it into runtime-local
state.

It should not write authoritative heartbeat mutations to shared refs directly on every event.

That separation gives PRISM the right control points:

- the bridge is cheap and local
- the runtime owns renewal policy
- shared coordination changes only when truly needed

### 4.2 File activity is the default liveness signal

Recent file writes are the primary signal that an agent is alive and actively working on its task.

That covers the common case with no extra model effort.

### 4.3 Command activity must be explicit

PRISM cannot reliably detect arbitrary shell commands launched outside its control.

Therefore command activity should come from an explicit wrapper:

```sh
prism run -- <command> [args...]
```

Agents should be instructed to use `prism run` for commands that represent meaningful work, such as:

- builds
- tests
- search or grep runs that may take time
- scripts
- git operations
- migrations

Short trivial one-shot reads can remain unwrapped when ergonomics matter.

### 4.4 Shared refs should carry compact renewal summaries, not raw telemetry

Shared coordination refs should publish:

- lease truth
- lease freshness
- compact activity sketches since the last renewal
- compact activity sketches since lease start

They should not publish:

- raw shell command lines
- output chunks
- full file lists
- verbose timelines

### 4.5 Rich activity history should be runtime-local and query-federated

The detailed activity timeline is most useful for real-time inspection of an ongoing agent session.

That should live in the local runtime SQLite store and be queryable by:

- local runtime queries
- federated peer-runtime queries

Shared git-backed history remains the compact authoritative substrate, not the full telemetry store.

---

## 5. Activity Sources

The runtime should treat activity as the union of two primary sources.

### 5.1 File activity

The bridge receives worktree file-write signals from the existing local file-watch path and records
them as lease activity.

Useful local aggregates include:

- write event count
- distinct files changed
- changed-line churn as `added + removed`
- file-kind counts such as code, docs, and config
- per-bucket changed-file and changed-line intensity for histogram construction

File reads are intentionally out of scope for this shared summary surface. PRISM should treat file
activity here as edit activity, not passive inspection.

### 5.2 Command activity

`prism run` records command lifecycle events into the runtime:

- command started
- command still running
- command finished

Useful local aggregates include:

- command count
- command runtime milliseconds
- command-kind counts such as build, test, git, search, and other
- per-bucket command-count and command-time intensity for histogram construction

Command activity should be visible to automatic lease renewal only when it comes from the explicit
wrapper. PRISM should not guess based on the process table.

---

## 6. Runtime Policy

The runtime should renew a lease only when all of the following are true:

- this worktree currently owns the write-enabled bridge slot
- exactly one lease-bearing declared-work target is active
- the current lease-start-aligned renewal tick is due
- recent trusted activity is present
- PRISM has not already renewed too recently

This keeps renewals low-frequency and intentional.

### 6.1 Recent trusted activity

Recent trusted activity should mean at least one of:

- file writes were observed within the active renewal window
- a `prism run` command is still running
- a `prism run` command finished recently enough that the session should still count as active

### 6.2 Renewal timing

The runtime should treat renewal as a scheduled policy decision, not as a direct edge-trigger on
every local event.

The recommended default policy is:

- `heartbeat cadence = 5 minutes`
- `lease ttl = 60 minutes`
- renewal ticks are measured from lease start
- the `n`th tick fires at `lease_started_at + n * 5 minutes`

At each tick:

- if trusted activity occurred in the preceding 5-minute window, publish one authoritative renewal
- if a tracked `prism run` command is still running, publish one authoritative renewal even without
  a recent file write
- if there was no trusted activity in that window, skip the renewal silently

That policy should:

- coalesce bursts of local activity into one renewal per 5-minute window
- avoid publishing empty `0000...`-style heartbeat deltas
- still renew early enough to avoid accidental lease expiry during active work
- let an inactive session expire naturally after 12 skipped windows

### 6.3 Safe stop

If the bridge exits, or trusted activity goes cold long enough, the runtime stops renewing. The
lease then expires naturally and becomes reclaimable by the swarm.

With the recommended defaults, a completely inactive session that skips 12 consecutive 5-minute
windows simply ages out after 60 minutes without needing a special "idle" heartbeat write.

---

## 7. Local SQLite State

The runtime should persist lease activity into the worktree-local SQLite store so the activity
record survives daemon restarts and crashes.

This state is local telemetry, not shared authority.

### 7.1 Lease activity session

PRISM should persist a local row or snapshot keyed to the current lease session, including:

- lease identifier or task identifier
- lease start time
- current renewal sequence
- last local activity time
- last authoritative renewal time
- cumulative file-write totals
- cumulative changed-file totals
- cumulative line-change totals
- cumulative command count
- cumulative command runtime milliseconds
- coarse file-kind totals
- coarse command-kind totals

### 7.2 Lease activity buckets

PRISM should persist rolling bucket state for four activity channels, each in delta and
lease-lifetime form:

- changed files since last renewal
- changed lines since last renewal
- command count since last renewal
- command time since last renewal
- changed files since lease start
- changed lines since lease start
- command count since lease start
- command time since lease start

Each histogram uses 16 time buckets.

The bucket timeline is relative:

- renewal histograms cover the interval from the previous authoritative renewal to now
- lease histograms cover the full interval from lease start to now

Persisting the bucket state locally lets PRISM survive restart without forgetting the cumulative
lease-shape summary.

### 7.3 Narrow `prism run` slice

For this implementation slice, the local flight recorder only needs enough structure to drive
renewal policy and later inspection:

- a durable `command_run` record
- current status
- start and finish time
- duration
- coarse command kind
- work attribution

Full output chunk retention and richer command-tail query are still part of the broader
`PRISM_RUN_FLIGHT_RECORDER` roadmap, but they are not required for the heartbeat v2 cut.

---

## 8. Shared-Ref Activity Summary

Each authoritative lease renewal should publish a very small activity payload.

### 8.1 Histogram strings

Shared coordination should store eight fixed-width histogram strings:

- `fileCountDelta16`
- `lineChangeDelta16`
- `commandCountDelta16`
- `commandTimeDelta16`
- `fileCountLease16`
- `lineChangeLease16`
- `commandCountLease16`
- `commandTimeLease16`

Each string contains 16 hex nybbles.

That yields:

- 16 buckets for recent changed-file breadth since the last renewal
- 16 buckets for recent changed-line volume since the last renewal
- 16 buckets for recent command churn since the last renewal
- 16 buckets for recent command runtime since the last renewal
- 16 buckets for cumulative changed-file breadth since lease start
- 16 buckets for cumulative changed-line volume since lease start
- 16 buckets for cumulative command churn since lease start
- 16 buckets for cumulative command runtime since lease start

This is compact, diff-friendly, and still visually interpretable.

### 8.2 Scale factors

A histogram alone describes shape but not magnitude.

Therefore each histogram should have an accompanying scalar scale factor:

- `fileCountDeltaScale`
- `lineChangeDeltaScale`
- `commandCountDeltaScale`
- `commandTimeDeltaScale`
- `fileCountLeaseScale`
- `lineChangeLeaseScale`
- `commandCountLeaseScale`
- `commandTimeLeaseScale`

Interpretation:

- the 16 nybbles encode a quantized intensity ladder
- the scale factor approximates the max bucket magnitude for that histogram
- together they expose both relative distribution and approximate absolute activity

This is intentionally approximate. Exact accounting belongs in separate totals.

### 8.3 Totals worth keeping

Even with histograms plus scales, a few exact or near-exact scalar totals remain useful:

- `lastActivityAt`
- `commandsSinceLeaseStart`
- `commandTimeMsSinceLeaseStart`
- `filesChangedSinceLeaseStart`
- `lineChangesSinceLeaseStart`
- `writeEventsSinceLeaseStart`

These totals are small, durable, and give consumers easy absolute context without requiring them to
reconstruct it from the histogram approximation.

### 8.4 What shared refs should not include

Shared coordination should not publish:

- raw command lines
- raw command arguments
- full file lists
- command output
- per-event local timeline details

Those belong in the local runtime telemetry plane.

---

## 9. Quantized Histogram Encoding

The bucket encoding should preserve both:

- relative shape across the 16 buckets
- rough absolute magnitude

### 9.1 Input weights

PRISM should first compute absolute per-bucket activity weights.

Recommended inputs:

- file-count buckets: distinct files changed in the bucket
- line-change buckets: changed-line churn in the bucket as `added + removed`
- command-count buckets: commands started or completed in the bucket
- command-time buckets: command runtime attributed into the bucket

### 9.2 Encoding model

The recommended encoding is:

1. compute absolute per-bucket weights
2. find the max bucket weight
3. store that approximate max as the histogram scale factor
4. quantize each bucket by its ratio to that max using a small logarithmic ladder into `0..f`

This has two important properties:

- bursty workloads still show quieter adjacent activity instead of flattening it to zero
- the scale factor preserves enough absolute context to distinguish light and heavy sessions

### 9.3 Why not pure normalization

Pure normalization loses absolute magnitude.

### 9.4 Why not pure raw counts

Pure raw-count buckets compress poorly and make quiet relative structure hard to see when one bucket
dominates.

### 9.5 Why delta and cumulative forms both matter

Per-renewal delta histograms make the git commit history of shared refs informative.

Cumulative lease histograms preserve the coarse whole-lease shape even if older renewal commits are
later compacted away.

Together they give PRISM both:

- recent activity visibility from git history
- coarse lifetime continuity after compaction

---

## 10. History and Query Model

### 10.1 Shared renewal history

PRISM should treat git history itself as the durable history for authoritative lease renewals.

That means:

- each renewal commit preserves the compact delta summary published at that time
- a later history query can reconstruct the recent renewal trail from local git state

This is fast enough because the relevant shared refs already exist locally after fetch or live sync.

### 10.2 Compaction

Shared coordination compaction may eventually remove older fine-grained renewal commits from the
live ref history.

That is acceptable for this activity data. It is primarily valuable for real-time inspection of an
ongoing execution.

The cumulative lease histograms in current shared state preserve coarse whole-lease continuity even
when earlier delta commits are compacted away.

### 10.3 Runtime-local detailed history

If another runtime wants the richer execution picture, it should ask the peer runtime through
federated query.

That richer view can include:

- recent command runs
- running command state
- command tail
- detailed activity timeline
- local file activity timeline

The shared ref is not the source of truth for that level of detail.

---

## 11. Proposed `prism_code` Surface

This feature should be usable without requiring callers to understand the raw shared-ref payloads.

PRISM should expose a small but comprehensive query surface that separates:

- compact authoritative answers from shared refs
- shared-ref history reconstruction from local git
- richer local or federated runtime telemetry

The same logical query shape should work for local and federated runtime inspection whenever
possible. The main difference is the data source, not the caller ergonomics.

### 11.1 Shared-authority current-state queries

These queries should answer the most common coordination questions directly from the latest shared
state, without walking history.

#### `lease_activity_current(...)`

Return the current shared-ref activity summary for one active or recent lease.

Filters should support:

- `lease_id`
- `task_id`
- `worktree_id`
- `runtime_id`
- `principal_id`

Useful response fields:

- lease identity and task identity
- `lease_started_at`
- `last_activity_at`
- current owner principal, runtime, and worktree
- `fileCountDelta16`
- `lineChangeDelta16`
- `commandCountDelta16`
- `commandTimeDelta16`
- `fileCountLease16`
- `lineChangeLease16`
- `commandCountLease16`
- `commandTimeLease16`
- matching scale factors
- `filesChangedSinceLeaseStart`
- `lineChangesSinceLeaseStart`
- `commandsSinceLeaseStart`
- `commandTimeMsSinceLeaseStart`
- whether the lease is still active, due soon, or stale

This query answers questions like:

- how many lines has the current owner changed so far?
- how much command time has the current owner accumulated so far?
- does this lease currently look like editing or validation work?

#### `task_activity_current(...)`

Return the current shared summary for the active lease on a task, plus task-level coordination
context.

Useful response fields:

- all current lease summary fields
- task title and status
- plan identity
- assignee and claim context
- lease expiry time

This is the default “what is happening on this task right now?” query.

#### `runtime_activity_current(...)`

Return the current shared summary for the lease currently owned by a runtime, if any.

This is useful when an agent wants to inspect a peer runtime by runtime id without first looking up
its current task.

#### `plan_activity_current(...)`

Return the current shared summaries for the active leases within one plan.

Useful response fields:

- plan identity and title
- active task count
- active lease count
- per-task compact current summaries
- aggregated current totals across active leases in the plan

This is the “how is this whole plan moving right now?” query.

#### `active_lease_activity_list(...)`

Return a paginated list of active leases with compact current summaries.

Useful sort keys:

- `last_activity_at_desc`
- `line_changes_desc`
- `command_time_desc`
- `commands_desc`
- `files_changed_desc`
- `lease_started_at_desc`
- `expires_at_asc`

Useful filters:

- active only
- stale or nearly stale only
- by principal
- by runtime
- by worktree
- by task status
- by plan

This is the operator or swarm dashboard query.

### 11.2 Shared-history queries from local git

These queries should reconstruct the recent authoritative renewal trail from local git state.

#### `lease_activity_history(...)`

Return the per-renewal shared summaries for one lease in reverse chronological order.

Filters should support:

- `lease_id`
- `task_id`
- `limit`
- `before_commit`
- `after_commit`

Useful response fields per renewal:

- commit id
- published-at timestamp
- delta histograms and scales
- cumulative histograms and scales
- exact cumulative totals at that renewal

This answers:

- what happened in the last few heartbeat windows?
- has the agent gone from editing to validating?
- how has command time accumulated over the life of the lease?

#### `task_activity_history(...)`

Return the sequence of lease activity segments associated with a task, across reclaims or handoffs.

Useful response shape:

- ordered lease segments
- per-segment owner identity
- per-segment cumulative lease totals
- optional reconstructed task-level totals by summing retained lease segments

This is the query that lets PRISM answer:

- how much work has this task accumulated across multiple owners?
- when did ownership change?
- what did each owner do?

#### `lease_activity_rollup(...)`

Return aggregated totals reconstructed from retained shared-ref history.

Supported rollups should include:

- per lease
- per task
- per plan
- per runtime
- per principal

Useful output:

- total files changed
- total line changes
- total command count
- total command time
- first seen and last seen timestamps
- number of retained renewal windows

This gives a cheap “sum the retained authoritative trail” view without forcing the caller to do it.

#### `plan_activity_rollup(...)`

Return aggregated activity totals for a plan across its retained task and lease history.

This is useful for questions like:

- which plan has consumed the most execution effort?
- which plan is still seeing active edit churn?
- which plan has mostly shifted into validation time?

### 11.3 Derived shared-state helper queries

These are convenience queries that derive higher-level answers from the compact histograms and
totals.

#### `task_execution_posture(...)`

Infer the dominant current mode for an active task, such as:

- `editing`
- `validating`
- `mixed`
- `quiet`
- `stale`

Suggested heuristics:

- line-change heavy with low command time implies editing
- high command time with low line change implies validating
- high values in both implies mixed

#### `task_activity_compare(...)`

Compare two or more active tasks by shared activity shape and totals.

Useful comparisons:

- which task is seeing the most edit churn?
- which task is blocked in long validation?
- which task has gone quiet?

#### `runtime_activity_compare(...)`

Compare current runtime activity across active runtimes.

This is useful for peer routing and operator triage.

### 11.4 Local and federated runtime telemetry queries

These queries should expose the richer local flight-recorder and timeline state. The same query
names should work locally and against a peer runtime through federation.

#### `runtime_lease_activity_timeline(...)`

Return a richer local timeline for one lease.

Useful contents:

- recent file-activity events
- bucketized activity windows
- recent command lifecycle events
- current running-command state

This is the detailed “what is the agent doing right now?” query.

#### `runtime_command_runs(...)`

Return the local command-run summaries for a worktree, runtime, task, plan, or lease.

Useful filters:

- `status`
- `task_id`
- `plan_id`
- `lease_id`
- `running_only`
- `since`
- `limit`

Useful fields:

- command id
- coarse command kind
- started at
- finished at
- duration
- status
- attributed task and plan

#### `runtime_command_run(...)`

Return one detailed local command-run record.

This is the lookup query for a specific build, test, or script invocation.

#### `runtime_command_run_tail(...)`

Return the recent output tail for a command, when the broader flight-recorder output retention
surface exists.

This is not required for the minimum heartbeat cut, but the query slot should be reserved in the
design because it is a natural follow-on capability.

#### `runtime_current_activity(...)`

Return the runtime-local current activity state, including:

- whether a wrapped command is currently running
- most recent file activity
- current lease activity window state
- next renewal tick
- whether the lease currently qualifies for automatic renewal

This is the runtime-internal “why will or won’t PRISM renew the lease?” query.

#### `runtime_lease_renewal_diagnostic(...)`

Return a structured explanation of the renewal decision for the current or specified lease.

Useful fields:

- current tick window
- last observed trusted activity
- whether a wrapped command is running
- next renewal tick
- last authoritative renewal time
- whether renewal is currently eligible
- if not eligible, the blocking reason

This is the direct debugging query for “why is my lease not renewing?”

### 11.5 Query design principles

The query surface should follow these rules:

- current-state coordination questions should read the latest shared state only
- history questions should read local git history, not require SQLite mirroring of shared-ref
  commits
- rich timelines and command detail should read runtime-local SQLite state
- federated queries should reuse the same logical API shape as local runtime queries
- all aggregate queries should expose enough identity fields to join results back to task, plan,
  runtime, principal, and worktree surfaces

---

## 12. `prism run` Slice for Heartbeat V2

This design intentionally implements only the narrow slice of `prism run` needed to support lease
renewal and agent observability.

### 11.1 Required behavior

`prism run -- ...` must:

- execute the child command transparently
- stream output live to the caller terminal
- return the child exit status
- persist command-run summary metadata locally
- emit command activity to the runtime so lease renewal policy can treat the command as active work

### 11.2 Required attribution

At command start, PRISM should snapshot the current work context and store:

- work identifier
- work kind
- work title
- parent work identifier when present
- coordination task identifier when present
- plan identifier and plan title when present

This makes command activity legible in both local and federated inspection.

### 11.3 Deferred follow-up

The full flight-recorder roadmap remains valid, including:

- output chunk persistence
- richer retention controls
- broader query UX
- deeper peer-runtime inspection

Heartbeat v2 should not block on that larger surface.

---

## 13. Instruction and UX Changes

PRISM instructions should stop telling agents to remember explicit heartbeat mutations during normal
work.

Instead, instructions should say:

- file edits are observed automatically
- use `prism run -- ...` for meaningful shell commands
- PRISM will renew the lease automatically when the bridge and runtime observe active work

This moves lease continuity out of the model prompt loop and into the runtime where it belongs.

---

## 14. Implementation Plan

### Phase 1: local activity substrate

- add durable lease-activity persistence to the worktree SQLite store
- record file-write activity into per-lease local state
- add the narrow `prism run` command summary path
- classify commands coarsely for aggregate metrics

### Phase 2: runtime renewal policy

- add the renewal scheduler to the runtime
- consume local file and command activity
- renew only when due and when trusted activity is present
- stop renewing automatically when activity goes cold

### Phase 3: shared-ref summary payload

- extend authoritative lease renewal payloads with the four histograms
- add histogram scale factors and minimal exact totals
- keep publication rate-limited and compaction-friendly

### Phase 4: query surfaces

- add the shared-authority current-state queries defined above
- add shared-history reconstruction queries from local git
- add local runtime queries for detailed lease activity and command runs
- add federated peer-runtime access to the same rich local data

---

## 15. Open Questions

The design is intentionally narrow, but a few implementation details still need concrete choices:

- exact SQLite schema shape for persisted lease-activity state
- exact logarithmic ladder used for `0..f` bucket quantization
- exact renewal lead time and cooldown defaults
- exact command-kind taxonomy for shared summary aggregates
- whether short one-shot `prism run` commands should contribute a minimum visible bucket weight

These are implementation choices, not architectural blockers.

---

## 16. Recommended Acceptance Criteria

Heartbeat v2 should be considered successful when all of the following are true:

- an active agent that is editing files but making no PRISM calls keeps its lease alive
- an active agent running long `prism run` commands keeps its lease alive without file edits
- a dead bridge or cold session stops renewing and the lease expires safely
- daemon restart preserves enough local lease activity state to continue with correct cumulative
  summaries
- shared-ref writes remain low-frequency and bounded
- another runtime can understand ongoing work from compact shared summaries
- another runtime can inspect the richer local activity timeline through federated query

That is the target behavior for the heartbeat v2 slice.
