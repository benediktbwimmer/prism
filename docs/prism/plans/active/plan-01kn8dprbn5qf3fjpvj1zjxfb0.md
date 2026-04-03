# Reduce full cargo test latency and improve test reliability by profiling the slowest test paths, separating heavy integration-style coverage from cheap unit-style coverage, removing avoidable setup and timing overhead, and validating the new default suite behavior with measured before/after timings.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:8ba1284a125f992d654c11383c313640e99507532033c666e478ab78ae069e34`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 5 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn8dprbn5qf3fjpvj1zjxfb0`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `5`

## Goal

Reduce full cargo test latency and improve test reliability by profiling the slowest test paths, separating heavy integration-style coverage from cheap unit-style coverage, removing avoidable setup and timing overhead, and validating the new default suite behavior with measured before/after timings.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn8dprbn5qf3fjpvj1zjxfb0.jsonl`

## Root Nodes

- `coord-task:01kn8dq98qhhd6m31kdcavrhvr`

## Nodes

### Profile the full cargo test suite and identify the slowest crates and test cases.

- Node id: `coord-task:01kn8dq98qhhd6m31kdcavrhvr`
- Kind: `edit`
- Status: `in_progress`

#### Acceptance

- A short hotspot taxonomy exists for setup-heavy, IO-heavy, and timing-sensitive tests. [any]
- The slowest crates and individual tests in the default cargo test path are identified with measured timings. [any]

### Audit repeated fixture, indexing, SQLite, and process-startup patterns in the slowest tests.

- Node id: `coord-task:01kn8dqgf7v523n7bbrp0p3bxm`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- The main repeated setup patterns behind the slowest tests are mapped to owning helpers or modules. [any]
- The repo has a prioritized list of the most leverageful test-speed interventions. [any]

### Implement first-pass test-speed wins in the highest-leverage fixtures and harness helpers.

- Node id: `coord-task:01kn8dqwqxreh653af5apa1yv0`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- The changes preserve existing test intent and coverage semantics. [any]
- The first wave of harness or fixture changes measurably reduces the default suite cost in the targeted hotspots. [any]

### Improve reliability by removing brittle timing waits and separating truly heavy end-to-end cases from the default suite.

- Node id: `coord-task:01kn8dqyvrpbxe16xxwwfd29dy`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- The targeted flaky or timing-sensitive tests use deterministic synchronization where feasible. [any]
- There is a justified split between default-path tests and explicitly heavy integration coverage. [any]

### Validate the new default test path with before/after timings and record the updated repo guidance.

- Node id: `coord-task:01kn8dr8dw1b6t5726j9sy76xe`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Before/after timings exist for the default suite and the targeted hotspots. [any]
- The repo guidance reflects the intended split between fast default coverage and explicit heavier coverage. [any]

## Edges

- `plan-edge:coord-task:01kn8dqgf7v523n7bbrp0p3bxm:depends-on:coord-task:01kn8dq98qhhd6m31kdcavrhvr`: `coord-task:01kn8dqgf7v523n7bbrp0p3bxm` depends on `coord-task:01kn8dq98qhhd6m31kdcavrhvr`
- `plan-edge:coord-task:01kn8dqwqxreh653af5apa1yv0:depends-on:coord-task:01kn8dqgf7v523n7bbrp0p3bxm`: `coord-task:01kn8dqwqxreh653af5apa1yv0` depends on `coord-task:01kn8dqgf7v523n7bbrp0p3bxm`
- `plan-edge:coord-task:01kn8dqyvrpbxe16xxwwfd29dy:depends-on:coord-task:01kn8dqgf7v523n7bbrp0p3bxm`: `coord-task:01kn8dqyvrpbxe16xxwwfd29dy` depends on `coord-task:01kn8dqgf7v523n7bbrp0p3bxm`
- `plan-edge:coord-task:01kn8dr8dw1b6t5726j9sy76xe:depends-on:coord-task:01kn8dqwqxreh653af5apa1yv0`: `coord-task:01kn8dr8dw1b6t5726j9sy76xe` depends on `coord-task:01kn8dqwqxreh653af5apa1yv0`
- `plan-edge:coord-task:01kn8dr8dw1b6t5726j9sy76xe:depends-on:coord-task:01kn8dqyvrpbxe16xxwwfd29dy`: `coord-task:01kn8dr8dw1b6t5726j9sy76xe` depends on `coord-task:01kn8dqyvrpbxe16xxwwfd29dy`

