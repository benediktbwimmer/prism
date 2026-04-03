# Path Identity Contract

This document is the contract artifact for `plan:01kn9vcavs4jft9eqw82ysmghq` /
`coord-task:01kn9vd1qxxaht9ap1ygbmd23g`.

Its job is to define one unambiguous path-identity model for PRISM before the
storage and API rewrites land. This is a target contract for the system to
converge on, not a claim that every current implementation detail already
matches it.

## Core Rule

Tracked `.prism` state and shared logical runtime state must never persist
machine-specific absolute filesystem paths.

Portable PRISM state must use anchors and repo-relative paths. Absolute paths
may exist only as local derived execution conveniences in code that is actually
bound to one worktree on one machine.

## Identity Layers

PRISM should distinguish four different identity forms:

1. `AnchorRef`
   The primary semantic reference. Use anchors when the consumer needs to talk
   about repo meaning, ownership, or behavioral scope rather than about a raw
   file location.

2. `FileId`
   The primary internal runtime key for file-backed graph state inside one
   runtime authority. `FileId` is not a portable cross-clone identity on its
   own, so any durable surface that must survive worktree changes needs an
   anchor or repo-relative path alongside it.

3. Repo-relative path
   The canonical stored file identity. This is the portable fallback when a
   file-level reference is required and no richer anchor is available.

4. Absolute path
   A local derived execution path. This exists only so a local worktree can
   touch the filesystem, launch a process, inspect a log, or describe its own
   machine-local runtime environment.

## Allowed By Plane

### Repo-published tracked state

Examples:

- `.prism/state/**`
- protected repo streams under `.prism/**`
- generated repo-published projections derived from tracked state

Rules:

- use anchors for semantic references whenever possible
- use repo-relative paths for durable file identity when a file path is needed
- do not persist absolute paths
- do not require one specific checkout root to interpret the record

### Shared logical runtime state

Examples:

- shared runtime journals and checkpoints that are meant to survive daemon
  restarts
- shared coordination, memory, outcome, or projection records that are reused
  across worktrees for the same repo

Rules:

- use anchors and repo-relative paths as the portable identity forms
- treat `FileId` as an internal key, not as the only durable locator
- do not persist machine-specific absolute paths
- do not encode one worktree root as semantic repo truth

### Local worktree runtime and discovery state

Examples:

- `worktree.json`
- repo/worktree discovery metadata
- daemon launch metadata
- process logs
- runtime files that exist only to reconnect a local bridge to a local daemon

Rules:

- absolute paths are allowed here when they are inherently local operational
  data
- these records are not portable semantic truth
- if a record leaves this local operational plane, the absolute path must be
  stripped or converted to repo-relative form first

### Public query, schema, and documentation surfaces

Rules:

- prefer anchors for semantic references
- expose repo-relative paths when a file path is required
- avoid presenting absolute paths as durable PRISM identity
- if a local-only surface needs an absolute path for debugging, label it as
  local runtime data rather than portable repo state

## Practical Interpretation

When choosing a representation:

- if the reference is semantic, use an anchor
- if the reference is file-level and must be portable, use a repo-relative path
- if the reference is an internal runtime lookup key, use `FileId`
- if the code needs to open a real file on disk, derive an absolute path from
  `worktree_root + repo_relative_path`

The key boundary is:

- portable state may be copied across clones, machines, and worktrees
- local execution state may depend on one machine-local checkout root

Portable state therefore cannot depend on absolute paths.

## Explicit Exceptions

The following are allowed to store absolute paths because they are local
discovery or operations metadata, not portable semantic state:

- `repo.json` locator metadata
- `worktree.json` canonical checkout root metadata
- daemon launch arguments and health metadata
- local daemon and bridge logs
- local cache or temp records whose sole purpose is to reopen machine-local
  files or processes

These exceptions do not extend to tracked `.prism/state/**`, protected
publication payloads, shared logical runtime records, or portable exported
views.

## Current Boundary Targets

The current code already shows the boundary that later nodes need to fix:

- `prism-store::Graph` still keys persisted file state by `PathBuf`
- `ObservedChangeSet.previous_path` and `ObservedChangeSet.current_path` still
  serialize raw path strings
- several MCP read surfaces still expose `workspaceRoot`
- local home metadata intentionally stores canonical repo/worktree locator paths

That means the migration sequence should be:

1. define the contract in docs and specs
2. canonicalize graph and workspace identity around `FileId` plus repo-relative
   paths
3. remove absolute-path leakage from tracked snapshots, shared runtime, and
   public read surfaces
4. keep absolute paths only at the local worktree runtime edge

## Non-goals

This contract does not require:

- removing all absolute paths from logs or machine-local diagnostics
- making `FileId` portable across independent clones
- eliminating local checkout identity from discovery metadata

It does require that those local details stop leaking into durable portable
PRISM state.
