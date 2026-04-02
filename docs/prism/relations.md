# PRISM Relations

> Generated from repo-scoped PRISM concept relations.
> Return to the concise entrypoint in `../../PRISM.md`.

## Overview

- Active repo relations: 206
- Active repo concepts covered: 95

- Active repo contracts: 8

## agent inference and curation

Source Handle: `concept://agent_inference_and_curation`

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.96)
  evidence: Curator output is only durable once it flows through the concept and knowledge publication path.
- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.95)
  evidence: Inference and curation form a durable architectural slice rather than a one-off workflow.

## anchor, change, and lineage IR

Source Handle: `concept://anchor_change_and_lineage_ir`

- part of: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.95)
  evidence: Anchor, change, event, and lineage modules are the temporal-traceability sublayer of prism-ir.

## CLI command and parse surface

Source Handle: `concept://cli_command_and_parse_surface`

- part of: `CLI surface` (`concept://cli_surface`) (confidence 0.95)
  evidence: CLI argument definition, parsing, and command dispatch are the input half of the CLI surface.
- depends on: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`) (confidence 0.87)
  evidence: Many CLI commands are thin wrappers around workspace indexing/session behavior provided by prism-core.

## CLI runtime and MCP control

Source Handle: `concept://cli_runtime_and_mcp_control`

- part of: `CLI surface` (`concept://cli_surface`) (confidence 0.95)
  evidence: Runtime execution, MCP lifecycle control, and terminal display are the execution half of the CLI surface.
- depends on: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`) (confidence 0.90)
  evidence: The CLI’s MCP commands control daemon startup, restart, status, and related runtime lifecycle behavior.

## CLI surface

Source Handle: `concept://cli_surface`

- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.92)
  evidence: The CLI is a thin but real operator-facing architectural surface in the repo.
- depends on: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`) (confidence 0.92)
  evidence: The CLI delegates indexing and workspace session behavior into prism-core rather than implementing its own runtime.

## co-change and validation projection indexes

Source Handle: `concept://cochange_and_validation_projection_indexes`

- depends on: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.90)
  evidence: These projection indexes are derived from history snapshots and persisted outcome/projection state.
- part of: `semantic projection indexes` (`concept://semantic_projection_indexes`) (confidence 0.96)
  evidence: Co-change and validation indexes are the derived-index half of the semantic projection subsystem.

## code language adapters

Source Handle: `concept://code_language_adapters`

- part of: `language adapter family` (`concept://language_adapter_family`) (confidence 0.96)
  evidence: Rust and Python parser/syntax/path modules are the executable-code subfamily inside the broader adapter stack.
- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`) (confidence 0.96)
  evidence: Rust and Python adapters implement the shared prism-parser LanguageAdapter contract and fingerprint helpers.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.97)
  evidence: The code adapters emit shared PRISM nodes, edges, anchors, and references through the common IR contract.

## compact discovery and opening flow

Source Handle: `concept://compact_discovery_and_opening`

- part of: `compact tool surface` (`concept://compact_tool_surface`) (confidence 0.96)
  evidence: Locate/open/workset/text-fragment handling is the navigation half of the compact tool surface.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.93)
  evidence: Compact discovery and bounded opening are orchestration over locate/search/query primitives rather than an independent semantic engine.

## compact expansion and concept views

Source Handle: `concept://compact_expansion_and_concept_views`

- part of: `compact tool surface` (`concept://compact_tool_surface`) (confidence 0.96)
  evidence: Expand, concept, and task-brief form the deeper decoding half of the compact tool surface.
- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.90)
  evidence: Compact concept views are only useful because the publication pipeline materializes durable concept packets and relations.
- often used with: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.88)
  evidence: Task brief decoding makes this compact view cluster a frequent companion to coordination and plan runtime reasoning.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.93)
  evidence: Compact expand and task decoding are derived read surfaces over query/projection data, not raw storage owners.

## compact tool surface

Source Handle: `concept://compact_tool_surface`

- specializes: `MCP runtime surface` (`concept://mcp_runtime_surface`) (confidence 0.98)
  evidence: Compact tools are the staged specialization of the broader MCP runtime surface for agent-first workflows.
- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.94)
  evidence: Compact tools are a distinct architectural slice of the repo’s agent-facing surface.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.96)
  evidence: Compact tools are thin orchestration over the query and projection layer rather than an independent semantic engine.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.94)
  evidence: Compact tools are part of the query-surface path that the repo expects to validate and dogfood directly.

## composite memory routing and entry storage

Source Handle: `concept://composite_memory_routing_and_entry_storage`

- part of: `memory recall and scoring` (`concept://memory_recall_and_scoring`) (confidence 0.95)
  evidence: Composite routing and entry storage are the orchestration/storage half of memory recall and scoring.

## concept and expand decode runtime

Source Handle: `concept://concept_and_expand_decode_runtime`

- part of: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`) (confidence 0.96)
  evidence: Concept packet resolution and bounded handle expansion are one half of compact expansion/concept views.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.92)
  evidence: Compact concept/expand decode runtime is a presentation layer over concept packets, validations, lineage, and other query results.

## concept and publication pipeline

Source Handle: `concept://concept_and_publication_pipeline`

- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.97)
  evidence: Concept publication is a dedicated architectural layer spanning core events, projections, query, and MCP.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.97)
  evidence: Concept publication becomes useful only after projection and query layers materialize concept packets and retrieval views.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.93)
  evidence: Concept maintenance and validation policy both require evidence-backed promotion and relation maintenance.

## concept and relation event streams

Source Handle: `concept://concept_and_relation_event_streams`

- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.96)
  evidence: Concept and relation event streams are the append-only event half of the publication pipeline.
- often used with: `semantic projection indexes` (`concept://semantic_projection_indexes`) (confidence 0.88)
  evidence: These event streams are most useful once projection indexes materialize them into queryable packets.

## concept, relation, and intent projections

Source Handle: `concept://concept_relation_and_intent_projections`

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.93)
  evidence: These projections are built from concept and relation publication events rather than raw source structure alone.
- part of: `semantic projection indexes` (`concept://semantic_projection_indexes`) (confidence 0.96)
  evidence: Concept, relation, and intent packet materialization is one half of the semantic projection subsystem.

## coordination and plan runtime

Source Handle: `concept://coordination_and_plan_runtime`

- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.98)
  evidence: Coordination and plan runtime are major architectural responsibilities with dedicated crates and specs.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.95)
  evidence: Plan, task, claim, and artifact runtime state is keyed by IR identities and shared event metadata.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.94)
  evidence: Coordination correctness is one of the named validation layers in docs/VALIDATION.md.

## coordination and plan-runtime queries

Source Handle: `concept://coordination_and_plan_runtime_queries`

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.93)
  evidence: These query views are a direct read surface over the live coordination and plan runtime state.
- part of: `query coordination and plan views` (`concept://query_coordination_and_plan_views`) (confidence 0.96)
  evidence: Runtime coordination and plan-overlay access is one half of the coordination query cluster.

## coordination mutation, lease, and policy helpers

Source Handle: `concept://coordination_mutation_and_policy_helpers`

- part of: `coordination operations and policy` (`concept://coordination_operations_and_policy`) (confidence 0.96)
  evidence: mutations.rs and helpers.rs implement the write-side and shared policy helper half of coordination operations.
- depends on: `coordination state model` (`concept://coordination_state_model`) (confidence 0.96)
  evidence: Coordination mutations and policy helpers operate against the shared coordination state model and runtime overlays.
- depends on: `principal identity and mutation provenance` (`concept://principal_identity_and_mutation_provenance`) (confidence 0.94)
  evidence: Lease ownership, resume/reclaim policy, and heartbeat enforcement derive holder identity from authoritative principal actors recorded on mutation events.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.92)
  evidence: Mutation-side coordination policy needs direct validation because it can violate plan and claim invariants without schema changes.

## coordination operations and policy

Source Handle: `concept://coordination_operations_and_policy`

- part of: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.99)
  evidence: Queries, mutations, blockers, and helpers are the executable behavior slice of the coordination subsystem.
- depends on: `coordination state model` (`concept://coordination_state_model`) (confidence 0.98)
  evidence: The executable behavior layer in prism-coordination is implemented over the shared state/types/runtime model.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.95)
  evidence: Operational plan/task policy is one of the repo areas where validation and dogfooding are explicitly required.

## coordination query and blocker operations

Source Handle: `concept://coordination_query_and_blocker_operations`

- part of: `coordination operations and policy` (`concept://coordination_operations_and_policy`) (confidence 0.96)
  evidence: queries.rs and blockers.rs implement the read-side and blocker policy half of coordination operations.
- depends on: `coordination state model` (`concept://coordination_state_model`) (confidence 0.96)
  evidence: Coordination queries and blocker evaluation execute over the shared coordination state store and types.

## coordination state model

Source Handle: `concept://coordination_state_model`

- part of: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.99)
  evidence: The state/type/runtime model is the foundational internal slice of the coordination subsystem.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.95)
  evidence: Coordination state and overlays are keyed by the same stable identities and event model exposed by prism-ir.

## coordination store and domain types

Source Handle: `concept://coordination_store_and_domain_types`

- part of: `coordination state model` (`concept://coordination_state_model`) (confidence 0.96)
  evidence: state.rs and types.rs provide the stored coordination state and domain schema that make up the coordination state model.

## core indexing pipeline

Source Handle: `concept://core_indexing_pipeline`

- depends on: `language adapter family` (`concept://language_adapter_family`) (confidence 0.98)
  evidence: The indexing pipeline relies on language adapters to convert files into parse results before resolution and reanchoring.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.99)
  evidence: Indexing, resolution, and reanchoring all target the shared IR identity and graph model.
- part of: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`) (confidence 0.99)
  evidence: This concept is the concrete module cluster behind the higher-level workspace indexing and refresh subsystem.

## curator backend execution and prompting

Source Handle: `concept://curator_backend_execution_and_prompting`

- part of: `curator execution flow` (`concept://curator_execution_flow`) (confidence 0.96)
  evidence: Backend config, context bounding, schema rendering, and Codex process launch are the execution half of the curator flow.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.90)
  evidence: Curator backend prompting and sandboxed execution need direct dogfooding because they can produce plausible but weak proposals.

## curator execution flow

Source Handle: `concept://curator_execution_flow`

- part of: `agent inference and curation` (`concept://agent_inference_and_curation`) (confidence 0.95)
  evidence: Curator and curator_support are the concrete execution slice inside the broader inference-and-curation subsystem.
- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.92)
  evidence: Curator proposals only become durable repo knowledge after they flow into the concept and publication path.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.90)
  evidence: Curator output quality is checked through the same dogfooding and validation loop that guards durable knowledge promotion.

## curator rule synthesis and proposal types

Source Handle: `concept://curator_rule_synthesis_and_proposal_types`

- part of: `curator execution flow` (`concept://curator_execution_flow`) (confidence 0.96)
  evidence: Proposal typing and rule synthesis are the proposal-construction half of the curator flow.
- depends on: `memory system` (`concept://memory_system`) (confidence 0.92)
  evidence: Curator synthesis explicitly consumes memories and outcomes when generating structural/semantic memory, risk, and validation proposals.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.90)
  evidence: Curator context and proposal rules consume co-change and validation-check projections when ranking or deduplicating proposals.

## daemon, process, and proxy lifecycle

Source Handle: `concept://daemon_process_and_proxy_lifecycle`

- part of: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`) (confidence 0.96)
  evidence: Daemon/process/proxy control is one half of the runtime lifecycle surface.

## dashboard events and read models

Source Handle: `concept://dashboard_events_and_read_models`

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.90)
  evidence: Dashboard read models surface active tasks, blockers, and runtime coordination state rather than only static metadata.
- part of: `dashboard surface` (`concept://dashboard_surface`) (confidence 0.96)
  evidence: Events and read models are the live-state half of the dashboard surface.

## dashboard routing and assets

Source Handle: `concept://dashboard_routing_and_assets`

- part of: `dashboard surface` (`concept://dashboard_surface`) (confidence 0.95)
  evidence: Router/assets/types define the browser-facing transport boundary inside the dashboard surface.
- depends on: `MCP runtime surface` (`concept://mcp_runtime_surface`) (confidence 0.89)
  evidence: Dashboard routing and asset delivery are exposed from the same prism-mcp runtime that hosts the product surface.

## dashboard surface

Source Handle: `concept://dashboard_surface`

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.93)
  evidence: Dashboard views surface active operations, coordination state, and mutation timelines from the runtime.
- specializes: `MCP runtime surface` (`concept://mcp_runtime_surface`) (confidence 0.95)
  evidence: The dashboard is a specialized product surface inside prism-mcp built on the general MCP runtime.
- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.93)
  evidence: The dashboard is an explicit first-class surface inside the overall PRISM architecture.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.90)
  evidence: The dashboard spec treats trust and validation as visible first-class concerns rather than hidden internals.

## document and config adapters

Source Handle: `concept://document_and_config_adapters`

- part of: `language adapter family` (`concept://language_adapter_family`) (confidence 0.96)
  evidence: Markdown, JSON, TOML, and YAML adapters are the document/config half of the shared adapter family.
- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`) (confidence 0.96)
  evidence: Markdown, JSON, TOML, and YAML adapters reuse the shared prism-parser document and intent/fingerprint helpers.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.95)
  evidence: Document and config adapters still materialize shared IR identities and structure even when their inputs are not executable source.

## graph, identity, and parse IR

Source Handle: `concept://graph_identity_and_parse_ir`

- part of: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.95)
  evidence: Graph, identity, primitives, and parse are the static-schema sublayer inside the broader structural IR concept.

## history snapshot and resolution

Source Handle: `concept://history_snapshot_and_resolution`

- part of: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.96)
  evidence: Resolver, snapshot, and history store form the temporal replay half of the persistence/history subsystem.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.93)
  evidence: Lineage snapshots and resolution replay IR identities, events, and anchors defined in prism-ir.

## impact and outcome queries

Source Handle: `concept://impact_and_outcome_queries`

- depends on: `memory system` (`concept://memory_system`) (confidence 0.90)
  evidence: Outcome history and validation context are backed by the repo’s memory and outcome subsystems.
- part of: `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`) (confidence 0.95)
  evidence: Impact and outcome reads are the judgment-oriented half of the symbol/change query cluster.

## indexer orchestration and snapshot loading

Source Handle: `concept://indexer_orchestration_and_snapshot_loading`

- part of: `core indexing pipeline` (`concept://core_indexing_pipeline`) (confidence 0.96)
  evidence: Indexer orchestration and snapshot restoration are one half of the core indexing pipeline.
- depends on: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.90)
  evidence: This orchestration path loads prior graph, history, outcomes, coordination, and projection snapshots from persisted state.

## inferred edge runtime and session store

Source Handle: `concept://inferred_edge_runtime_and_session_store`

- part of: `agent inference and curation` (`concept://agent_inference_and_curation`) (confidence 0.96)
  evidence: The inferred-edge store and mutation path are the non-curator half of the repo’s inference-and-curation subsystem.
- depends on: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`) (confidence 0.92)
  evidence: Live inferred-edge capture and curator promotion rely on the authenticated mutation host that accepts `infer_edge` and curator-driven mutation actions.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.88)
  evidence: Inferred edges are explicitly called out in the validation plan as a bounded overlay that must not silently rewrite authoritative structure.

## JavaScript API contract surface

Source Handle: `concept://javascript_api_contract_surface`

- part of: `JavaScript query ABI` (`concept://javascript_query_abi`) (confidence 0.95)
  evidence: Shared client/server payload contracts are the schema half of the JS-facing ABI surface.
- depends on: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`) (confidence 0.90)
  evidence: The JS contract surface must stay aligned with the server’s query/resource payload shapes.

## JavaScript query ABI

Source Handle: `concept://javascript_query_abi`

- often used with: `MCP runtime surface` (`concept://mcp_runtime_surface`) (confidence 0.95)
  evidence: The JS query ABI and the MCP runtime are separate layers, but in practice they travel together as the agent-facing programmable interface.
- part of: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.96)
  evidence: The JS/TS ABI is the language-level contract layer attached to the broader projection/query surface.

## JavaScript runtime and reference bridge

Source Handle: `concept://javascript_runtime_and_reference_bridge`

- part of: `JavaScript query ABI` (`concept://javascript_query_abi`) (confidence 0.95)
  evidence: Runtime prelude and generated reference docs are one half of the JS-facing ABI surface.

## language adapter family

Source Handle: `concept://language_adapter_family`

- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.98)
  evidence: Language adapters are one of the major repo subsystems declared in the workspace crate split.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.99)
  evidence: All language adapter crates depend on prism-ir through prism-parser to emit the shared node and edge schema.

## locate ranking and text-candidate flow

Source Handle: `concept://locate_ranking_and_text_candidate_flow`

- part of: `compact discovery and opening flow` (`concept://compact_discovery_and_opening`) (confidence 0.96)
  evidence: Locate ranking and text-fragment candidate generation are the first-hop navigation half of compact discovery/opening.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.92)
  evidence: Compact locate reranks semantic search results and text-derived symbol candidates produced from the underlying query/projection layer.

## Markdown heading and intent adapter

Source Handle: `concept://markdown_heading_and_intent_adapter`

- part of: `document and config adapters` (`concept://document_and_config_adapters`) (confidence 0.96)
  evidence: The Markdown adapter is one concrete branch of the document/config adapter family.
- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`) (confidence 0.96)
  evidence: The Markdown adapter uses shared document-name, document-path, fingerprint, and intent-target helpers from prism-parser.

## MCP authenticated mutation host

Source Handle: `concept://mcp_mutation_and_session_host`

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.96)
  evidence: Mutation hosting persists concepts, relations, memories, and outcomes through the publication/mutation pipeline.
- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.94)
  evidence: The authenticated mutation host fronts coordination, claim, artifact, and heartbeat mutations through `prism_mutate` over the shared coordination runtime.
- part of: `MCP runtime surface` (`concept://mcp_runtime_surface`) (confidence 0.99)
  evidence: Mutation hosting and session-state handling are core internal responsibilities of the MCP runtime surface.
- depends on: `principal identity and mutation provenance` (`concept://principal_identity_and_mutation_provenance`) (confidence 0.96)
  evidence: Before any authoritative write is persisted, the authenticated mutation host stamps event actors and execution-context snapshots through the principal/provenance layer.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.94)
  evidence: Authenticated mutation contracts, provenance stamping, and hosted runtime behavior are central to the repo’s MCP dogfooding loop.

## MCP query and resource serving

Source Handle: `concept://mcp_query_and_resource_serving`

- part of: `MCP runtime surface` (`concept://mcp_runtime_surface`) (confidence 0.99)
  evidence: Query/resource serving is a concrete internal slice of the broader MCP runtime surface.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.99)
  evidence: This serving layer is just the transport/runtime wrapper around the underlying projection and query semantics.

## MCP runtime lifecycle

Source Handle: `concept://mcp_runtime_lifecycle`

- part of: `MCP runtime surface` (`concept://mcp_runtime_surface`) (confidence 0.98)
  evidence: Daemon lifecycle and runtime health are another internal slice of the MCP runtime surface.
- depends on: `workspace session runtime` (`concept://workspace_session_runtime`) (confidence 0.90)
  evidence: The runtime lifecycle is useful only because it hosts and refreshes live WorkspaceSession-backed state.

## MCP runtime surface

Source Handle: `concept://mcp_runtime_surface`

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.95)
  evidence: Concept mutation, concept resolution, and relation mutation are served from the MCP runtime surface.
- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.96)
  evidence: MCP tools and resources expose plan, task, claim, artifact, and blocker state through the coordination runtime.
- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.99)
  evidence: The MCP runtime is the main agent-facing product surface layered on top of the core crates.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.99)
  evidence: The MCP runtime serves query, discovery, and concept-resolution results produced by the projection and query layer.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.95)
  evidence: The validation pipeline explicitly calls out PRISM MCP and query-surface validation.

## memory outcomes and session history

Source Handle: `concept://memory_outcomes_and_session_history`

- part of: `memory system` (`concept://memory_system`) (confidence 0.98)
  evidence: Outcome/session history is a distinct internal slice of the broader memory system.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.92)
  evidence: Outcome and replay history are part of the evidence loops the repo expects to validate and dogfood directly.

## memory projection persistence

Source Handle: `concept://memory_projection_persistence`

- often used with: `memory system` (`concept://memory_system`) (confidence 0.90)
  evidence: This persistence slice exists to hydrate and durably back the repo’s memory subsystem rather than the raw graph alone.
- part of: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.94)
  evidence: Memory projection and memory_store are the memory-specific persistence slice inside the broader persistence/history layer.

## memory recall and scoring

Source Handle: `concept://memory_recall_and_scoring`

- part of: `memory system` (`concept://memory_system`) (confidence 0.99)
  evidence: Recall/scoring is a concrete internal slice of the broader memory system.
- often used with: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`) (confidence 0.90)
  evidence: Recall/scoring is distinct from semantic/structural models, but they are commonly used together to improve retrieval quality.

## memory refresh and patch outcome recording

Source Handle: `concept://memory_refresh_and_patch_outcome_recording`

- depends on: `memory system` (`concept://memory_system`) (confidence 0.91)
  evidence: This path explicitly reanchors persisted session memory and records outcome events into the memory subsystem.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.88)
  evidence: Reanchored memory and patch-derived validation deltas need direct validation because they can look right while silently drifting.
- part of: `workspace session runtime` (`concept://workspace_session_runtime`) (confidence 0.96)
  evidence: Memory reanchor and patch outcome recording are the persistence-feedback half of the workspace session runtime.

## memory system

Source Handle: `concept://memory_system`

- depends on: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.96)
  evidence: Memory recall and reanchoring rely on persisted history, snapshots, and stored memory projections.
- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.98)
  evidence: Memory is a first-class subsystem in the repo architecture and product model.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.95)
  evidence: The validation pipeline includes memory anchoring and PRISM memory re-anchoring validation.

## mutation argument and schema surface

Source Handle: `concept://mutation_argument_and_schema_surface`

- part of: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`) (confidence 0.96)
  evidence: Argument decoding and mutation schemas are one half of the authenticated mutation host surface, including authenticated coordination and heartbeat contracts.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.90)
  evidence: Schema examples and argument validation are explicitly part of the repo’s MCP/query-surface validation story.

## open and workset follow-through

Source Handle: `concept://open_and_workset_followthrough`

- part of: `compact discovery and opening flow` (`concept://compact_discovery_and_opening`) (confidence 0.96)
  evidence: Open and workset follow-through are the bounded read-assembly half of compact discovery/opening.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.92)
  evidence: Compact open/workset resolves handles, previews slices, and assembles supporting reads from the underlying query surface.

## outcome event and replay memory

Source Handle: `concept://outcome_event_and_replay_memory`

- part of: `memory outcomes and session history` (`concept://memory_outcomes_and_session_history`) (confidence 0.96)
  evidence: Outcome replay is one half of the memory outcomes/session-history branch.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.90)
  evidence: Outcome replay matters because it preserves prior failures and validations, so it needs direct dogfooding and validation.

## parse, resolution, and reanchor flow

Source Handle: `concept://parse_resolution_and_reanchor_flow`

- part of: `core indexing pipeline` (`concept://core_indexing_pipeline`) (confidence 0.96)
  evidence: Parsing, resolution, and reanchor logic are the structural-truth half of the core indexing pipeline.
- depends on: `language adapter family` (`concept://language_adapter_family`) (confidence 0.92)
  evidence: This flow only exists because language adapters produce unresolved parse results and structure for later resolution.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.92)
  evidence: Parse, resolution, and reanchor all target the shared node, edge, and identity model in prism-ir.

## parser contract and fingerprint utilities

Source Handle: `concept://parser_contract_and_fingerprint_utilities`

- part of: `language adapter family` (`concept://language_adapter_family`) (confidence 0.96)
  evidence: prism-parser provides the shared adapter contract and fingerprint utilities beneath both code and document/config adapter families.

## persistence and history layer

Source Handle: `concept://persistence_and_history`

- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.98)
  evidence: Store and history crates form a durable architectural slice of the repo.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.97)
  evidence: Store and history layers persist and replay IR identities, anchors, and event metadata rather than defining their own schema.

## plan and coordination IR

Source Handle: `concept://plan_and_coordination_ir`

- often used with: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.90)
  evidence: The runtime coordination layer is built on top of the shared plan/coordination IR schema defined in prism-ir.
- part of: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.94)
  evidence: Plan and coordination schema live in prism-ir as a specialized shared type family rather than a separate subsystem.

## plan and repo-layout publication

Source Handle: `concept://plan_and_repo_layout_publication`

- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.93)
  evidence: Published plan files, layout helpers, and artifact path helpers are the filesystem publication sub-layer of the broader publication pipeline.
- often used with: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.86)
  evidence: Published plan artifacts are a durability surface for coordination state even though the layout helpers are not the coordination engine itself.
- depends on: `repo publication guards` (`concept://repo_publication_guards`) (confidence 0.88)
  evidence: Layout and durable-plan writes rely on the publication rules that decide what may safely become repo knowledge.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.88)
  evidence: Path/layout publication is only trustworthy when validation catches misplaced or stale durable artifacts.

## plan completion and insight queries

Source Handle: `concept://plan_completion_and_insight_queries`

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.92)
  evidence: Plan completion and insight evaluation only make sense over current coordination and plan runtime state.
- part of: `query coordination and plan views` (`concept://query_coordination_and_plan_views`) (confidence 0.96)
  evidence: Completion and insight heuristics are the higher-judgment half of the coordination query cluster.

## plan-graph compatibility and runtime overlays

Source Handle: `concept://plan_graph_compatibility_and_runtime_overlays`

- part of: `coordination state model` (`concept://coordination_state_model`) (confidence 0.96)
  evidence: compat.rs and runtime.rs export plan graph compatibility and runtime overlays as the second half of the coordination state model.

## principal identity and mutation provenance

Source Handle: `concept://principal_identity_and_mutation_provenance`

- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.93)
  evidence: Principal actors and execution-context snapshots are durable IR event types rather than MCP-only transport metadata.

## projection and query layer

Source Handle: `concept://projection_and_query_layer`

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.96)
  evidence: The Prism read surface exposes plan, task, blocker, claim, and artifact views from coordination state.
- depends on: `memory system` (`concept://memory_system`) (confidence 0.98)
  evidence: Projection and query surfaces incorporate recall, outcomes, and memory-backed context.
- depends on: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.97)
  evidence: Queries and projections read graph and history snapshots from the persistence layer.
- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.99)
  evidence: Projection and query are central repo subsystems surfaced across PRISM tools and docs.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.96)
  evidence: Projection validation and MCP/query-surface validation are explicit parts of the repo validation model.

## published knowledge and memory event logs

Source Handle: `concept://published_knowledge_and_memory_event_logs`

- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.95)
  evidence: Published knowledge artifacts and memory event logs are the durable-log half of the publication pipeline.
- often used with: `memory system` (`concept://memory_system`) (confidence 0.89)
  evidence: Memory event logs are consumed by the repo’s memory subsystem rather than living as an isolated publication detail.
- depends on: `repo publication guards` (`concept://repo_publication_guards`) (confidence 0.87)
  evidence: Durable knowledge and memory publication logs still rely on the publication/trust rules that decide what is safe to persist.

## Python tree-sitter adapter pipeline

Source Handle: `concept://python_tree_sitter_adapter_pipeline`

- part of: `code language adapters` (`concept://code_language_adapters`) (confidence 0.96)
  evidence: The Python parser/syntax/path split is one concrete branch of the code language adapter family.
- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`) (confidence 0.96)
  evidence: The Python adapter implements LanguageAdapter and reuses shared parse input/result and fingerprint helpers from prism-parser.

## query coordination and plan views

Source Handle: `concept://query_coordination_and_plan_views`

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.97)
  evidence: These query views are built over coordination and plan runtime state rather than over raw graph structure alone.
- part of: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.98)
  evidence: Coordination-aware query views are another internal slice of the overall projection/query layer.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.93)
  evidence: Task/plan query views are part of the surfaced behavior the repo expects to validate end to end.

## query execution and semantic-context serving

Source Handle: `concept://query_execution_and_semantic_context_serving`

- part of: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`) (confidence 0.96)
  evidence: Live query execution and semantic context serving are one half of the MCP query/resource surface.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.94)
  evidence: This serving path is a thin runtime wrapper over the repo’s projection and query layer.

## query symbol and change contexts

Source Handle: `concept://query_symbol_and_change_contexts`

- part of: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.99)
  evidence: Symbol and change-context bundling is one internal slice of the overall projection/query layer.
- depends on: `semantic projection indexes` (`concept://semantic_projection_indexes`) (confidence 0.94)
  evidence: Symbol/change read bundles rely on the derived indexes and rankings produced by the projection layer.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.93)
  evidence: These are exactly the kinds of context bundles that can look convincing while drifting, so they need direct dogfooding and validation.

## recall signal and text scoring

Source Handle: `concept://recall_signal_and_text_scoring`

- part of: `memory recall and scoring` (`concept://memory_recall_and_scoring`) (confidence 0.96)
  evidence: Scoring signals and text helpers are one half of the memory recall/scoring subsystem.

## repo publication guards

Source Handle: `concept://repo_publication_guards`

- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.98)
  evidence: Publication guards are the policy and validation sub-layer inside the concept/publication pipeline.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.95)
  evidence: Publication guards encode evidence, trust, provenance, and quality thresholds that align directly with the repo validation philosophy.

## resource schemas and host-resource serving

Source Handle: `concept://resource_schemas_and_host_resource_serving`

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.88)
  evidence: Many host resources expose published concepts, relations, memories, and related durable knowledge state.
- part of: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`) (confidence 0.96)
  evidence: Host resources and schema-backed resource publication are the other half of the MCP query/resource surface.

## Rust tree-sitter adapter pipeline

Source Handle: `concept://rust_tree_sitter_adapter_pipeline`

- part of: `code language adapters` (`concept://code_language_adapters`) (confidence 0.96)
  evidence: The Rust parser/syntax/path split is one concrete branch of the code language adapter family.
- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`) (confidence 0.96)
  evidence: The Rust adapter implements LanguageAdapter and reuses shared parse input/result and fingerprint helpers from prism-parser.

## semantic and structural memory models

Source Handle: `concept://semantic_and_structural_memory_models`

- part of: `memory system` (`concept://memory_system`) (confidence 0.98)
  evidence: Semantic and structural retrieval models are a distinct internal slice of the broader memory system.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.90)
  evidence: Semantic and structural retrieval models need explicit validation because their outputs can look plausible while drifting semantically.

## semantic memory backend runtime

Source Handle: `concept://semantic_memory_backend_runtime`

- depends on: `recall signal and text scoring` (`concept://recall_signal_and_text_scoring`) (confidence 0.89)
  evidence: Semantic recall still layers its backend signals onto the shared recall/text scoring substrate.
- part of: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`) (confidence 0.96)
  evidence: Semantic backend configuration/runtime is the embedding-aware half of the model-backed memory branch.

## semantic projection indexes

Source Handle: `concept://semantic_projection_indexes`

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`) (confidence 0.95)
  evidence: Projection indexes hydrate published concepts and concept relations emitted by the publication pipeline.
- part of: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.99)
  evidence: Derived projection indexes are the projection-side internal slice of the broader read layer.

## server surface and runtime health views

Source Handle: `concept://server_surface_and_runtime_health_views`

- part of: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`) (confidence 0.96)
  evidence: Server capability, runtime state, and diagnostics are the health/status half of the runtime lifecycle surface.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.88)
  evidence: Server diagnostics, runtime status, and health reporting are only trustworthy if the repo validates them directly.

## session and episodic memory store

Source Handle: `concept://session_and_episodic_memory_store`

- part of: `memory outcomes and session history` (`concept://memory_outcomes_and_session_history`) (confidence 0.95)
  evidence: Session/episodic storage is the continuity-storage half of the outcomes/session-history branch.
- depends on: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`) (confidence 0.90)
  evidence: Session memory composes structural and semantic modules into the session-scoped memory runtime.

## session context and hosted mutation runtime

Source Handle: `concept://session_state_and_mutation_runtime`

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.90)
  evidence: Many hosted mutations and follow-up runtime views touch shared task, claim, artifact, and plan state.
- part of: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`) (confidence 0.96)
  evidence: Session context, hosted mutation execution, and follow-up runtime views are the execution half of the authenticated mutation host.

## SQLite and graph persistence

Source Handle: `concept://sqlite_and_graph_persistence`

- part of: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.96)
  evidence: SQLite, graph, and persist-batch modules are the primary durable-store sublayer inside persistence and history.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.94)
  evidence: The store persists graph snapshots and ids defined by prism-ir rather than inventing a separate semantic schema.

## structural IR and identity model

Source Handle: `concept://structural_ir`

- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.99)
  evidence: The structural IR is a foundational subsystem within the top-level PRISM monorepo architecture.
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.95)
  evidence: The validation plan explicitly starts with structural truth and identity correctness.

## structural memory feature model

Source Handle: `concept://structural_memory_feature_model`

- depends on: `recall signal and text scoring` (`concept://recall_signal_and_text_scoring`) (confidence 0.88)
  evidence: Structural recall still uses the shared recall/base-signal and text-overlap helpers when it ranks entries.
- part of: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`) (confidence 0.96)
  evidence: Structural feature extraction and structural recall are one half of the model-backed memory branch.

## structured config value adapters

Source Handle: `concept://structured_config_value_adapters`

- part of: `document and config adapters` (`concept://document_and_config_adapters`) (confidence 0.96)
  evidence: JSON, TOML, and YAML config walkers are the structured-value branch of the document/config adapter family.
- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`) (confidence 0.96)
  evidence: The JSON, TOML, and YAML adapters all reuse shared document, fingerprint, whole-file-span, and intent helper logic from prism-parser.

## symbol, source, and relation queries

Source Handle: `concept://symbol_source_and_relation_queries`

- part of: `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`) (confidence 0.96)
  evidence: Symbol and source resolution is the direct-read half of the symbol/change query cluster.
- depends on: `semantic projection indexes` (`concept://semantic_projection_indexes`) (confidence 0.88)
  evidence: Symbol and relation reads rely on projected graph/context structures even when they also touch raw source slices.

## task-brief and coordination summary views

Source Handle: `concept://task_brief_and_coordination_summary_views`

- part of: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`) (confidence 0.95)
  evidence: Task-brief summaries are the coordination-specific summary half of compact expansion/concept views.
- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`) (confidence 0.95)
  evidence: Task-brief summaries are built directly from coordination task, blocker, claim, conflict, and plan-runtime query state.

## validation and dogfooding loop

Source Handle: `concept://validation_and_dogfooding`

- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.96)
  evidence: Validation is treated as a cross-cutting architectural concern rather than a local testing detail.

## validation feedback and metrics loop

Source Handle: `concept://validation_feedback_and_metrics_loop`

- often used with: `dashboard surface` (`concept://dashboard_surface`) (confidence 0.84)
  evidence: The validation spec explicitly calls for a metrics dashboard, making this loop a frequent companion to the dashboard surface.
- depends on: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`) (confidence 0.89)
  evidence: The validation feedback loop relies on the hosted mutation path that records `validation_feedback` entries into repo state.
- depends on: `projection and query layer` (`concept://projection_and_query_layer`) (confidence 0.90)
  evidence: Validation checks and scorecards are materialized through projection state rather than only handwritten notes.
- part of: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.96)
  evidence: Feedback capture, projection checks, metrics dashboards, and correction loops are the live calibration half of validation and dogfooding.

## validation policy and release gates

Source Handle: `concept://validation_policy_and_release_gates`

- part of: `validation and dogfooding loop` (`concept://validation_and_dogfooding`) (confidence 0.96)
  evidence: The validation principles, pipeline, PRISM plan, and trust gates are the policy half of the validation subsystem.

## workspace indexing and refresh

Source Handle: `concept://workspace_indexing_and_refresh`

- depends on: `language adapter family` (`concept://language_adapter_family`) (confidence 0.96)
  evidence: The workspace indexer relies on language adapters to turn concrete files into parse results.
- depends on: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.97)
  evidence: Index refresh materializes and updates its state through the store and history layers.
- part of: `PRISM architecture` (`concept://prism_architecture`) (confidence 0.99)
  evidence: prism-core indexing and refresh orchestration is a central architecture subsystem.
- depends on: `structural IR and identity model` (`concept://structural_ir`) (confidence 0.98)
  evidence: Index refresh orchestrates parsing and resolution into the shared IR model before any higher-level projections exist.

## workspace session refresh runtime

Source Handle: `concept://workspace_session_refresh_runtime`

- part of: `workspace session runtime` (`concept://workspace_session_runtime`) (confidence 0.96)
  evidence: Session refresh state and filesystem watch handling are one half of the workspace session runtime.

## workspace session runtime

Source Handle: `concept://workspace_session_runtime`

- depends on: `memory system` (`concept://memory_system`) (confidence 0.95)
  evidence: Workspace session refresh explicitly coordinates memory snapshot loading and refresh behavior.
- depends on: `persistence and history layer` (`concept://persistence_and_history`) (confidence 0.95)
  evidence: Hydrated workspace sessions and patch outcomes are loaded from and synchronized back to persisted workspace state.
- part of: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`) (confidence 0.97)
  evidence: Workspace session hydration and refresh runtime are a second internal slice of the broader indexing/refresh subsystem.

