# PRISM Home Retention And Garbage Collection

This document defines the retention and cleanup policy for user-local PRISM
state under `~/.prism`.

It complements
[PRISM_HOME_LAYOUT.md](PRISM_HOME_LAYOUT.md)
by turning the layout's cleanup rules into a concrete product policy.

## Goal

PRISM must keep useful local state long enough to preserve warm startup,
cross-session continuity, and local debugging value without allowing
`~/.prism` to grow without bound.

The retention policy must balance:

- preserving expensive-to-rebuild shared runtime state
- reclaiming disposable worktree-local state quickly
- avoiding premature deletion for live or recently used checkouts
- making cleanup behavior explainable and inspectable

## Problem

`~/.prism/repos/` stores several different classes of local state:

- repo-scoped shared runtime authority
- worktree-scoped caches and process handoff files
- logs and rotated traces
- backups and crash leftovers
- feedback and dogfooding data

Those classes do not have the same value, rebuild cost, or safety profile.

Without an explicit retention policy:

- local state grows monotonically across many repos and worktrees
- dead test or temporary workspaces accumulate indefinitely
- users cannot predict what is safe to remove
- the product has no principled answer to "why was this deleted?" or "why was
  this kept?"

## Non-Goals

This policy does not:

- delete repo-published truth in `<repo>/.prism`
- replace per-file log rotation or per-database internal vacuuming
- define cross-machine or server-side retention for future remote backends
- optimize every possible corner case in the first rollout

## Product Principles

The policy should follow these rules:

1. Value-aware retention, not one global TTL.
2. Path disappearance is stronger evidence than age alone.
3. Shared repo runtime state should outlive worktree-local acceleration state.
4. Cleanup must be inspectable with a dry run and an explanation per candidate.
5. Automatic cleanup should prefer obviously safe classes first.
6. Large destructive cleanup should happen on idle or by explicit command, not
   by surprise on every startup.

## State Classes

PRISM home state should be retained by class rather than by one directory-wide
rule.

### Repo-scoped shared runtime state

Examples:

- `~/.prism/repos/<repo_id>/shared/runtime/state.db*`
- `~/.prism/repos/<repo_id>/feedback/validation_feedback.jsonl`

Properties:

- highest rebuild value
- shared across worktrees
- often the best source for warm restart and continuity

Policy:

- keep longest
- only reclaim after all worktrees for the repo are gone or cold
- never reclaim while a live process is bound to the repo

### Worktree-scoped hot state

Examples:

- `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/cache/**`
- `~/.prism/repos/<repo_id>/worktrees/<worktree_id>/mcp/state/**`
- future `acceleration/` contents

Properties:

- cheaper to rebuild
- path-specific
- becomes stale quickly when a checkout disappears

Policy:

- reclaim aggressively when the checkout path no longer exists
- otherwise retain for a moderate cold period

### Logs, rotated traces, backups, crash leftovers

Examples:

- `mcp/logs/**`
- rotated daemon logs
- rotated call logs
- database backups

Properties:

- lowest semantic value
- useful for recent debugging only
- easiest source of unbounded growth

Policy:

- always bound by size and count
- reclaim before deleting higher-value state

## Lifecycle Signals

Retention decisions should be based on explicit signals rather than directory
names alone.

Primary signals:

- `repo.json.last_seen_at`
- `worktree.json.last_seen_at`
- `worktree.json.canonical_root`
- `worktree.json.branch_ref`
- process liveness from runtime state and PID inspection
- existence of the checkout path on disk

Secondary signals:

- directory size by subtree
- recent log writes
- explicit user actions such as `gc --aggressive`

## Metadata Requirements

The existing manifests already store the minimum useful timing fields:

- `repo.json.created_at`
- `repo.json.last_seen_at`
- `worktree.json.created_at`
- `worktree.json.last_seen_at`

That is enough for a first GC implementation.

Follow-up metadata that would improve decisions, but is not required for v1:

- last successful daemon bind timestamp
- last successful shared-runtime open timestamp
- last explicit user query or mutation timestamp by repo
- optional cached byte counts per subtree
- optional GC generation or last sweep timestamp

## Retention Tiers

The correct policy is a tiered lifecycle rather than a single deadline.

### Tier 0: Immediate bounded pruning

Apply automatically with no user intervention:

- rotated daemon logs: bounded by count and total bytes
- rotated call logs: bounded by count and total bytes
- stale URI files and runtime snapshots: delete once their owning process is
  dead and the file is older than one day
- future `acceleration/` data: first eviction target under any pressure

This tier is low-risk and should run automatically.

### Tier 1: Missing-path worktree reclamation

Apply automatically when safe:

- if `worktree.json.canonical_root` no longer exists, the worktree is an orphan
- orphaned worktree-local cache, MCP state, logs, and acceleration data become
  cleanup candidates immediately
- retain shared repo runtime state for now, because another worktree for the
  same repo may still exist

This is the strongest automatic reclaim rule and should ship early.

### Tier 2: Cold worktree retention

For existing but unused worktrees:

- reclaim worktree-local hot state after 30 days without `last_seen_at`
- reclaim the entire worktree directory after 60 days cold if no live process
  references it

This keeps recent checkouts warm while preventing long-term accumulation.

### Tier 3: Cold repo retention

For repo-scoped shared runtime state:

- only consider repo-level cleanup after all worktrees are gone or cold
- reclaim shared runtime state after 90 days cold by default
- allow a more conservative 180 day retention window if product experience
  shows warm-restart value is high for infrequently used repos

This is intentionally much longer than worktree retention because the rebuild
cost is higher.

## Default Policy

The recommended default policy is:

| Class | Default policy |
| --- | --- |
| Rotated logs | bound by size and count immediately |
| Runtime URI files and runtime snapshots | reclaim after owner process is dead and file is older than 1 day |
| Future `acceleration/` data | reclaim first under pressure, or after 7 days cold |
| Worktree cache and MCP local state | reclaim after 30 days cold |
| Entire orphaned worktree dir | reclaim immediately if checkout path is gone and no live process owns it |
| Entire cold worktree dir with existing checkout | reclaim after 60 days cold and no live process owns it |
| Repo-shared runtime state | reclaim after all worktrees are inactive and repo is cold for 90 days |
| Validation feedback logs | retain with repo-shared state on the repo retention window |

## Cleanup Algorithm

GC should run as a class-aware sweep, not as a blind recursive delete.

Recommended sweep order:

1. enumerate repo homes under `~/.prism/repos/`
2. read `repo.json`
3. enumerate worktrees under each repo
4. read each `worktree.json`
5. classify each subtree into:
   - live
   - cold
   - orphaned
   - reclaimable
6. prune bounded logs and stale process artifacts first
7. prune orphaned worktree-local state
8. prune cold worktree-local state
9. prune repo-shared state only if all worktrees are already gone or cold
10. remove empty parent directories

Each deletion candidate should carry a reason string, for example:

- `orphaned worktree path missing`
- `cold worktree cache older than retention window`
- `dead process handoff file older than 1 day`
- `repo shared runtime older than repo retention window with no active worktrees`

## Automatic Versus Manual GC

The product should distinguish safe automatic cleanup from deeper retention
decisions.

### Automatic cleanup

Safe to run opportunistically on startup or daemon idle:

- log pruning
- dead-process MCP state cleanup
- orphaned worktree-local cleanup when the checkout path is missing
- `acceleration/` eviction

### Manual or pressure-triggered cleanup

Better as an explicit action or a disk-pressure response:

- deletion of cold but still existing worktree caches
- deletion of repo-shared runtime state
- deletion of validation feedback history

This avoids surprise deletion of valuable warm state during routine use.

## Pressure Policy

PRISM should support cleanup triggered by total home size, not only by age.

Recommended pressure model:

- if `~/.prism` is below a soft budget, do only Tier 0 and orphan cleanup
- if it exceeds a soft budget, run cold worktree cleanup
- if it exceeds a hard budget, allow repo-level cleanup according to the normal
  safety rules

Suggested initial defaults:

- soft budget: 5 GiB
- hard budget: 10 GiB

These should be configurable and visible to the user.

## Product Surface

The retention policy should be inspectable from the CLI.

Recommended commands:

- `prism home stats`
  - show bytes by repo, worktree, and class
- `prism home gc --dry-run`
  - show planned deletions and the reason for each one
- `prism home gc`
  - apply default retention policy
- `prism home gc --aggressive`
  - use shorter cold windows for manual recovery under pressure
- `prism home gc --repo <repo_id>`
  - narrow cleanup to one repo home

The UI should be able to surface the same information later, but CLI should
land first.

## Safety Rules

GC must never:

- delete `<repo>/.prism`
- delete a worktree directory whose checkout path exists and is held by a live
  process
- delete repo-shared state while any active worktree for that repo is live
- rely only on name heuristics when metadata is present

If metadata is missing or corrupt:

- treat the directory as suspicious
- prefer dry-run reporting over automatic deletion
- allow explicit manual cleanup

## Suggested Rollout

### Phase 1

- add `prism home stats`
- add `prism home gc --dry-run`
- implement Tier 0 pruning
- implement orphaned worktree cleanup

### Phase 2

- implement cold worktree retention windows
- add size accounting and pressure-triggered sweeps

### Phase 3

- implement repo-shared retention windows
- expose configuration knobs
- add UI affordances

## Recommended Decision

The product should adopt this rule of thumb:

- worktree-local state is short-lived
- repo-shared runtime state is long-lived

That is the simplest policy that matches PRISM's storage value model and avoids
the current unbounded-growth behavior without throwing away useful warm state
too early.
