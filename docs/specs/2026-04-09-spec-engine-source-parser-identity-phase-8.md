# Spec Engine Source, Parser, and Identity Phase 8

Status: in_progress
Audience: spec-engine, query, MCP, CLI, docs, and repo-integration maintainers
Scope: complete roadmap Phase 8 by implementing configurable spec discovery, markdown-plus-frontmatter parsing, stable repo-local spec identity, checklist extraction, dependency parsing, and source metadata capture

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 8:

- make spec files real native PRISM inputs
- parse them deterministically into structured local records
- establish stable repo-local identity for specs and checklist items

This phase does not build the whole spec engine.
It builds the first layer that later phases depend on:

- source discovery
- parser/schema validation
- deterministic local identity

The result should be that repo files in the configured spec root can become structured local spec
objects without involving coordination mutation, sync, coverage, or materialized persistence yet.

## 2. Status

Current state:

- [x] the native spec engine contract exists
- [x] the broader native spec engine design doc exists
- [x] configurable spec-root resolution exists in code
- [x] markdown-plus-frontmatter spec parsing exists in code
- [ ] stable repo-unique `spec_id` validation does not yet exist in code
- [ ] checklist extraction and stable checklist identity do not yet exist in code
- [ ] dependency parsing and source metadata capture do not yet exist in code

Current slice notes:

- Phase 7 closed the coordination platform freeze, so this phase can now build on settled
  coordination seams instead of a moving migration target
- the goal here is deterministic local structure, not query or storage fanout yet
- Slice 1 now uses `.prism/spec-engine.json` as the repo-local override path with one `root`
  field and preserves `.prism/specs/` as the default when the config file is absent
- Slice 2 now parses YAML frontmatter plus markdown body, validates the minimum required fields,
  and returns structured local diagnostics instead of silently skipping malformed specs

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 8: implement spec engine source, parser, and identity model

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/shared-scope-and-identity.md](../contracts/shared-scope-and-identity.md)
- [../contracts/reference-and-binding.md](../contracts/reference-and-binding.md)

This spec refines the broader target in:

- [2026-04-08-native-spec-engine.md](2026-04-08-native-spec-engine.md)

## 5. Scope

This phase includes:

- configurable repo-local spec-root resolution
- deterministic discovery of candidate spec files
- markdown-plus-frontmatter parsing
- minimum structured-field validation
- repo-unique `spec_id` validation
- checklist extraction
- checklist requirement-level parsing
- stable checklist identity generation
- dependency parsing
- source file path and source revision metadata capture

This phase does not include:

- local spec materialized persistence
- CLI or MCP query surfaces
- coverage view computation
- sync provenance
- coordination linking or sync actions

## 6. Non-goals

This phase should not:

- make spec files authoritative coordination truth
- introduce fuzzy semantic understanding of spec prose
- build spec-to-coordination sync
- invent a second canonical source format beside markdown plus frontmatter
- start writing spec data into SQLite before the parser/identity layer is settled

## 7. Design

### 7.1 Source-root rule

Phase 8 must support a configurable spec root.

Initial behavior:

- default root: `.prism/specs/`
- v1 repo-local override config path: `.prism/spec-engine.json`
- repo-local override may point to another repo-relative path such as `docs/specs/`
- the configured root must resolve inside the repo

The discovery layer should not assume PRISM's own dogfooding layout is universal.

### 7.2 Candidate-file rule

The discovery layer should identify markdown files under the configured spec root and ignore other
files by default.

Initial candidate rules:

- include `*.md`
- ignore directories and non-markdown files
- stable repo-relative path ordering for deterministic outputs

### 7.3 Parsing rule

The parser should treat one markdown file as:

- YAML frontmatter for structured fields
- markdown body for prose, sections, and checklist items

Minimum required fields:

- `id`
- `title`
- `status`
- `created`

If parsing fails, the phase should produce structured local diagnostics rather than silently
skipping malformed specs.

### 7.4 Identity rule

Phase 8 must settle the stable local identity model:

- `spec_id` is repo-unique
- checklist items prefer explicit inline ids
- checklist items otherwise use deterministic generated ids

Preferred explicit form:

```md
- [ ] implement parser <!-- id: parser -->
```

Fallback identity should derive from:

- `spec_id`
- section path
- normalized label text
- local disambiguator

Raw positional index is not acceptable as the sole identity basis.

### 7.5 Requirement-level rule

Checklist items must support:

- `required`
- `informational`

Initial behavior:

- default to `required`
- parse explicit informational markers when present
- expose the effective level in the parsed record

This phase only parses and normalizes the level.
Later phases decide how derived status uses it.

### 7.6 Dependency rule

Phase 8 should parse explicit `depends_on` entries from frontmatter into structured dependency
records, but it does not need to compute full graph posture yet.

This phase should ensure:

- dependencies are structurally captured
- dependency references preserve input order
- invalid self-dependencies can be diagnosed early

### 7.7 Source metadata rule

Every parsed spec should carry source metadata sufficient for later materialization and sync
provenance.

At minimum:

- repo-relative path
- source file digest or equivalent deterministic content marker
- source git revision when cheaply available

If source revision lookup is not yet available everywhere, Phase 8 may leave it nullable while
still preserving the field in the object model.

## 8. Implementation slices

### Slice 1: Spec root configuration and discovery

- add one repo-local spec-root configuration path
- resolve it safely relative to the repo root
- discover candidate markdown files deterministically

Exit criteria:

- PRISM can enumerate candidate spec files in one configured root deterministically

Slice 1 landed with:

- `.prism/spec-engine.json` as the repo-local override file
- one `root` field in that JSON file
- recursive deterministic discovery of `*.md` files under the resolved root
- rejection of absolute or repo-escaping overrides

### Slice 2: Frontmatter parser and minimum schema validation

- parse markdown files into frontmatter plus body
- validate minimum required fields
- surface structured parse diagnostics for malformed specs

Exit criteria:

- well-formed spec files produce structured parsed records
- malformed files produce deterministic diagnostics

Slice 2 landed with:

- one parsed document shape carrying frontmatter, body, and the required normalized fields
- declared-status validation against the shared minimum status vocabulary
- structured diagnostics for missing frontmatter, malformed YAML, missing required fields, and
  invalid status values

### Slice 3: Checklist extraction and identity

- extract markdown checkbox items
- attach section context
- parse explicit checklist ids when present
- generate deterministic fallback ids otherwise
- parse requirement level markers

Exit criteria:

- parsed checklist items have stable local identity beyond raw ordinal position

### Slice 4: Dependency and source metadata capture

- parse `depends_on`
- capture source path and digest metadata
- capture source revision when available

Exit criteria:

- parsed spec records contain the inputs later phases need for materialization and sync provenance

## 9. Validation

Minimum validation for this phase:

- targeted tests in the crate that owns spec parsing/discovery
- downstream tests for immediate consumers only if the parsed types are exposed across crate
  boundaries
- `git diff --check`

Important regression checks for this phase:

- identical source trees produce deterministic parsed outputs
- malformed specs fail deterministically
- checklist ids remain stable across reorderings when explicit ids are present
- generated checklist ids remain stable for unchanged section context and label text
- spec-root overrides cannot escape the repo root

## 10. Completion criteria

Phase 8 is complete only when:

- PRISM can discover candidate spec files from a configured root deterministically
- valid spec markdown files parse into structured local records
- invalid spec files surface structured diagnostics
- `spec_id`, checklist identity, dependencies, and source metadata are available in the parsed
  object model

## 11. Implementation checklist

- [x] Add configurable spec-root resolution
- [x] Add deterministic spec discovery
- [x] Add markdown/frontmatter parser and schema validation
- [ ] Add stable `spec_id` validation
- [ ] Add checklist extraction and stable checklist identity
- [ ] Add checklist requirement-level parsing
- [ ] Add dependency parsing
- [ ] Add source path and revision metadata capture
- [ ] Validate changed crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land

## 12. Current implementation status

This phase should leave PRISM with one reliable answer to:

- what spec files exist here?
- what does each one structurally say?
- what stable ids do its checklist items and dependencies have?

Later phases can then build materialization, querying, coverage, and sync on top of that stable
parser layer instead of mixing parsing and product integration together.
