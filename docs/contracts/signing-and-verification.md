# PRISM Signing And Verification

Status: normative contract  
Audience: auth, coordination, knowledge, runtime, query, MCP, CLI, storage, and future service maintainers  
Scope: what PRISM signs, what signatures attest, how verification works, trust states, and fail-closed behavior

---

## 1. Goal

PRISM must define one explicit **signing and verification** contract because trust in authoritative
state depends on more than chronology.

This contract exists so that:

- authoritative writes are attributable and tamper-evident
- verification semantics are explicit and portable
- freshness states like `verified_current` and `verified_stale` have trust meaning

## 2. Core invariants

The signing and verification layer must preserve these rules:

1. What is signed and what a signature means must be explicit.
2. Verification must be required before protected authoritative state is accepted.
3. Verification failure must fail closed for authority-sensitive use.
4. Mutable trust roots and secret signing material must stay out of repo-published truth.
5. Historical verification must remain possible with portable public verification material.

## 3. What gets signed

The contract must distinguish at least:

- authoritative published knowledge or repo-state manifests
- authoritative coordination state transitions or equivalent backend attestations
- runtime descriptors or other published runtime authority records when designated by contract
- exported trust or verification bundles

Not every derived artifact must be signed.
The signing boundary must be explicit per authority family.

## 4. What signatures attest

At minimum, a valid signature should attest that:

- the signed payload bytes are intact
- a trusted signing authority or key attested that PRISM admitted or published that payload
- the attested payload carries explicit identity and provenance metadata

Signatures do not by themselves prove:

- the signer behaved benevolently
- the signer is still trusted for new writes today
- a human personally typed the change

## 5. Verification states

The contract must support at least these trust postures:

- verified current
- verified stale but trusted
- unavailable
- failed verification

These trust states refine the freshness language from
[consistency-and-freshness.md](./consistency-and-freshness.md).

## 6. Fail-closed rules

PRISM must fail closed when:

- required signatures are invalid
- required signer or trust material is unavailable or untrusted
- protected authoritative payloads are malformed or tampered

Fail closed means:

- do not treat the candidate state as authoritative
- surface diagnostics explicitly
- preserve last trusted local materialization when allowed by contract

## 7. Portable verification material

The contract must allow historical verification on another machine through portable public
verification material such as:

- trust bundles
- authority-root certificates
- exported public verifier metadata

Portable verification material may be imported locally.
Private signing keys must not be required for historical verification.

## 8. Relationship to authority

Signing and verification underpin authoritative truth.

That means:

- authority stores should expose verification status and provenance metadata
- materialized stores should only materialize verified authority for authority-sensitive uses
- mutation and publication protocols must identify signing boundaries explicitly

## 9. Relationship to existing design notes

This contract captures the normative trust boundary distilled from:

- `docs/PROTECTED_PRISM_STATE_SIGNATURES.md`

That broader design note may continue to carry deeper implementation detail and migration strategy.

## 10. Minimum implementation bar

This contract is considered implemented only when:

- authoritative protected state is never accepted without required verification
- trust states are explicit in authority-sensitive reads
- portable public verification material is part of the design rather than an afterthought
