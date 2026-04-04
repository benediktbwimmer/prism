# Finish PRISM shared coordination refs end to end

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:5d3294d10a583ae7997808fafb425b622aa724fe644e6eeb2ca2dd384f13941f`
- Source logical timestamp: `unknown`
- Source snapshot: `14 nodes, 23 edges, 4 overlays`

## Overview

- Plan id: `plan:01knav51cj8vgw0zp49qgktzps`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `14`
- Edges: `23`

## Goal

Close every remaining implementation gap in docs/PRISM_SHARED_COORDINATION_REFS.md by demoting branch-local mirrors, separating branch publication from coordination publication, completing all integration modes and evidence models, tightening lease and heartbeat semantics, hardening degraded and recovery paths, and validating the full shared-ref model against the design doc section by section.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`
- Max fetch age seconds: `300`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01knav51cj8vgw0zp49qgktzps.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01knav6qm2h72t0pc3xqmv92zt`

## Nodes

### Freeze the final section-by-section shared-ref gap matrix and acceptance contract

- Node id: `coord-task:01knav6qm2h72t0pc3xqmv92zt`
- Kind: `edit`
- Status: `completed`
- Summary: Turn the current doc audit into an explicit tracked contract so every remaining partial section in PRISM_SHARED_COORDINATION_REFS.md has an owner, boundary, and definition of done before more code lands.
- Priority: `100`

#### Acceptance

- Every partial or not-implemented section in PRISM_SHARED_COORDINATION_REFS.md is mapped to concrete owning code paths and an implementation task. [any]
- The repo records which shared-ref design points are done, partial, or intentionally deferred with no ambiguous remaining gaps. [any]
- Follow-on tasks can use this matrix as the authoritative checklist for closing the doc end to end. [any]

#### Tags

- `acceptance`
- `audit`
- `shared-ref`

### Demote branch-local shared coordination mirrors to minimal derived exports

- Node id: `coord-task:01knavc6b8qhznt2e1yrn7mmwh`
- Kind: `edit`
- Status: `completed`
- Summary: Stop mirroring full shared coordination authority into branch-local `.prism/state/**` by default and leave only bounded, explicitly derived exports so the shared ref becomes the real source of truth.
- Priority: `98`

#### Acceptance

- Branch-local `.prism/state/**` no longer persists full shared coordination mirrors by default; remaining files are explicitly marked derived and non-authoritative. [any]
- Query, hydration, and MCP views remain correct when branch-local mirrors are absent, stale, or intentionally pruned. [any]
- Tests prove shared-ref reads and operator workflows still work after removing mirror-first assumptions. [any]

#### Tags

- `mirror`
- `shared-ref`
- `state`

### Represent branch publication, coordination publication, and target integration as distinct durable states

- Node id: `coord-task:01knavcv05nn8t2gc1z31gg0fy`
- Kind: `edit`
- Status: `completed`
- Summary: Separate branch publication, shared coordination publication, and verified target integration in the task state machine so require-mode workflows can recover cleanly and query surfaces stop collapsing materially different states.
- Priority: `97`

#### Acceptance

- Task and plan views distinguish `published_to_branch`, `coordination_published`, and `integrated_to_target` without ambiguity. [any]
- Require-mode completion only becomes authoritative after shared coordination publication succeeds, with partial publication states preserved durably. [any]
- Recovery logic and operator views can resume from any intermediate publication state without manual state surgery. [any]

#### Tags

- `git-execution`
- `lifecycle`
- `shared-ref`

### Add trusted landing records for squash and rebase integration evidence

- Node id: `coord-task:01knavdeaf5qs3zp6vycxgkg9t`
- Kind: `edit`
- Status: `in_progress`
- Summary: Make target-integration verification work for non-merge landings by recording trusted landing evidence that binds rebased or squashed target commits back to the originating task and review artifacts.
- Priority: `96`

#### Acceptance

- Squash and rebase landings can be verified through trusted PRISM records rather than raw branch reachability alone. [any]
- Landing evidence can bind review artifacts, target commits, and task ids without relying on ambiguous commit ancestry. [any]
- Ambiguous or unverifiable target commits remain rejected until explicit trusted landing evidence exists. [any]

#### Tags

- `evidence`
- `integration`
- `shared-ref`

### Complete manual_pr integration mode with review-backed observation and landing verification

- Node id: `coord-task:01knave01nvcrq91vnt8np1mkg`
- Kind: `edit`
- Status: `ready`
- Summary: Finish the manual PR mode so PRISM requires review artifacts, observes external PR landings correctly, and upgrades integration state only when landing evidence is trusted and complete.
- Priority: `95`

#### Acceptance

- `manual_pr` mode blocks integration advancement until a valid review artifact is linked to the task. [any]
- PRISM can observe external PR landing and record verified integration for merge, squash, and rebase outcomes. [any]
- Tests cover retry, partial failure, and late-observation flows without manual state repair. [any]

#### Tags

- `integration`
- `manual-pr`
- `shared-ref`

### Complete auto_pr mode with review artifact creation, update flow, and post-land verification

- Node id: `coord-task:01knavehyd4a49fk7b4rnjxz67`
- Kind: `edit`
- Status: `ready`
- Summary: Implement the autonomous PR mode end to end so PRISM can create or refresh review artifacts, drive merge enablement within policy, and verify the eventual landing on the target branch.
- Priority: `94`

#### Acceptance

- `auto_pr` mode can create or update the review artifact and persist the linkage on the task durably. [any]
- Policy-approved merge automation can be enabled and its result observed without losing integration evidence. [any]
- Failures leave an actionable retry state instead of collapsing to a generic pending status. [any]

#### Tags

- `auto-pr`
- `integration`
- `shared-ref`

### Complete direct_integrate mode with trusted direct landing execution and immediate verification

- Node id: `coord-task:01knavf33fkvpsc79jd5k9tkdj`
- Kind: `edit`
- Status: `ready`
- Summary: Finish the direct integration mode so PRISM can land eligible task branches itself under policy, emit trusted landing metadata, and verify the target branch update immediately.
- Priority: `93`

#### Acceptance

- `direct_integrate` mode checks freshness, validation, branch policy, and target movement preconditions before landing. [any]
- Allowed direct landings emit trusted landing metadata that can be verified without external manual repair. [any]
- Immediate post-land verification and failure reporting are explicit and tested. [any]

#### Tags

- `direct`
- `integration`
- `shared-ref`

### Make scheduling, dependency gating, and query views integration-aware

- Node id: `coord-task:01knavfm5fk9gsvb56pbsb58hh`
- Kind: `edit`
- Status: `ready`
- Summary: Teach scheduling and blocker resolution to distinguish completion, coordination publication, and target integration so downstream work can depend on the right state instead of plain task completion.
- Priority: `92`

#### Acceptance

- Blockers and dependency checks can gate separately on completion, coordination publication, and verified target integration. [any]
- Query surfaces expose the distinct integration-aware lifecycle states without ad hoc interpretation in clients. [any]
- Follow-on tasks can explicitly depend on `integrated_to_target` when the target branch state matters. [any]

#### Tags

- `query`
- `scheduling`
- `shared-ref`

### Finish durable lease publication and low-frequency authoritative renewal on the shared ref

- Node id: `coord-task:01knavg5dce3ytrw4pbsfrbz1g`
- Kind: `edit`
- Status: `ready`
- Summary: Complete the lease model so authoritative lease facts, renewals, and staleness decisions live on the shared ref instead of drifting between local heartbeats and branch mirrors.
- Priority: `91`

#### Acceptance

- Shared coordination refs publish durable lease lifecycle facts and authoritative renewal timestamps. [any]
- Renewal writes extend meaningful lease state rather than producing heartbeat-style churn on the shared ref. [any]
- Claim recovery and stale-owner detection read authoritative lease facts first and behave correctly after restarts. [any]

#### Tags

- `lease`
- `liveness`
- `shared-ref`

### Keep assisted heartbeat renewal local, off by default, bounded, and non-authoritative

- Node id: `coord-task:01knavgtmdhbdm9hr190t7n7c7`
- Kind: `edit`
- Status: `ready`
- Summary: Constrain assisted renewal so it helps local liveness only when explicitly enabled and never becomes a substitute for authoritative lease extension on the shared ref.
- Priority: `90`

#### Acceptance

- Assisted renewal remains off by default and is never treated as authoritative identity or claim proof. [any]
- Only authoritative lease extensions publish to the shared ref; local heartbeat assistance stays local and bounded. [any]
- Diagnostics clearly distinguish local liveness assistance from durable shared-ref lease state. [any]

#### Tags

- `heartbeat`
- `lease`
- `shared-ref`

### Finish compaction, archive-boundary trust continuity, and operator diagnostics

- Node id: `coord-task:01knavpwvj8r99x8yarrsam19g`
- Kind: `edit`
- Status: `ready`
- Summary: Complete the operational side of shared coordination refs so compaction preserves trust semantics, archive boundaries stay intelligible, and operators can see enough history and health to debug live systems.
- Priority: `88`

#### Acceptance

- Compaction preserves explicit trust continuity or archive-boundary semantics instead of discarding verification history implicitly. [any]
- Operator surfaces expose current head, last verified manifest, last successful publish, retry counts, and compaction health. [any]
- Bounded recent-history and archive/export behavior are explicit, documented, and covered by tests. [any]

#### Tags

- `compaction`
- `observability`
- `shared-ref`

### Harden degraded verification behavior, silent-fallback prevention, and partial publication recovery

- Node id: `coord-task:01knax3zr6tyxkcev2vzd2ptyg`
- Kind: `edit`
- Status: `ready`
- Summary: Close the recovery-path gaps so verification failures and split publication outcomes are surfaced explicitly, block authoritative hydration when necessary, and remain recoverable without hidden fallback behavior.
- Priority: `89`

#### Acceptance

- Verification failures move runtime state into an explicit degraded mode rather than silently falling back to stale local assumptions. [any]
- Branch-push-success/shared-publish-failure and branch-push-failure-before-shared-publish flows are durable, diagnosable, and retryable. [any]
- Repair commands and runtime recovery operate against explicit shared-ref publication state instead of inferred local heuristics. [any]

#### Tags

- `recovery`
- `shared-ref`
- `verification`

### Validate the full shared-ref model end to end against the design doc

- Node id: `coord-task:01knax49ge8qvv6yabya0k2xrc`
- Kind: `validate`
- Status: `ready`
- Summary: Run the completed implementation against the PRISM_SHARED_COORDINATION_REFS.md contract with targeted and release-binary validation so every remaining section is either proven implemented or explicitly deferred.
- Priority: `87`

#### Acceptance

- Tests cover cold hydration, CAS races, claim visibility, authoritative lease renewal and expiry, self-write suppression, mirror-derivedness, publication orderings, and all integration modes. [any]
- Live release-binary dogfooding validates the MCP, bridge, and shared-ref runtime behavior rather than only in-process test hosts. [any]
- Any intentionally deferred behavior is named explicitly instead of being left as an ambiguous gap. [any]

#### Tags

- `dogfooding`
- `shared-ref`
- `validation`

### Update PRISM_SHARED_COORDINATION_REFS.md to implemented reality and close the final gap list

- Node id: `coord-task:01knax5mvp5x40c6wnae7s3r9d`
- Kind: `edit`
- Status: `ready`
- Summary: Bring the design doc and related published plan docs into sync with the implemented system so the repo stops advertising this as a purely proposed target design once the remaining work is complete.
- Priority: `86`

#### Acceptance

- PRISM_SHARED_COORDINATION_REFS.md is updated from target-design wording to implemented reality with explicit remaining exceptions, if any. [any]
- Published plan docs, PRISM.md, and the shared-ref design doc agree on what is complete, partial, or intentionally deferred. [any]
- No remaining shared-ref gaps are left only in chat context; the repo documentation is sufficient for the next agent to resume without reconstructing intent. [any]

#### Tags

- `closure`
- `docs`
- `shared-ref`

## Edges

- `plan-edge:coord-task:01knave01nvcrq91vnt8np1mkg:depends-on:coord-task:01knavdeaf5qs3zp6vycxgkg9t`: `coord-task:01knave01nvcrq91vnt8np1mkg` depends on `coord-task:01knavdeaf5qs3zp6vycxgkg9t`
- `plan-edge:coord-task:01knavehyd4a49fk7b4rnjxz67:depends-on:coord-task:01knavdeaf5qs3zp6vycxgkg9t`: `coord-task:01knavehyd4a49fk7b4rnjxz67` depends on `coord-task:01knavdeaf5qs3zp6vycxgkg9t`
- `plan-edge:coord-task:01knavf33fkvpsc79jd5k9tkdj:depends-on:coord-task:01knavdeaf5qs3zp6vycxgkg9t`: `coord-task:01knavf33fkvpsc79jd5k9tkdj` depends on `coord-task:01knavdeaf5qs3zp6vycxgkg9t`
- `plan-edge:coord-task:01knavfm5fk9gsvb56pbsb58hh:depends-on:coord-task:01knavcv05nn8t2gc1z31gg0fy`: `coord-task:01knavfm5fk9gsvb56pbsb58hh` depends on `coord-task:01knavcv05nn8t2gc1z31gg0fy`
- `plan-edge:coord-task:01knavg5dce3ytrw4pbsfrbz1g:depends-on:coord-task:01knav6qm2h72t0pc3xqmv92zt`: `coord-task:01knavg5dce3ytrw4pbsfrbz1g` depends on `coord-task:01knav6qm2h72t0pc3xqmv92zt`
- `plan-edge:coord-task:01knavgtmdhbdm9hr190t7n7c7:depends-on:coord-task:01knavg5dce3ytrw4pbsfrbz1g`: `coord-task:01knavgtmdhbdm9hr190t7n7c7` depends on `coord-task:01knavg5dce3ytrw4pbsfrbz1g`
- `plan-edge:coord-task:01knavpwvj8r99x8yarrsam19g:depends-on:coord-task:01knav6qm2h72t0pc3xqmv92zt`: `coord-task:01knavpwvj8r99x8yarrsam19g` depends on `coord-task:01knav6qm2h72t0pc3xqmv92zt`
- `plan-edge:coord-task:01knax3zr6tyxkcev2vzd2ptyg:depends-on:coord-task:01knav6qm2h72t0pc3xqmv92zt`: `coord-task:01knax3zr6tyxkcev2vzd2ptyg` depends on `coord-task:01knav6qm2h72t0pc3xqmv92zt`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavc6b8qhznt2e1yrn7mmwh`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavc6b8qhznt2e1yrn7mmwh`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavcv05nn8t2gc1z31gg0fy`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavcv05nn8t2gc1z31gg0fy`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavdeaf5qs3zp6vycxgkg9t`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavdeaf5qs3zp6vycxgkg9t`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knave01nvcrq91vnt8np1mkg`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knave01nvcrq91vnt8np1mkg`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavehyd4a49fk7b4rnjxz67`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavehyd4a49fk7b4rnjxz67`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavf33fkvpsc79jd5k9tkdj`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavf33fkvpsc79jd5k9tkdj`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavfm5fk9gsvb56pbsb58hh`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavfm5fk9gsvb56pbsb58hh`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavg5dce3ytrw4pbsfrbz1g`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavg5dce3ytrw4pbsfrbz1g`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavgtmdhbdm9hr190t7n7c7`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavgtmdhbdm9hr190t7n7c7`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knavpwvj8r99x8yarrsam19g`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knavpwvj8r99x8yarrsam19g`
- `plan-edge:coord-task:01knax49ge8qvv6yabya0k2xrc:depends-on:coord-task:01knax3zr6tyxkcev2vzd2ptyg`: `coord-task:01knax49ge8qvv6yabya0k2xrc` depends on `coord-task:01knax3zr6tyxkcev2vzd2ptyg`
- `plan-edge:coord-task:01knax5mvp5x40c6wnae7s3r9d:depends-on:coord-task:01knax49ge8qvv6yabya0k2xrc`: `coord-task:01knax5mvp5x40c6wnae7s3r9d` depends on `coord-task:01knax49ge8qvv6yabya0k2xrc`
- `plan-edge:coord-task:01knavc6b8qhznt2e1yrn7mmwh:depends-on:coord-task:01knav6qm2h72t0pc3xqmv92zt`: `coord-task:01knavc6b8qhznt2e1yrn7mmwh` depends on `coord-task:01knav6qm2h72t0pc3xqmv92zt`
- `plan-edge:coord-task:01knavcv05nn8t2gc1z31gg0fy:depends-on:coord-task:01knav6qm2h72t0pc3xqmv92zt`: `coord-task:01knavcv05nn8t2gc1z31gg0fy` depends on `coord-task:01knav6qm2h72t0pc3xqmv92zt`
- `plan-edge:coord-task:01knavdeaf5qs3zp6vycxgkg9t:depends-on:coord-task:01knavcv05nn8t2gc1z31gg0fy`: `coord-task:01knavdeaf5qs3zp6vycxgkg9t` depends on `coord-task:01knavcv05nn8t2gc1z31gg0fy`

## Execution Overlays

- Node: `coord-task:01knav6qm2h72t0pc3xqmv92zt`
  git execution status: `published`
  source ref: `task/shared-coordination-refs-gaps`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs-gaps`
- Node: `coord-task:01knavc6b8qhznt2e1yrn7mmwh`
  git execution status: `published`
  source ref: `task/shared-coordination-refs-gaps`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs-gaps`
- Node: `coord-task:01knavcv05nn8t2gc1z31gg0fy`
  git execution status: `published`
  source ref: `task/shared-coordination-refs-gaps`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs-gaps`
- Node: `coord-task:01knavdeaf5qs3zp6vycxgkg9t`
  git execution status: `in_progress`
  source ref: `task/shared-coordination-refs-gaps`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs-gaps`

