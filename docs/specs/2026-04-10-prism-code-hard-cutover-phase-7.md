# `prism_code` Hard Cutover Phase 7

Status: partially implemented  
Audience: prism-mcp, prism-js, prism-core, CLI, and runtime maintainers  
Scope: introduce `prism_code` as the canonical programmable surface, land the production transport cutover, and drive the removal of `prism_query` / `prism_mutate` as the long-term public model

---

## 1. Goal

Phase 7 exists to replace the split public programmable model:

- `prism_query`
- `prism_mutate`

with one canonical programmable surface:

- `prism_code`

The target architecture is already defined by:

- `docs/adrs/2026-04-10-prism-code-canonical-surface.md`
- `docs/designs/2026-04-10-prism-code-and-unified-js-sdk.md`

This spec is the implementation target for making the product surface real in MCP, CLI, docs, and
the JS/TS entrypoint.

It does not by itself close the native builder or compiler authoring gap. That remaining work is
tracked separately in:

- `docs/specs/2026-04-10-native-prism-code-builder-and-compiler-phase-7b.md`

---

## 2. Required outcomes

Phase 7 is complete only when all of the following are true:

- `prism_code` is the canonical programmable MCP tool and JS/TS surface
- the public transport and docs are coherently `prism_code`-first
- the minimum read-side compiler or lowering runtime exists and supports the current read behavior
- the public programming model uses source-level bindings and handles, not surfaced client ids
- docs, instructions, schema resources, examples, and API reference all teach `prism_code`
- `prism_query` and `prism_mutate` are no longer the target public architecture

This phase does not require richer reusable-plan compilation yet. It does require that later phases
can extend the same compiler rather than introducing a second one.

---

## 3. Implementation slices

Phase 7 should land in bounded slices, not one risky cutover.

Current progress:

- Slice 1 is landed: `prism_code` is the canonical programmable read transport and the public docs/resources teach it first.
- Slice 2 is only partially landed: authenticated `prism_code` currently exposes write access through a transitional `prism.mutate(...)` bridge plus `dryRun`.
- Slice 3 is landed: the public MCP transport, schema catalog, capabilities surface, self-description examples, and bootstrap proxy cache now present a coherently `prism_code`-first product surface while keeping residual legacy lowering machinery internal-only.

### Slice 1: Canonical read-side `prism_code` transport

Deliver:

- `prism_code` MCP tool with JS/TS input contract
- canonical `prism_code` naming in docs, API reference, schema examples, and instructions
- `prism_code` error naming instead of `prism_query` naming on the new surface
- direct tests covering successful `prism_code` reads and structured parse/runtime failures

Success condition:

- callers can use `prism_code` as the canonical programmable read surface immediately
- the public docs stop teaching `prism_query` as the primary programmable entrypoint

### Slice 2: Transitional write-capable lowering through `prism_code`

Deliver:

- authenticated write-capable `prism_code`
- one-call, one-transaction semantics
- minimum lowering path from authored code into authoritative mutation effects
- dry-run support for the write-capable surface
- source-level write errors without surfaced client ids

Success condition:

- the canonical programmable write transport runs through `prism_code`, not `prism_mutate`
- any remaining bridge through `prism.mutate(...)` is explicitly documented as transitional rather
  than treated as the finished authoring model

### Slice 3: Retire split public transport assumptions

Deliver:

- remove legacy docs and resource emphasis on `prism_query` / `prism_mutate`
- remove `prism_query` / `prism_mutate` from the public MCP tool catalog
- migrate remaining public-facing recipes, schema resources, and tests to `prism_code`
- keep any residual internals clearly marked as implementation detail only

Success condition:

- the product surface is coherently `prism_code`-first rather than a mixed transport story

---

## 4. Hard rules

The implementation must preserve these architectural constraints:

- one `prism_code` invocation is one bounded execution in v1
- authored code uses lexical bindings, variables, and object handles
- client ids may exist internally during lowering, but must not surface in the public model
- the service may broker or route evaluation, but must not drift into a generic guest-code host
- PRISM Execution IR remains the persisted and executed truth
- ambient nondeterminism remains disallowed; controlled inputs must be explicit and provenance-bearing

This spec deliberately does not bless `prism.mutate(...)` as the finished write model. If callers
still need to think in mutation payloads rather than native handles and builder objects, that
remaining work belongs to Phase 7b.

---

## 5. Validation

Minimum validation during the phase:

- `cargo test -p prism-js`
- targeted `cargo test -p prism-mcp ...` coverage for:
  - tool catalog and schema resources
  - API reference resource content
  - `prism_code` success and failure transport behavior

Run the full workspace only when this phase changes shared transport or runtime behavior broadly
enough to meet the repo’s Tier 3 threshold.

---

## 6. Exit note

Phase 7 should be judged by whether later roadmap phases can build directly on:

- one canonical programmable surface
- one JS/TS SDK
- one public transport surface

If later work still has to choose between `prism_query` and `prism_mutate`, Phase 7 is not done.

If later work still has to author native coordination changes by calling `prism.mutate(...)`,
Phase 7b is not done.
