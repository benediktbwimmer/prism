# PRISM Repo Snapshot Rewrite

Status: design note  
Audience: PRISM core, storage, runtime, coordination, and MCP maintainers  
Scope: rewrite of tracked `.prism` from repo-committed append logs to repo-committed snapshots plus signed publish manifests

---

## 1. Summary

PRISM should stop treating tracked repo `.prism` as an append-only event-log substrate.
Tracked `.prism` is no longer a repo-committed event log.
It becomes the repo-published authoritative snapshot format.

Git already gives PRISM:

- durable commit boundaries
- time travel
- branching
- merges
- history inspection

That means tracked `.prism` does not need its own second append log for those same purposes.

The new model should be:

- tracked `.prism` stores compact, self-contained snapshot state
- each publish writes a signed manifest for that snapshot boundary
- fine-grained append-only history moves to runtime/shared state
- cold clone restoration comes from the repo snapshot itself, not replay of a large committed log

This keeps the strong property that a fresh clone can reconstruct trustworthy PRISM state from the repo alone, while avoiding Git bloat and expensive replay on hot or cold startup.

The transitional migration state may still carry legacy tracked `.jsonl` artifacts for some
domains, but the target architecture is stricter:

- no repo-published semantic domain should require a tracked append log
- no tracked `.prism/plans/*.jsonl` file should remain authoritative
- no tracked `.prism/*/events.jsonl` file should remain authoritative
- tracked `.prism/state/**` plus the signed manifest become the only repo-published authority

---

## 2. Problem

The current repo-published append-log design has created two coupled failures:

1. Hot startup and cold startup are too expensive.
2. Tracked `.prism` is carrying far too much data for Git.

The current failure mode is structural:

- tracked repo streams are append-only
- startup re-reads and re-verifies full protected streams
- large event payloads are committed directly into Git
- the same stream may be inspected multiple times during one startup

This is the wrong shape for repo-published durable state.

The root mistake is treating tracked `.prism` as if it needs to be its own event-sourcing system, even though Git already provides the durable branchable history boundary.

---

## 3. Design Goals

The rewrite should preserve the good properties of the current system while removing the expensive ones.

Required goals:

- a fresh cold clone can reconstruct meaningful PRISM state from the repo alone
- tracked `.prism` remains branchable and time-travelable through Git
- durable publisher identity and work attribution remain cryptographically attestable
- hot startup should restore fast from current tracked state, not replay a large history
- cold startup should verify and load compact snapshots, not re-materialize everything from large repo event streams
- tracked `.prism` growth should stay sane for a long-lived repo

Required non-goals:

- tracked `.prism` does not need to preserve every fine-grained runtime mutation forever
- tracked `.prism` does not need to mirror the runtime append log
- Git does not need to become a storage layer for high-frequency watcher flushes or rich operational breadcrumbs

---

## 4. Core Decision

### 4.1 Git is the durable history for tracked `.prism`

PRISM should rely on Git for:

- version history
- branch history
- snapshot branching
- time travel across published repo states

Tracked `.prism` therefore only needs to represent the durable state *at the current commit*.

### 4.2 Tracked `.prism` becomes snapshot-oriented

Tracked `.prism` should evolve toward:

- compact state files
- modular snapshot artifacts
- one signed publish manifest per publish boundary

Tracked `.prism` should stop depending on committed append-only event logs for its durable history semantics.

The steady state is not "snapshot authority plus a few surviving repo logs."
The steady state is "snapshot authority only."

These snapshot artifacts are not "projections" in the weak old sense.
They become the repo authority format for published PRISM state.

### 4.3 Fine-grained append-only history lives in runtime/shared state

The high-resolution mutation log should move to runtime/shared state:

- local runtime journal when running locally
- shared runtime journal when a shared runtime exists
- later optional compaction or export if a deployment wants long-term high-resolution retention

This journal is for:

- audit detail
- operational replay
- coordination continuity
- mutation provenance between publish boundaries
- optional deep lineage/debugging

It is not the default durable repo payload anymore.

Legacy tracked append logs may exist temporarily during migration or repair, but they should be
treated as transient bridge artifacts rather than a second durable authority plane.

---

## 5. State Planes

This rewrite sharpens the existing persistence classification.

### 5.1 Repo-published plane

Tracked `.prism` in Git should contain:

- compact durable semantic state
- signed publish manifests
- enough attribution and integrity data to trust and interpret the snapshot on a cold clone

Tracked `.prism` should not contain:

- high-frequency operational append logs
- huge expanded anchor arrays
- large inline derived payloads
- verbose mutation-by-mutation watcher checkpoints

### 5.2 Runtime/shared state plane

Runtime/shared state should contain:

- append-only fine-grained mutation journals
- coordination churn
- watcher-observed change batches
- high-resolution audit and recovery detail
- optional intermediate checkpoints and compaction outputs

This is the right home for detailed operational history.

### 5.3 Process-local plane

In-process caches continue to hold:

- hot runtime state
- local materializations
- request-path caches
- convenience projections

These remain disposable and rebuildable.

---

## 6. Snapshot Model

Tracked `.prism` should be organized as coherent state snapshots rather than durable event streams.
Snapshot-oriented does not mean monolithic.
It means stable overwrite-in-place files with narrow conflict surfaces.

An illustrative target layout:

```text
.prism/
  state/
    plans/<plan-id>.json
    concepts/<concept-id>.json
    contracts/<contract-id>.json
    memory/<memory-id>.json
    indexes/...
    manifest.json
```

The exact file split can change, but the contract should stay:

- state files are durable snapshot views
- state files should prefer stable shard- or object-oriented paths over large monolithic domain files
- state files are updated in place at publish boundaries
- Git commit history carries prior versions automatically

### 6.1 Shard strategy

Tracked snapshot layout should optimize for:

- narrow Git diffs
- narrow merge conflict surfaces
- deterministic overwrite-in-place publication
- cold-clone self-containment without replaying committed logs

The default bias should therefore be:

- one stable file per durable object where that object has a natural stable id
- small grouped shards only when objects are too small or too numerous for one-file-per-object to stay practical
- separate lookup indexes for discovery convenience rather than stuffing many authoritative records into one domain blob

Good shard examples:

- one file per plan
- one file per concept
- one file per contract
- one file per durable memory record, or small stable memory shards if count pressure requires grouping

Bad shard examples:

- one giant `plans.json`
- one giant `concepts.json`
- one giant `contracts.json`
- any layout where touching one object rewrites a broad unrelated domain file

Snapshot-oriented means overwrite-in-place authority files, not monolithic per-domain blobs.

### 6.2 Snapshot requirements

Each snapshot file should be:

- deterministic
- compact
- self-contained for interpretation
- independent of runtime-only state

Each snapshot should be understandable from the repo alone.

### 6.3 File classes and responsibilities

Tracked snapshot files should fall into three explicit classes:

1. authoritative object files
   These are the durable published state shards, such as `plans/<plan-id>.json` or `concepts/<concept-id>.json`.

2. stable manifest files
   These define one coherent publish boundary and attest to the exact authoritative object files in that boundary.

3. convenience indexes
   These support efficient discovery, lookup, or human navigation, but they are not the authority for the underlying objects.

The boundary must stay crisp:

- per-object snapshot files hold the durable semantic state
- `manifest.json` attests the current coherent snapshot boundary
- `indexes/**` accelerate lookup and browsing only

Indexes must be rebuildable from the authoritative object files and manifest.
They should never be the only place where a durable record exists.

### 6.4 Suggested tracked layout

A more explicit target layout is:

```text
.prism/
  state/
    manifest.json
    plans/
      <plan-id>.json
    concepts/
      <concept-id>.json
    contracts/
      <contract-id>.json
    memory/
      <memory-id>.json
    coordination/
      tasks/<task-id>.json
      artifacts/<artifact-id>.json
    indexes/
      plans.json
      concepts.json
      contracts.json
      memory.json
```

This layout is illustrative, but the structural rules should hold:

- manifest is one stable file, overwritten in place
- authoritative records live in stable object-oriented paths
- indexes are grouped convenience files
- adding or updating one durable object should usually touch one object file, one manifest, and only the relevant index shards

### 6.5 Snapshot scope

Tracked snapshot content should represent:

- durable coordination state worth carrying across clones
- durable repo-published knowledge
- durable plan, concept, contract, and memory state

It should not attempt to preserve every intermediate runtime action that led there.

Only coordination state that a fresh clone actually needs belongs here.
Examples of good published coordination state:

- durable plan and task state
- durable authored task/work bindings
- durable artifacts and outcomes that later agents need to understand current repo state
- durable review or validation state when it changes the repo's shared understanding of whether work is ready, accepted, or blocked

Examples of coordination state that should stay out of tracked snapshots:

- ephemeral leases
- watcher churn
- transient execution internals
- heartbeat noise
- short-lived handoff mechanics that do not change durable repo understanding

### 6.6 Coordination publication rule

The decision test for coordination state should be:

- would a fresh clone need this state to understand current repo intent, execution state, or review state?

If the answer is no, it should stay out of tracked snapshots.

State that should normally be published:

- current durable plan structure
- current durable task status
- authored dependencies and bindings
- accepted or pending review artifacts that later agents must inspect
- durable validation or outcome markers that change shared execution understanding

State that should normally remain runtime-only:

- active claim leases
- lease renewals and heartbeats
- watcher-derived change batches
- publish-in-flight bookkeeping
- temporary assignee/session continuity details
- short-lived handoff transport mechanics once their durable result is reflected elsewhere

The snapshot should prefer the minimum stable coordination state that makes the current repo collaboration state intelligible on a fresh clone.

### 6.7 Review and merge pressure

Published coordination shards should also be judged against Git review and merge behavior.

Good published coordination shape:

- stable per-task or per-artifact files where changes are localized
- indexes or manifests that update predictably
- durable review state that produces understandable diffs

Bad published coordination shape:

- broad grouped files that churn when unrelated tasks change
- embedding ephemeral runtime fields into durable task snapshots
- publication that makes parallel task work constantly conflict in Git

Tracked coordination state should be stable enough for narrow diffs and practical multi-agent merging, not just semantically correct in the abstract.

---

## 7. Signed Publish Manifest

Git commit hashes give content integrity.
They do not give trusted publisher identity.

PRISM therefore still needs signatures, but the signature boundary should move upward.

### 7.1 Signed unit

The signed unit should be the publish manifest, not every committed event line.

Each publish manifest should attest:

- which principal published the snapshot
- under which declared work context
- which snapshot files belong to this publish boundary
- hashes of those snapshot files
- the previous manifest digest or previous snapshot root digest
- optional linkage to the summarized runtime journal range

There should be one manifest authority file per publish boundary:

- `manifest.json` describes the current published snapshot boundary in `HEAD`
- Git history preserves prior manifest versions automatically
- PRISM should not accumulate `manifest-<timestamp>.json` or similar files in tracked state

The manifest is therefore both:

- the current publish-boundary attestation for the checked-out repo state
- the stable file whose historical versions Git exposes for time travel and branching

An illustrative manifest shape:

```json
{
  "version": 1,
  "publishedAt": "2026-04-03T08:30:00Z",
  "publisher": {
    "principalId": "principal:codex-a:...",
    "authorityId": "runtime-authority:..."
  },
  "workContext": {
    "workId": "work:...",
    "title": "Rewrite tracked .prism to snapshot publishing"
  },
  "files": {
    "coordination": {
      "path": ".prism/state/coordination.json",
      "sha256": "..."
    },
    "concepts": {
      "path": ".prism/state/concepts.json",
      "sha256": "..."
    }
  },
  "previousManifestDigest": "sha256:...",
  "runtimeLineage": {
    "fromCheckpoint": "runtime-checkpoint:...",
    "toCheckpoint": "runtime-checkpoint:...",
    "eventCount": 42,
    "eventDigest": "sha256:..."
  },
  "signature": "..."
}
```

Required manifest fields should be treated as contract, not example:

- manifest version
- publish timestamp
- publisher principal id
- publishing runtime authority id
- declared work context summary
- exact file digest map for authoritative snapshot files
- previous manifest digest or previous snapshot-root digest
- signature metadata and signature bytes

Optional manifest fields may include:

- runtime journal linkage
- migration continuity linkage
- publish tooling metadata that does not affect snapshot interpretation

### 7.2 Manifest chaining

Each manifest should chain to the previous publish boundary by digest.

The preferred default is:

- `previousManifestDigest` points to the immediately prior manifest digest

If migration or bootstrapping needs a slightly different first boundary, PRISM may also support:

- `previousSnapshotRootDigest`
- `migrationSourceDigest`

But the steady-state rule should be simple:

- every publish manifest links to the previous publish manifest digest

This internal chain is still valuable even though Git carries history, because it gives PRISM a continuity primitive that survives:

- rebases
- squash merges
- mirrored history
- partial Git history transport
- future non-Git publication/export contexts

### 7.3 Why this boundary is correct

This lets PRISM answer:

- what snapshot was published?
- who published it?
- under what work context?
- has it been tampered with?

without requiring tracked `.prism` to preserve a full fine-grained event ledger.

### 7.4 Publish boundary

The publish boundary should be a single logical PRISM publication step:

1. PRISM computes the next authoritative object files and indexes for the new snapshot state
2. PRISM computes the next `manifest.json` against those exact files
3. PRISM writes the authoritative object files and indexes
4. PRISM writes the updated stable `manifest.json`
5. Git commit records that coherent snapshot state

The boundary should be treated as atomic in PRISM's semantics.
It should not be modeled as a fuzzy sequence of loosely related repo updates.

The important semantic rule is:

- the manifest is only valid for the exact snapshot file set it digests
- readers should treat `manifest.json` as the final authority gate for the current publish boundary
- partially written intermediate states are implementation detail, not observable PRISM publish states

### 7.5 Manifest handling

The manifest should itself be snapshot-shaped.

- `manifest.json` should be overwritten in place
- old manifests should not accumulate in HEAD as a new append pile
- Git history already preserves earlier manifest versions

Manifest chaining is still valuable even with Git history because it gives PRISM an internal continuity chain across publishes, including rebases, squashes, mirrors, or other Git history rewrites.

---

## 8. Runtime Journal Model

The runtime/shared append log remains useful and should not disappear.

It becomes the place for:

- fine-grained coordination mutations
- high-frequency watcher flushes
- detailed audit history
- recovery and continuity detail
- optional later export of high-resolution lineage

The intended authority boundary is:

- tracked `.prism` answers what durable repo state is currently published
- runtime/shared journals answer how the runtime moved between publish boundaries

That means the journal is authoritative for operational history, but not required for interpreting the published repo snapshot.

### 8.1 Runtime journal requirements

The runtime/shared journal should be:

- append-only
- fine-grained
- compactable
- namespaceable by logical repo identity once that layer exists

It should also support two concrete serving modes:

- local journal mode for a single local runtime or offline clone
- shared journal mode when multiple worktrees, clones, or machines participate in one shared runtime

The shared mode is the long-term authoritative home for high-resolution operational history.
The local mode is the fallback and offline spool.

### 8.2 Local versus shared behavior

PRISM should treat local and shared journals as the same logical model with different placement:

- local runtime keeps the journal when no shared runtime is available
- shared runtime absorbs or supersedes that journal when shared coordination exists
- publication manifests may reference either local or shared journal checkpoints through a neutral journal namespace field

The repo snapshot should not need to know which storage tier held the detailed journal.
It only needs a stable optional reference to the summarized journal range.

### 8.3 Runtime journal retention

PRISM may later support:

- journal segmentation
- periodic runtime checkpoints
- compaction of old ranges into summarized runtime snapshots
- optional export for long-term archival

But tracked `.prism` does not need to carry that detailed history by default.

---

## 9. Linking Snapshots to Runtime History

Tracked repo snapshots must remain understandable even when the runtime journal is absent.

That means:

- snapshot interpretation must not depend on runtime/shared logs
- references back to runtime history are optional provenance links, not required dependencies

The publish manifest may include:

- journal namespace
- checkpoint range
- event count
- digest over the summarized runtime range

If that runtime history still exists, PRISM can reconstruct the fine-grained path.
If it does not, the repo snapshot remains valid and interpretable.

The practical rule is:

- repo snapshot restore must succeed without the journal
- journal linkage enriches provenance and deep replay only when the journal is available

### 9.1 Snapshot-to-journal linkage shape

The snapshot boundary should only link to the journal at a coarse summarized range, for example:

- `journalNamespace`
- `fromCheckpoint`
- `toCheckpoint`
- `eventCount`
- `eventDigest`

That is enough to:

- prove which journal slice the publish summarized
- let a later runtime reconnect the durable snapshot to high-resolution operational history
- avoid embedding the detailed journal payload into tracked repo state

It is intentionally not enough to make the snapshot depend on replaying the journal.

This preserves self-contained cold-clone semantics while keeping optional high-resolution traceability.

The trust downgrade should be explicit:

- the repo alone guarantees published state authenticity and publisher attestation
- the runtime/shared journal carries the fine-grained path that led there
- losing the runtime journal loses high-resolution offline lineage, but not the ability to trust and interpret the published repo snapshot

---

## 10. Startup and Restore Contract

### 10.1 Hot startup

Hot startup should:

- open local runtime/cache state
- verify current tracked snapshot manifests
- load current snapshot files directly
- restore hot runtime state from snapshot plus runtime authority journals only where necessary

It should not:

- fully replay large committed repo event streams
- fully reverify a historical committed append log on every restart
- reconstruct current durable repo state by replaying old tracked repo events

### 10.2 Cold startup

Cold startup should:

- verify the current snapshot manifest
- verify referenced snapshot file hashes
- load snapshot files directly
- optionally reconcile with runtime/shared journals if those are available

Cold startup correctness should come from current snapshot verification, not full historical replay of tracked repo logs.

---

## 11. Implications for Current `.prism` Streams

The current committed append-log surfaces should be treated as transition-era artifacts, not the long-term target.

In particular, committed streams like:

- `.prism/changes/events.jsonl`
- `.prism/memory/*.jsonl`
- `.prism/plans/streams/*.jsonl`
- `.prism/plans/index.jsonl`
- `.prism/plans/active/*.jsonl`
- `.prism/concepts/events.jsonl`
- `.prism/contracts/events.jsonl`

should be reevaluated against the new boundary:

- if the file exists only to provide Git history, it should become snapshot state instead
- if the file exists only to preserve fine-grained operational detail, it should move to runtime/shared state instead
- only compact durable snapshot state and signed publish manifests should remain as tracked repo truth

The steady-state target is stronger than "legacy logs exist but are no longer authoritative":

- legacy tracked append-log files should disappear from the tracked repo entirely
- migration continuity should live in snapshot metadata, not in indefinitely retained tracked logs
- cold-clone restore should depend only on tracked snapshots and manifests

This is a true rewrite, not a small optimization.

By adopting this model, PRISM also explicitly inherits more of the repo's Git governance model for durable history semantics:

- squash merges matter
- force pushes matter
- branch protection matters
- retention policy matters

That is acceptable and desirable, but it should be treated as part of the trust envelope.

---

## 12. Migration Direction

The rewrite should proceed in stages.

### Stage 1: lock the contract

- define the tracked snapshot artifact set
- define the signed publish manifest schema
- define which domains publish durable snapshots
- define which runtime journals remain outside tracked `.prism`

### Stage 2: stop growing tracked append logs

- stop publishing new bulky committed append-log payloads
- keep runtime/shared append logs for fine-grained history
- introduce compact snapshot generation and verification

### Stage 3: move startup to snapshot restore

- load tracked repo snapshots directly
- remove replay dependence on committed repo event logs
- keep runtime journal reconciliation only where operational continuity requires it

### Stage 4: retire tracked append logs

- migrate current tracked streams to snapshot artifacts
- remove old replay-first startup paths
- keep compatibility readers only as a temporary migration bridge

### Stage 5: preserve provenance continuity

- the first snapshot-format manifest should reference the final event-log-era checkpoint or digest it supersedes
- migration should record the source stream range or root digest that the first snapshot-format publish was derived from
- this creates an explicit continuity bridge instead of a provenance cliff between old PRISM and the new snapshot authority format

---

## 13. Open Questions

The rewrite leaves a few explicit follow-up questions:

- should publish manifests be one-per-domain or one-per-publish boundary?
- should runtime journal checkpoints be referenced by digest, logical sequence, or both?
- how much work-level provenance belongs inside every snapshot file versus only in the publish manifest?
- how should signed manifest verification integrate with future logical repo identity?

These are design details.
They do not change the central decision:

tracked `.prism` should become compact signed snapshots, and the fine-grained append log should move out of Git and into runtime/shared state.

---

## 14. Recommendation

Adopt the snapshot rewrite as the target architecture for tracked `.prism`.

Specifically:

- use Git history as the durable branchable/time-travelable history layer
- publish compact tracked snapshot files instead of committed append logs
- sign publish manifests at snapshot boundaries
- keep fine-grained append-only history in runtime/shared state
- keep cold-clone restoration self-contained from the repo alone

This is the simplest architecture that preserves PRISM's strongest property without dragging a blob store or replay-heavy committed ledger into every clone.
