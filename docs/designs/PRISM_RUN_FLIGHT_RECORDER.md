# PRISM Run Flight Recorder

## Status

Proposed design.

## Goal

PRISM should provide a user-space command wrapper:

```sh
prism run -- <command> [args...]
```

The wrapper must:

- execute the command transparently
- stream `stdout` and `stderr` to the invoking terminal in real time
- record the command and its output into the local PRISM runtime SQLite store
- preserve enough structure for later `prism_code` access and future peer runtime inspection

This is intentionally not OS-level process monitoring. It is a deterministic, explicit execution
surface for agent-invoked shell work.

## Why

PRISM already records:

- MCP call history
- runtime logs
- coordination state
- outcome and validation history

But it does not yet provide durable memory for the shell commands an agent actually ran while
debugging a task.

That leaves a real observability gap:

- an agent may run `cargo test`, `cargo check`, `npm test`, or custom scripts repeatedly
- the output may scroll out of context
- another runtime or later session cannot reliably inspect the command history
- peer triage and stuck-agent diagnosis are much weaker than they should be

`prism run` closes that gap while staying explicit and portable.

## Non-goals

- no kernel or OS instrumentation
- no attempt to observe arbitrary shell commands not launched through `prism run`
- no global process spying
- no requirement that command output live forever
- no coupling of command logs to authoritative coordination truth

The command log is runtime telemetry, not repo authority.

## User experience

### Primary invocation

```sh
prism run -- cargo test -p prism-core
prism run -- cargo build --release -p prism-cli -p prism-mcp
prism run -- ./scripts/benchmark.sh
```

Behavior:

- PRISM spawns the command as a child process
- `stdout` and `stderr` are streamed live to the terminal
- terminal exit status mirrors the child exit status
- command execution is recorded in the local runtime store

The wrapper should feel transparent. Agents should not experience buffering or delayed output.

### Instruction update

PRISM instructions should add a strict rule for direct shell execution:

> When invoking shell commands directly, especially tests, builds, scripts, or validation steps,
> use `prism run -- ...` instead of running the command raw. This ensures command history and
> output are recorded in runtime memory for later retrieval and triage.

This update should flow through the instruction markdown sources under
[`docs/prism/instructions`](../prism/instructions), not through
an ad hoc standalone file.

## Data model

The command flight recorder should live in the local runtime SQLite store.

### Table: `command_run`

One row per command execution.

Recommended fields:

- `id TEXT PRIMARY KEY`
- `repo_id TEXT NOT NULL`
- `worktree_id TEXT NOT NULL`
- `runtime_id TEXT`
- `principal_id TEXT`
- `session_id TEXT`
- `command TEXT NOT NULL`
- `argv_json TEXT NOT NULL`
- `cwd TEXT NOT NULL`
- `shell TEXT`
- `status TEXT NOT NULL`
  - `running`
  - `succeeded`
  - `failed`
  - `signaled`
  - `aborted`
- `exit_code INTEGER`
- `signal TEXT`
- `started_at TEXT NOT NULL`
- `finished_at TEXT`
- `duration_ms INTEGER`
- `last_output_at TEXT`
- `last_heartbeat_at TEXT`
- `stdout_bytes INTEGER NOT NULL DEFAULT 0`
- `stderr_bytes INTEGER NOT NULL DEFAULT 0`
- `stdout_line_count INTEGER NOT NULL DEFAULT 0`
- `stderr_line_count INTEGER NOT NULL DEFAULT 0`
- `stdout_tail TEXT`
- `stderr_tail TEXT`
- `stdout_truncated INTEGER NOT NULL DEFAULT 0`
- `stderr_truncated INTEGER NOT NULL DEFAULT 0`
- `work_id TEXT`
- `work_kind TEXT`
- `work_title TEXT`
- `work_summary TEXT`
- `parent_work_id TEXT`
- `coordination_task_id TEXT`
- `plan_id TEXT`
- `plan_title TEXT`

Indexes:

- `(repo_id, worktree_id, started_at DESC)`
- `(status, started_at DESC)`
- `(exit_code, started_at DESC)`
- `(coordination_task_id, started_at DESC)`
- `(plan_id, started_at DESC)`
- `command` should support substring search later, either through:
  - a normal indexed text prefix path for cheap filters, or
  - a future FTS table if needed

### Table: `command_output_chunk`

Append-only live output chunks for active and recent commands.

Recommended fields:

- `command_run_id TEXT NOT NULL`
- `seq INTEGER NOT NULL`
- `stream TEXT NOT NULL`
  - `stdout`
  - `stderr`
- `emitted_at TEXT NOT NULL`
- `text TEXT NOT NULL`
- `byte_len INTEGER NOT NULL`
- `line_count INTEGER NOT NULL`

Primary key:

- `(command_run_id, seq)`

Indexes:

- `(command_run_id, seq)`
- `(emitted_at DESC)`

### Why two tables

`command_run` gives a durable summary and queryable metadata.

`command_output_chunk` gives:

- live tailing
- in-progress command inspection
- partial retrieval for peer triage

The chunk table should be treated as hot telemetry with bounded retention.

## Attribution and work context

The existing PRISM work-context machinery is sufficient for attribution.

Today:

- MCP session state tracks `current_work`
- `declare_work` establishes that context
- the workspace session binds it as active work context
- provenance snapshots already carry:
  - `work_id`
  - `kind`
  - `title`
  - `summary`
  - `parent_work_id`
  - `coordination_task_id`
  - `plan_id`
  - `plan_title`

`prism run` should snapshot that active work context at command start and store it directly on the
`command_run` row.

That enables future queries like:

- commands for the current worktree
- commands for the current task
- commands for the current plan
- commands for delegated child work

This fits well with the one-agent-per-worktree model. The worktree, runtime, and active work
binding make command attribution legible.

## Execution flow

### Start

When `prism run -- cargo test` begins:

1. resolve the current repo/worktree/runtime identity
2. resolve current session and active work context if available
3. insert a `command_run` row with:
   - `status = "running"`
   - `started_at`
   - command metadata
   - work-context metadata
4. spawn the child process
5. start streaming output

### Live streaming

While the child runs:

- read child `stdout`
- read child `stderr`
- write through to the caller terminal immediately
- append output chunks into `command_output_chunk`
- periodically update the parent `command_run` row with:
  - `last_output_at`
  - `last_heartbeat_at`
  - cumulative byte counts
  - cumulative line counts
  - rolling stdout/stderr tails

The write policy should be output-driven first, not timer-driven alone.

### Completion

When the child exits:

1. determine final status
2. write:
   - `finished_at`
   - `duration_ms`
   - `exit_code` or `signal`
   - final status
   - final byte/line counters
   - final tails
3. return the child exit status to the caller

## Streaming policy

### Real-time terminal passthrough

PRISM must not wait for process completion before showing output.

Requirements:

- line-buffered behavior should feel natural
- long-running tests/builds should display output incrementally
- both `stdout` and `stderr` should preserve order within their own streams
- inter-stream ordering only needs to be approximately wall-clock correct

### Chunking

Recommended approach:

- accumulate bytes until one of:
  - newline seen
  - chunk byte budget reached
  - flush timeout expires
- then write one chunk

This avoids:

- one-row-per-byte churn
- pathological SQLite write amplification
- giant monolithic blobs

Suggested initial defaults:

- target chunk size: 2-8 KB
- forced flush interval while output is active: 100-250 ms
- heartbeat update interval while quiet but still running: 1-2 s

These numbers are operational defaults, not protocol constraints.

## Query surface

The schema should be designed now for a future `prism_code` API.

### Top-level namespace

Recommended namespace:

- `prism.runtime.commands`

This should be separate from MCP call logs, because it represents user-space shell execution, not
MCP transport.

### Proposed API

#### `prism.runtime.commands.recent(...)`

Return recent command runs.

Example:

```ts
prism.runtime.commands.recent({
  limit: 20,
  status: "failed",
  worktree: "current",
})
```

Filters:

- `limit`
- `status`
- `exitCode`
- `contains`
- `since`
- `worktree`
- `repoScope`
- `coordinationTaskId`
- `planId`

#### `prism.runtime.commands.search(...)`

Search by command text and metadata.

Example:

```ts
prism.runtime.commands.search({
  contains: "cargo test",
  exitCode: 101,
  worktree: "current",
})
```

#### `prism.runtime.commands.running(...)`

Return in-progress commands.

Example:

```ts
prism.runtime.commands.running({
  olderThanMs: 30000,
  repoScope: true,
})
```

This is a key triage surface for peer runtimes and stuck-agent detection.

#### `prism.runtime.commands.tail(...)`

Return the latest output for a run.

Example:

```ts
prism.runtime.commands.tail({
  commandRunId: "cmdrun:...",
  lines: 80,
  stream: "both",
})
```

Should support:

- `stdout`
- `stderr`
- merged view

#### `prism.runtime.commands.forTask(...)`

Return commands tied to a coordination task or active work item.

Example:

```ts
prism.runtime.commands.forTask({
  coordinationTaskId: "coord-task:...",
  limit: 20,
})
```

This depends on work-context attribution and is one of the highest-value views.

#### `prism.runtime.commands.lastFailed(...)`

Convenience helper for the most recent failed command in scope.

Example:

```ts
prism.runtime.commands.lastFailed({
  worktree: "current",
})
```

### Returned shape

A `command_run` result should include:

- command metadata
- work-context metadata
- duration/status
- compact output summary
- whether full chunk history is still retained
- maybe a `tailAvailable` / `chunkRetentionState` field

### Repo-wide and peer-ready semantics

For local query scope:

- `worktree: "current"` should be default
- repo-wide views should aggregate across all local worktrees for the repo

For future peer runtime support:

- running-command views should be exposeable through bounded peer reads
- tail views should support last N lines without requiring full chunk export

## Retention and compaction

This is the part that must be disciplined from day one.

### Principle

`command_run` is durable enough to keep longer.

`command_output_chunk` is hot telemetry and must be budgeted.

### Required behavior

- keep all chunks for currently running commands
- keep recent chunks for recently finished commands
- compact older runs into the summary stored on `command_run`
- prune old chunks aggressively once summary data is preserved

### Suggested v1 policy

- keep full chunks for:
  - all running commands
  - the most recent 200 completed commands per worktree
  - or the last 7 days, whichever is smaller
- always retain `command_run` summary rows longer
- retain failed commands more aggressively than successful ones

### Suggested summary fields

When a run completes or is compacted, ensure `command_run` preserves:

- `stdout_tail`
- `stderr_tail`
- total bytes
- total line counts
- truncation flags
- maybe first/last failure excerpts later

### Future compaction job

A background maintenance job may:

1. find completed runs beyond chunk retention policy
2. ensure summary tails are present on `command_run`
3. delete old `command_output_chunk` rows
4. mark the run as compacted

### Query semantics after compaction

Readers must be able to tell the difference between:

- full chunk history retained
- only tail retained
- no output retained

That avoids misleading triage behavior.

## Failure and recovery behavior

### PRISM crash during command execution

If PRISM dies while a command is running:

- the `command_run` row may remain `running`
- startup recovery should detect stale in-progress runs whose child process no longer exists
- those rows should transition to a terminal status like:
  - `aborted`
  - or `unknown_termination`

### Child process detached or daemon restart

The first implementation should avoid pretending to supervise commands across CLI process death.

If the `prism run` wrapper exits unexpectedly, it is acceptable for v1 that:

- the run is marked abnormal
- future process adoption is out of scope

## Privacy and safety

Command output may contain:

- secrets
- credentials
- environment-derived tokens
- private file paths

So:

- output should remain local by default
- peer access must be capability-scoped later
- command logs must not automatically become repo-published state

If future peer reads are added, they should expose bounded query/tail surfaces, not unrestricted DB
access.

## Scope split

### V1

- `prism run -- ...`
- live terminal passthrough
- `command_run` and `command_output_chunk` SQLite storage
- work-context attribution
- basic chunk retention/compaction
- query-ready schema
- instruction updates that tell agents to use it

### V1.1

- initial `prism_code` support:
  - `recent`
  - `search`
  - `running`
  - `tail`
  - `lastFailed`

### V1.2

- task/plan-scoped command queries
- peer-observable bounded runtime command reads
- stuck-command heuristics and triage helpers

## Why this is the right first implementation

Before harnesses or external execution environments cooperate with PRISM, this is the cleanest
workable model.

It is:

- explicit
- portable
- deterministic
- compatible with existing PRISM work-context provenance
- useful for both memory and live observability

It does not require:

- kernel tracing
- OS-specific process inspection
- invasive monitoring
- mandatory central logging infrastructure

That makes it a strong first-class observability primitive for PRISM’s federated runtime model.
