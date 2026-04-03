# PRISM Tracked Changes Removal

Status: design note  
Audience: PRISM core, storage, runtime, and coordination maintainers  
Scope: remove tracked `changes` history from repo-owned `.prism/state` while preserving coarse-grained durable change history, signed publisher attribution, and cold-clone usefulness

---

## 1. Summary

Tracked `.prism/state` should stop carrying durable `changes` history.

The correct long-term boundary is:

- shared runtime owns append-only operational history
- tracked `.prism/state` owns current durable semantic state
- Git commit history owns coarse-grained durable repo change history
- signed commits plus the PRISM manifest provide publisher authenticity and publish context at commit boundaries

This means:

- `.prism/state/changes/**` should leave tracked repo state
- coarse-grained durable "what changed" history should come from Git commits and Git diffs
- fine-grained "how did we get here" history should come from the shared-runtime journal
- tracked `.prism/state` should keep only durable semantic state that a cold clone needs now

The repo should remain self-contained for current semantic understanding, not for replaying every historical patch outcome forever.

---

## 2. Problem

The current tracked snapshot rewrite fixed the old append-only JSONL model, but it did not finish the retention-model rewrite.

Right now, tracked `.prism/state` still includes a `changes` subtree that behaves like a projection of historical operational activity:

- one file per observed outcome or publishable patch outcome
- largely additive growth over time
- size that still scales with activity rather than only with current semantic state

This has two direct consequences:

1. tracked `.prism/state` will still balloon over time
2. tracked `.prism` is still carrying information Git and the shared runtime already model better

That is the wrong shape for repo-owned semantic state.

---

## 3. Core Principle

The mental model should be simple:

- the shared runtime owns append logs
- tracked `.prism/state` is a stable-by-identity projection of those logs

Tracked `.prism/state` is not a second durable operational ledger.
It is the current repo-published semantic checkpoint.

This is the same boundary already established in the snapshot rewrite:

- Git owns branch/time-travel history
- tracked `.prism/state` owns current semantic state
- runtime/shared state owns churn

Removing tracked `changes` is the step that makes this boundary real instead of partial.

---

## 4. What Belongs in Tracked `.prism/state`

Tracked `.prism/state` should retain durable semantic state that a fresh clone actually needs.

This includes:

- repo memories
- concepts
- concept relations
- contracts
- plans
- durable coordination state a fresh clone must understand
- indexes that help discover or load those objects
- the signed PRISM publish manifest

These should all be stored as stable identity shards:

- one durable object per file where a stable object id exists
- small grouped shards only where the object cardinality makes that necessary
- overwrite in place at publish boundaries

Tracked `.prism/state` should scale roughly with current semantic state, not with lifetime activity.

---

## 5. What Leaves Tracked `.prism/state`

Tracked `.prism/state` should not keep durable historical `changes` state.

This includes:

- one-file-per-outcome patch history
- high-frequency watcher-observed file change batches
- detailed per-mutation change summaries
- rich file-level or symbol-level operational breadcrumbs whose main purpose is historical audit rather than current semantic understanding

Those belong in shared-runtime append logs instead.

If a deployment later wants to retain or export very fine-grained historical change data, that can happen from shared-runtime storage or a later runtime archival pipeline. It should not be the default repo payload.

---

## 6. Why Signed Git Commits Are the Right Coarse-Grained Replacement

Removing tracked `changes` does not mean PRISM loses durable change history.

Git already gives PRISM:

- commit boundaries
- tree history
- branch history
- diffs between durable publishes
- merge and branch semantics
- time travel

If commits are signed, Git also gives:

- publisher authenticity at commit boundaries
- tamper-evident durable repo history

That makes signed Git history a much better coarse-grained durable "changes" substrate than repo-owned PRISM change shards.

In this model:

- "What changed between durable publishes?" comes from Git history and Git diffs
- "Who signed and published this repo state?" comes from signed commits plus the PRISM manifest
- "What happened in detail between commits?" comes from the shared-runtime journal

That is cleaner than making tracked `.prism/state` duplicate Git with its own historical change files.

---

## 7. Why the PRISM Manifest Still Matters

Signed Git commits are necessary, but not sufficient on their own.

Git commit signatures can say:

- this commit is authentic
- someone with a trusted key signed this repo tree

But Git commits do not naturally carry structured PRISM publish metadata.

The PRISM manifest should therefore remain the structured semantic publish boundary.

It should continue to include:

- publisher principal identity
- publisher authority identity
- credential identity when appropriate
- work context
- plan or task context when applicable
- optionally a concise publish reason or publish summary
- file hash inventory for the semantic snapshot
- previous manifest chaining
- migration continuity metadata where needed

So the durable trust model becomes:

1. Git commit signature proves the durability/authenticity of the repo commit
2. PRISM manifest proves the semantic publish context within that commit
3. shared-runtime journal explains the fine-grained path that led there, if retained

---

## 8. Shared Runtime Becomes the Sole Append-Log Owner

The shared runtime should own all append-only operational logs.

This includes:

- fine-grained mutation journals
- patch outcomes
- file observation batches
- intermediate operational checkpoints
- detailed coordination churn
- other high-resolution audit and replay detail

The local hot runtime can still cache or spool local operational state, but the architectural owner of append-only operational history should be the shared runtime.

This has several advantages:

- one clear home for operational history
- no repo growth driven by operational churn
- easier future compaction or archival
- better alignment with a future remote shared runtime backend

Tracked `.prism/state` then becomes what it should be:

- a stable projection of shared-runtime history into repo-owned semantic state

---

## 9. Cold Clone Story After Removing Tracked `changes`

Cold clones should still remain useful and trustworthy.

The repo should still be able to answer:

- what is the current semantic state?
- who published it?
- under which work or plan context was it published?

The repo does not need to answer, by itself:

- every intermediate operational step that led there
- every fine-grained patch change between commits

That distinction is acceptable because:

- the semantic checkpoint remains self-contained
- Git already provides the coarse-grained durable history boundary
- shared runtime holds the detailed operational path where available

So removing tracked `changes` is not a loss of cold-clone usefulness.
It is a sharpening of the boundary between semantic state and operational history.

---

## 10. Time Travel and Branching

Branching and time travel for repo-owned PRISM state should come from Git, not from tracked PRISM history files.

This means:

- `git checkout <commit>` gives the PRISM semantic snapshot for that commit
- Git branches give semantic branches automatically
- merges and rebases apply at the semantic snapshot layer

Tracked `changes` files do not improve this.
They only duplicate history Git already owns.

Removing tracked `changes` therefore simplifies, rather than weakens, PRISM's branching and time-travel model.

---

## 11. Target State

After this rewrite:

- `.prism/state/changes/**` no longer exists in tracked repo state
- tracked `.prism/state` contains only stable semantic shards plus indexes and the manifest
- shared runtime owns all append-only operational change history
- signed Git commits provide coarse-grained durable change history
- the PRISM manifest carries structured publish metadata
- cold clones load current semantic state from tracked snapshot shards
- detailed historical change replay no longer depends on tracked repo files

This is the intended steady state.

---

## 12. Migration Strategy

The rewrite should be staged carefully.

### 12.1 Stop writing new tracked `changes`

First, tracked publication should stop producing new `changes` shards in `.prism/state`.

Shared runtime should continue recording append-only change history as usual.

### 12.2 Preserve current semantic state without tracked `changes`

Any durable information currently only recoverable from tracked `changes` must either:

- move into another stable semantic shard if it is truly current-state semantic knowledge, or
- remain only in shared-runtime history if it is operational history

The default bias should be the latter.

### 12.3 Repair existing tracked repos

Existing tracked repos may already contain `.prism/state/changes/**`.

Migration should:

- stop treating those files as authoritative tracked state
- remove them from future publishes
- optionally prune or archive them in a controlled migration step
- retain any required migration continuity metadata in the PRISM manifest

### 12.4 Keep trust continuity explicit

The manifest should carry enough continuity metadata to explain the boundary between:

- the old tracked-with-changes world
- the new tracked-without-changes world

That continuity belongs in structured manifest metadata, not in permanently retaining tracked historical change shards.

---

## 13. Locked Decisions

The following decisions are part of this design and should not be reopened casually during implementation:

1. Tracked `changes` leaves entirely.

- No bounded "recent changes" summary remains in tracked `.prism/state` by default.
- If a future design wants any tracked change-like artifact, it must be justified from scratch as current semantic state rather than historical convenience or audit residue.

2. The PRISM manifest should carry a short structured `publishSummary`.

- The manifest should include a concise semantic summary of what the publish represents.
- This summary should remain short and state-oriented.
- It must not turn into a changelog, diff dump, or replay of operational history.

3. Generated semantic docs should read tracked snapshots, while operational reports should read shared runtime.

- Docs and projections that describe current durable semantic state should use tracked `.prism/state`.
- Reports, diagnostics, and operational views about change history or execution churn should use shared runtime or projections derived from shared runtime.

4. Performance improvement is a direct consequence of the storage-boundary correction.

- The old tracked `changes` payload is not being "optimized" into another tracked form.
- It ceases to exist as a tracked repo file.
- Hot startup should therefore stop paying to load or verify tracked change history.
- Cold clones should no longer download tracked change history as part of repo-owned PRISM state.

---

## 14. Recommendation

Proceed with a dedicated rewrite that removes tracked `changes` from `.prism/state`.

The implementation rule should be:

- shared runtime is the sole append-log owner
- tracked `.prism/state` is stable semantic state only
- signed Git commits provide coarse-grained durable change history
- the PRISM manifest provides structured semantic publish metadata

This aligns with the larger snapshot rewrite and finishes the storage-boundary correction cleanly.
