# Full `prism_code` Compiler Cutover Phase 7b

Status: in progress  
Audience: prism-js, prism-mcp, prism-core, prism-cli, compiler, runtime, and coordination maintainers  
Scope: build the real `prism_code` compiler for all currently supported PRISM semantics, hard-cut the runtime onto that compiler, remove the old mutation API entirely, and expose one proper SDK that mirrors the compiler-owned `prism_code` surface

---

## 1. Summary

Phase 7 established `prism_code` as the canonical public programmable surface.

That public cutover is not sufficient.

The current implementation still carries mutation-era internals and a partial staged-builder
layer. That is not the target architecture.

Phase 7b is complete only when PRISM has one real compiler/runtime path for every currently
supported `prism_code` capability and no compatibility seam remains to the old mutation API.

This phase is not about adding a few more helper methods.

It is about replacing the mutation-era programmable core with:

- one authored-program compiler or lowering pipeline
- one runtime integration path that `prism_code` uses
- one SDK that exposes the same API that compiler/runtime path exposes
- one fixture corpus that proves the compiler covers the full currently supported surface

## 2. Strict goal

Build the full `prism_code` compiler now for everything PRISM supports today.

That means:

- read-only `prism_code`
- direct coordination authoring
- direct plan and task lifecycle updates
- dependency wiring through lexical bindings and object handles
- claim lifecycle operations
- artifact and review lifecycle operations
- `dryRun`
- deterministic host inputs that are already part of the approved `prism_code` model

This does not mean prebuilding future semantics that do not exist yet.

It does mean that every currently supported PRISM-native read or write capability must compile and
lower through the same compiler/runtime implementation now.

## 3. Hard cutover rule

This phase is a hard cutover.

No compatibility seam is allowed.

Specifically:

- no public `prism.mutate(...)`
- no public `prism_query`
- no public `prism_mutate`
- no internal bridge path where `prism_code` writes are translated through the old mutation API
- no legacy executor or legacy mutation adapter on the product path
- no temporary dual stack where the compiler exists but normal writes still bypass it

The only acceptable state at phase completion is:

- `prism_code` enters the compiler/runtime path
- the compiler/runtime path performs reads, lowering, and writes
- the old mutation API is gone from product code paths

Historical mentions may remain only in superseded docs.

## 4. Required outcomes

Phase 7b is complete only when all of the following are true:

- `prism_code` uses one real compiler/runtime path for every currently supported read and write
  capability
- one `prism_code` invocation remains one transaction boundary in v1
- public authoring uses lexical bindings and object handles rather than client ids or mutation
  payload stitching
- the compiler/runtime path is the same whether the code comes from inline `prism_code` or a repo
  file or fixture
- the SDK surface is the same API that the compiler/runtime exposes
- source-level errors remain source-mapped and must not leak mutation-era action language or
  lowering ids
- `dryRun` uses the same compiler/runtime path and skips only the final commit

## 5. Acceptance criteria

The acceptance criteria for this phase are strict.

Phase 7b is not done until every item below is true.

### AC1. Comprehensive fixture corpus exists and compiles cleanly

A comprehensive fixture set must exist for the full currently supported `prism_code` surface.

That fixture corpus must cover at least:

- read-only programmable queries
- declared work bootstrap
- plan creation, reopen, update, and archive
- task creation, reopen, update, completion, handoff, accept-handoff, resume, and reclaim
- dependency wiring between created and existing tasks
- claim acquire, renew, and release
- artifact propose, review, and supersede
- mixed read-plus-write programs within one invocation
- `dryRun`
- deterministic host input usage that is allowed today

Those fixtures must compile cleanly through the new compiler.

### AC2. `prism_code` uses the compiler through the runtime

The runtime path for `prism_code` must enter the new compiler/runtime implementation directly.

It is not acceptable for:

- `prism_code` writes to go through `prism.mutate(...)`
- `prism_code` writes to be lowered into legacy action payloads through a bridge layer
- direct write helpers to bypass the compiler/runtime path

### AC3. Any traces of the old mutation API are gone

The old mutation API must be gone from the product path.

That includes:

- runtime prelude surface
- MCP tool surface
- query-runtime host bindings
- builder or compiler internals
- docs, schemas, examples, and guidance
- tests that still teach the old mutation model as a valid path

The remaining allowed mentions are only historical or superseded documentation.

### AC4. We have a proper SDK

The SDK must be a real first-class surface, not an ad hoc collection of hand-written wrappers.

It must:

- expose the same API that `prism_code` exposes
- be owned by the same registry or method-definition family as the runtime compiler surface
- be easy to extend because new compiler surface area and SDK surface area are the same thing
- keep docs, examples, and API reference generation aligned with the same source of truth

Inline `prism_code`, repo-authored code, fixtures, and SDK docs must all describe the same API.

## 6. Non-goals

This phase does not require:

- future Actions semantics that are not implemented yet
- future graph-wide dataflow semantics that are not implemented yet
- future rich reusable plan composition features beyond today’s supported surface
- introducing a compatibility layer to make migration easier

This phase explicitly rejects the compatibility-layer option.

## 7. Implementation target

The implementation should converge on these shapes:

### 7.1 One compiler/runtime core

There must be one authored-program compiler or lowering pipeline that:

- evaluates or captures `prism_code`
- records reads and staged writes
- resolves lexical object references and handles
- lowers the authored program into PRISM-native transaction operations or PRISM Execution IR
- commits once at the end of the invocation when not in `dryRun`

### 7.2 Async authored control-flow support

The compiler/runtime must be able to support the current authored-program model rather than only a
method-by-method mutation facade.

That means the implementation should be shaped for:

- ordinary lexical bindings
- object handles
- async sequencing
- staged dependency wiring
- ordinary read-then-write programs in one invocation

The implementation does not need to fully deliver every future control-flow feature yet, but it
must no longer be architected as a thin mutation-payload adapter.

### 7.3 Unified surface registry

The SDK, runtime prelude, docs, and API reference should be driven from one shared surface
definition family.

The runtime and SDK must not drift into separate hand-maintained surfaces.

### 7.4 Fixture-first validation

The compiler surface must be validated through source fixtures rather than only through imperative
MCP tests.

The fixture corpus should become the primary proof that the compiler supports the current language
surface.

## 8. Exit validation

Minimum validation for phase completion:

- `cargo test -p prism-js`
- `cargo test -p prism-mcp server_tool_calls`
- `cargo test -p prism-mcp coordination_surface`
- `cargo test -p prism-cli`

Also add targeted validation that proves:

- the fixture corpus compiles through the compiler cleanly
- runtime `prism_code` execution uses the compiler path
- no old mutation API traces remain in product code paths

The last item should be enforced with explicit tests or codebase assertions, not left as a manual
expectation.

## 9. Exit note

Phase 7b should be judged with a simple question:

Can a new caller learn one SDK, write `prism_code`, and know that both inline execution and
repo-authored code go through the same compiler/runtime path without ever learning the old mutation
API?

If the answer is not unequivocally yes, this phase is not complete.
