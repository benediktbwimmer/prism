# Repo Init And Validation Ejection

Status: proposed design  
Audience: CLI, repo policy, validation, service, runtime, and docs maintainers  
Scope: explicit repo bootstrap and ejection commands for `.prism/` scaffolding, including validation runners, seeded capability classes, and default repo policy

---

## 1. Summary

PRISM should ship useful built-in defaults for validation and repo policy, but it should not silently create or mutate `.prism/` during runtime boot.

The intended model is:

- PRISM ships bundled validation runners and seeded capability classes in the binary
- repos may use those built-ins immediately without repo-local customization
- repos may explicitly materialize those defaults into `.prism/` for inspection, review, and editing
- this materialization happens through explicit CLI commands rather than automatic runtime behavior

The primary bootstrap command should be:

- `prism repo init`

This design is the configuration and UX companion to [2026-04-09-warm-state-validation-feedback.md](./2026-04-09-warm-state-validation-feedback.md).

## 2. Goals

Required goals:

- make repo bootstrap explicit and reviewable
- let repos start with useful defaults immediately
- allow built-in validation runners and capability classes to be ejected into `.prism/`
- avoid surprising automatic repo mutation during runtime boot
- keep repo policy and validation configuration inspectable and versionable
- support both zero-config use and later customization

Required non-goals:

- runtime boot must not silently create `.prism/`
- repo initialization must not silently publish or promote anything to authority
- built-ins must not require ejection before first use
- repo initialization should not force one docs or spec layout beyond what policy needs

## 3. Built-In Versus Repo-Local Model

### 3.1 Built-in defaults

The PRISM binary should include:

- bundled validation runners
- seeded capability classes
- default repo policy template or templates

These defaults should be usable immediately, even in a repo with no `.prism/` customization yet.

### 3.2 Repo-local customization

A repo may later materialize those built-ins into `.prism/` so they can be:

- inspected
- customized
- committed
- reviewed
- extended

Repo-local copies should override or extend built-ins through normal repo policy rules.

### 3.3 Effective configuration transparency

PRISM should be able to explain whether a given configuration element is:

- built-in
- repo-customized
- inherited from a default template
- locally overridden

That provenance should become visible in CLI and UI diagnostics.

## 4. Why `prism repo init` Should Be Explicit

Runtime boot should not automatically create `.prism/` because that would create:

- surprising repo changes
- unclear authorship
- hidden policy bootstrapping
- inconsistent initialization across runtimes
- a weaker trust story

Explicit initialization is more aligned with PRISM’s broader posture:

- explicit config
- explicit promotion
- explicit sync
- explicit policy

Runtimes may detect missing repo-local config and emit a helpful diagnostic, but only an explicit command should create repo-local PRISM scaffolding.

## 5. Main Command: `prism repo init`

The main bootstrap command should be:

- `prism repo init`

### 5.1 Intended behavior

This command should:

- create `.prism/` if absent
- write a standard repo policy template
- optionally materialize bundled validation runners
- optionally materialize seeded capability classes
- optionally write other useful repo-local PRISM artifacts that belong in `.prism/`

### 5.2 Sensible defaults

The default posture should be explicit but ergonomic:

- create `.prism/`
- write default repo policy
- write validation capability vocabulary
- write bundled validation runners

The command is explicit, so richer default scaffolding is acceptable there even though runtime boot must remain non-invasive.

### 5.3 Non-destructive behavior

`prism repo init` should:

- not overwrite existing files unless explicitly asked
- show what it created
- be safe to rerun
- preserve local customization by default

## 6. Suggested `.prism/` Structure

An initial layout could look like:

```text
.prism/
  policy/
    repo-policy.yaml
    validation-capabilities.yaml
  validation/
    runners/
      cargo_test.ts
      pytest.ts
      vitest.ts
      playwright.ts
```

The exact layout may evolve, but the conceptual split should remain:

- repo policy
- capability vocabulary
- validation runner adapters

## 7. Validation Capability Vocabulary Ejection

Seeded capability classes should ship in the binary, for example:

- `cargo`
- `cargo:test`
- `cargo:workspace-test`
- `pytest`
- `npm`
- `npm:test`
- `playwright`

These seeded classes should define:

- name
- optional description
- proof commands
- proof success policy
- optional TTL
- intended runner kinds

`prism repo init` should be able to write these seeded classes into repo-local policy so the repo can later:

- keep them unchanged
- customize proof commands or TTLs
- add repo-specific capability classes
- deprecate or rename classes deliberately

## 8. Validation Runner Ejection

PRISM should ship bundled JS or TS validation runners such as:

- `cargo_test`
- `pytest`
- `vitest`
- `playwright`

`prism repo init` should be able to write repo-local copies under `.prism/validation/runners/`.

That gives repos a clean progression:

- day 1: use built-ins with zero config
- day 2: run `prism repo init` and inspect real repo-local policy
- later: customize runner behavior to fit repo-specific needs

## 9. Separate Eject Commands

In addition to `prism repo init`, PRISM should eventually support narrower commands such as:

- `prism repo eject validation-runners`
- `prism repo eject validation-capabilities`
- `prism repo eject policy`

These are useful when:

- a repo already uses built-ins
- the team later decides it wants to customize only one part of the default surface

This avoids forcing every repo into a one-shot “init everything forever” model.

## 10. Runtime Behavior When `.prism/` Is Absent

If a runtime starts in a repo with no `.prism/`, it should not create files automatically.

Instead it may:

- continue using built-in defaults where possible
- emit a diagnostic that repo-local config is absent
- explain that built-in defaults are currently in use
- point users at `prism repo init` when they want materialized policy and runner files

That is helpful without being invasive.

## 11. Query, CLI, and UI Expectations

Product surfaces should eventually be able to answer:

- whether repo policy is using built-ins or repo-local overrides
- which capability classes are seeded defaults
- which validation runners are bundled defaults versus repo-local customizations
- whether `.prism/` has been initialized
- whether built-ins are in use because repo-local config is absent

This transparency matters for operator trust and for debugging why a runtime interpreted validation policy a certain way.

## 12. Validation and Safety Expectations

`prism repo init` and future eject commands should:

- validate that target files are not overwritten unexpectedly
- report what was created
- support dry-run preview where feasible
- preserve local edits by default
- fail clearly on invalid or conflicting target paths

## 13. Recommendation

PRISM should follow this posture:

- ship bundled validation runners and seeded capability classes in the binary
- allow zero-config use through those built-ins
- require an explicit `prism repo init` command to scaffold `.prism/`
- allow later targeted eject commands for narrower customization
- never auto-create `.prism/` during runtime boot

That gives PRISM the right balance of immediate usability, explicit repo ownership, inspectability, reviewability, and clean trust boundaries.
