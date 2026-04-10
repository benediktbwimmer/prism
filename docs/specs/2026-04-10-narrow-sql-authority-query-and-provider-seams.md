# Narrow SQL Authority Query And Provider Seams

Status: implemented
Audience: coordination, storage, runtime, query, MCP, CLI, and service maintainers
Scope: remove the remaining coarse authority provider/query shapes so hot callers open only the SQL authority responsibilities they actually need

---

## 1. Summary

The SQL authority contract is much better than before, but two important leaks remained:

- the public hot query seam still exposed `read_current_state`, which returned full snapshot plus runtime state
- the provider still exposed a coarse `open(...)` entrypoint that handed callers one broad authority object

Those two shapes still encouraged generic “open the store, then ask for whatever” usage.

This spec finishes that cleanup so:

- the provider opens explicit responsibility-scoped seams
- the hot query side exposes projection-shaped reads instead of full current-state reads
- full snapshot-plus-runtime assembly happens only in explicit helper code that opts into snapshot and runtime composition

## 2. Goals

Progress note (2026-04-10):

- implementation landed by removing the public `read_current_state` hot read, adding explicit
  provider openings for projection, mutation, runtime, event-execution, history, diagnostics, and
  snapshot seams, and migrating hot callers onto those narrower surfaces

### 2.1 Remove the public hot full-current-state read

The public SQL authority seam should not expose `read_current_state`.

That read is too broad for normal hot traffic and invites the wrong access pattern.

Hot callers should instead use narrower reads such as:

- authority summary
- canonical snapshot v2 / projection-facing state
- runtime descriptor listing

### 2.2 Remove the coarse provider opening

`CoordinationAuthorityStoreProvider` should not expose one generic `open(...)` entrypoint.

Instead, it should expose explicit openings by responsibility, for example:

- projection
- mutation
- runtime
- event execution
- history
- diagnostics
- snapshot

That makes call-site intent visible and keeps future Postgres work aligned with narrower SQL seams.

### 2.3 Keep full-state assembly explicit and rare

Some flows still genuinely need snapshot-plus-runtime composition.

Those flows should assemble that state explicitly by combining:

- snapshot seam reads
- runtime seam reads

That assembly should live in helper code, not in the public hot-path authority trait.

## 3. Non-goals

This spec does not:

- implement the full Postgres backend
- design every future projection-specific query API
- remove snapshot-oriented recovery/import paths
- change coordination-domain semantics
- implement execution-substrate work

## 4. Design

### 4.1 Projection-oriented hot query seam

Replace the broad hot current-state read with a narrower projection-oriented store, for example:

- `read_summary`
- `read_canonical_snapshot_v2`

That is enough for the current hot callers:

- MCP coordination surface derivation
- authority-stamp reads
- other projection-oriented consumers

### 4.2 Explicit provider openings

Add explicit provider methods and free functions for:

- `open_projection`
- `open_mutation`
- `open_runtime`
- `open_event_execution`
- `open_history`
- `open_diagnostics`
- `open_snapshot`

Remove the coarse public `open(...)` path.

### 4.3 Narrow call-site migration

Migrate callers so they open only what they need:

- read broker uses projection reads
- runtime gateway uses runtime reads
- trust/diagnostics helpers use diagnostics or projection reads
- persistence uses mutation opens
- event engine uses event-execution and snapshot opens

### 4.4 Explicit full-state assembly helpers

Where code still genuinely needs snapshot plus runtime state, add helper assembly logic that
combines:

- snapshot store reads
- runtime store reads

Those helpers should be explicit in name and location so future query work can replace them with
projection-specific APIs deliberately rather than accidentally.

## 5. Implementation plan

1. Update the roadmap and add this spec.
2. Update doc indices.
3. Replace the hot current-state trait with a projection-oriented trait.
4. Add explicit provider openings for each authority responsibility.
5. Remove the coarse public provider `open(...)` path.
6. Update SQLite and the Postgres stub to the narrower trait/provider surface.
7. Migrate product callers.
8. Add explicit helper assembly for flows that still need full snapshot-plus-runtime state.
9. Run targeted and full validation.

## 6. Validation

Minimum validation for this seam refactor:

- `cargo test -p prism-core -p prism-mcp -p prism-cli`

Required broader validation because this changes the shared SQL authority contract:

- `cargo test`

## 7. Exit criteria

- the public hot SQL authority seam no longer exposes `read_current_state`
- the provider no longer exposes a coarse generic `open(...)` path
- hot callers use explicit responsibility-scoped openings
- hot query callers use narrower projection-oriented reads
- full current-state assembly exists only in explicit helper code
- SQLite and the Postgres stub compile and pass behind that narrower provider/query surface
