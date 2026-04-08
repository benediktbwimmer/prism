# PRISM Coordination Conflict Handling

Status: normative coordination write contract
Audience: PRISM coordination, core, MCP, service, and shared-ref maintainers
Scope: authoritative concurrent coordination writes, currently to `refs/prism/coordination/**` for
the Git backend, including CAS retry, transaction replay, semantic merge, and deterministic rejection

---

## 1. Summary

PRISM must answer one coordination question unambiguously:

> When two writers race, when do we replay, when do we semantically merge, and when do we reject?

The rule set is:

- each coordination root has exactly one active authority backend
- every logical coordination change is expressed as one transaction over the canonical plan/task
  model
- writers stage that transaction against the latest verified authoritative state for the affected
  write set
- publication uses backend-appropriate conflict detection against the staged authority base
- if that authority base advanced, the default recovery path is refetch, restage, and replay the full
  transaction intent
- semantic merge is used only when replay reaches the same authoritative payload and the schema
  defines a deterministic three-way merge rule
- PRISM rejects the write when replay or merge would violate lifecycle or graph invariants, or when
  the schema does not define a deterministic winner

The important split is:

- replay is the default stale-head recovery mechanism
- semantic merge is a bounded tool used inside replay when the same payload changed concurrently
- rejection is the required outcome when the system cannot preserve correctness honestly

## 2. Authority And Write Set

The conflict model operates on the authoritative write set, not on derived views.

For the current Git backend, that write set is implemented as coordination refs under
`refs/prism/coordination/**`.

Authoritative coordination writes may touch:

- task shard refs
- claim shard refs
- any future authoritative artifact, review, or event shard families
- runtime descriptor refs when those descriptors are authoritative

Derived or aggregate surfaces do not define merge policy:

- summary refs
- operator-console read models
- local SQLite materializations
- startup checkpoints
- projection bundles

If an implementation still republishes a derived summary ref, that summary is regenerated from the
successful authoritative write set. It is not an independent conflict arena.

## 3. Default Write Algorithm

Every coordination mutation follows this algorithm:

1. determine the affected authoritative ref set
2. fetch and verify the latest heads for that ref set
3. materialize one staged canonical coordination snapshot from those heads
4. apply the full logical transaction to that staged snapshot
5. validate the final staged graph and lifecycle state
6. publish with CAS against the heads used for staging
7. if CAS loses a race, refetch, rebuild the staged snapshot, and replay the original transaction
8. only surface success after the authoritative write set has been published successfully

The transaction intent is replayed, not a stale partially-materialized result.

That distinction is mandatory. PRISM must not preserve correctness by accident.

## 4. When To Rebase And Replay

Replay is the default answer whenever the base moved but the original transaction still has a clear
meaning against the newer head.

Replay is appropriate when:

- another writer changed a different shard
- another writer changed the same shard but different stable objects
- another writer changed the same object, but the local mutation is expressed as an intent that can
  be re-evaluated against the newer state
- another writer changed derived or summary surfaces only

Examples:

- a task status update races with an unrelated claim update in another shard
- two writers add different dependency edges inside the same task shard
- one writer archives a completed plan while another updates a different task under another plan

In all of those cases, PRISM should not attempt a bespoke merge first. It should rebuild the latest
authoritative snapshot and replay the transaction.

## 5. When To Semantically Merge

Semantic merge is only used when replay reaches a payload that was changed concurrently by both
writers and the schema defines a deterministic result.

The merge happens at the payload or field-semantics layer, not with Git text conflict markers.

Allowed semantic-merge classes:

- set-like arrays: union with stable ordering
- append-only histories: append or union with de-duplication by stable id
- maps keyed by stable id: key-wise recursive merge
- purely observational scalars: last-writer-wins only when the schema explicitly allows it
- lifecycle enums: only if the schema defines a precedence or dominance rule

Semantic merge is therefore a narrow tool. It is not a blanket license to auto-resolve every race.

## 6. When To Reject

PRISM must reject the write instead of silently guessing when:

- replay invalidates required optimistic preconditions
- the mutation depends on a base assumption that no longer holds
- a merge would violate containment, dependency, DAG, or lifecycle invariants
- two writers assert incompatible lease or claim ownership for the same epoch
- two writers assert incompatible terminal outcomes without a schema-defined precedence rule
- required referenced objects disappeared or changed identity in a way the transaction cannot
  reinterpret safely
- the reader or writer sees a schema version it does not support

Rejection is part of the contract, not a failure of the architecture. Honest rejection is better
than silently manufacturing incorrect shared state.

## 7. Practical Decision Table

| Situation | Response |
| --- | --- |
| Different authoritative shards changed | Refetch affected heads and replay |
| Same shard, different stable objects changed | Refetch that shard and replay |
| Same object changed, schema defines deterministic field merge | Refetch, replay, and semantic-merge that payload as needed |
| Same object changed, schema has no deterministic merge rule | Reject |
| Final replayed graph violates containment or DAG invariants | Reject |
| Publish loses CAS after successful local staging | Refetch and replay again |

## 8. Transaction Boundaries

Graph-structural edits must remain transactional.

That means:

- one logical graph rewrite is one transaction
- validation runs against the final staged graph, not against piecemeal intermediate writes
- no partially-applied graph rewrite may become visible in authoritative shared state

This is the bridge between the graph rewrite and the shard/CAS model:

- the graph rewrite defines what counts as one logical state transition
- shared-ref conflict handling defines how that transition survives concurrency safely

## 9. Diagnostics

Operators should be able to tell which path happened:

- CAS retry count by ref family and shard
- replay count
- semantic-merge success count
- semantic-merge rejection count by payload kind
- optimistic-precondition rejection count
- stale-head rejection count
- highest-contention shards

These diagnostics are part of the architecture because concurrency bugs are otherwise impossible to
debug under real multi-agent load.

## 10. Relationship To Other Docs

- `PRISM_COORDINATION_TARGET_ARCHITECTURE.md` defines the overall authority and service model
- `PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md` defines why sharding and schema-aware merging
  matter at scale
- `PRISM_COORDINATION_GRAPH_REWRITE.md` defines the canonical plan/task transaction model
- this document defines the exact concurrent-write decision tree that connects those designs
