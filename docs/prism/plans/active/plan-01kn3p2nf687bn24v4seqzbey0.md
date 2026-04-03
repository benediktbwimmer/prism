# Implement principal identity, authenticated mutation provenance, task and claim lease lifecycle, heartbeat advisories, and CLI/bootstrap auth so PRISM coordination becomes principal-authenticated instead of session-authenticated.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:7059853c9a5b4338905671a12068a327727e6583d234d76d781262d2714b7bed`
- Source logical timestamp: `unknown`
- Source snapshot: `10 nodes, 20 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn3p2nf687bn24v4seqzbey0`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `10`
- Edges: `20`

## Goal

Implement principal identity, authenticated mutation provenance, task and claim lease lifecycle, heartbeat advisories, and CLI/bootstrap auth so PRISM coordination becomes principal-authenticated instead of session-authenticated.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn3p2nf687bn24v4seqzbey0.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn3p2nf687bn24v4seqzbey0.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn3p2s5d0jx3bgtzpt61sk10`

## Nodes

### Inventory current session, actor, and coordination touchpoints for the cutover

- Node id: `coord-task:01kn3p2s5d0jx3bgtzpt61sk10`
- Kind: `investigate`
- Status: `completed`
- Summary: Identify every mutation entrypoint, persistence path, query/read surface, and prism_session dependency that currently relies on ambient session semantics so the implementation can cut over cleanly without hidden authority leaks.
- Priority: `1`

#### Tags

- `auth`
- `cutover`
- `principal-identity`

### Add principal, authority, credential, and capability primitives to shared runtime state

- Node id: `coord-task:01kn3p2xd54tw03hvpwb7bgd9c`
- Kind: `edit`
- Status: `completed`
- Summary: Define the principal core types, parent linkage, lifecycle fields, credential verifier storage, and coarse capability model in the shared runtime plane so later mutation auth and lease work has a stable substrate.
- Priority: `1`

#### Tags

- `credentials`
- `principal-identity`
- `shared-runtime`

### Implement trusted bootstrap and CLI auth flows for human and child principals

- Node id: `coord-task:01kn3p329sn75tz26x0z73xb3s`
- Kind: `edit`
- Status: `completed`
- Summary: Add machine-local bootstrap, prism auth init/login flows, and child-principal minting paths so humans and parent agents can obtain credentials without assembling raw mutation payloads by hand.
- Priority: `2`

#### Tags

- `bootstrap`
- `cli`
- `credentials`

### Require authenticated mutation envelopes and capability checks on authoritative writes

- Node id: `coord-task:01kn3p363cmz6sfqstd0s5tgac`
- Kind: `edit`
- Status: `completed`
- Summary: Thread credential verification and coarse capability checks through prism_mutate, coordination, claim, artifact, memory, and any remaining authoritative mutation paths so ambient session state no longer acts as the authority boundary.
- Priority: `1`

#### Tags

- `auth`
- `capabilities`
- `mutation-envelope`

### Persist canonical provenance and execution-context snapshots in mutation events

- Node id: `coord-task:01kn3p3cy48hz6m3g42e0374xj`
- Kind: `edit`
- Status: `completed`
- Summary: Authoritative mutation events now snapshot request-level correlation alongside principal, worktree, session, instance, and credential context, and authenticated coordination/event-path coverage verifies the persisted provenance.
- Priority: `1`

#### Tags

- `audit`
- `event-log`
- `provenance`

### Implement task and claim lease lifecycle with stale, expired, resume, and reclaim semantics

- Node id: `coord-task:01kn3p3gsrp451he4fbaj7a74c`
- Kind: `edit`
- Status: `completed`
- Summary: Task and claim execution ownership is modeled through explicit leases with narrow refresh rules, stale-versus-expired evaluation, resume/reclaim transitions for tasks, and same-principal claim renewal semantics; focused coordination tests covering those paths are green.
- Priority: `1`

#### Tags

- `coordination`
- `leases`
- `reclaim`

### Add heartbeat_lease, server-side due-state evaluation, and bounded assisted watcher renewal

- Node id: `coord-task:01kn3p3mfw8cnhvv3m8s2bdzgq`
- Kind: `edit`
- Status: `completed`
- Summary: Added the authenticated `heartbeat_lease` mutation for tasks and claims, server-side lease due-state evaluation, and explicit opt-in bounded assisted watcher renewal for trusted single-principal single-lease worktrees; focused coordination, MCP, and watcher tests cover the new paths.
- Priority: `1`

#### Tags

- `heartbeat`
- `leases`
- `mutation`

### Surface lease holder, heartbeat advisories, and task-scoped next actions on reads

- Node id: `coord-task:01kn3p3rzhgskr8vc63c5yr9af`
- Kind: `edit`
- Status: `completed`
- Summary: Task brief and session/task read surfaces now emit server-authored heartbeat nextAction guidance when a bound task lease is due, carry lease-related read advisories through shared helpers, and the agent instructions explicitly require obeying heartbeat prompts before other follow-up work.
- Priority: `1`

#### Tags

- `heartbeat`
- `instructions`
- `read-surfaces`

### Cut over away from prism_session as coordination authority and migrate legacy actor history

- Node id: `coord-task:01kn3p3ya2p1aemdxb2bbye2sy`
- Kind: `edit`
- Status: `completed`
- Summary: Legacy coarse actor history now canonicalizes through synthetic fallback principals at query/index boundaries, fallback mutation provenance writes principal-shaped actor snapshots, and session task reads derive stale-task repair context from `coord-task:` identities even when older metadata omitted an explicit coordination binding; focused cutover tests, the full workspace suite, and release-binary MCP restart/status/health validation all passed.
- Priority: `2`

#### Tags

- `cutover`
- `migration`
- `prism-session`

### Validate the end-to-end principal-authenticated coordination cutover

- Node id: `coord-task:01kn3p44q0f2yjwf1nhg936ekj`
- Kind: `validate`
- Status: `completed`
- Summary: Focused heartbeat/lease tests, the full workspace test suite, and the release-binary MCP restart/status/health checks all passed; a live post-restart task brief also succeeded, and dogfooding feedback feedback:01kn4rsn3j27xxq4ad6bphk1ef captured one remaining noisy nextAction on completed tasks.
- Priority: `1`

#### Tags

- `dogfooding`
- `release`
- `validation`

## Edges

- `plan-edge:coord-task:01kn3p2xd54tw03hvpwb7bgd9c:depends-on:coord-task:01kn3p2s5d0jx3bgtzpt61sk10`: `coord-task:01kn3p2xd54tw03hvpwb7bgd9c` depends on `coord-task:01kn3p2s5d0jx3bgtzpt61sk10`
- `plan-edge:coord-task:01kn3p329sn75tz26x0z73xb3s:depends-on:coord-task:01kn3p2xd54tw03hvpwb7bgd9c`: `coord-task:01kn3p329sn75tz26x0z73xb3s` depends on `coord-task:01kn3p2xd54tw03hvpwb7bgd9c`
- `plan-edge:coord-task:01kn3p363cmz6sfqstd0s5tgac:depends-on:coord-task:01kn3p2xd54tw03hvpwb7bgd9c`: `coord-task:01kn3p363cmz6sfqstd0s5tgac` depends on `coord-task:01kn3p2xd54tw03hvpwb7bgd9c`
- `plan-edge:coord-task:01kn3p3cy48hz6m3g42e0374xj:depends-on:coord-task:01kn3p2xd54tw03hvpwb7bgd9c`: `coord-task:01kn3p3cy48hz6m3g42e0374xj` depends on `coord-task:01kn3p2xd54tw03hvpwb7bgd9c`
- `plan-edge:coord-task:01kn3p3gsrp451he4fbaj7a74c:depends-on:coord-task:01kn3p363cmz6sfqstd0s5tgac`: `coord-task:01kn3p3gsrp451he4fbaj7a74c` depends on `coord-task:01kn3p363cmz6sfqstd0s5tgac`
- `plan-edge:coord-task:01kn3p3mfw8cnhvv3m8s2bdzgq:depends-on:coord-task:01kn3p3gsrp451he4fbaj7a74c`: `coord-task:01kn3p3mfw8cnhvv3m8s2bdzgq` depends on `coord-task:01kn3p3gsrp451he4fbaj7a74c`
- `plan-edge:coord-task:01kn3p3rzhgskr8vc63c5yr9af:depends-on:coord-task:01kn3p3cy48hz6m3g42e0374xj`: `coord-task:01kn3p3rzhgskr8vc63c5yr9af` depends on `coord-task:01kn3p3cy48hz6m3g42e0374xj`
- `plan-edge:coord-task:01kn3p3rzhgskr8vc63c5yr9af:depends-on:coord-task:01kn3p3mfw8cnhvv3m8s2bdzgq`: `coord-task:01kn3p3rzhgskr8vc63c5yr9af` depends on `coord-task:01kn3p3mfw8cnhvv3m8s2bdzgq`
- `plan-edge:coord-task:01kn3p3ya2p1aemdxb2bbye2sy:depends-on:coord-task:01kn3p329sn75tz26x0z73xb3s`: `coord-task:01kn3p3ya2p1aemdxb2bbye2sy` depends on `coord-task:01kn3p329sn75tz26x0z73xb3s`
- `plan-edge:coord-task:01kn3p3ya2p1aemdxb2bbye2sy:depends-on:coord-task:01kn3p363cmz6sfqstd0s5tgac`: `coord-task:01kn3p3ya2p1aemdxb2bbye2sy` depends on `coord-task:01kn3p363cmz6sfqstd0s5tgac`
- `plan-edge:coord-task:01kn3p3ya2p1aemdxb2bbye2sy:depends-on:coord-task:01kn3p3cy48hz6m3g42e0374xj`: `coord-task:01kn3p3ya2p1aemdxb2bbye2sy` depends on `coord-task:01kn3p3cy48hz6m3g42e0374xj`
- `plan-edge:coord-task:01kn3p3ya2p1aemdxb2bbye2sy:depends-on:coord-task:01kn3p3gsrp451he4fbaj7a74c`: `coord-task:01kn3p3ya2p1aemdxb2bbye2sy` depends on `coord-task:01kn3p3gsrp451he4fbaj7a74c`
- `plan-edge:coord-task:01kn3p3ya2p1aemdxb2bbye2sy:depends-on:coord-task:01kn3p3rzhgskr8vc63c5yr9af`: `coord-task:01kn3p3ya2p1aemdxb2bbye2sy` depends on `coord-task:01kn3p3rzhgskr8vc63c5yr9af`
- `plan-edge:coord-task:01kn3p44q0f2yjwf1nhg936ekj:depends-on:coord-task:01kn3p329sn75tz26x0z73xb3s`: `coord-task:01kn3p44q0f2yjwf1nhg936ekj` depends on `coord-task:01kn3p329sn75tz26x0z73xb3s`
- `plan-edge:coord-task:01kn3p44q0f2yjwf1nhg936ekj:depends-on:coord-task:01kn3p363cmz6sfqstd0s5tgac`: `coord-task:01kn3p44q0f2yjwf1nhg936ekj` depends on `coord-task:01kn3p363cmz6sfqstd0s5tgac`
- `plan-edge:coord-task:01kn3p44q0f2yjwf1nhg936ekj:depends-on:coord-task:01kn3p3cy48hz6m3g42e0374xj`: `coord-task:01kn3p44q0f2yjwf1nhg936ekj` depends on `coord-task:01kn3p3cy48hz6m3g42e0374xj`
- `plan-edge:coord-task:01kn3p44q0f2yjwf1nhg936ekj:depends-on:coord-task:01kn3p3gsrp451he4fbaj7a74c`: `coord-task:01kn3p44q0f2yjwf1nhg936ekj` depends on `coord-task:01kn3p3gsrp451he4fbaj7a74c`
- `plan-edge:coord-task:01kn3p44q0f2yjwf1nhg936ekj:depends-on:coord-task:01kn3p3mfw8cnhvv3m8s2bdzgq`: `coord-task:01kn3p44q0f2yjwf1nhg936ekj` depends on `coord-task:01kn3p3mfw8cnhvv3m8s2bdzgq`
- `plan-edge:coord-task:01kn3p44q0f2yjwf1nhg936ekj:depends-on:coord-task:01kn3p3rzhgskr8vc63c5yr9af`: `coord-task:01kn3p44q0f2yjwf1nhg936ekj` depends on `coord-task:01kn3p3rzhgskr8vc63c5yr9af`
- `plan-edge:coord-task:01kn3p44q0f2yjwf1nhg936ekj:depends-on:coord-task:01kn3p3ya2p1aemdxb2bbye2sy`: `coord-task:01kn3p44q0f2yjwf1nhg936ekj` depends on `coord-task:01kn3p3ya2p1aemdxb2bbye2sy`

