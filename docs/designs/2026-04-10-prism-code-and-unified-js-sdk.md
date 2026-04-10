# PRISM Code And The Unified JS/TS SDK

Status: proposed design  
Audience: runtime, service, MCP, CLI, coordination, compiler, query, and plan-authoring maintainers  
Scope: the canonical `prism_code` surface, the unified JS/TS SDK, deterministic input capabilities, and the repo-authored `.prism/code/*` source model

---

## 1. Summary

PRISM should hard-cut to one canonical programmable surface:

- `prism_code`

`prism_code` is the single JS/TS execution surface for:

- reads
- authoritative writes
- plan authoring
- Action code
- validation code
- event-hook or event-job code

The key architectural split is:

- authored code is JS/TS
- persisted and executed runtime truth remains native PRISM IR and native authority state

This means:

- repo-authored code and inline `prism_code` use the same SDK family
- one `prism_code` call is one transaction in v1
- the runtime evaluates code in a controlled host environment
- that environment lowers reads and writes into explicit native operations
- reusable repo-authored plans are just one important subfamily of reusable PRISM code, not a
  separate programming model

## 2. Why PRISM should do this now

Separating `prism_query` and `prism_mutate` made early implementation easier, but it is now the
wrong public boundary.

PRISM wants:

- one learnable SDK
- one programmable surface for agents and humans
- one compiler pipeline that later phases can extend
- one documentation and discoverability model
- one place where auth, determinism, and transaction boundaries are enforced

If PRISM waits until the end of the roadmap, later phases will build more logic on top of the old
surface split and the migration will get larger.

The better move is:

- introduce the minimum viable compiler or lowering runtime now
- cut the surface to `prism_code` now
- let later phases extend that same compiler

## 3. Core decisions

### 3.1 `prism_code` is the canonical public API

`prism_code` replaces:

- `prism_query`
- `prism_mutate`

as the canonical public programming surface.

The old split should not survive as a long-term compatibility layer.

### 3.2 One invocation is one transaction in v1

The v1 rule should be hard and simple:

- one `prism_code` invocation is one transaction boundary

That means:

- read-only calls execute and return data
- write-capable calls stage changes and commit once at the end
- if execution throws or lowering fails, nothing commits
- nested or long-lived transactions are out of scope in v1

### 3.3 Native IR remains the persisted and executed truth

PRISM should not keep hand-authored native plan IR as a user-facing source format.

But PRISM absolutely should keep native IR as:

- the persisted plan or graph truth
- the inspected and rendered plan truth
- the executed service or runtime truth
- the provenance-bearing artifact form

The authored code defines the DAG by compilation.
The DAG remains explicit native IR after compilation.

### 3.4 One compiler pipeline, multiple source origins

PRISM should have one compiler or lowering pipeline with two initial source origins:

- inline `prism_code`
- repo-authored `.prism/code/**/*.ts`

The same core should later support:

- reusable plans
- Action and validation code
- shared libraries
- interactive ad hoc mutations

## 4. Repo-authored source model

Repo-authored PRISM code should live under:

```text
.prism/
  code/
    plans/
      deploy-to-staging.ts
      run-full-test-matrix.ts
    actions/
      cargo-test.ts
      k8s-deploy.ts
    runners/
      docker-build.ts
      npm-publish.ts
    validators/
      release-readiness.ts
    libraries/
      release-helpers.ts
      review-helpers.ts
```

This is the repo-authored source plane.

It should be:

- reviewable
- versioned by git
- composable
- importable within the controlled PRISM code environment

It is not itself the authoritative runtime state.

### 4.1 Why `.prism/code` rather than `.prism/plans` alone

PRISM needs more than reusable plans.

It also needs reusable:

- Action implementations
- validator logic
- runner helpers
- general composable shared libraries

Using `.prism/code` as the parent family makes that explicit.

### 4.2 Plans remain a named subfamily

The user-facing concept should still be `plans`, not `workflows`.

So the plan family should be:

- `.prism/code/plans/`

That keeps the repo language aligned with the coordination model.

## 5. Execution model for `prism_code`

### 5.1 Read-only execution

Read-only `prism_code`:

- evaluates JS/TS against a pre-bound `prism` object
- may call read capabilities only
- returns structured serializable results
- does not require mutation auth

### 5.2 Write-capable execution

Write-capable `prism_code`:

- evaluates JS/TS against the same `prism` object
- stages native mutation intents during execution
- validates and lowers them into the canonical mutation protocol
- commits once at the end of the call

### 5.3 `dryRun`

`dryRun` should be supported for write-capable `prism_code`.

That mode should:

- execute normally through validation and lowering
- return predicted write set, diagnostics, and affected objects
- skip authoritative commit

## 6. Async control flow defines the DAG

PRISM should lean into a Temporal-like authoring model at the source level:

- normal async JS/TS control flow defines the DAG
- `await` boundaries and async composition shape dependency structure
- `Promise.all(...)` expresses parallelism naturally

This applies both to:

- reusable plan authoring
- ad hoc live-plan extension through `prism_code`

But the important boundary remains:

- control flow defines the DAG at authoring time
- the compiled or lowered result is explicit native IR or explicit transaction ops

PRISM should not persist live guest code as the plan truth.

## 7. Determinism and controlled inputs

### 7.1 No ambient nondeterminism

The `prism_code` environment should reject or hide ambient nondeterminism such as:

- raw `Date.now()`
- raw `Math.random()`
- arbitrary filesystem access
- uncontrolled network access
- arbitrary shell side effects

### 7.2 Explicit controlled inputs

Instead, PRISM should provide explicit host capabilities such as:

- `prism.time.now()`
- `prism.random.uuid()`
- `prism.fs.readText(path)`
- `prism.fs.readLines(path, { start, end })`
- `prism.fs.readAround(path, { line, before, after })`
- `prism.fs.exists(path)`
- `prism.fs.glob(pattern)`
- `prism.fs.readJson(path)`
- `prism.fs.readToml(path)`
- `prism.fs.readYaml(path)`

These inputs should be:

- repo-scoped or explicitly capability-scoped
- provenance-bearing
- auditable
- policy-gated where needed

### 7.3 Provenance requirement

Every controlled dynamic input should be capturable as provenance, for example:

- input kind
- path or source
- requested slice parameters
- value digest or returned value when policy allows
- timestamp

Determinism in PRISM should therefore mean:

- no hidden inputs
- all dynamic inputs are explicit and recorded

not:

- no useful dynamic inputs at all

## 8. SDK shape

The `prism` SDK family should unify read and write capabilities.

The same SDK should be usable in:

- inline `prism_code`
- repo-authored plan code
- Action code
- validation code
- event-hook code

Different subfamilies may expose small convenience wrappers, but they should share one underlying
capability model.

Representative families:

- graph and code reads
- coordination reads
- coordination writes
- plan construction and extension
- artifact, review, and evidence operations
- runtime and diagnostics reads
- filesystem and controlled dynamic-input helpers

## 9. Lowering targets

The compiler or lowering runtime should be able to produce different result classes from the same
code model:

- pure read plans or returned values
- native mutation transactions
- native reusable plan artifacts
- native Action or validation definitions later

This is why PRISM should think of the compiler as a general PRISM code compiler, not only as a
“plan compiler.”

## 10. Auth model

The auth rule should stay simple:

- read-only `prism_code` may run unauthenticated or under a weak read context
- write-capable `prism_code` requires authenticated context
- authorization still applies per capability and operation

This preserves the important distinction between:

- exploration
- authoritative mutation

without forcing two different programming models.

## 11. Service and runtime posture

The service should not become a generic always-on guest-code interpreter for long-lived plan
truth.

But the runtime stack should own the `prism_code` compiler or evaluator as a first-class
capability.

That means:

- the compiler core lives inside the PRISM runtime stack
- CLI can front that compiler
- service may request or host controlled evaluations where appropriate
- repo-authored code and inline `prism_code` share the same core

Later reusable-plan compilation is therefore an extension of the same compiler, not a second
compiler.

## 12. Documentation and discoverability

One major advantage of this model is documentation generation.

If `prism_code` is the canonical surface, PRISM can generate:

- human-readable docs
- machine-readable API references
- examples
- recipes
- auth and capability notes

from one SDK definition instead of separately documenting:

- a query language
- a mutation action schema
- plan authoring helpers

## 13. Relationship to current roadmap items

This design changes the execution roadmap materially.

It means:

- the minimum viable compiler and `prism_code` cutover should happen before the remaining execution
  substrate phases
- later phases extend the same compiler instead of introducing compilation for the first time
- reusable repo-authored plans become one later source family of the already-existing compiler

## 14. Recommendation

PRISM should:

1. hard-cut to `prism_code` now
2. build the minimum viable compiler or lowering runtime now
3. retire `prism_query` and `prism_mutate` as target architecture
4. establish `.prism/code/{plans,actions,runners,validators,libraries}` as the repo-authored source
   model
5. let later roadmap phases extend that same compiler and SDK rather than replacing them

This gives PRISM:

- one programming model
- one compiler
- one transaction model
- one discoverability story
- explicit native IR as the durable truth underneath
