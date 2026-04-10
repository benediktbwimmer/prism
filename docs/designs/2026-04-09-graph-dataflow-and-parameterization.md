# PRISM Graph Dataflow And Parameterization

Status: proposed design
Audience: coordination, service, runtime, query, MCP, CLI, UI, and plan maintainers
Scope: typed inputs, outputs, bindings, reusable definitions, and carried state across plans, tasks, actions, reviews, and related coordination nodes

---

## 1. Summary

PRISM should evolve from a static coordination DAG toward a typed plan graph with explicit
dataflow.

This means adding graph-wide support for:

- typed inputs
- typed outputs
- explicit bindings between nodes
- plan-level parameterization
- reusable repo-authored plan definitions under `.prism/code/plans/`
- continue-as-new with carried state

This is not only about parameterizing Plans. It is about letting multiple coordination objects consume
and produce typed data.

The target scope includes at least:

- plans
- tasks
- actions
- reviews
- artifacts and evidence refs where relevant

## 2. Why this layer matters

Without explicit graph dataflow, PRISM plans remain powerful but stiff.

Downstream work then depends mostly on:

- implicit shared state
- ad hoc query lookups
- conventions outside the graph

Typed inputs, outputs, and bindings make the graph itself much more meaningful.

That unlocks:

- reusable plan definitions
- explicit machine-work dependencies
- better explainability
- stronger provenance
- richer continue-as-new behavior

## 3. Core stance

### 3.1 Plans remain non-executing

Plans should remain:

- structural
- non-claimable
- non-executed themselves

This should not change.

### 3.2 Plans can still be reusable and parameterized

Plans can nevertheless become:

- parameterized
- reusable
- instantiable
- input and output carrying
- lineage-aware

That is not a contradiction. It is the right way to borrow useful workflow ideas without turning
Plans into executable actors.

### 3.3 Dataflow is graph-wide, not plan-only

The data model should not stop at Plan inputs.

Tasks, Actions, Reviews, and related nodes should also be able to:

- consume typed input
- emit typed output
- bind to upstream values

## 4. Core concepts

### 4.1 Typed inputs

Nodes should be able to declare typed inputs.

Examples:

- plan input: target environment, artifact id, release channel
- task input: spec refs, target files, expected deliverable
- action input: artifact ref, deployment target, previous action result
- review input: artifact ref, validation ref, policy summary

### 4.2 Typed outputs

Nodes should be able to produce typed outputs.

Examples:

- build action produces `artifact_ref`
- validation execution produces `validation_ref`
- deploy action produces `deployment_ref`
- review produces `verdict_ref` or normalized approval output
- task produces structured handoff or outcome outputs when policy requires it

### 4.3 Bindings

PRISM should support explicit bindings such as:

- this node input comes from a plan input
- this node input comes from another node output
- this node input comes from an artifact or review ref
- this plan output is derived from selected child outputs

Bindings should be explicit, inspectable, and queryable.

## 5. Plan definitions and instances

PRISM should distinguish conceptually between:

- a reusable plan definition
- a concrete plan instance

### 5.1 Plan definition

A plan definition may include:

- graph topology
- parameter schema
- expected node kinds
- binding rules
- policies

### 5.2 Plan instance

A plan instance may include:

- bound input values
- concrete node identities
- accumulated outputs
- execution state
- lineage to previous or future instances

This gives PRISM reusable plan structure without making Plans executable actors.

### 5.3 Explicit control constructs

Plan definitions and plan instances should eventually support explicit native control constructs in
the compiled IR rather than requiring everything to be flattened into only static nodes and edges.

Examples:

- typed conditions
- joins
- bounded fan-out or fan-in constructs
- loop-like control forms with carried state
- explicit continue-as-new boundaries

These constructs should remain:

- explicit
- typed
- inspectable
- queryable

They are one of the key prerequisites for large compiled Plans and for selective materialization of
machine-only subgraphs.

## 6. Node-level dataflow

### 6.1 Tasks

Tasks should be able to declare:

- input bindings
- expected input shape
- structured outputs when useful

Tasks remain claimable human or agent work.

### 6.2 Actions

Actions should be the most natural early consumer of graph dataflow.

They should be able to consume:

- plan parameters
- upstream outputs
- artifact refs
- review refs

And produce:

- structured outputs
- evidence refs
- diagnostics refs

### 6.3 Reviews

Reviews may also need typed inputs and outputs.

Examples:

- review input includes artifact ref and validation summary
- review output includes structured verdict and gating metadata

This keeps review transitions explicit and easier to compose into downstream bindings.

## 7. Value model

The value system should stay explicit and conservative at first.

At minimum:

- strings
- numbers
- booleans
- lists
- objects
- explicit refs to plans, tasks, actions, artifacts, reviews, specs, or validation records

PRISM should prefer explicit typed refs over hidden implicit lookup conventions.

## 8. Continue-as-new and carried state

Continue-as-new becomes much stronger once dataflow exists.

PRISM should support carrying forward selected outputs or derived state into a new plan instance.

Examples:

- recurring release plan carries previous release evidence
- recurring maintenance plan carries last successful execution outputs
- deployment wave carries forward artifact refs and environment context

This should remain explicit rather than magical.

Continue-as-new should be understood as part of the intended v1 native Plan model, not only as a
future extension.

## 9. Reusable repo-authored plan definitions

Once graph-wide dataflow exists, reusable repo-authored plan definitions become natural.

Examples:

- feature rollout template
- library release template
- deployment template
- hotfix template
- review and validation template

These should remain explicit native definitions after compilation, even when the authored source is
ordinary JS/TS under `.prism/code/plans/`.

## 10. Guardrails

PRISM should not jump straight to general-purpose plan programming.

Avoid:

- hidden mutable plan variables
- arbitrary code-driven graph mutation everywhere
- implicit control flow hidden inside templates
- opaque executable plan logic

PRISM’s strengths are currently:

- explicit graph
- explicit leaves
- explicit lifecycle
- explicit policy
- explicit provenance

Graph dataflow should preserve those strengths.

## 11. Relationship to the shared execution substrate

This dataflow layer is distinct from the shared execution substrate.

The split should be:

- shared execution substrate = how machine work runs
- graph dataflow and parameterization = how coordination objects consume and produce typed data

They are complementary but not the same design problem.

This dataflow layer is also the semantic target for JS or TS-authored Plans compiled into PRISM
native IR.

That means the compiler should target explicit native constructs such as:

- plan inputs and outputs
- task inputs and outputs
- Action inputs and outputs
- review inputs and outputs
- explicit bindings between those values

JS or TS-authored plan code is the ergonomic authoring surface.
This native typed dataflow model remains the persisted and queryable truth.

## 12. Recommendation

PRISM should add graph-wide typed inputs, outputs, and bindings across plans, tasks, actions,
reviews, and related coordination objects.

Plans should remain structural and non-executing, but they should become:

- parameterized
- reusable
- instantiable
- lineage-aware

This gives PRISM a much stronger plan architecture without sacrificing its explicit graph and
provenance model.
