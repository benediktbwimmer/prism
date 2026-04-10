# ADR: `prism_code` As The Canonical Programmable Surface

Status: accepted  
Date: 2026-04-10  
Audience: runtime, service, MCP, CLI, query, coordination, compiler, and plan-authoring maintainers

---

## Context

PRISM currently exposes separate programmable surfaces:

- `prism_query` for read-only TypeScript snippets
- `prism_mutate` for tagged authoritative writes

That split was useful while the coordination and authority layers were being stabilized, but it is
no longer the right long-term public model.

The split has several problems:

- agents and humans must learn two overlapping APIs instead of one SDK
- `prism_mutate` has grown into a large tagged-action schema that is harder to discover and extend
  than ordinary code
- repo-authored plan sources, interactive code, and future Action or validation code risk
  diverging into separate mental models
- later roadmap phases would keep accumulating surface area on the wrong public boundary

At the same time, PRISM still requires:

- native IR and native coordination objects as the persisted and executed truth
- deterministic lowering from authored code into explicit native IR or transaction ops
- controlled dynamic inputs with provenance
- a thin service that does not treat arbitrary guest code as long-lived runtime truth

## Decision

PRISM hard-cuts to one canonical programmable surface:

- `prism_code`

The accepted model is:

- `prism_code` replaces both `prism_query` and `prism_mutate` as the canonical public API
- one `prism_code` invocation is one transaction boundary in v1
- read-only `prism_code` execution may run without mutation authorization
- write-capable `prism_code` execution requires authenticated context
- `dryRun` is supported as an explicit non-commit execution mode
- native PRISM IR remains the persisted and executed truth
- repo-authored code and inline `prism_code` use the same SDK family and the same compiler or
  lowering pipeline
- there are no compatibility shims in the target architecture; `prism_query` and `prism_mutate`
  are retired rather than preserved as long-term parallel surfaces

## Repo-authored source model

Repo-authored PRISM code lives under:

```text
.prism/
  code/
    plans/
    actions/
    runners/
    validators/
    libraries/
```

This directory contains authored source, not runtime authority.

Native compiled artifacts and authoritative live state remain separate from repo-authored source:

- repo-authored source is versioned in git
- compiled artifacts carry explicit compiler and input provenance
- live plan instances and coordination state live in the authority store

## Determinism rule

PRISM forbids ambient nondeterminism in `prism_code`.

Allowed dynamic inputs must be explicit host capabilities such as:

- `prism.time.*`
- `prism.random.*`
- `prism.fs.*`
- explicit PRISM config or environment reads

These inputs must be:

- capability-gated
- auditable
- captured in provenance

Ambient access such as raw `Date.now()`, raw `Math.random()`, arbitrary filesystem access, or
unbounded network access is not the target architecture.

## Consequences

Positive:

- one SDK for reads, writes, plan authoring, Action code, validation code, and event hooks
- one compiler or lowering pipeline that can grow incrementally with the roadmap
- easier API discovery and documentation generation
- less schema memorization for agents
- earlier compiler investment pays off across all later phases

Costs:

- the current `prism_query` and `prism_mutate` product shape must be replaced decisively
- instruction docs, specs, contracts, and roadmap phases must be updated together
- the runtime must own a stronger JS or TS evaluation and lowering environment earlier than
  previously planned

## Superseded assumptions

This ADR supersedes the previous public-surface assumption that:

- `prism_query` remains the long-term programmable read escape hatch
- `prism_mutate` remains the long-term programmable write surface
- the JS or TS compiler arrives only after native reusable plan semantics are otherwise complete

The updated direction is:

- compiler and SDK foundation now
- later phases extend that same compiler and SDK rather than introducing compilation for the first
  time
