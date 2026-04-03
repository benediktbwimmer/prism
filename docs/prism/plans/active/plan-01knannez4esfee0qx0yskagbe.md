# Implement external bootstrap attestation auth

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:ee0af6267131c893a452b02dc9303ee00df00aea4878e41012b34a43596f1f97`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 12 edges, 0 overlays`

## Overview

- Plan id: `plan:01knannez4esfee0qx0yskagbe`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `12`

## Goal

Replace weak local-empty-registry bootstrap with externally attested human-root bootstrap, portable human principals, delegated server enrollment, assurance-labeled provenance, and recovery that cannot be bypassed by wiping local runtime state.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01knannez4esfee0qx0yskagbe.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01knannsnntbnbg3pm51nacctc`

## Nodes

### Define bootstrap authority, issuer trust, and assurance model

- Node id: `coord-task:01knannsnntbnbg3pm51nacctc`
- Kind: `decide`
- Status: `ready`
- Summary: Lock the implementation contract for external bootstrap attestations, trusted issuers, assurance levels, consumed-attestation tracking, and the exact boundary between bootstrap, recovery, and normal principal minting.
- Priority: `100`

#### Acceptance

- Bootstrap authority, recovery, issuer trust, and assurance semantics are explicit enough to implement without re-opening the trust model. [any]

### Implement bootstrap authority state and recovery gating

- Node id: `coord-task:01knanny5sqedpzn8jzfk9mzpz`
- Kind: `edit`
- Status: `ready`
- Summary: Add durable bootstrap authority metadata separate from the ordinary principal registry so deleting runtime state cannot trigger fresh bootstrap, and route missing-registry cases into explicit recovery instead.
- Priority: `98`

#### Acceptance

- Fresh bootstrap is blocked once bootstrap authority exists, even if the ordinary principal registry is missing or empty. [any]
- Recovery is a distinct path from bootstrap and is keyed off bootstrap authority state rather than registry emptiness. [any]

### Implement attestation verification with GitHub high assurance and SSH moderate assurance

- Node id: `coord-task:01knanp4706nc8ty9z1qzfjy1s`
- Kind: `edit`
- Status: `ready`
- Summary: Add issuer abstraction, attestation verification, explicit assurance levels, GitHub-backed device-flow bootstrap as the preferred high-assurance path, and SSH/GPG signing as a clearly labeled moderate-assurance fallback.
- Priority: `97`

#### Acceptance

- GitHub-backed bootstrap attestations can mint a human root with high assurance provenance. [any]
- SSH/GPG-backed bootstrap attestations remain supported as moderate assurance and are surfaced honestly in provenance. [any]

### Make human principals portable and server enrollment delegation-only

- Node id: `coord-task:01knanp9q855k55awdbrckvr1j`
- Kind: `edit`
- Status: `ready`
- Summary: Keep one durable human principal identity across machines, issue machine-local credentials to act as that principal, and forbid headless/server root bootstrap in favor of explicit imported delegation from an interactive trusted machine.
- Priority: `96`

#### Acceptance

- A second trusted machine can acquire a machine-local credential for the same human principal rather than minting a second human root. [any]
- Headless or server runtimes cannot bootstrap human roots and must import delegated identities or credentials instead. [any]

### Wire assurance provenance, inspection surfaces, and legacy migration

- Node id: `coord-task:01knanpf1an4c1r36a2bpep0de`
- Kind: `edit`
- Status: `ready`
- Summary: Persist bootstrap assurance on roots and lineage, surface it in CLI/MCP auth inspection, and add a clear migration path that records legacy roots honestly instead of silently upgrading them.
- Priority: `94`

#### Acceptance

- Root bootstrap provenance exposes high, moderate, or legacy assurance in auth inspection and audit surfaces. [any]
- Legacy roots migrate with explicit assurance labeling rather than being treated as externally attested. [any]

### Validate and dogfood bootstrap, recovery, and delegated enrollment flows

- Node id: `coord-task:01knanpmrbr3ap0fac36qhwtgf`
- Kind: `validate`
- Status: `ready`
- Summary: Exercise interactive-machine bootstrap, second-machine credential acquisition, server delegation import, recovery after registry loss, and negative cases where fresh bootstrap should be refused.
- Priority: `95`

#### Acceptance

- Bootstrap, recovery, multi-machine credential acquisition, and server enrollment each have passing end-to-end validation paths. [any]
- Registry deletion or loss no longer enables silent re-bootstrap in dogfooding scenarios. [any]

## Edges

- `plan-edge:coord-task:01knanny5sqedpzn8jzfk9mzpz:depends-on:coord-task:01knannsnntbnbg3pm51nacctc`: `coord-task:01knanny5sqedpzn8jzfk9mzpz` depends on `coord-task:01knannsnntbnbg3pm51nacctc`
- `plan-edge:coord-task:01knanp4706nc8ty9z1qzfjy1s:depends-on:coord-task:01knannsnntbnbg3pm51nacctc`: `coord-task:01knanp4706nc8ty9z1qzfjy1s` depends on `coord-task:01knannsnntbnbg3pm51nacctc`
- `plan-edge:coord-task:01knanp9q855k55awdbrckvr1j:depends-on:coord-task:01knannsnntbnbg3pm51nacctc`: `coord-task:01knanp9q855k55awdbrckvr1j` depends on `coord-task:01knannsnntbnbg3pm51nacctc`
- `plan-edge:coord-task:01knanp9q855k55awdbrckvr1j:depends-on:coord-task:01knanny5sqedpzn8jzfk9mzpz`: `coord-task:01knanp9q855k55awdbrckvr1j` depends on `coord-task:01knanny5sqedpzn8jzfk9mzpz`
- `plan-edge:coord-task:01knanp9q855k55awdbrckvr1j:depends-on:coord-task:01knanp4706nc8ty9z1qzfjy1s`: `coord-task:01knanp9q855k55awdbrckvr1j` depends on `coord-task:01knanp4706nc8ty9z1qzfjy1s`
- `plan-edge:coord-task:01knanpf1an4c1r36a2bpep0de:depends-on:coord-task:01knanny5sqedpzn8jzfk9mzpz`: `coord-task:01knanpf1an4c1r36a2bpep0de` depends on `coord-task:01knanny5sqedpzn8jzfk9mzpz`
- `plan-edge:coord-task:01knanpf1an4c1r36a2bpep0de:depends-on:coord-task:01knanp4706nc8ty9z1qzfjy1s`: `coord-task:01knanpf1an4c1r36a2bpep0de` depends on `coord-task:01knanp4706nc8ty9z1qzfjy1s`
- `plan-edge:coord-task:01knanpf1an4c1r36a2bpep0de:depends-on:coord-task:01knanp9q855k55awdbrckvr1j`: `coord-task:01knanpf1an4c1r36a2bpep0de` depends on `coord-task:01knanp9q855k55awdbrckvr1j`
- `plan-edge:coord-task:01knanpmrbr3ap0fac36qhwtgf:depends-on:coord-task:01knanny5sqedpzn8jzfk9mzpz`: `coord-task:01knanpmrbr3ap0fac36qhwtgf` depends on `coord-task:01knanny5sqedpzn8jzfk9mzpz`
- `plan-edge:coord-task:01knanpmrbr3ap0fac36qhwtgf:depends-on:coord-task:01knanp4706nc8ty9z1qzfjy1s`: `coord-task:01knanpmrbr3ap0fac36qhwtgf` depends on `coord-task:01knanp4706nc8ty9z1qzfjy1s`
- `plan-edge:coord-task:01knanpmrbr3ap0fac36qhwtgf:depends-on:coord-task:01knanp9q855k55awdbrckvr1j`: `coord-task:01knanpmrbr3ap0fac36qhwtgf` depends on `coord-task:01knanp9q855k55awdbrckvr1j`
- `plan-edge:coord-task:01knanpmrbr3ap0fac36qhwtgf:depends-on:coord-task:01knanpf1an4c1r36a2bpep0de`: `coord-task:01knanpmrbr3ap0fac36qhwtgf` depends on `coord-task:01knanpf1an4c1r36a2bpep0de`

