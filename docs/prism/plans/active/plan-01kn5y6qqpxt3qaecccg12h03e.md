# Implement the protected repo .prism state signatures design from docs/PROTECTED_PRISM_STATE_SIGNATURES.md, covering signed protected streams, trust-bundle verification, fail-closed hydration, protected write-path enforcement, migration semantics, and validation coverage.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:4d9d4f3dd899af2eef69c1d6b57922c98fecc825362b16d9919cce70e5d78ba5`
- Source logical timestamp: `unknown`
- Source snapshot: `9 nodes, 14 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn5y6qqpxt3qaecccg12h03e`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `9`
- Edges: `14`

## Goal

Implement the protected repo .prism state signatures design from docs/PROTECTED_PRISM_STATE_SIGNATURES.md, covering signed protected streams, trust-bundle verification, fail-closed hydration, protected write-path enforcement, migration semantics, and validation coverage.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn5y6qqpxt3qaecccg12h03e.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn5y6qqpxt3qaecccg12h03e.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn5ydetjt5w7xfn8x71qk7x7`

## Nodes

### Inventory current unsigned repo publication and hydration paths

- Node id: `coord-task:01kn5ydetjt5w7xfn8x71qk7x7`
- Kind: `investigate`
- Status: `completed`
- Summary: Map the existing unsigned JSONL append/load paths, current authenticated mutation seams, and behavior-changing tests that will need migration or replacement under the protected-state design.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:100`
- Anchor: `file:108`
- Anchor: `file:113`
- Anchor: `file:77`
- Anchor: `file:78`
- Anchor: `file:79`
- Anchor: `file:90`

#### Acceptance

- Call sites for repo event append/load and published-plan sync are identified. [any]
- Existing tests that currently accept external edits or legacy unsigned hydration are identified as migration targets. [any]

#### Tags

- `inventory`
- `planning`
- `protected-state`

### Define the protected-state foundation and canonical envelope model

- Node id: `coord-task:01kn5ydewgd87bncbkdehbrd41`
- Kind: `decide`
- Status: `completed`
- Summary: Introduce the shared protected-state subsystem that freezes the v1 protected stream set, verification statuses, canonical JCS envelope schema, hash/signature rules, predecessor linkage, and stream-family abstractions used by all protected repo `.prism` logs.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:340`
- Anchor: `file:88`
- Anchor: `file:99`

#### Acceptance

- One protected-state module owns stream ids, verification statuses, envelope/payload types, and canonical serialization rules. [any]
- The design keeps `lib.rs` as a facade and places substantive logic in dedicated submodules. [any]
- Protected authoritative scope is fixed to the spec’s v1 stream set and does not silently expand. [any]

#### Tags

- `foundation`
- `protected-state`
- `schema`

### Extend the shared-runtime trust plane with signing keys and trust bundles

- Node id: `coord-task:01kn5ydey4tjr0n6na2ymkejfj`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented repo-external trust state, runtime signing key persistence/selection, trust bundle bootstrap/import/export/verification, and verified the resulting protected-state path with full workspace validation.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:110`
- Anchor: `file:405`
- Anchor: `file:51`
- Anchor: `file:96`
- Anchor: `file:98`

#### Acceptance

- Runtime authority ids, runtime key ids, trust bundle ids, public verification metadata, and revocation semantics exist outside repo `.prism`. [any]
- The active runtime key for new writes is selected from non-revoked trust state. [any]
- Portable trust-bundle import/export flows exist for fresh-machine verification. [any]

#### Tags

- `protected-state`
- `shared-runtime`
- `trust`

### Build the shared protected-stream append and verify engine

- Node id: `coord-task:01kn5ydezjztsey6m03tvhrgn3`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented canonical JCS hashing, Ed25519 protected envelopes, protected JSONL append/verify classification, predecessor chaining, tamper/conflict/truncation detection, and deterministic legacy migration primitives.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:100`
- Anchor: `file:340`
- Anchor: `file:99`

#### Acceptance

- Protected streams are verified as `Verified`, `LegacyUnsigned`, `UnknownTrust`, `Tampered`, `Corrupt`, `Truncated`, `Conflict`, or `MigrationRequired`. [any]
- Append rejects orphan tails, duplicate ids, bad predecessor hashes, and non-verified stream states. [any]
- Legacy migration emits a signed `LegacyImported` baseline and archives unsigned bytes outside repo `.prism`. [any]

#### Tags

- `migration`
- `protected-state`
- `verification`

### Migrate repo knowledge streams onto the protected engine

- Node id: `coord-task:01kn5ydf14v5ce5m5qbd22ykbe`
- Kind: `edit`
- Status: `completed`
- Summary: Moved repo concepts, contracts, concept relations, and memory streams onto signed protected events with fail-closed hydration of verified data only.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:108`
- Anchor: `file:77`
- Anchor: `file:78`
- Anchor: `file:79`
- Anchor: `file:84`
- Anchor: `file:90`

#### Acceptance

- Repo-scoped concept, contract, memory, and concept-relation publication uses signed protected events instead of raw filesystem edits as authority. [any]
- Hydration of these streams is fail-closed and only projects verified data into hot state. [any]
- Derived docs or convenience artifacts remain regenerated outputs, not authoritative mutation inputs. [any]

#### Tags

- `hydration`
- `knowledge-streams`
- `protected-state`

### Rework published plan persistence into signed per-plan streams with derived indexes

- Node id: `coord-task:01kn5ydf2qhfjjb50zav19rvm0`
- Kind: `edit`
- Status: `completed`
- Summary: Reworked plan persistence so authoritative repo truth lives in signed per-plan streams under .prism/plans/streams with derived index and mirror artifacts.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:100`
- Anchor: `file:108`
- Anchor: `file:66`
- Anchor: `file:80`

#### Acceptance

- Per-plan streams become the only authoritative repo plan truth in v1. [any]
- Index and active/archived placement are derived from verified streams, not mutation inputs. [any]
- Conflict and reconcile semantics are explicit and prevent silent continuation on diverged valid heads. [any]

#### Tags

- `coordination`
- `plans`
- `protected-state`

### Integrate verification gates into hydration, mutation, and watcher handling

- Node id: `coord-task:01kn5ydf4876gvnh5c20zaw9w6`
- Kind: `edit`
- Status: `completed`
- Summary: Integrated protected-stream verification into hydration and authoritative write paths so unsigned, tampered, conflicting, truncated, or unknown-trust streams are refused as authoritative inputs.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:108`
- Anchor: `file:116`
- Anchor: `file:118`
- Anchor: `file:84`

#### Acceptance

- Authoritative writes refuse `Conflict`, `Corrupt`, `Truncated`, `Tampered`, `UnknownTrust`, `LegacyUnsigned`, and `MigrationRequired` streams as required by the spec. [any]
- Hydration only projects verified protected data into hot memory and clearly labels diagnostic-only access to raw invalid content. [any]
- Watcher loop suppression uses recent self-write matching and never treats filesystem observation as identity proof. [any]

#### Tags

- `protected-state`
- `watchers`
- `write-gates`

### Expose operator tooling and diagnostics for verification, migration, repair, and trust workflows

- Node id: `coord-task:01kn5ydf5tdhdry9ftd4wjwq7c`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented protected-state operator tooling and diagnostics across prism-core, prism-cli, and prism-mcp: verify/diagnose/trust import-export/quarantine/repair/reconcile CLI flows, a `prism://protected-state` MCP resource with per-stream verification metadata, and matching schema/capability/example/test coverage.
- Priority: `2`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:227`
- Anchor: `file:229`
- Anchor: `file:251`
- Anchor: `file:51`

#### Acceptance

- CLI surfaces exist for verification, migration, trust import/export, quarantine, repair, and reconcile flows listed in the spec. [any]
- Read and diagnostic surfaces include verification status, stream id, protected path, last verified event/hash, trust bundle id, diagnostic code/summary, and repair hint. [any]
- Normal hydration never auto-heals protected streams; repair and reconcile remain explicit operator actions. [any]

#### Tags

- `cli`
- `diagnostics`
- `mcp`
- `protected-state`

### Validate migration semantics, tamper handling, and full-workspace behavior

- Node id: `coord-task:01kn5ydf7fed6cqt4xw7jftw6g`
- Kind: `validate`
- Status: `completed`
- Summary: Validation completed for the protected-state rollout. Targeted prism-core operator tests, prism-cli protected-state command tests, focused prism-mcp protected-state resource tests, `cargo test -p prism-mcp --quiet`, and `cargo test --workspace --quiet` all passed, with the known `queries_defer_request_path_refresh_when_runtime_sync_is_busy` parallel-suite flake passing immediately in isolated rerun per repo policy. Release binaries were rebuilt and the live daemon restart/status/health sequence passed on the rebuilt prism-cli and prism-mcp binaries.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:113`
- Anchor: `file:21`
- Anchor: `file:242`
- Anchor: `file:340`

#### Acceptance

- Existing tests that allow unsigned legacy hydration or external authoritative edits are replaced with migration or failure-mode expectations. [any]
- Targeted tests cover canonical serialization bytes, signature verification, trust-bundle resolution, conflict detection, truncation, quarantine/repair, and self-write suppression. [any]
- After targeted validation passes, the full workspace test suite passes and the release MCP binaries are rebuilt and restarted if required by the changed surface. [any]

#### Tags

- `protected-state`
- `tests`
- `validation`

## Edges

- `plan-edge:coord-task:01kn5ydewgd87bncbkdehbrd41:depends-on:coord-task:01kn5ydetjt5w7xfn8x71qk7x7`: `coord-task:01kn5ydewgd87bncbkdehbrd41` depends on `coord-task:01kn5ydetjt5w7xfn8x71qk7x7`
- `plan-edge:coord-task:01kn5ydey4tjr0n6na2ymkejfj:depends-on:coord-task:01kn5ydewgd87bncbkdehbrd41`: `coord-task:01kn5ydey4tjr0n6na2ymkejfj` depends on `coord-task:01kn5ydewgd87bncbkdehbrd41`
- `plan-edge:coord-task:01kn5ydezjztsey6m03tvhrgn3:depends-on:coord-task:01kn5ydewgd87bncbkdehbrd41`: `coord-task:01kn5ydezjztsey6m03tvhrgn3` depends on `coord-task:01kn5ydewgd87bncbkdehbrd41`
- `plan-edge:coord-task:01kn5ydezjztsey6m03tvhrgn3:depends-on:coord-task:01kn5ydey4tjr0n6na2ymkejfj`: `coord-task:01kn5ydezjztsey6m03tvhrgn3` depends on `coord-task:01kn5ydey4tjr0n6na2ymkejfj`
- `plan-edge:coord-task:01kn5ydf14v5ce5m5qbd22ykbe:depends-on:coord-task:01kn5ydezjztsey6m03tvhrgn3`: `coord-task:01kn5ydf14v5ce5m5qbd22ykbe` depends on `coord-task:01kn5ydezjztsey6m03tvhrgn3`
- `plan-edge:coord-task:01kn5ydf2qhfjjb50zav19rvm0:depends-on:coord-task:01kn5ydezjztsey6m03tvhrgn3`: `coord-task:01kn5ydf2qhfjjb50zav19rvm0` depends on `coord-task:01kn5ydezjztsey6m03tvhrgn3`
- `plan-edge:coord-task:01kn5ydf4876gvnh5c20zaw9w6:depends-on:coord-task:01kn5ydf14v5ce5m5qbd22ykbe`: `coord-task:01kn5ydf4876gvnh5c20zaw9w6` depends on `coord-task:01kn5ydf14v5ce5m5qbd22ykbe`
- `plan-edge:coord-task:01kn5ydf4876gvnh5c20zaw9w6:depends-on:coord-task:01kn5ydf2qhfjjb50zav19rvm0`: `coord-task:01kn5ydf4876gvnh5c20zaw9w6` depends on `coord-task:01kn5ydf2qhfjjb50zav19rvm0`
- `plan-edge:coord-task:01kn5ydf5tdhdry9ftd4wjwq7c:depends-on:coord-task:01kn5ydey4tjr0n6na2ymkejfj`: `coord-task:01kn5ydf5tdhdry9ftd4wjwq7c` depends on `coord-task:01kn5ydey4tjr0n6na2ymkejfj`
- `plan-edge:coord-task:01kn5ydf5tdhdry9ftd4wjwq7c:depends-on:coord-task:01kn5ydf4876gvnh5c20zaw9w6`: `coord-task:01kn5ydf5tdhdry9ftd4wjwq7c` depends on `coord-task:01kn5ydf4876gvnh5c20zaw9w6`
- `plan-edge:coord-task:01kn5ydf7fed6cqt4xw7jftw6g:depends-on:coord-task:01kn5ydf14v5ce5m5qbd22ykbe`: `coord-task:01kn5ydf7fed6cqt4xw7jftw6g` depends on `coord-task:01kn5ydf14v5ce5m5qbd22ykbe`
- `plan-edge:coord-task:01kn5ydf7fed6cqt4xw7jftw6g:depends-on:coord-task:01kn5ydf2qhfjjb50zav19rvm0`: `coord-task:01kn5ydf7fed6cqt4xw7jftw6g` depends on `coord-task:01kn5ydf2qhfjjb50zav19rvm0`
- `plan-edge:coord-task:01kn5ydf7fed6cqt4xw7jftw6g:depends-on:coord-task:01kn5ydf4876gvnh5c20zaw9w6`: `coord-task:01kn5ydf7fed6cqt4xw7jftw6g` depends on `coord-task:01kn5ydf4876gvnh5c20zaw9w6`
- `plan-edge:coord-task:01kn5ydf7fed6cqt4xw7jftw6g:depends-on:coord-task:01kn5ydf5tdhdry9ftd4wjwq7c`: `coord-task:01kn5ydf7fed6cqt4xw7jftw6g` depends on `coord-task:01kn5ydf5tdhdry9ftd4wjwq7c`

