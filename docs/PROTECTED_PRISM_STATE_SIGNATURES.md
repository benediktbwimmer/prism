# Protected `.prism` State Signatures

Status: pre-implementation design note  
Audience: PRISM core, MCP, coordination, storage, and security maintainers  
Scope: authenticated publication, verification, and tamper handling for repo `.prism` state

---

## 1. Summary

PRISM should stop treating raw filesystem edits to repo `.prism` state as authoritative mutations.

Instead:

- repo `.prism` remains the durable published repo plane
- shared runtime state remains the mutable identity and coordination plane
- authoritative tracked repo `.prism` state is accepted only when it is emitted as a canonical
  runtime-attested signed publish boundary from an authenticated PRISM mutation path
- external or unknown filesystem edits to protected repo `.prism` paths are treated as tamper or
  drift, not as valid state transitions

The key design moves are:

- sign canonical publish manifests, not informal file edits and not individual lines
- require PRISM runtime attestation for authoritative repo publication

This preserves the properties we want from repo `.prism`:

- reviewable
- branchable
- hydratable
- diffable

without sacrificing the identity and provenance guarantees that motivated authenticated runtime
mutations in the first place.

---

## 2. Problem

If PRISM allows arbitrary filesystem edits to repo `.prism` files to become authoritative state
changes, then PRISM loses an important guarantee:

- it no longer knows which authenticated principal authored that state transition

Filesystem observation can usually tell PRISM only that:

- a path changed
- the change happened in a repo/worktree
- the local process observed it

Filesystem observation does **not** reliably tell PRISM:

- which principal caused the change
- whether the change came through a valid PRISM mutation
- whether the content was manually edited, tool-generated, merged, replayed, or tampered with

That is acceptable for cache invalidation.
It is not acceptable for authoritative mutation provenance.

---

## 3. Design Goal

PRISM should provide a strong answer to:

- which principal authorized this durable `.prism` state transition?
- did PRISM itself admit that transition through an authenticated mutation path?

The system should not settle for:

- some local process touched this file

The target guarantee is:

- if a protected repo `.prism` record is accepted as authoritative, PRISM can verify that a trusted
  PRISM runtime authority signed an event attesting that an authenticated principal mutation was
  admitted, and that the record has not been modified afterward

---

## 4. Core Decision

### 4.1 Repo `.prism` is PRISM-managed

Repo `.prism` should be treated as PRISM-managed state, not as a general editable workspace for
arbitrary tools.

That means:

- PRISM may write protected repo `.prism` files
- PRISM may hydrate from protected repo `.prism` files
- raw external edits to protected repo `.prism` files are not authoritative mutations

This is a semantic rule, not an operating system guarantee.

PRISM cannot stop a user or tool from editing a file on disk.
What it can do is refuse to treat such an edit as an authoritative state transition unless it
verifies as one.

### 4.2 Runtime-attest canonical publish boundaries, not file edits

PRISM should sign canonical publish manifests with a trusted PRISM runtime authority key.

PRISM should **not** attempt to:

- sign individual lines
- sign informal file edits
- infer authorship from watcher events
- infer authorship from file timestamps

Authoritative state should flow as:

1. authenticated mutation request
2. runtime verification of principal identity and policy
3. canonical publish payload containing principal identity, authority identity, work context, and
   snapshot file digests
4. runtime attestation signature over the canonical publish payload
5. write the updated tracked snapshot files and stable protected manifest
6. hydrate only from verified runtime-attested snapshot state

Optional principal countersignatures can be added later, but they should not be the authoritative
mechanism in v1.

### 4.3 Identity authority stays out of the repo

Trusted runtime authority public keys, principal and credential verification state, and revocation
state must remain in the shared runtime / local home plane, not in repo `.prism`.

Repo `.prism` carries signed published facts.

It does not carry the mutable trust root that decides which keys are valid today.

### 4.4 Verification material must be portable

The mutable trust registry should stay out of the repo, but verification of historical protected
repo state must remain possible on another machine.

That means PRISM needs portable public verification material, such as:

- signed authority-root certificates
- signed trust checkpoint exports
- or an explicit `prism trust export` / `prism trust import` flow

A fresh clone should not need the original machine's hidden local database just to verify old
protected `.prism` history.

---

## 5. Protected Surface

The long-term goal should be:

- all remaining repo `.prism/**` files are PRISM-managed

That does **not** mean every repo `.prism` file is equally authoritative.

PRISM should distinguish:

- authoritative protected snapshot artifacts and manifests
- derived or convenience artifacts regenerated from authoritative snapshot state

V1 protected authoritative scope is fixed to exactly:

- snapshot-form plan, concept, relation, contract, and memory state under tracked `.prism/state/**`
- the stable protected publish manifest that attests to that snapshot boundary

Plan-specific rule:

- authoritative plan truth in v1 lives in signed snapshot-form published plan state
- plan lifecycle state such as `Active` or `Archived` is carried in the signed snapshot and its
  protected manifest
- repo indexes and generated placement files remain derived convenience state
- any `active/` or `archived/` plan placement is derived convenience state, not authoritative truth

Derived repo artifacts such as generated docs may still be PRISM-managed, but they should not be
treated as mutation inputs.

If they are edited externally, PRISM should prefer regeneration from authoritative events over
accepting those edits as truth.

Everything else under repo `.prism/**` is non-authoritative in v1, even if PRISM generates it.
Implementation should not expand the protected authoritative set without an explicit spec revision.

---

## 6. Non-Goals

This design does **not** aim to prove:

- which human physically typed a line
- which OS process wrote a file
- whether the signer behaved honestly
- whether a valid signing key was compromised

Signatures prove a narrower and more useful property:

- a trusted runtime authority key attested that PRISM admitted this canonical event payload

That is the right level of proof for PRISM.

---

## 7. Security Model

### 7.1 What signatures prove

Given a verified record, PRISM can say:

- the payload bytes are intact
- the envelope was signed by trusted runtime authority key `K`
- `K` was trusted for runtime authority `A` under the recorded verification material
- the runtime attested that authenticated principal `P` caused or authorized the admitted mutation

### 7.2 What signatures do not prove

Given a verified record, PRISM cannot automatically say:

- the signer was benign
- the signer was the only actor on the machine
- the signer still owns the key today
- the change was reviewed or semantically correct
- a human personally typed the change instead of an approved agent acting for that principal

### 7.3 Threats this design addresses

- anonymous filesystem edits to protected `.prism` files
- accidental edits by editors, formatters, or scripts
- post-hoc tampering with already published protected records
- replay of unsigned content as if it were authenticated
- bypass of the PRISM runtime as the admission point for authoritative repo publication

### 7.4 Threats this design does not solve alone

- stolen or leaked signing keys
- malicious but valid principals
- merge conflicts between independently valid signed histories
- unsafe trust-root updates

---

## 8. Invariants

PRISM should enforce the following invariants:

1. A protected authoritative repo `.prism` record is accepted only if its signature verifies.
2. Verification uses a trusted runtime authority key with recorded principal identity inside the
   attested event.
3. Protected authoritative repo `.prism` logs are append-only.
4. Invalid, malformed, unsigned, truncated, or tampered protected records are never silently
   accepted.
5. External filesystem observation is never treated as identity proof.
6. Shared runtime identity state is authoritative for trust and key binding, not the repo tree.
7. Hydrated hot-memory state must never claim authenticated authorship for a record that did not
   verify.

---

## 9. Data Model

### 9.1 Trust and signer material

PRISM needs trusted runtime authority material containing at least:

- `authority_root_id`
- `runtime_authority_id`
- `runtime_key_id`
- `public_key`
- `algorithm`
- `status`
- `activated_at`
- `revoked_at` or equivalent lifecycle metadata
- trust-epoch, checkpoint, or certificate-chain metadata sufficient for historical verification

PRISM also needs principal identity metadata associated with admitted mutations, such as:

- `principal_authority_id`
- `principal_id`
- credential or authenticator reference
- admission context metadata needed for audit trails

This belongs in the shared runtime / local home plane.

### 9.2 Portable verification material

V1 should use an explicit trust-bundle format stored outside the repo, for example:

- `~/.prism/trust/bundles/<bundle_id>.json`

Each bundle should contain only public verification material and lifecycle metadata, never private
keys.

The v1 bundle format includes:

- `bundle_version`
- `bundle_id`
- `authority_root_id`
- `issued_at`
- `issuer_key_id`
- `runtime_authorities`
- `runtime_keys`
- `signature`

Where:

- `runtime_authorities` binds stable `runtime_authority_id` values to their issuing root and
  validity window
- `runtime_keys` records:
  - `runtime_key_id`
  - `runtime_authority_id`
  - `algorithm`
  - `public_key`
  - `activated_at`
  - `revoked_at`
  - `revocation_kind`
  - optional `historically_compromised_from`

Fresh machines verify protected repo state by importing one or more bundles through
`prism trust import`.

If the required bundle is absent locally, the stream status is `UnknownTrust` and the stream is not
authoritative for hydration.

V1 import semantics should be:

- if `authority_root_id` is already trusted locally, verify the bundle signature and install it
- if `authority_root_id` is unknown locally, require explicit operator trust pinning before install
- once imported, the bundle is immutable; later updates arrive as new bundle ids, not in-place edits

### 9.3 Signed event envelope

Each protected event should carry an envelope conceptually like:

```json
{
  "envelope_version": 1,
  "stream": "repo_plan_events",
  "stream_id": "plan:...",
  "event_id": "event:...",
  "prev_event_id": "event:...",
  "prev_entry_hash": "sha256:...",
  "sequence": 42,
  "runtime_authority_id": "authority:runtime:...",
  "runtime_key_id": "key:runtime:...",
  "trust_bundle_id": "trust-bundle:...",
  "principal_authority_id": "authority:...",
  "principal_id": "principal:...",
  "credential_id": "credential:...",
  "algorithm": "ed25519",
  "payload_hash": "sha256:...",
  "signature": "base64:...",
  "payload": { ...canonical event payload... }
}
```

The v1 envelope field names are fixed as shown above. The important properties are:

- stable event identity
- predecessor linkage
- runtime authority identity
- runtime key identity
- principal identity
- sequence metadata for diagnostics
- deterministic payload hashing
- verifiable signature

`sequence` is a required runtime-emission counter in v1, but it is diagnostic rather than
authoritative. Implementations may surface sequence anomalies, but predecessor linkage and
signature verification decide stream validity.

### 9.4 Canonical payload

The payload should include at least:

- event kind
- target object ids
- mutation body
- logical timestamp
- any required expected revision information

The payload should **not** rely on the surrounding file layout for meaning.

The signed unit is the event payload, not its current line number in a file.

The payload schema should also avoid floating-point values in v1.
Prefer:

- strings
- integers
- booleans
- arrays and objects
- explicit timestamp strings

### 9.5 Hashing and signing rules

V1 should freeze the hashing and signing algorithm as:

1. `payload_bytes = JCS(payload)`
2. `payload_hash = sha256(payload_bytes)`
3. `envelope_to_sign = JCS(envelope_without_signature)` where `envelope_without_signature`
   contains every envelope field except `signature`
4. `signature = Ed25519Sign(runtime_private_key, envelope_to_sign)`
5. `entry_bytes = JCS(full_envelope_with_signature)`
6. `entry_hash = sha256(entry_bytes)`
7. the next event stores:
   - `prev_event_id = previous.event_id`
   - `prev_entry_hash = previous.entry_hash`

`entry_hash` is a derived verification value, not a separately authored semantic field. It may be
cached by implementations, but it must always recompute from the canonical signed envelope bytes.

The root event for a protected stream uses:

- `prev_event_id = null`
- `prev_entry_hash = null`

### 9.6 JSONL framing

Protected stream files use a fixed JSONL framing contract in v1:

- file encoding is UTF-8
- each logical record is one canonical JCS envelope serialized onto exactly one line
- line delimiter is `\n`
- the trailing newline delimiter is not part of the signed bytes
- blank lines are invalid
- a final unterminated or partially written line is `Truncated`, not best-effort parsed

---

## 10. Canonicalization

Signatures only work if the signed bytes are deterministic.

PRISM therefore needs one canonical serialization rule for signed payloads.

Requirements:

- deterministic field ordering
- deterministic string escaping
- deterministic numeric formatting
- deterministic omission/default handling
- stable encoding version

PRISM should not sign ad hoc JSON produced by whichever serializer happens to run in that code
path.

PRISM should define one canonical byte encoding for protected signed payloads and use it
everywhere.

For JSON payloads, PRISM should standardize on RFC 8785 JSON Canonicalization Scheme (JCS) rather
than inventing an ad hoc serializer contract.

That gives PRISM:

- deterministic object key ordering
- deterministic UTF-8 encoding
- deterministic escaping and spacing rules
- a published interoperability target instead of repo-local folklore

If PRISM later adopts a binary encoding for some protected streams, that should be introduced as an
explicit new encoding/version, not as an implicit alternative to the JSON path.

Canonical JSON remains the better fit for repo `.prism` because it is inspectable, diffable, and
review-friendly, but the implementation must still verify crate behavior carefully for numeric
formatting and other edge cases before trusting it as the canonical source of bytes.

PRISM should pin:

- one exact JCS implementation for v1
- one explicit payload schema version
- cross-platform fixture tests with exact byte snapshots

---

## 11. Write Path

Tracked repo publication now uses signed snapshot manifests as the durable boundary.
The append-only protected event-log path below should be treated as the legacy or migration model
for repo-tracked `.prism` state, while runtime/shared journals may still use append-only signed
event streams for high-resolution operational history.

The authoritative write path for protected repo `.prism` state should be:

1. client sends authenticated mutation with credential
2. PRISM verifies the credential against the shared runtime trust state
3. PRISM verifies that the target stream status is `Verified`, or `MigrationRequired` during an
   explicit `prism migrate-sign` flow
4. PRISM verifies the current stream tail under the runtime write lock before appending
5. PRISM constructs a canonical mutation event payload containing principal identity, authority
   identity, and mutation body
6. PRISM chooses the active `runtime_key_id` and `trust_bundle_id` for the admitting runtime
   authority
7. PRISM signs the payload with the trusted runtime authority signing key
8. PRISM computes the derived `entry_hash`
9. PRISM writes the updated tracked snapshot shards for the affected published state
10. PRISM computes and signs the stable `.prism/state/manifest.json` over that exact file set
11. PRISM fsyncs the manifest and any newly written authoritative snapshot shards
12. if a file was newly created or replaced, PRISM fsyncs the parent directory as well
13. PRISM updates hot memory from the accepted publication
14. watcher echoes of PRISM's own write are suppressed as self-writes, not re-ingested as new
    mutations

The protected repo write should be the published artifact of an authenticated mutation, not an
alternative mutation mechanism.

Optional principal countersignatures may later be stored as supplementary audit material, but they
are not required for authoritative publication in v1.

Authoritative writes must refuse to append when the stream status is:

- `Conflict`
- `Corrupt`
- `Truncated`
- `Tampered`
- `UnknownTrust`
- `LegacyUnsigned`

---

## 12. Hydration and Verification

For tracked repo `.prism` state, hydration should verify the current snapshot manifest and shard
digests directly. Full append-log replay remains relevant for runtime/shared journals and for
legacy migration inputs, but it is no longer the steady-state tracked repo restore path.

Hydration of protected repo `.prism` state should:

1. open `.prism/state/manifest.json`
2. canonicalize the manifest signing view exactly as the signer did
3. resolve `trust_bundle_id` to imported public verification material
4. verify that `runtime_key_id` was valid for the declared `runtime_authority_id` under that bundle
5. verify the manifest signature against the trusted public key for `runtime_key_id`
6. verify every authoritative shard digest referenced by the manifest
7. classify the snapshot result
8. project only `Verified` snapshot sets into hot state for authoritative reads and writes

Portable verification should work with imported public trust material on a fresh machine, even when
the original mutable local trust database is not present.

The verifier should classify each protected stream as exactly one of:

- `Verified`
  - exactly one linear valid head is reachable from the root
- `Conflict`
  - multiple individually valid heads are reachable from a common valid base and no
    `StreamReconciled` event resolves them
- `Corrupt`
  - broken predecessor reference, duplicate id, cycle, bad hash, malformed record, or other
    structural invalidity exists in the authoritative path
- `Truncated`
  - the file ends on an incomplete final record boundary
- `UnknownTrust`
  - the required trust bundle or runtime key material is unavailable or not trusted locally
- `LegacyUnsigned`
  - the stream has not yet been migrated into signed mode
- `MigrationRequired`
  - the stream is intentionally in an operator-controlled pre-migration state awaiting
    `prism migrate-sign`

Hydration should be fail-closed for protected authoritative records.

Fail-closed means:

- invalid signature is not best-effort accepted
- malformed event is not best-effort accepted
- unknown key is not best-effort accepted

Instead PRISM should surface:

- tamper
- drift
- trust failure
- migration required

depending on the exact condition.

---

## 13. External Edit Detection

### 13.1 What PRISM can know

PRISM can positively know only one safe case:

- PRISM itself just wrote this exact protected record through its own authenticated path

This is useful for watcher loop suppression.

### 13.2 What PRISM usually cannot know

PRISM usually cannot prove from the filesystem alone:

- who edited a file
- whether the edit was manual
- whether the edit came from another tool

So PRISM should not try to classify edits as:

- definitely human
- definitely formatter
- definitely a specific agent

The only safe classification is:

- `self-write`
- `external_or_unknown`

### 13.3 Self-write suppression

To suppress watcher echoes, PRISM may keep a short-lived in-memory cache of recent self-writes:

- path
- content hash
- byte length
- write time
- local monotonic sequence

If a watcher event matches a recent self-write, PRISM may ignore it as an echo.

If it does not match, PRISM should treat it as `external_or_unknown`.

This classification is for loop suppression only.

It is **not** a source of authenticated authorship.

---

## 14. Replay Protection

Signature validity alone is not enough.

PRISM must also prevent replay of old but valid events as if they were fresh.

Needed protections:

- globally unique `event_id`
- `prev_event_id`
- required `prev_entry_hash` chaining within a protected append-only stream
- required `sequence` for diagnostics and local ordering insight only

At minimum, PRISM should reject:

- duplicate event ids
- orphan append attempts that do not reference the known tail
- events that break stream predecessor linkage
- events whose `prev_entry_hash` does not match the canonical hash of the referenced predecessor

Predecessor-hash chaining matters even when each surviving event still has a valid signature.
Without it, an attacker or buggy merge can truncate the tail of a protected log and leave the
remaining events individually valid. With predecessor-hash linkage, the next append or full-stream
verification can detect that the protected history was truncated or reordered.

In a DVCS setting, predecessor linkage should be treated as the authoritative integrity mechanism.
Sequence may still be useful, but it should not be the core truth model for cross-branch
reconciliation.

---

## 15. Branch and Merge Semantics

Branching is a feature, not a bug, for repo `.prism`.

Two branches may each contain valid signed histories.

Signature validity answers:

- was this event authentically signed?

It does **not** answer:

- how should two valid but conflicting histories merge?

PRISM therefore needs explicit merge semantics for protected logs:

- append-only per stream
- duplicate elimination by `event_id`
- conflict handling when two branches publish incompatible mutations to the same protected stream
- explicit runtime-authored reconcile events to restore one authoritative continuation

V1 should stay strict:

- two independently signed tails on the same protected stream are a conflict
- git line merge alone is not a semantic merge
- PRISM should not silently choose one tail as authoritative

This is a semantic resolution problem layered on top of cryptographic authenticity.

V1 should use one explicit reconcile event shape, conceptually:

- event kind: `StreamReconciled`
- envelope predecessor:
  - `prev_event_id = accepted_head_event_id`
  - `prev_entry_hash = hash(accepted_head_event)`
- payload fields:
  - `conflict_base_event_id`
  - `accepted_head_event_id`
  - `rejected_head_event_ids`
  - `resolution_reason`
  - optional operator note or artifact reference

Semantics:

- the accepted head becomes the parent of future authoritative continuation
- rejected heads remain preserved in history for audit and diagnostics
- reads surface the stream as `Conflict` until a valid reconcile event exists
- mutation paths must refuse to append to a conflicted stream until it is reconciled

---

## 16. Key Lifecycle

### 16.1 Key storage

Private signing keys must not live in repo `.prism`.

They belong in the local/shared runtime plane, such as:

- `~/.prism/...`
- OS keychain integration later if desired

### 16.2 Rotation

PRISM must support rotating keys without invalidating old signed history.

That implies:

- historical records remain verifiable against historical trusted keys
- active signing uses only non-revoked current keys
- exported public verification material remains sufficient to verify older events after rotation

### 16.3 Revocation

PRISM must not treat the payload's own `issued_at` or logical timestamp as sufficient proof that a
key was trusted when the event was signed.

Historical verification should instead rely on durable trust lifecycle records outside the repo,
such as:

- recorded key activation and deactivation epochs
- signed trust checkpoint material
- revocation entries with explicit effective semantics

This matters because PRISM needs at least two distinct revocation meanings:

- `revoked_for_future_use`
  - the key may no longer authorize new writes, but previously valid historical events may remain
    valid
- `historically_compromised`
  - prior events may need quarantine, distrust, or explicit operator review because the key cannot
    be trusted even for some historical period

Verification policy should therefore be:

- use current trust state to decide whether a key may sign new events
- use imported trust-bundle lifecycle state to decide whether an old event remains valid in v1

PRISM should not rely on hydration-time trust state alone, and it should not rely on event payload
timestamps alone.

PRISM should make authority roots and stable authority ids explicit from day one.

---

## 17. Migration

PRISM already has unsigned repo `.prism` state.

V1 should use one migration rule for protected streams:

- archive the unsigned source as legacy diagnostic input
- emit one signed migration baseline
- continue only in the new signed stream

The migration must avoid the worst hybrid state:

- old unsigned and new signed records mixed without clear authority rules

For protected streams in active signed mode, PRISM should not mix unsigned and signed records in the
same live authoritative stream.

The v1 migration steps are:

- mark the old stream `legacy_unsigned`
- archive the old raw stream under local home state, outside the repo plane, for example
  `~/.prism/repos/<repo_id>/migration/legacy_unsigned/...`
- compute a deterministic import digest from the raw legacy bytes
- emit a new signed baseline event such as `LegacyImported`
- continue authoritative publication only in the new signed stream

V1 migration should freeze `LegacyImported` as the baseline shape for protected streams. The payload
should include at least:

- `legacy_relative_path`
- `legacy_sha256`
- `legacy_record_count`
- `migration_tool_version`
- `migrated_at`
- optional operator note

After migration:

- the original protected path contains only the new signed stream
- the archived unsigned source is diagnostic input only
- hydration never mixes legacy unsigned records into the active signed stream

The system should surface one clear mode per protected stream:

- legacy unsigned
- migrated signed
- tampered/invalid

---

## 18. Failure Policy

PRISM should define explicit operator-facing outcomes.

### 18.1 Invalid signature

Result:

- reject the record
- mark the stream as tampered or invalid
- do not hydrate invalid state into hot memory

### 18.2 Unknown key

Result:

- reject the record
- surface trust failure
- do not hydrate as authoritative

This includes:

- unknown `trust_bundle_id`
- missing imported public trust material
- unknown `runtime_key_id` inside an otherwise known bundle

### 18.3 Malformed canonical payload

Result:

- reject the record
- surface corruption or incompatible format version

### 18.4 External edit to protected file

Result:

- do not treat it as an authoritative mutation
- if verification still passes, it is a valid published event
- if verification fails, surface tamper or drift

### 18.5 Partial write or truncated record

Result:

- reject the incomplete record
- surface corruption
- require repair or rollback to the last valid record boundary

### 18.6 Diagnostic states and repair tools

PRISM should expose first-class verification state for protected streams, at least:

- `Verified`
- `LegacyUnsigned`
- `UnknownTrust`
- `Tampered`
- `Corrupt`
- `Truncated`
- `Conflict`
- `MigrationRequired`

Read surfaces and diagnostics should carry these fields at minimum:

- `verification_status`
- `stream_id`
- `protected_path`
- `last_verified_event_id`
- `last_verified_entry_hash`
- `trust_bundle_id`
- `diagnostic_code`
- `diagnostic_summary`
- `repair_hint`

PRISM should distinguish:

- authoritative mode
  - invalid or unverified protected data is not used for mutation or trust-sensitive reasoning
- diagnostic mode
  - raw content may still be inspectable, but is clearly labeled non-authoritative

PRISM should also provide explicit operator tools rather than silent healing, for example:

- `prism verify`
- `prism diagnose`
- `prism migrate-sign`
- `prism trust export`
- `prism trust import`
- `prism quarantine`
- `prism repair --to-last-valid`
- `prism reconcile-stream`

V1 command expectations should be:

- `prism verify`
  - scan protected streams, verify trust, signatures, and predecessor linkage, and exit non-zero on
    any non-`Verified` protected stream
- `prism diagnose`
  - print stream-local failure details including the first bad event boundary and the failing trust
    or linkage check
- `prism migrate-sign`
  - archive an unsigned protected stream, emit `LegacyImported`, and leave the stream in signed mode
- `prism quarantine`
  - move an invalid or conflicting tail out of the active protected path without destroying evidence
- `prism repair --to-last-valid`
  - rewrite the active protected stream to the last fully verified event boundary after
    quarantining the invalid tail
- `prism reconcile-stream`
  - emit the required `StreamReconciled` event after the operator chooses the authoritative head

The repair policy in v1 should be:

- never auto-heal a protected stream during normal hydration
- never discard invalid or conflicting tails without first quarantining them
- truncate only to the last fully verified event boundary
- require an explicit PRISM-authored repair or reconcile action before authoritative writes resume

---

## 19. Interaction With the Three Planes

This design preserves the intended three-plane model:

- repo `.prism`
  - published runtime-attested signed repo truth
- shared runtime / local home
  - mutable identity authority, trust root, credentials, coordination continuity, and portable
    public verification material
- hot memory
  - live serving state hydrated from verified repo truth plus shared runtime authority

This avoids dual truth:

- repo files remain reviewable published state
- identity and trust remain mutable runtime concerns
- hot memory remains the live serving surface

---

## 20. Recommended V1 Scope

A minimal but defensible first version should:

1. protect authoritative repo event logs only, not generated docs or convenience artifacts
2. use runtime-attested signatures only for authoritative publication
3. use `Ed25519` only in v1
4. use RFC 8785 / JCS canonical JSON only in v1
5. require predecessor-hash linkage for protected append-only streams
6. make per-plan streams authoritative and keep plan indexes or active/archived placement derived
7. keep trust roots and private keys out of the repo, with exportable public verification material
8. verify signatures during hydration
9. fail closed on invalid protected records
10. classify watcher-detected non-self edits as `external_or_unknown`
11. surface tamper or drift instead of ingesting unknown edits as authority
12. leave derived repo artifacts unsigned and regenerated from authoritative logs
13. avoid automatic semantic merge of conflicting signed tails
14. archive legacy unsigned material outside repo `.prism`

This is enough to recover the guarantee we almost gave up:

- protected repo `.prism` state is not anonymous filesystem state

Recommended rollout order:

1. implement one simple protected stream family first, preferably `contracts/events.jsonl` or
   `concepts/events.jsonl`
2. validate signing, verification, trust import/export, diagnose, quarantine, and repair end to
   end on that stream family
3. extend to memory and concept-relation streams
4. add protected plan streams only after the new per-plan authoritative `plans/streams/` topology is
   in place and the derived index/projection path is working correctly

---

## 21. Deferred Work After V1

The following items are intentionally deferred and are not prerequisites for the first
implementation:

- optional principal countersignatures for stronger cross-runtime non-repudiation
- online or federated trust distribution beyond explicit bundle export/import
- binary canonical encodings for selected streams
- automatic or stream-specific commutative merge for carefully chosen event kinds
- OS keychain or HSM-backed runtime signing-key storage
- signing of derived repo artifacts
- richer repair UX beyond the required verify, diagnose, quarantine, truncate-to-last-valid, and
  reconcile flows

---

## 22. Recommendation

Proceed with the runtime-attested protected-event design, not with anonymous filesystem-ingest
authority.

That gives PRISM a much stronger contract:

- repo `.prism` remains portable and reviewable
- authoritative repo state is cryptographically attributable and runtime-admitted
- external edits are detectable as invalid authority, not silently accepted
- identity-sensitive guarantees continue to come from authenticated mutation provenance rather than
  from optimistic watcher behavior

With the v1 decisions frozen above, this document should be treated as the implementation contract
for the first protected-state rollout.
