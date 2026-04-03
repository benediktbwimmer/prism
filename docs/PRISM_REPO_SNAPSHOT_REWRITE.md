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

### 6.1 Snapshot requirements

Each snapshot file should be:

- deterministic
- compact
- self-contained for interpretation
- independent of runtime-only state

Each snapshot should be understandable from the repo alone.

### 6.2 Snapshot scope

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

Examples of coordination state that should stay out of tracked snapshots:

- ephemeral leases
- watcher churn
- transient execution internals
- heartbeat noise
- short-lived handoff mechanics that do not change durable repo understanding

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

### 7.2 Why this boundary is correct

This lets PRISM answer:

- what snapshot was published?
- who published it?
- under what work context?
- has it been tampered with?

without requiring tracked `.prism` to preserve a full fine-grained event ledger.

### 7.3 Publish boundary

The publish boundary should be a single logical PRISM publication step:

- PRISM computes the new tracked snapshot files
- PRISM writes the updated stable snapshot files
- PRISM writes the updated stable `manifest.json`
- Git commit records that coherent snapshot state

The boundary should be treated as atomic in PRISM's semantics.
It should not be modeled as a fuzzy sequence of loosely related repo updates.

### 7.4 Manifest handling

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

### 8.1 Runtime journal requirements

The runtime/shared journal should be:

- append-only
- fine-grained
- compactable
- namespaceable by logical repo identity once that layer exists

### 8.2 Runtime journal retention

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
- `.prism/concepts/events.jsonl`
- `.prism/contracts/events.jsonl`

should be reevaluated against the new boundary:

- if the file exists only to provide Git history, it should become snapshot state instead
- if the file exists only to preserve fine-grained operational detail, it should move to runtime/shared state instead
- only compact durable snapshot state and signed publish manifests should remain as tracked repo truth

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
