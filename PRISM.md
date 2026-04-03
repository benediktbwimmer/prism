# PRISM

> This file is generated from repo-scoped PRISM knowledge. The concise summary lives here,
> while the full generated catalog lives under `docs/prism/`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:331e68edf71fbdb6a06a7b24c390ab85dd4e102d09a20086ac199072dd4fa6c7`
- Source logical timestamp: `1775113302`
- Source snapshot: `95` concepts, `206` relations, `8` contracts

## Overview

- Active repo concepts: 95
- Active repo relations: 206
- Active repo contracts: 8
- Active repo memories: 27
- Published plans: 56
- Published patch events: 194
- Full concept catalog: `docs/prism/concepts.md`
- Full relation catalog: `docs/prism/relations.md`
- Full contract catalog: `docs/prism/contracts.md`
- Published memory catalog: `docs/prism/memory.md`
- Published change summary: `docs/prism/changes.md`
- Published plan catalog: `docs/prism/plans/index.md`

## How to Read This Repo

- Start with this file for the main architecture map and the most central repo concepts.
- Use `docs/prism/concepts.md` when you need the full generated concept encyclopedia.
- Use `docs/prism/relations.md` when you need the typed concept-to-concept graph.
- Use `docs/prism/contracts.md` when you need published guarantees, assumptions, validations, and compatibility guidance.
- Use `docs/prism/memory.md` when you need the current repo-published memory surface.
- Use `docs/prism/changes.md` when you need the summarized repo-published patch history.
- Use `docs/prism/plans/index.md` when you need the current published plan catalog and per-plan markdown projections.
- Treat `.prism/concepts/events.jsonl`, `.prism/concepts/relations.jsonl`, `.prism/contracts/events.jsonl`, `.prism/memory/events.jsonl`, `.prism/changes/events.jsonl`, and `.prism/plans/**/*` as the source of truth; these markdown files are derived artifacts.

## Architecture

- `PRISM architecture` (`concept://prism_architecture`): Top-level monorepo architecture spanning semantic IR, language parsing, workspace indexing, persistence/history, memory/projections/query, coordination/curation, and MCP-facing product surfaces.

## Subsystem Map

- `validation and dogfooding loop` (`concept://validation_and_dogfooding`): Cross-cutting validation pipeline for structural truth, lineage, memory anchoring, projections, coordination, and MCP/query behavior, plus direct dogfooding feedback capture.
- `projection and query layer` (`concept://projection_and_query_layer`): Derived indexes and read APIs that turn graph, history, memory, and coordination state into semantic discovery, impact, and programmable query results.
- `coordination and plan runtime` (`concept://coordination_and_plan_runtime`): Shared plan, task, claim, artifact, and blocker subsystem that models active intent and exposes runtime coordination views across PRISM.
- `structural IR and identity model` (`concept://structural_ir`): Authoritative semantic schema for nodes, edges, anchors, events, and stable identities that every higher PRISM layer depends on.
- `concept and publication pipeline` (`concept://concept_and_publication_pipeline`): Event-sourced concept lifecycle that promotes repo-quality concepts and concept relations from core events into projected retrieval packets and MCP-facing mutation surfaces.
- `memory system` (`concept://memory_system`): Layered session, episodic, structural, outcome, and semantic memory subsystem with recall and re-anchoring support across PRISM runtime workflows.
- `MCP runtime surface` (`concept://mcp_runtime_surface`): MCP- and daemon-facing runtime surface that bridges PRISM query and mutation capabilities into resources, schemas, compact tools, and long-lived server state.
- `persistence and history layer` (`concept://persistence_and_history`): SQLite-backed graph and temporal persistence layer that stores structural state, history snapshots, memory projections, and other durable PRISM artifacts.
- `language adapter family` (`concept://language_adapter_family`): Language- and format-specific adapters that translate Rust, Python, Markdown, JSON, TOML, and YAML inputs into the shared parser and IR pipeline.
- `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`): prism-core orchestration that watches the workspace, runs parse and resolution pipelines, reanchors history and memory, and maintains the live PRISM session state.
- `dashboard surface` (`concept://dashboard_surface`): First-class observability and UI surface inside prism-mcp that publishes dashboard read models, event streams, and router endpoints instead of reusing the raw MCP tool catalog.
- `compact tool surface` (`concept://compact_tool_surface`): Staged locate/open/gather/workset/expand/concept/task-brief tool layer inside prism-mcp that serves as the intended default agent path above raw prism_query.
- `CLI surface` (`concept://cli_surface`): Local operator surface for indexing, querying, and MCP lifecycle control, built as a thin binary over prism-core and related crates.
- `agent inference and curation` (`concept://agent_inference_and_curation`): Inference and curator stack that records inferred edges, synthesizes curator output, and promotes reviewed knowledge into persistent PRISM state.

## Key Concepts

- `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`): prism-mcp modules that decode `prism_mutate` actions, host authenticated state-changing mutations, and route agent-side writes into durable PRISM state without relying on `prism_session` authority.
- `coordination state model` (`concept://coordination_state_model`): Core coordination modules that define state, types, runtime overlays, and compatibility projections for plans, tasks, claims, and artifacts.
- `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`): Shared prism-parser contract that defines parse inputs/results, language adapter behavior, intent extraction, document naming, and stable fingerprint helpers reused by every adapter crate.
- `semantic projection indexes` (`concept://semantic_projection_indexes`): prism-projections modules that materialize derived concept, relation, intent, and projection indexes from lower-level events and snapshots.
- `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`): compact-tool modules that decode concept packets, expand bounded diagnostics, and summarize coordination tasks without dropping to raw query output.
- `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`): Memory modules that derive structural features, manage structural recall, and integrate semantic matching backends for deeper retrieval beyond simple text scoring.
- `workspace session runtime` (`concept://workspace_session_runtime`): Session-oriented prism-core modules that hydrate WorkspaceSession state, refresh memory snapshots, and record patch outcomes around the live workspace.
- `code language adapters` (`concept://code_language_adapters`): Rust and Python adapter modules that parse executable source code into PRISM IR using parser, syntax, and path-resolution helpers.
- `coordination operations and policy` (`concept://coordination_operations_and_policy`): Coordination modules that implement query paths, mutations, blocker calculation, and helper logic over the shared coordination state model.
- `core indexing pipeline` (`concept://core_indexing_pipeline`): prism-core modules that parse files, resolve structure, reanchor temporal identity, and watch workspace changes to build the live PRISM graph.
- `curator execution flow` (`concept://curator_execution_flow`): prism-core and prism-curator modules that run curator jobs, prepare support context, and transform reviewable proposals into publishable outcomes.
- `document and config adapters` (`concept://document_and_config_adapters`): Markdown, JSON, TOML, and YAML adapter modules that map documents and configuration files into PRISM IR and intent surfaces.

## Generated Docs

- `docs/prism/concepts.md`: full concept catalog with members, evidence, and risk hints.
- `docs/prism/relations.md`: full typed relation catalog with evidence and confidence.
- `docs/prism/contracts.md`: full contract catalog with guarantees, assumptions, validations, and compatibility guidance.
- `docs/prism/memory.md`: current repo-published memory entries with anchors, provenance, and trust.
- `docs/prism/changes.md`: summarized repo-published patch events and the files they touched.
- `docs/prism/plans/index.md`: published plan catalog plus per-plan markdown projections under `docs/prism/plans/`.
