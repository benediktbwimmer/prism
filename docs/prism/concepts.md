# PRISM Concepts

> Generated from repo-scoped PRISM concept and relation knowledge.
> Return to the concise entrypoint in `../../PRISM.md`.

## Overview

- Active repo concepts: 95
- Active repo relations: 206

- Active repo contracts: 8

## Published Concepts

- `agent inference and curation` (`concept://agent_inference_and_curation`): Inference and curator stack that records inferred edges, synthesizes curator output, and promotes reviewed knowledge into persistent PRISM state.
- `anchor, change, and lineage IR` (`concept://anchor_change_and_lineage_ir`): prism-ir modules that connect source anchors, observed changes, event metadata, and lineage history so structural identities can survive edits over time.
- `CLI command and parse surface` (`concept://cli_command_and_parse_surface`): prism-cli modules that define CLI arguments, parse user input, and dispatch commands while keeping the binary entrypoint thin.
- `CLI runtime and MCP control` (`concept://cli_runtime_and_mcp_control`): prism-cli modules that manage operator-facing runtime behavior, MCP daemon control, and terminal display concerns after command dispatch is resolved.
- `CLI surface` (`concept://cli_surface`): Local operator surface for indexing, querying, and MCP lifecycle control, built as a thin binary over prism-core and related crates.
- `co-change and validation projection indexes` (`concept://cochange_and_validation_projection_indexes`): prism-projections modules that derive co-change neighbors, validation deltas, and projection snapshots used by higher query and compact-tool surfaces.
- `code language adapters` (`concept://code_language_adapters`): Rust and Python adapter modules that parse executable source code into PRISM IR using parser, syntax, and path-resolution helpers.
- `compact discovery and opening flow` (`concept://compact_discovery_and_opening`): compact-tool modules that rank likely targets, normalize text fragments, open bounded slices, and assemble small worksets for the default agent path.
- `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`): compact-tool modules that decode concept packets, expand bounded diagnostics, and summarize coordination tasks without dropping to raw query output.
- `compact tool surface` (`concept://compact_tool_surface`): Staged locate/open/gather/workset/expand/concept/task-brief tool layer inside prism-mcp that serves as the intended default agent path above raw prism_query.
- `compact_tools` (`concept://compact_tools`): The staged agent-facing surface for locate, open, workset, and expand, plus the ranking and next-action rules that keep compact follow-through bounded.
- `composite memory routing and entry storage` (`concept://composite_memory_routing_and_entry_storage`): prism-memory modules that deduplicate recalls across modules, route stores by memory kind, and persist per-module entry snapshots with anchor indexes.
- `concept and expand decode runtime` (`concept://concept_and_expand_decode_runtime`): Compact-tool decode path that resolves concept packets and turns handles into bounded diagnostics, lineage, neighbor, validation, memory, and drift views without dropping to raw query output.
- `concept and publication pipeline` (`concept://concept_and_publication_pipeline`): Event-sourced concept lifecycle that promotes repo-quality concepts and concept relations from core events into projected retrieval packets and MCP-facing mutation surfaces.
- `concept and relation event streams` (`concept://concept_and_relation_event_streams`): prism-core modules that append and replay durable concept and concept-relation event streams before projections materialize them into queryable packets.
- `concept, relation, and intent projections` (`concept://concept_relation_and_intent_projections`): prism-projections modules that materialize curated concept packets, concept-to-concept relation packets, and intent indexes from lower-level events.
- `coordination and plan runtime` (`concept://coordination_and_plan_runtime`): Shared plan, task, claim, artifact, and blocker subsystem that models active intent and exposes runtime coordination views across PRISM.
- `coordination and plan-runtime queries` (`concept://coordination_and_plan_runtime_queries`): prism-query modules that expose coordination snapshots, plan runtime overlays, intent views, and related query types over live coordination state.
- `coordination mutation, lease, and policy helpers` (`concept://coordination_mutation_and_policy_helpers`): prism-coordination modules that apply coordination mutations and enforce lease lifecycle, resume/reclaim, heartbeat, rejection recording, and shared policy/conflict helpers.
- `coordination operations and policy` (`concept://coordination_operations_and_policy`): Coordination modules that implement query paths, mutations, blocker calculation, and helper logic over the shared coordination state model.
- `coordination query and blocker operations` (`concept://coordination_query_and_blocker_operations`): prism-coordination modules that answer readiness, conflict, pending-review, and blocker questions over coordination state.
- `coordination state model` (`concept://coordination_state_model`): Core coordination modules that define state, types, runtime overlays, and compatibility projections for plans, tasks, claims, and artifacts.
- `coordination store and domain types` (`concept://coordination_store_and_domain_types`): prism-coordination modules that hold the canonical in-memory coordination snapshot and define plans, tasks, claims, artifacts, reviews, and policy records.
- `core indexing pipeline` (`concept://core_indexing_pipeline`): prism-core modules that parse files, resolve structure, reanchor temporal identity, and watch workspace changes to build the live PRISM graph.
- `curator backend execution and prompting` (`concept://curator_backend_execution_and_prompting`): Curator modules that bound curator context, render backend schemas/config, and execute the Codex-backed curator process under explicit sandbox, approval, and local-provider settings.
- `curator execution flow` (`concept://curator_execution_flow`): prism-core and prism-curator modules that run curator jobs, prepare support context, and transform reviewable proposals into publishable outcomes.
- `curator rule synthesis and proposal types` (`concept://curator_rule_synthesis_and_proposal_types`): Curator modules that define curator jobs, bounded graph/outcome/memory/projection context, proposal payload types, and rule-based synthesis/merge of edge, memory, concept, and validation proposals.
- `daemon, process, and proxy lifecycle` (`concept://daemon_process_and_proxy_lifecycle`): prism-mcp modules that spawn, proxy, and supervise the daemonized server process and its transport-level lifecycle.
- `dashboard events and read models` (`concept://dashboard_events_and_read_models`): dashboard modules inside prism-mcp that derive read models, shape event streams, and keep the dashboard synchronized with runtime truth.
- `dashboard routing and assets` (`concept://dashboard_routing_and_assets`): dashboard modules inside prism-mcp that define browser-facing routes, static assets, and typed payload boundaries for the PRISM dashboard surface.
- `dashboard surface` (`concept://dashboard_surface`): First-class observability and UI surface inside prism-mcp that publishes dashboard read models, event streams, and router endpoints instead of reusing the raw MCP tool catalog.
- `document and config adapters` (`concept://document_and_config_adapters`): Markdown, JSON, TOML, and YAML adapter modules that map documents and configuration files into PRISM IR and intent surfaces.
- `facade_modularity_rule` (`concept://facade_modularity_rule`): Repo-wide architectural rule that keeps `main.rs` and `lib.rs` as thin facades and pushes substantive logic into dedicated modules.
- `graph, identity, and parse IR` (`concept://graph_identity_and_parse_ir`): Core prism-ir modules that define graph objects, stable ids, primitive spans/types, and parser-side unresolved payloads consumed by the rest of PRISM.
- `history snapshot and resolution` (`concept://history_snapshot_and_resolution`): prism-history modules that resolve lineage, materialize snapshots, and expose the history store used to replay temporal identity over time.
- `impact and outcome queries` (`concept://impact_and_outcome_queries`): prism-query modules that compute blast radius, co-change, validation recipes, and outcome history context over projected and historical PRISM state.
- `indexer orchestration and snapshot loading` (`concept://indexer_orchestration_and_snapshot_loading`): prism-core modules that bootstrap workspace indexing, load prior snapshots and curated knowledge, and assemble the initial indexer/session state around the workspace graph.
- `inferred edge runtime and session store` (`concept://inferred_edge_runtime_and_session_store`): Modules that define inferred-edge records and scopes, layer session-only versus persisted inference state, and route inferred-edge mutations and curator edge promotion through the live MCP runtime.
- `JavaScript API contract surface` (`concept://javascript_api_contract_surface`): Shared type-contract modules that define the JavaScript-facing view schema between prism-js clients and prism-mcp resource/query payloads.
- `JavaScript query ABI` (`concept://javascript_query_abi`): prism-js modules that define the agent-facing TypeScript/JS API types, runtime prelude, and generated reference docs for programmable queries.
- `JavaScript runtime and reference bridge` (`concept://javascript_runtime_and_reference_bridge`): prism-js modules that expose the runtime prelude and generated API reference used when JavaScript or MCP-facing consumers need the PRISM query surface.
- `language adapter family` (`concept://language_adapter_family`): Language- and format-specific adapters that translate Rust, Python, Markdown, JSON, TOML, and YAML inputs into the shared parser and IR pipeline.
- `locate ranking and text-candidate flow` (`concept://locate_ranking_and_text_candidate_flow`): Compact-tool locate path that blends semantic search results, text-fragment candidates, exact identifier matches, glob filtering, and reranking heuristics into first-hop ranked targets.
- `Markdown heading and intent adapter` (`concept://markdown_heading_and_intent_adapter`): Markdown adapter flow that turns headings into nested document structure, fingerprints markdown sections, and extracts unresolved intent targets from prose context.
- `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`): prism-mcp modules that decode `prism_mutate` actions, host authenticated state-changing mutations, and route agent-side writes into durable PRISM state without relying on `prism_session` authority.
- `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`): prism-mcp serving layer that runs query execution, semantic context assembly, and MCP resource reads over the live Prism workspace.
- `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`): prism-mcp daemon and runtime-state modules that launch the server, track process/runtime health, and expose lifecycle-oriented status views.
- `MCP runtime surface` (`concept://mcp_runtime_surface`): MCP- and daemon-facing runtime surface that bridges PRISM query and mutation capabilities into resources, schemas, compact tools, and long-lived server state.
- `memory outcomes and session history` (`concept://memory_outcomes_and_session_history`): Memory modules that record task/session-local history, outcome events, and replay-oriented recall over prior work.
- `memory projection persistence` (`concept://memory_projection_persistence`): prism-store modules that persist memory projections and memory-store state separately from the primary graph/history tables.
- `memory recall and scoring` (`concept://memory_recall_and_scoring`): Memory modules that retrieve, score, and compose remembered context from text and common recall utilities into usable results.
- `memory refresh and patch outcome recording` (`concept://memory_refresh_and_patch_outcome_recording`): prism-core modules that reanchor persisted memory snapshots after lineage changes and record patch-derived outcome events and validation deltas during refresh.
- `memory system` (`concept://memory_system`): Layered session, episodic, structural, outcome, and semantic memory subsystem with recall and re-anchoring support across PRISM runtime workflows.
- `mutation argument and schema surface` (`concept://mutation_argument_and_schema_surface`): prism-mcp modules that define mutation/query argument types, tool schemas, and executable examples for the hosted MCP surface.
- `open and workset follow-through` (`concept://open_and_workset_followthrough`): Compact-tool open/workset path that resolves session handles, expands concept and text-fragment targets, assembles supporting reads and likely tests, and enforces workset budget limits.
- `outcome event and replay memory` (`concept://outcome_event_and_replay_memory`): prism-memory modules that store outcome events, index them by anchors and tasks, and replay prior validation or failure history for resume flows.
- `parse, resolution, and reanchor flow` (`concept://parse_resolution_and_reanchor_flow`): prism-core modules that parse files, resolve static edges, and infer lineage-preserving reanchors across file moves and symbol changes during refresh.
- `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`): Shared prism-parser contract that defines parse inputs/results, language adapter behavior, intent extraction, document naming, and stable fingerprint helpers reused by every adapter crate.
- `persistence and history layer` (`concept://persistence_and_history`): SQLite-backed graph and temporal persistence layer that stores structural state, history snapshots, memory projections, and other durable PRISM artifacts.
- `persistence_split` (`concept://persistence_split`): The target three-plane persistence model that separates repo-published `.prism` truth, shared mutable backend state, and process-local cache/materializations.
- `plan and coordination IR` (`concept://plan_and_coordination_ir`): prism-ir modules that define plan graphs, blockers, validation refs, claims, capabilities, review states, and other shared coordination schema used above the raw graph layer.
- `plan and repo-layout publication` (`concept://plan_and_repo_layout_publication`): prism-core modules that govern published plan material, repo layout paths, and supporting helpers used when durable artifacts are written back into the workspace.
- `plan completion and insight queries` (`concept://plan_completion_and_insight_queries`): prism-query modules that evaluate plan completion, blockers, recommendations, and higher-level plan insights beyond raw runtime overlays.
- `plan-graph compatibility and runtime overlays` (`concept://plan_graph_compatibility_and_runtime_overlays`): prism-coordination modules that translate between coordination snapshots and plan graphs and expose runtime overlay state above the canonical store.
- `principal identity and mutation provenance` (`concept://principal_identity_and_mutation_provenance`): Cross-crate principal-authentication and provenance-stamping layer that turns authenticated local principals into durable event actors and execution-context snapshots for every authoritative mutation.
- `PRISM architecture` (`concept://prism_architecture`): Top-level monorepo architecture spanning semantic IR, language parsing, workspace indexing, persistence/history, memory/projections/query, coordination/curation, and MCP-facing product surfaces.
- `projection and query layer` (`concept://projection_and_query_layer`): Derived indexes and read APIs that turn graph, history, memory, and coordination state into semantic discovery, impact, and programmable query results.
- `published knowledge and memory event logs` (`concept://published_knowledge_and_memory_event_logs`): prism-core modules that persist published knowledge artifacts and append memory event logs used to hydrate durable repo knowledge and memory views.
- `Python tree-sitter adapter pipeline` (`concept://python_tree_sitter_adapter_pipeline`): Python adapter modules that parse Python source with tree-sitter and combine syntax extraction and import/path normalization into PRISM nodes, fingerprints, unresolved calls, imports, and intents.
- `query coordination and plan views` (`concept://query_coordination_and_plan_views`): prism-query modules that project coordination state, plan runtime overlays, intent, and shared query types into the programmable read surface.
- `query execution and semantic-context serving` (`concept://query_execution_and_semantic_context_serving`): prism-mcp modules that execute PRISM reads, normalize diagnostics, and serve semantic context bundles over the live MCP runtime.
- `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`): prism-query modules that resolve symbols, source slices, impact, and outcome/change context into high-signal read bundles for agent work.
- `recall signal and text scoring` (`concept://recall_signal_and_text_scoring`): prism-memory modules that compute anchor overlap, recency, trust, token/substring/embedding text signals, and final scored recall ordering across memory entries.
- `repo publication guards` (`concept://repo_publication_guards`): prism-core publication and validation modules that govern which memory, concept, relation, and plan artifacts are durable enough to publish into repo knowledge.
- `resource schemas and host-resource serving` (`concept://resource_schemas_and_host_resource_serving`): prism-mcp modules that publish resource payload schemas, capability resources, and host-backed resource reads over the MCP surface.
- `Rust tree-sitter adapter pipeline` (`concept://rust_tree_sitter_adapter_pipeline`): Rust adapter modules that parse Rust source with tree-sitter and map syntax and path-resolution into PRISM nodes, fingerprints, unresolved calls, impls, and imports.
- `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`): Memory modules that derive structural features, manage structural recall, and integrate semantic matching backends for deeper retrieval beyond simple text scoring.
- `semantic memory backend runtime` (`concept://semantic_memory_backend_runtime`): prism-memory semantic modules that configure semantic backends, compute local and remote semantic signals, and rank semantic memory matches across embeddings and lexical bridges.
- `semantic projection indexes` (`concept://semantic_projection_indexes`): prism-projections modules that materialize derived concept, relation, intent, and projection indexes from lower-level events and snapshots.
- `server surface and runtime health views` (`concept://server_surface_and_runtime_health_views`): prism-mcp modules that expose top-level server capabilities, feature flags, runtime status, and diagnostics for the live server surface.
- `session and episodic memory store` (`concept://session_and_episodic_memory_store`): prism-memory modules that assemble the session-scoped memory composite, persist episodic entries, and snapshot memory state across workspace reloads.
- `session context and hosted mutation runtime` (`concept://session_state_and_mutation_runtime`): prism-mcp modules that hold per-session runtime context, execute authenticated hosted mutations, and shape follow-up runtime/read-model views such as task context and heartbeat guidance.
- `SQLite and graph persistence` (`concept://sqlite_and_graph_persistence`): prism-store modules that own the SQLite backend, graph snapshots, and persist batches for the durable structural store behind PRISM sessions.
- `structural IR and identity model` (`concept://structural_ir`): Authoritative semantic schema for nodes, edges, anchors, events, and stable identities that every higher PRISM layer depends on.
- `structural memory feature model` (`concept://structural_memory_feature_model`): prism-memory modules that derive structural tags and rule features from memory entries and use them to rank structural recall results.
- `structured config value adapters` (`concept://structured_config_value_adapters`): JSON, TOML, and YAML adapter modules that parse structured configuration documents into document/key trees, record stable shape fingerprints, and mine intent targets from configuration values.
- `symbol, source, and relation queries` (`concept://symbol_source_and_relation_queries`): prism-query modules that resolve symbols, source excerpts, relation views, and read-oriented helper logic for direct code navigation.
- `task-brief and coordination summary views` (`concept://task_brief_and_coordination_summary_views`): Compact-tool coordination briefing path that condenses plan/task status, blockers, claims, conflicts, recent outcomes, likely validations, and next reads into bounded coordination summaries.
- `validation and dogfooding loop` (`concept://validation_and_dogfooding`): Cross-cutting validation pipeline for structural truth, lineage, memory anchoring, projections, coordination, and MCP/query behavior, plus direct dogfooding feedback capture.
- `validation feedback and metrics loop` (`concept://validation_feedback_and_metrics_loop`): Runtime and spec elements that capture validation feedback, materialize validation checks, and define the metrics/dashboard loop used to calibrate PRISM over time.
- `validation policy and release gates` (`concept://validation_policy_and_release_gates`): Core validation spec headings that define layered validation principles, the evaluation pipeline, the PRISM-first validation plan, and risk-tiered release gates.
- `validation_pipeline` (`concept://validation_pipeline`): The cross-crate path that derives likely validations from impact, promotes them into MCP views, and compresses them into compact follow-through.
- `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`): prism-core orchestration that watches the workspace, runs parse and resolution pipelines, reanchors history and memory, and maintains the live PRISM session state.
- `workspace session refresh runtime` (`concept://workspace_session_refresh_runtime`): prism-core modules that maintain the live workspace session, track filesystem dirtiness, and run refresh cycles through file watching and guarded session state.
- `workspace session runtime` (`concept://workspace_session_runtime`): Session-oriented prism-core modules that hydrate WorkspaceSession state, refresh memory snapshots, and record patch outcomes around the live workspace.

## agent inference and curation

Handle: `concept://agent_inference_and_curation`

Inference and curator stack that records inferred edges, synthesizes curator output, and promotes reviewed knowledge into persistent PRISM state.

Aliases: `curation pipeline`, `inference layer`, `curator stack`

### Core Members

- `prism_agent`
- `prism_curator`
- `prism_core`
- `prism_projections`

### Related Concepts

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- has part: `curator execution flow` (`concept://curator_execution_flow`)
- has part: `inferred edge runtime and session store` (`concept://inferred_edge_runtime_and_session_store`)
- part of: `PRISM architecture` (`concept://prism_architecture`)

### Evidence

- prism-agent defines inferred-edge identifiers, scopes, and storage semantics for agent-discovered relationships.
- prism-curator synthesizes curator runs while prism-core and prism-mcp integrate reviewed promotion of edges, concepts, and memories.

### Risk Hint

- Incorrect inference or curator promotion can publish misleading durable knowledge, so this layer depends on strong review and validation boundaries.

## anchor, change, and lineage IR

Handle: `concept://anchor_change_and_lineage_ir`

prism-ir modules that connect source anchors, observed changes, event metadata, and lineage history so structural identities can survive edits over time.

Aliases: `temporal ir`, `lineage ir`

### Core Members

- `prism_ir::anchor`
- `prism_ir::change`
- `prism_ir::events`
- `prism_ir::history`

### Related Concepts

- part of: `structural IR and identity model` (`concept://structural_ir`)

### Evidence

- `prism-ir/src/lib.rs` re-exports anchor, change, events, and history together as the temporal/traceability portion of the shared schema.
- These modules are what let PRISM relate source positions, change observations, and lineage evidence across refreshes.

### Risk Hint

- Drift here breaks reanchoring, event interpretation, and lineage-backed reasoning across the repo.

## CLI command and parse surface

Handle: `concept://cli_command_and_parse_surface`

prism-cli modules that define CLI arguments, parse user input, and dispatch commands while keeping the binary entrypoint thin.

Aliases: `cli dispatch`, `cli command path`

### Core Members

- `prism_cli::cli`
- `prism_cli::commands`
- `prism_cli::parsing`

### Related Concepts

- depends on: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)
- part of: `CLI surface` (`concept://cli_surface`)

### Evidence

- `crates/prism-cli/src/main.rs` declares `cli`, `commands`, and `parsing` as the input/dispatch half of the CLI surface.
- The main function only parses args and forwards into `commands::run`, which makes this a stable sub-boundary for operator command handling.

### Risk Hint

- If command parsing drifts, the CLI becomes an unreliable wrapper around otherwise healthy core behavior.

## CLI runtime and MCP control

Handle: `concept://cli_runtime_and_mcp_control`

prism-cli modules that manage operator-facing runtime behavior, MCP daemon control, and terminal display concerns after command dispatch is resolved.

Aliases: `cli runtime control`, `cli mcp lifecycle`

### Core Members

- `prism_cli::runtime`
- `prism_cli::mcp`
- `prism_cli::display`

### Related Concepts

- depends on: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`)
- part of: `CLI surface` (`concept://cli_surface`)

### Evidence

- `crates/prism-cli/src/main.rs` separates `runtime`, `mcp`, and `display` from parsing/dispatch modules.
- This is the execution/output half of the CLI surface, especially for MCP lifecycle operations and operator-facing presentation.

### Risk Hint

- Breakage here makes the CLI feel flaky even when lower-level MCP and core crates are behaving correctly.

## CLI surface

Handle: `concept://cli_surface`

Local operator surface for indexing, querying, and MCP lifecycle control, built as a thin binary over prism-core and related crates.

Aliases: `prism cli`, `operator cli`, `command surface`

### Core Members

- `prism_cli::main`
- `prism_core`
- `prism_query`
- `prism_store`

### Related Concepts

- depends on: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)
- has part: `CLI command and parse surface` (`concept://cli_command_and_parse_surface`)
- has part: `CLI runtime and MCP control` (`concept://cli_runtime_and_mcp_control`)
- part of: `PRISM architecture` (`concept://prism_architecture`)

### Evidence

- prism-cli depends on prism-core, prism-ir, prism-memory, prism-query, and prism-store and keeps most logic in cli, commands, mcp, parsing, display, and runtime modules.
- The binary entrypoint stays thin and dispatches into command modules rather than owning core semantics.

### Risk Hint

- Because the CLI is intentionally thin, architecture drift here usually signals deeper changes in core crate contracts.

## co-change and validation projection indexes

Handle: `concept://cochange_and_validation_projection_indexes`

prism-projections modules that derive co-change neighbors, validation deltas, and projection snapshots used by higher query and compact-tool surfaces.

Aliases: `projection indexes`, `validation projections`

### Core Members

- `prism_projections::projections`
- `prism_projections::common`
- `prism_projections::types`

### Related Concepts

- depends on: `persistence and history layer` (`concept://persistence_and_history`)
- part of: `semantic projection indexes` (`concept://semantic_projection_indexes`)

### Evidence

- `crates/prism-projections/src/lib.rs` exposes `ProjectionIndex`, co-change delta generation, validation delta generation, and projection snapshots from the `projections` path.
- This is the derived-index side of the projection layer, distinct from concept packet materialization.

### Risk Hint

- Errors here distort blast radius, validation checks, and projection snapshots that many later reads trust.

## code language adapters

Handle: `concept://code_language_adapters`

Rust and Python adapter modules that parse executable source code into PRISM IR using parser, syntax, and path-resolution helpers.

Aliases: `code adapters`, `source language adapters`

### Core Members

- `prism_lang_rust::parser`
- `prism_lang_rust::syntax`
- `prism_lang_rust::paths`
- `prism_lang_python::parser`
- `prism_lang_python::syntax`

### Supporting Members

- `prism_lang_python::paths`

### Related Concepts

- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `Python tree-sitter adapter pipeline` (`concept://python_tree_sitter_adapter_pipeline`)
- has part: `Rust tree-sitter adapter pipeline` (`concept://rust_tree_sitter_adapter_pipeline`)
- part of: `language adapter family` (`concept://language_adapter_family`)

### Evidence

- Rust and Python crates are the true code-language adapters in the workspace and split their behavior into parser, syntax, and path helpers.
- These modules are distinct from the document/config adapters because they model executable code structure and references.

### Risk Hint

- Bugs here corrupt the primary code cognition path, not just secondary document/config understanding.

## compact discovery and opening flow

Handle: `concept://compact_discovery_and_opening`

compact-tool modules that rank likely targets, normalize text fragments, open bounded slices, and assemble small worksets for the default agent path.

Aliases: `compact locate/open path`, `compact navigation flow`

### Core Members

- `prism_mcp::compact_tools::locate`
- `prism_mcp::compact_tools::open`
- `prism_mcp::compact_tools::workset`
- `prism_mcp::compact_tools::text_fragments`

### Supporting Members

- `prism_mcp::compact_tools::suggested_actions`

### Related Concepts

- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- has part: `locate ranking and text-candidate flow` (`concept://locate_ranking_and_text_candidate_flow`)
- has part: `open and workset follow-through` (`concept://open_and_workset_followthrough`)
- part of: `compact tool surface` (`concept://compact_tool_surface`)

### Evidence

- `prism-mcp/src/compact_tools.rs` declares locate, open, workset, text_fragments, and suggested_actions as one orchestration cluster inside the compact surface.
- This is the discovery/navigation half of the compression layer before deeper concept or task decoding begins.

### Risk Hint

- Ranking or bounded-open regressions here destroy the default compact path even if raw query APIs remain correct.

## compact expansion and concept views

Handle: `concept://compact_expansion_and_concept_views`

compact-tool modules that decode concept packets, expand bounded diagnostics, and summarize coordination tasks without dropping to raw query output.

Aliases: `compact expand path`, `compact concept/task views`

### Core Members

- `prism_mcp::compact_tools::expand`
- `prism_mcp::compact_tools::concept`
- `prism_mcp::compact_tools::task_brief`

### Related Concepts

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- has part: `concept and expand decode runtime` (`concept://concept_and_expand_decode_runtime`)
- has part: `task-brief and coordination summary views` (`concept://task_brief_and_coordination_summary_views`)
- often used with: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `compact tool surface` (`concept://compact_tool_surface`)

### Evidence

- `prism-mcp/src/compact_tools.rs` groups expand, concept, and task_brief beside the discovery modules as the deeper decode surface of the compact layer.
- These modules are what let later agents stay on concept/task handles instead of falling back to raw repo reads.

### Risk Hint

- If these views drift, handle reuse stops compressing architecture work and agents revert to code rereads.

## compact tool surface

Handle: `concept://compact_tool_surface`

Staged locate/open/gather/workset/expand/concept/task-brief tool layer inside prism-mcp that serves as the intended default agent path above raw prism_query.

Aliases: `compact tools`, `compression layer`, `default agent path`

### Core Members

- `prism_mcp::compact_tools`
- `prism_mcp`
- `prism::document::docs::AGENT_COMPRESSION_LAYER_md::target_default_agent_path`
- `prism::document::docs::AGENT_COMPRESSION_LAYER_md::compact_primary_tools`

### Related Concepts

- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- has part: `compact discovery and opening flow` (`concept://compact_discovery_and_opening`)
- has part: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- specializes: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- The compact tool modules live inside prism-mcp and define a staged default path around locate, gather, open, workset, expand, concept, and task-brief tools.
- docs/AGENT_COMPRESSION_LAYER.md explicitly positions this layer as the future default over direct prism_query use.

### Risk Hint

- If ranking or handle-follow-through degrades here, agents lose most of the intended compression value even when the raw query surface still works.

## compact_tools

Handle: `concept://compact_tools`

The staged agent-facing surface for locate, open, workset, and expand, plus the ranking and next-action rules that keep compact follow-through bounded.

Aliases: `compact tools`, `compact surface`, `locate/open/workset/expand`, `staged agent ABI`

### Core Members

- `prism_mcp::compact_tools::locate::QueryHost::compact_locate`
- `prism_mcp::compact_tools::open::QueryHost::compact_open`
- `prism_mcp::compact_tools::workset::QueryHost::compact_workset`
- `prism_mcp::compact_tools::expand::QueryHost::compact_expand`

### Supporting Members

- `prism_mcp::compact_tools::locate::rank_locate_candidate`
- `prism_mcp::compact_tools::open::compact_open_related_handles`
- `prism_mcp::compact_tools::workset::compact_workset_next_action`
- `prism_mcp::compact_tools::expand::compact_expand_next_action`

### Likely Tests

- `prism_mcp::tests::compact_locate_prefers_identifier_matches_over_test_helpers`
- `prism_mcp::tests::compact_open_returns_compact_related_handles`
- `prism_mcp::tests::compact_workset_for_spec_targets_prefers_owner_paths_over_text_adjacent_helpers`
- `prism_mcp::tests::compact_expand_perception_lenses_surface_impact_timeline_and_memory`

### Evidence

- QueryHost::compact_locate, compact_open, compact_workset, and compact_expand are the staged MCP entrypoints agents actually reuse together.
- rank_locate_candidate plus compact_open_related_handles and the compact next-action helpers shape first-hop ranking and bounded follow-through, not just transport plumbing.
- The compact-tools test cluster exercises locate, open, workset, and expand as one product surface rather than four unrelated helpers.

### Risk Hint

- Compact-tool regressions usually show up first as weak ranking, noisy follow-through, or over-budget payloads rather than hard failures.

## composite memory routing and entry storage

Handle: `concept://composite_memory_routing_and_entry_storage`

prism-memory modules that deduplicate recalls across modules, route stores by memory kind, and persist per-module entry snapshots with anchor indexes.

Aliases: `memory module routing`, `entry store layer`

### Core Members

- `prism_memory::composite`
- `prism_memory::entry_store`
- `prism_memory::types`

### Related Concepts

- part of: `memory recall and scoring` (`concept://memory_recall_and_scoring`)

### Evidence

- `composite.rs` merges module-specific recalls and routes stores by memory kind, while `entry_store.rs` provides the snapshot and anchor-index backing store used by concrete memory modules.
- This is the storage-and-routing half of memory recall orchestration rather than the scoring half.

### Risk Hint

- Breakage here causes duplicate, dropped, or misrouted memory entries even if individual scoring modules remain correct.

## concept and expand decode runtime

Handle: `concept://concept_and_expand_decode_runtime`

Compact-tool decode path that resolves concept packets and turns handles into bounded diagnostics, lineage, neighbor, validation, memory, and drift views without dropping to raw query output.

Aliases: `compact decode runtime`, `concept and expand views`

### Core Members

- `prism_mcp::compact_tools::concept`
- `prism_mcp::compact_tools::expand`

### Related Concepts

- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- part of: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`)

### Evidence

- concept.rs resolves concept packets, alternates, decode lenses, binding metadata, recent patches, memories, and validation recipes into compact concept views.
- expand.rs turns handles into bounded diagnostic, lineage, neighbor, validation, and memory expansions and reuses structured-target previews where needed.

## concept and publication pipeline

Handle: `concept://concept_and_publication_pipeline`

Event-sourced concept lifecycle that promotes repo-quality concepts and concept relations from core events into projected retrieval packets and MCP-facing mutation surfaces.

Aliases: `concept pipeline`, `concept publication`, `repo knowledge publication`

### Core Members

- `prism_core`
- `prism_projections`
- `prism_query`
- `prism_mcp`

### Supporting Members

- `prism::document::docs::CONCEPT_MAINTENANCE_md::3_1_concepts_are_evidence_backed_semantic_packets`
- `prism::document::docs::CONCEPT_MAINTENANCE_md::15_integration_with_concept_to_concept_edges`

### Related Concepts

- depended on by: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- depended on by: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- depended on by: `agent inference and curation` (`concept://agent_inference_and_curation`)
- depended on by: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`)
- depended on by: `concept, relation, and intent projections` (`concept://concept_relation_and_intent_projections`)
- depended on by: `curator execution flow` (`concept://curator_execution_flow`)
- depended on by: `resource schemas and host-resource serving` (`concept://resource_schemas_and_host_resource_serving`)
- depended on by: `semantic projection indexes` (`concept://semantic_projection_indexes`)
- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- has part: `concept and relation event streams` (`concept://concept_and_relation_event_streams`)
- has part: `plan and repo-layout publication` (`concept://plan_and_repo_layout_publication`)
- has part: `published knowledge and memory event logs` (`concept://published_knowledge_and_memory_event_logs`)
- has part: `repo publication guards` (`concept://repo_publication_guards`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-core contains concept_events, concept_relation_events, published_knowledge, and curator support for durable knowledge publication.
- prism-projections materializes curated concept packets and concept relations, while prism-mcp exposes concept mutation, relation mutation, and concept resolution surfaces.

### Risk Hint

- Overbroad promotion or stale bindings here reduce the compression value of the entire repo concept layer.

## concept and relation event streams

Handle: `concept://concept_and_relation_event_streams`

prism-core modules that append and replay durable concept and concept-relation event streams before projections materialize them into queryable packets.

Aliases: `concept event streams`, `relation event streams`

### Core Members

- `prism_core::concept_events`
- `prism_core::concept_relation_events`

### Related Concepts

- often used with: `semantic projection indexes` (`concept://semantic_projection_indexes`)
- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)

### Evidence

- `crates/prism-core/src/lib.rs` declares `concept_events` and `concept_relation_events` as dedicated publication modules beside other core concerns.
- These modules are the event-sourced write path that precedes projected concept packets and relation graphs.

### Risk Hint

- If these streams drift, projection refresh can materialize stale or malformed concept knowledge even when reads still look plausible.

## concept, relation, and intent projections

Handle: `concept://concept_relation_and_intent_projections`

prism-projections modules that materialize curated concept packets, concept-to-concept relation packets, and intent indexes from lower-level events.

Aliases: `concept projections`, `intent projections`

### Core Members

- `prism_projections::concepts`
- `prism_projections::concept_relations`
- `prism_projections::intent`
- `prism_projections::types`

### Related Concepts

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- part of: `semantic projection indexes` (`concept://semantic_projection_indexes`)

### Evidence

- `crates/prism-projections/src/lib.rs` groups `concepts`, `concept_relations`, `intent`, and `types` as the packet-building side of projection materialization.
- These modules are the durable semantic projection path that turns concept and intent events into compact queryable packets.

### Risk Hint

- If these projections drift, later concept-centric reasoning looks coherent while binding the wrong semantic packet shapes.

## coordination and plan runtime

Handle: `concept://coordination_and_plan_runtime`

Shared plan, task, claim, artifact, and blocker subsystem that models active intent and exposes runtime coordination views across PRISM.

Aliases: `coordination system`, `plan runtime`, `shared work model`

### Core Members

- `prism_coordination`
- `prism_coordination::state::CoordinationStore`
- `prism_ir`
- `prism_query`

### Supporting Members

- `prism::document::docs::PRISM_FIRST_CLASS_PLANS_SPEC_md::10_integration_with_concepts_memories_outcomes_impact_and_validation`
- `prism::document::docs::PRISM_VAULT_HARBOR_md::11_coordination_across_the_family`

### Related Concepts

- depended on by: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- depended on by: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- depended on by: `coordination and plan-runtime queries` (`concept://coordination_and_plan_runtime_queries`)
- depended on by: `dashboard events and read models` (`concept://dashboard_events_and_read_models`)
- depended on by: `dashboard surface` (`concept://dashboard_surface`)
- depended on by: `plan completion and insight queries` (`concept://plan_completion_and_insight_queries`)
- depended on by: `projection and query layer` (`concept://projection_and_query_layer`)
- depended on by: `query coordination and plan views` (`concept://query_coordination_and_plan_views`)
- depended on by: `session context and hosted mutation runtime` (`concept://session_state_and_mutation_runtime`)
- depended on by: `task-brief and coordination summary views` (`concept://task_brief_and_coordination_summary_views`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `coordination operations and policy` (`concept://coordination_operations_and_policy`)
- has part: `coordination state model` (`concept://coordination_state_model`)
- often used with: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`)
- often used with: `plan and coordination IR` (`concept://plan_and_coordination_ir`)
- often used with: `plan and repo-layout publication` (`concept://plan_and_repo_layout_publication`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-coordination owns blockers, mutations, queries, runtime, state, and type modules around plans, tasks, claims, and artifacts.
- prism-query and prism-js depend on coordination types and runtime state to expose plan and task surfaces to agents.

### Risk Hint

- Invariants span IR identities, runtime state, and query projections, so partial changes here create subtle coordination inconsistencies.

## coordination and plan-runtime queries

Handle: `concept://coordination_and_plan_runtime_queries`

prism-query modules that expose coordination snapshots, plan runtime overlays, intent views, and related query types over live coordination state.

Aliases: `coordination queries`, `plan runtime queries`

### Core Members

- `prism_query::coordination`
- `prism_query::plan_runtime`
- `prism_query::intent`
- `prism_query::types`

### Related Concepts

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `query coordination and plan views` (`concept://query_coordination_and_plan_views`)

### Evidence

- `crates/prism-query/src/lib.rs` groups `coordination`, `plan_runtime`, `intent`, and shared `types` as the runtime planning side of the query crate.
- These modules turn coordination state into programmable views rather than raw store objects.

### Risk Hint

- Drift here misrepresents task, claim, or runtime plan state to every higher surface that depends on query views.

## coordination mutation, lease, and policy helpers

Handle: `concept://coordination_mutation_and_policy_helpers`

prism-coordination modules that apply coordination mutations and enforce lease lifecycle, resume/reclaim, heartbeat, rejection recording, and shared policy/conflict helpers.

Aliases: `coordination mutations`, `lease policy helper layer`, `coordination lease helpers`

### Core Members

- `prism_coordination::mutations::enforce_task_lease_for_standard_mutation`
- `prism_coordination::lease::refresh_task_lease`
- `prism_coordination::lease::task_heartbeat_due_state`

### Supporting Members

- `prism_coordination::lease::refresh_claim_lease`

### Related Concepts

- depends on: `coordination state model` (`concept://coordination_state_model`)
- depends on: `principal identity and mutation provenance` (`concept://principal_identity_and_mutation_provenance`)
- part of: `coordination operations and policy` (`concept://coordination_operations_and_policy`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `mutations.rs` now applies authenticated coordination transitions together with lease-aware rejection, resume, reclaim, and heartbeat semantics.
- `lease.rs` and shared helpers centralize holder comparison, due-state evaluation, lease refresh, and policy evidence used by coordination mutations.

### Risk Hint

- If mutation or lease helpers drift, stale or expired work can be renewed incorrectly, valid resumptions can be rejected, and ownership can be attributed to the wrong principal.

## coordination operations and policy

Handle: `concept://coordination_operations_and_policy`

Coordination modules that implement query paths, mutations, blocker calculation, and helper logic over the shared coordination state model.

Aliases: `coordination operations`, `policy and blockers`

### Core Members

- `prism_coordination::queries`
- `prism_coordination::mutations`
- `prism_coordination::blockers`
- `prism_coordination::helpers`

### Supporting Members

- `prism::document::docs::PRISM_FIRST_CLASS_PLANS_SPEC_md::10_integration_with_concepts_memories_outcomes_impact_and_validation`

### Related Concepts

- depends on: `coordination state model` (`concept://coordination_state_model`)
- has part: `coordination mutation, lease, and policy helpers` (`concept://coordination_mutation_and_policy_helpers`)
- has part: `coordination query and blocker operations` (`concept://coordination_query_and_blocker_operations`)
- part of: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- queries.rs, mutations.rs, blockers.rs, and helpers.rs implement the behavior layer over CoordinationStore and related types.
- This is where plan/task workflow policy becomes executable instead of remaining a passive schema.

### Risk Hint

- Behavioral regressions here can violate plan and claim invariants without any change to the underlying state structs.

## coordination query and blocker operations

Handle: `concept://coordination_query_and_blocker_operations`

prism-coordination modules that answer readiness, conflict, pending-review, and blocker questions over coordination state.

Aliases: `coordination queries`, `blocker operations`

### Core Members

- `prism_coordination::queries`
- `prism_coordination::blockers`

### Related Concepts

- depends on: `coordination state model` (`concept://coordination_state_model`)
- part of: `coordination operations and policy` (`concept://coordination_operations_and_policy`)

### Evidence

- `queries.rs` serves ready-task, conflict, pending-review, and blocker reads, while `blockers.rs` computes readiness, completion, and policy blockers.
- These modules are the read and gating half of coordination operations.

### Risk Hint

- If these operations drift, agents can be blocked by the wrong tasks or miss real coordination conflicts entirely.

## coordination state model

Handle: `concept://coordination_state_model`

Core coordination modules that define state, types, runtime overlays, and compatibility projections for plans, tasks, claims, and artifacts.

Aliases: `coordination state`, `plan/task model`

### Core Members

- `prism_coordination::state`
- `prism_coordination::types`
- `prism_coordination::runtime`
- `prism_coordination::compat`

### Supporting Members

- `prism_coordination::state::CoordinationStore`

### Related Concepts

- depended on by: `coordination mutation, lease, and policy helpers` (`concept://coordination_mutation_and_policy_helpers`)
- depended on by: `coordination operations and policy` (`concept://coordination_operations_and_policy`)
- depended on by: `coordination query and blocker operations` (`concept://coordination_query_and_blocker_operations`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `coordination store and domain types` (`concept://coordination_store_and_domain_types`)
- has part: `plan-graph compatibility and runtime overlays` (`concept://plan_graph_compatibility_and_runtime_overlays`)
- part of: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)

### Evidence

- state.rs, types.rs, runtime.rs, and compat.rs define the durable coordination model plus the overlays exported to other layers.
- This slice is the shared data model beneath both query and mutation behavior in prism-coordination.

### Risk Hint

- Schema drift here propagates into every coordination read/write path and into MCP/dashboard views built on top of them.

## coordination store and domain types

Handle: `concept://coordination_store_and_domain_types`

prism-coordination modules that hold the canonical in-memory coordination snapshot and define plans, tasks, claims, artifacts, reviews, and policy records.

Aliases: `coordination domain model`, `coordination store`

### Core Members

- `prism_coordination::state`
- `prism_coordination::types`

### Related Concepts

- part of: `coordination state model` (`concept://coordination_state_model`)

### Evidence

- `state.rs` stores the canonical coordination maps and snapshot conversion, while `types.rs` defines the durable domain objects and policy records the runtime operates on.
- This is the state-bearing half of the coordination subsystem.

### Risk Hint

- If these types or store semantics drift, every coordination read and mutation can become internally inconsistent.

## core indexing pipeline

Handle: `concept://core_indexing_pipeline`

prism-core modules that parse files, resolve structure, reanchor temporal identity, and watch workspace changes to build the live PRISM graph.

Aliases: `indexing pipeline`, `parse and resolution pipeline`

### Core Members

- `prism_core::indexer`
- `prism_core::parse_pipeline`
- `prism_core::resolution`
- `prism_core::reanchor`
- `prism_core::watch`

### Supporting Members

- `prism_core::indexer::WorkspaceIndexer`

### Related Concepts

- depends on: `language adapter family` (`concept://language_adapter_family`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `indexer orchestration and snapshot loading` (`concept://indexer_orchestration_and_snapshot_loading`)
- has part: `parse, resolution, and reanchor flow` (`concept://parse_resolution_and_reanchor_flow`)
- part of: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)

### Evidence

- prism-core groups indexer, parse_pipeline, resolution, reanchor, and watch as the modules that build and refresh structural truth from workspace files.
- This slice is the concrete implementation behind the higher-level workspace indexing and refresh concept.

### Risk Hint

- Breakage here distorts graph truth early and poisons later projections, memory anchoring, and query quality.

## curator backend execution and prompting

Handle: `concept://curator_backend_execution_and_prompting`

Curator modules that bound curator context, render backend schemas/config, and execute the Codex-backed curator process under explicit sandbox, approval, and local-provider settings.

Aliases: `curator backend runtime`, `curator prompting`

### Core Members

- `prism_curator::codex`
- `prism_curator::support`

### Related Concepts

- part of: `curator execution flow` (`concept://curator_execution_flow`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- codex.rs owns executable backend configuration and process launch behavior, while support.rs bounds context and renders the schema/config passed to the backend.
- Together they form the operational execution half of the curator subsystem, separate from proposal synthesis and type modeling.

## curator execution flow

Handle: `concept://curator_execution_flow`

prism-core and prism-curator modules that run curator jobs, prepare support context, and transform reviewable proposals into publishable outcomes.

Aliases: `curator flow`, `curation execution`

### Core Members

- `prism_core::curator`
- `prism_core::curator_support`
- `prism_curator`

### Supporting Members

- `prism_core::layout`
- `prism_core::util`

### Related Concepts

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- has part: `curator backend execution and prompting` (`concept://curator_backend_execution_and_prompting`)
- has part: `curator rule synthesis and proposal types` (`concept://curator_rule_synthesis_and_proposal_types`)
- part of: `agent inference and curation` (`concept://agent_inference_and_curation`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-core’s curator and curator_support modules form the repo-local execution bridge to the standalone prism-curator crate.
- This slice is distinct from publication guards because it concerns generating and reviewing proposals before they are published.

### Risk Hint

- If this flow drifts, PRISM may produce curator outputs that look grounded but are poorly scoped, weakly justified, or hard to publish safely.

## curator rule synthesis and proposal types

Handle: `concept://curator_rule_synthesis_and_proposal_types`

Curator modules that define curator jobs, bounded graph/outcome/memory/projection context, proposal payload types, and rule-based synthesis/merge of edge, memory, concept, and validation proposals.

Aliases: `curator synthesis`, `curator proposal model`

### Core Members

- `prism_curator::synthesis`
- `prism_curator::types`

### Related Concepts

- depends on: `memory system` (`concept://memory_system`)
- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- part of: `curator execution flow` (`concept://curator_execution_flow`)

### Evidence

- synthesis.rs contains the repeated-failure, migration, co-change, hotspot, and episodic-promotion rules plus merge logic for curator runs.
- types.rs defines the curator job, context slices, budgets, and proposal payloads that synthesis emits and later review/apply paths consume.

## daemon, process, and proxy lifecycle

Handle: `concept://daemon_process_and_proxy_lifecycle`

prism-mcp modules that spawn, proxy, and supervise the daemonized server process and its transport-level lifecycle.

Aliases: `daemon lifecycle`, `proxy lifecycle`

### Core Members

- `prism_mcp::daemon_mode`
- `prism_mcp::process_lifecycle`
- `prism_mcp::proxy_server`
- `prism_mcp::logging`

### Related Concepts

- part of: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`)

### Evidence

- `crates/prism-mcp/src/lib.rs` keeps `daemon_mode`, `process_lifecycle`, `proxy_server`, and `logging` together on the server-management side of the runtime.
- These modules own the long-lived process lifecycle rather than query or mutation semantics.

### Risk Hint

- Lifecycle bugs here make the server unreachable or misleadingly healthy even when higher semantic layers are correct.

## dashboard events and read models

Handle: `concept://dashboard_events_and_read_models`

dashboard modules inside prism-mcp that derive read models, shape event streams, and keep the dashboard synchronized with runtime truth.

Aliases: `dashboard read layer`, `dashboard event stream`

### Core Members

- `prism_mcp::dashboard_events`
- `prism_mcp::dashboard_read_models`

### Related Concepts

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `dashboard surface` (`concept://dashboard_surface`)

### Evidence

- The dashboard concept already centers on `dashboard_events` and `dashboard_read_models` as the live data side of the UI.
- These modules are the state-derivation half of the dashboard, distinct from routing/assets/types.

### Risk Hint

- Stale or mis-shaped read models here can make operators trust the wrong runtime picture.

## dashboard routing and assets

Handle: `concept://dashboard_routing_and_assets`

dashboard modules inside prism-mcp that define browser-facing routes, static assets, and typed payload boundaries for the PRISM dashboard surface.

Aliases: `dashboard router`, `dashboard transport layer`

### Core Members

- `prism_mcp::dashboard_router`
- `prism_mcp::dashboard_assets`
- `prism_mcp::dashboard_types`

### Related Concepts

- depends on: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- part of: `dashboard surface` (`concept://dashboard_surface`)

### Evidence

- `crates/prism-mcp/src/lib.rs` declares `dashboard_router`, `dashboard_assets`, and `dashboard_types` as dedicated dashboard modules beside the generic MCP surface.
- These modules form the transport/presentation boundary that exposes dashboard functionality to the browser.

### Risk Hint

- If this layer drifts, the dashboard can fail at the HTTP/UI boundary before read models or events are even consulted.

## dashboard surface

Handle: `concept://dashboard_surface`

First-class observability and UI surface inside prism-mcp that publishes dashboard read models, event streams, and router endpoints instead of reusing the raw MCP tool catalog.

Aliases: `dashboard`, `observability ui`, `live runtime dashboard`

### Core Members

- `prism_mcp::dashboard_router`
- `prism_mcp::dashboard_read_models`
- `prism_mcp::dashboard_events`
- `prism_mcp`

### Supporting Members

- `prism::document::docs::DASHBOARD_IMPLEMENTATION_SPEC_md::backend_architecture`
- `prism::document::docs::DASHBOARD_IMPLEMENTATION_SPEC_md::product_decision_summary`

### Related Concepts

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- has part: `dashboard events and read models` (`concept://dashboard_events_and_read_models`)
- has part: `dashboard routing and assets` (`concept://dashboard_routing_and_assets`)
- often used with: `validation feedback and metrics loop` (`concept://validation_feedback_and_metrics_loop`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- specializes: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- Dashboard modules live inside prism-mcp alongside dedicated router, read-model, event, asset, and type layers.
- The dashboard spec explicitly separates browser-facing observability APIs from the normal MCP tool catalog.

### Risk Hint

- If dashboard read models drift from runtime or mutation truth, the UI can mislead operators while the backend is unhealthy.

## document and config adapters

Handle: `concept://document_and_config_adapters`

Markdown, JSON, TOML, and YAML adapter modules that map documents and configuration files into PRISM IR and intent surfaces.

Aliases: `document adapters`, `config adapters`

### Core Members

- `prism_lang_markdown`
- `prism_lang_json::adapter`
- `prism_lang_toml`
- `prism_lang_yaml`

### Related Concepts

- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `Markdown heading and intent adapter` (`concept://markdown_heading_and_intent_adapter`)
- has part: `structured config value adapters` (`concept://structured_config_value_adapters`)
- part of: `language adapter family` (`concept://language_adapter_family`)

### Evidence

- Markdown, JSON, TOML, and YAML adapters capture repo-native docs and config artifacts rather than executable code.
- These adapters are important because PRISM’s architecture and policy knowledge lives partly in markdown and structured config, not only source code.

### Risk Hint

- Quality issues here degrade architecture/spec grounding and config search even when code parsing still looks healthy.

## facade_modularity_rule

Handle: `concept://facade_modularity_rule`

Repo-wide architectural rule that keeps `main.rs` and `lib.rs` as thin facades and pushes substantive logic into dedicated modules.

Aliases: `modularity rule`, `facade-only entrypoints`, `lib/main facade rule`

### Core Members

- `prism::document::AGENTS_md::architectural_rule`
- `prism::document::AGENTS_md::modularity_expectations`

### Evidence

- AGENTS.md defines `main.rs` and `lib.rs` as facade-only files and forbids core logic from accumulating there.
- The same document requires narrowly scoped modules, explicit ownership, and refactoring away from mixed-purpose entrypoint files.

### Risk Hint

- If this rule drifts, crate boundaries become harder to reason about and the repo architecture degrades into large facade files with hidden business logic.

## graph, identity, and parse IR

Handle: `concept://graph_identity_and_parse_ir`

Core prism-ir modules that define graph objects, stable ids, primitive spans/types, and parser-side unresolved payloads consumed by the rest of PRISM.

Aliases: `core graph ir`, `identity ir`

### Core Members

- `prism_ir::graph`
- `prism_ir::identity`
- `prism_ir::primitives`
- `prism_ir::parse`

### Related Concepts

- part of: `structural IR and identity model` (`concept://structural_ir`)

### Evidence

- `prism-ir/src/lib.rs` groups graph, identity, primitives, and parse as the reusable structural substrate re-exported across the repo.
- These modules collectively define the static node/edge/id/types and unresolved parser outputs that downstream adapters, query, and coordination consume.

### Risk Hint

- Mistakes here skew the meaning of node identity and parse payloads before higher layers can compensate.

## history snapshot and resolution

Handle: `concept://history_snapshot_and_resolution`

prism-history modules that resolve lineage, materialize snapshots, and expose the history store used to replay temporal identity over time.

Aliases: `history replay`, `lineage snapshotting`

### Core Members

- `prism_history::resolver`
- `prism_history::snapshot`
- `prism_history::store`

### Related Concepts

- depends on: `structural IR and identity model` (`concept://structural_ir`)
- part of: `persistence and history layer` (`concept://persistence_and_history`)

### Evidence

- `prism-history/src/lib.rs` is a tight split across resolver, snapshot, and store, which together define the temporal/history layer.
- These modules are the repo’s authoritative center for lineage replay and historical snapshot materialization.

### Risk Hint

- Bugs here cause lineage drift that later memory and query layers cannot easily diagnose.

## impact and outcome queries

Handle: `concept://impact_and_outcome_queries`

prism-query modules that compute blast radius, co-change, validation recipes, and outcome history context over projected and historical PRISM state.

Aliases: `impact queries`, `outcome queries`

### Core Members

- `prism_query::impact`
- `prism_query::outcomes`

### Related Concepts

- depends on: `memory system` (`concept://memory_system`)
- part of: `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`)

### Evidence

- `crates/prism-query/src/lib.rs` keeps `impact` and `outcomes` as a distinct query family beside symbol/source reads.
- These modules are the main read path for risk, validation recipe, and prior-outcome context.

### Risk Hint

- If these queries drift, PRISM can suggest persuasive but weak blast-radius and validation guidance.

## indexer orchestration and snapshot loading

Handle: `concept://indexer_orchestration_and_snapshot_loading`

prism-core modules that bootstrap workspace indexing, load prior snapshots and curated knowledge, and assemble the initial indexer/session state around the workspace graph.

Aliases: `indexer orchestration`, `snapshot loading`

### Core Members

- `prism_core::indexer`
- `prism_core::indexer_support`

### Related Concepts

- depends on: `persistence and history layer` (`concept://persistence_and_history`)
- part of: `core indexing pipeline` (`concept://core_indexing_pipeline`)

### Evidence

- `indexer.rs` loads graph/history/outcomes/projections and orchestrates workspace indexing, while `indexer_support.rs` provides shared helpers for collecting parses and building sessions.
- This is the orchestration and restore half of the indexing subsystem before parse/resolution work finishes.

### Risk Hint

- If this layer drifts, refreshes can start from the wrong persisted state even when lower parsing logic is still correct.

## inferred edge runtime and session store

Handle: `concept://inferred_edge_runtime_and_session_store`

Modules that define inferred-edge records and scopes, layer session-only versus persisted inference state, and route inferred-edge mutations and curator edge promotion through the live MCP runtime.

Aliases: `inference store`, `inferred edge runtime`

### Core Members

- `prism_agent`
- `prism_mcp::session_state`
- `prism_mcp::host_mutations`
- `prism_curator::types`

### Related Concepts

- depends on: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- part of: `agent inference and curation` (`concept://agent_inference_and_curation`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `prism-agent` defines `InferredEdgeScope`, `InferredEdgeRecord`, snapshots, and the underlying `InferenceStore`.
- `prism-mcp::session_state` overlays session-only and persisted inferred edges, while `host_mutations` implements `infer_edge` and `curator_promote_edge`, and `prism_curator::types` carries `CandidateEdge` proposals through curator jobs.

### Risk Hint

- If this path drifts, PRISM can present inferred structure that is mis-scoped, silently dropped, or promoted without the right task/session semantics.

## JavaScript API contract surface

Handle: `concept://javascript_api_contract_surface`

Shared type-contract modules that define the JavaScript-facing view schema between prism-js clients and prism-mcp resource/query payloads.

Aliases: `JS ABI types`, `JS view contracts`

### Core Members

- `prism_js::api_types`
- `prism_mcp::resource_schemas`
- `prism_mcp::query_types`

### Related Concepts

- depends on: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`)
- part of: `JavaScript query ABI` (`concept://javascript_query_abi`)

### Evidence

- `crates/prism-js/src/lib.rs` exposes `api_types` as the typed JS view contract surface.
- On the server side, `prism-mcp::resource_schemas` and `prism-mcp::query_types` define the payload shapes that the JS ABI must stay aligned with.

### Risk Hint

- Schema drift here breaks client/server compatibility without necessarily breaking lower-level query logic.

## JavaScript query ABI

Handle: `concept://javascript_query_abi`

prism-js modules that define the agent-facing TypeScript/JS API types, runtime prelude, and generated reference docs for programmable queries.

Aliases: `js query abi`, `typescript query abi`

### Core Members

- `prism_js::api_types`
- `prism_js::runtime`
- `prism_js::docs`

### Related Concepts

- has part: `JavaScript API contract surface` (`concept://javascript_api_contract_surface`)
- has part: `JavaScript runtime and reference bridge` (`concept://javascript_runtime_and_reference_bridge`)
- often used with: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- part of: `projection and query layer` (`concept://projection_and_query_layer`)

### Evidence

- prism-js separates API types, runtime prelude behavior, and API-reference documentation into a small ABI-focused crate.
- This is the stable JS-facing contract consumed by the MCP runtime and documented for agent use.

### Risk Hint

- ABI drift here can silently desynchronize docs, runtime behavior, and exposed query capabilities.

## JavaScript runtime and reference bridge

Handle: `concept://javascript_runtime_and_reference_bridge`

prism-js modules that expose the runtime prelude and generated API reference used when JavaScript or MCP-facing consumers need the PRISM query surface.

Aliases: `JS runtime bridge`, `JS docs bridge`

### Core Members

- `prism_js::runtime`
- `prism_js::docs`

### Related Concepts

- part of: `JavaScript query ABI` (`concept://javascript_query_abi`)

### Evidence

- `crates/prism-js/src/lib.rs` exposes `runtime` and `docs` as the execution/reference half of the JS-facing surface.
- These modules connect JS consumers to runtime helpers and the API reference rather than defining the typed view schema itself.

### Risk Hint

- If this bridge drifts, JS consumers may execute against stale examples or incomplete runtime helpers.

## language adapter family

Handle: `concept://language_adapter_family`

Language- and format-specific adapters that translate Rust, Python, Markdown, JSON, TOML, and YAML inputs into the shared parser and IR pipeline.

Aliases: `parser adapters`, `language adapters`, `format adapters`

### Core Members

- `prism_parser`
- `prism_lang_rust`
- `prism_lang_python`
- `prism_lang_markdown`
- `prism_lang_toml`

### Supporting Members

- `prism_lang_json`
- `prism_lang_yaml`

### Related Concepts

- depended on by: `core indexing pipeline` (`concept://core_indexing_pipeline`)
- depended on by: `parse, resolution, and reanchor flow` (`concept://parse_resolution_and_reanchor_flow`)
- depended on by: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `code language adapters` (`concept://code_language_adapters`)
- has part: `document and config adapters` (`concept://document_and_config_adapters`)
- has part: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`)
- part of: `PRISM architecture` (`concept://prism_architecture`)

### Evidence

- Every language crate depends on prism-parser and prism-ir to emit nodes, edges, fingerprints, and unresolved references through a shared adapter contract.
- The adapter family covers code and document formats: Rust, Python, Markdown, JSON, TOML, and YAML.

### Risk Hint

- Adapter drift causes structural truth failures early, before later layers can recover.

## locate ranking and text-candidate flow

Handle: `concept://locate_ranking_and_text_candidate_flow`

Compact-tool locate path that blends semantic search results, text-fragment candidates, exact identifier matches, glob filtering, and reranking heuristics into first-hop ranked targets.

Aliases: `compact locate ranking`, `locate heuristics`

### Core Members

- `prism_mcp::compact_tools::locate`
- `prism_mcp::compact_tools::text_fragments`

### Related Concepts

- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- part of: `compact discovery and opening flow` (`concept://compact_discovery_and_opening`)

### Evidence

- locate.rs orchestrates search, glob filtering, text-candidate expansion, identifier boosts, reranking, diagnostics, and optional preview generation.
- text_fragments.rs supplies the text-fragment and semantic-symbol side channel that makes locate work for exact prose/config snippets instead of only symbol queries.

## Markdown heading and intent adapter

Handle: `concept://markdown_heading_and_intent_adapter`

Markdown adapter flow that turns headings into nested document structure, fingerprints markdown sections, and extracts unresolved intent targets from prose context.

Aliases: `Markdown adapter`, `heading and intent parsing`

### Core Members

- `prism_lang_markdown`
- `prism_lang_markdown::HeadingSection`

### Supporting Members

- `prism_lang_markdown::MarkdownAdapter`

### Related Concepts

- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`)
- part of: `document and config adapters` (`concept://document_and_config_adapters`)

### Evidence

- The Markdown adapter is a single-module implementation that constructs heading containment edges, section spans, stable heading slugs, and unresolved intents from markdown text.
- Its behavior is meaningfully different from structured config adapters because it builds semantic document hierarchy and prose-linked intent targets rather than key/value trees.

## MCP authenticated mutation host

Handle: `concept://mcp_mutation_and_session_host`

prism-mcp modules that decode `prism_mutate` actions, host authenticated state-changing mutations, and route agent-side writes into durable PRISM state without relying on `prism_session` authority.

Aliases: `authenticated mutation host`, `mutation host`, `tool payload host`

### Core Members

- `prism_mcp::mutation_provenance::MutationProvenance`
- `prism_mcp::host_mutations::mutation_provenance`
- `prism_mcp::tool_args::PrismHeartbeatLeaseArgs`

### Related Concepts

- depended on by: `inferred edge runtime and session store` (`concept://inferred_edge_runtime_and_session_store`)
- depended on by: `validation feedback and metrics loop` (`concept://validation_feedback_and_metrics_loop`)
- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- depends on: `principal identity and mutation provenance` (`concept://principal_identity_and_mutation_provenance`)
- has part: `mutation argument and schema surface` (`concept://mutation_argument_and_schema_surface`)
- has part: `session context and hosted mutation runtime` (`concept://session_state_and_mutation_runtime`)
- part of: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-mcp keeps mutation handling, provenance stamping, tool arguments, tool schemas, and schema examples as one coherent hosted mutation surface.
- After the principal-identity cutover, `prism://session` is read-only context and the authoritative write path flows through authenticated `prism_mutate` actions.

### Risk Hint

- Schema drift or provenance mistakes here can make authenticated mutations persist the wrong actor, lease, or execution context.

## MCP query and resource serving

Handle: `concept://mcp_query_and_resource_serving`

prism-mcp serving layer that runs query execution, semantic context assembly, and MCP resource reads over the live Prism workspace.

Aliases: `query serving`, `resource serving`, `semantic serving layer`

### Core Members

- `prism_mcp::query_runtime`
- `prism_mcp::resources`
- `prism_mcp::host_resources`
- `prism_mcp::semantic_contexts`
- `prism_mcp::query_helpers`

### Supporting Members

- `prism_mcp::resource_schemas`
- `prism::document::docs::AGENT_COMPRESSION_LAYER_md::target_default_agent_path`

### Related Concepts

- depended on by: `JavaScript API contract surface` (`concept://javascript_api_contract_surface`)
- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- has part: `query execution and semantic-context serving` (`concept://query_execution_and_semantic_context_serving`)
- has part: `resource schemas and host-resource serving` (`concept://resource_schemas_and_host_resource_serving`)
- part of: `MCP runtime surface` (`concept://mcp_runtime_surface`)

### Evidence

- prism-mcp groups query_runtime, resources, host_resources, semantic_contexts, and query_helpers around serving semantic reads to agents and clients.
- This slice is the operational read path behind PRISM resources, query helpers, and context-rich MCP responses.

### Risk Hint

- Serving regressions here surface as missing context, broken resources, or inconsistent query envelopes even if the underlying graph is healthy.

## MCP runtime lifecycle

Handle: `concept://mcp_runtime_lifecycle`

prism-mcp daemon and runtime-state modules that launch the server, track process/runtime health, and expose lifecycle-oriented status views.

Aliases: `daemon lifecycle`, `runtime lifecycle`, `server health surface`

### Core Members

- `prism_mcp::daemon_mode`
- `prism_mcp::process_lifecycle`
- `prism_mcp::runtime_state`
- `prism_mcp::runtime_views`

### Related Concepts

- depended on by: `CLI runtime and MCP control` (`concept://cli_runtime_and_mcp_control`)
- depends on: `workspace session runtime` (`concept://workspace_session_runtime`)
- has part: `daemon, process, and proxy lifecycle` (`concept://daemon_process_and_proxy_lifecycle`)
- has part: `server surface and runtime health views` (`concept://server_surface_and_runtime_health_views`)
- part of: `MCP runtime surface` (`concept://mcp_runtime_surface`)

### Evidence

- daemon_mode, process_lifecycle, runtime_state, and runtime_views collectively define how prism-mcp starts, refreshes, and reports health for the live server.
- This layer is distinct from semantic query serving because it governs server process behavior and observability.

### Risk Hint

- Lifecycle bugs here can leave a healthy graph inaccessible or make runtime health appear better than it is.

## MCP runtime surface

Handle: `concept://mcp_runtime_surface`

MCP- and daemon-facing runtime surface that bridges PRISM query and mutation capabilities into resources, schemas, compact tools, and long-lived server state.

Aliases: `mcp surface`, `server runtime`, `daemon surface`

### Core Members

- `prism_mcp`
- `prism_mcp::daemon_mode::serve_with_mode`
- `prism_mcp::compact_tools`
- `prism_js`
- `prism_query`

### Supporting Members

- `prism::document::docs::AGENT_COMPRESSION_LAYER_md::target_default_agent_path`

### Related Concepts

- depended on by: `dashboard routing and assets` (`concept://dashboard_routing_and_assets`)
- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- has part: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- has part: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`)
- has part: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`)
- often used with: `JavaScript query ABI` (`concept://javascript_query_abi`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- specialized by: `compact tool surface` (`concept://compact_tool_surface`)
- specialized by: `dashboard surface` (`concept://dashboard_surface`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-mcp owns daemon_mode, query_runtime, resources, tool schemas, session state, host mutations, runtime views, and compact tool modules.
- prism-js supplies the documented agent API and JS runtime types consumed by the MCP surface.

### Risk Hint

- This layer couples product behavior, schema contracts, and runtime lifecycle, so edits here have wide blast radius.

## memory outcomes and session history

Handle: `concept://memory_outcomes_and_session_history`

Memory modules that record task/session-local history, outcome events, and replay-oriented recall over prior work.

Aliases: `outcome memory`, `session memory history`

### Core Members

- `prism_memory::outcome`
- `prism_memory::outcome_query`
- `prism_memory::session`

### Related Concepts

- has part: `outcome event and replay memory` (`concept://outcome_event_and_replay_memory`)
- has part: `session and episodic memory store` (`concept://session_and_episodic_memory_store`)
- part of: `memory system` (`concept://memory_system`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- outcome.rs, outcome_query.rs, and session.rs form the memory slice that stores and retrieves task/session outcomes rather than generic semantic recall.
- This is the memory pathway behind task replay and filtered outcome history.

### Risk Hint

- If this layer drifts, task replay and recent-failure context become misleading even when generic memory recall still works.

## memory projection persistence

Handle: `concept://memory_projection_persistence`

prism-store modules that persist memory projections and memory-store state separately from the primary graph/history tables.

Aliases: `memory store persistence`, `memory snapshot persistence`

### Core Members

- `prism_store::memory_projection`
- `prism_store::memory_store`

### Related Concepts

- often used with: `memory system` (`concept://memory_system`)
- part of: `persistence and history layer` (`concept://persistence_and_history`)

### Evidence

- `prism-store/src/lib.rs` exposes memory_projection and memory_store as distinct persistence pieces beside graph/sqlite/store.
- This split matters because PRISM memory durability rides on projection-specific storage rather than only the main graph tables.

### Risk Hint

- If this layer drifts, memory hydration may look semantically valid while reading stale or mismatched persisted state.

## memory recall and scoring

Handle: `concept://memory_recall_and_scoring`

Memory modules that retrieve, score, and compose remembered context from text and common recall utilities into usable results.

Aliases: `recall pipeline`, `memory scoring`

### Core Members

- `prism_memory::recall`
- `prism_memory::text`
- `prism_memory::common`
- `prism_memory::composite`

### Related Concepts

- has part: `composite memory routing and entry storage` (`concept://composite_memory_routing_and_entry_storage`)
- has part: `recall signal and text scoring` (`concept://recall_signal_and_text_scoring`)
- often used with: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`)
- part of: `memory system` (`concept://memory_system`)

### Evidence

- prism-memory groups recall, text, common, and composite helpers around turning stored memory into ranked and assembled read context.
- This slice represents retrieval behavior rather than storage shape or semantic embedding plumbing.

### Risk Hint

- Ranking and composition errors here silently degrade the usefulness of recalled context before they look like hard failures.

## memory refresh and patch outcome recording

Handle: `concept://memory_refresh_and_patch_outcome_recording`

prism-core modules that reanchor persisted memory snapshots after lineage changes and record patch-derived outcome events and validation deltas during refresh.

Aliases: `memory refresh path`, `patch outcome recording`

### Core Members

- `prism_core::memory_refresh`
- `prism_core::patch_outcomes`

### Related Concepts

- depends on: `memory system` (`concept://memory_system`)
- part of: `workspace session runtime` (`concept://workspace_session_runtime`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `memory_refresh.rs` reapplies lineage events to persisted memory snapshots, while `patch_outcomes.rs` emits patch outcome events and validation deltas from observed changes.
- This is the persistence-feedback half of workspace session runtime.

### Risk Hint

- If this path drifts, refreshes can preserve the wrong memory anchors or lose the patch validation trail.

## memory system

Handle: `concept://memory_system`

Layered session, episodic, structural, outcome, and semantic memory subsystem with recall and re-anchoring support across PRISM runtime workflows.

Aliases: `memory layer`, `recall system`, `memory stack`

### Core Members

- `prism_memory`
- `prism_memory::composite::MemoryComposite`
- `prism_core`
- `prism_query`

### Supporting Members

- `prism::document::docs::VALIDATION_md::7_validation_pipeline`

### Related Concepts

- depended on by: `curator rule synthesis and proposal types` (`concept://curator_rule_synthesis_and_proposal_types`)
- depended on by: `impact and outcome queries` (`concept://impact_and_outcome_queries`)
- depended on by: `memory refresh and patch outcome recording` (`concept://memory_refresh_and_patch_outcome_recording`)
- depended on by: `projection and query layer` (`concept://projection_and_query_layer`)
- depended on by: `workspace session runtime` (`concept://workspace_session_runtime`)
- depends on: `persistence and history layer` (`concept://persistence_and_history`)
- has part: `memory outcomes and session history` (`concept://memory_outcomes_and_session_history`)
- has part: `memory recall and scoring` (`concept://memory_recall_and_scoring`)
- has part: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`)
- often used with: `memory projection persistence` (`concept://memory_projection_persistence`)
- often used with: `published knowledge and memory event logs` (`concept://published_knowledge_and_memory_event_logs`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-memory exports composite, episodic, outcome, recall, semantic, session, structural, and text subsystems.
- prism-core owns memory_events and memory_refresh while prism-query consumes memory- and outcome-backed recall surfaces.

### Risk Hint

- Anchoring and freshness failures here quietly degrade retrieval quality before they become obvious product bugs.

## mutation argument and schema surface

Handle: `concept://mutation_argument_and_schema_surface`

prism-mcp modules that define mutation/query argument types, tool schemas, and executable examples for the hosted MCP surface.

Aliases: `mutation schemas`, `tool arg surface`

### Core Members

- `prism_mcp::tool_args`
- `prism_mcp::tool_schemas`
- `prism_mcp::schema_examples`
- `prism_mcp::query_types`

### Related Concepts

- part of: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `crates/prism-mcp/src/lib.rs` groups `tool_args`, `tool_schemas`, `schema_examples`, and `query_types` as the typed contract surface for hosted tools.
- These modules are the mutation/schema half of the MCP server, distinct from runtime state and resource reads.

### Risk Hint

- If this contract layer drifts, the MCP surface can accept or describe the wrong payloads while deeper runtime logic stays unchanged.

## open and workset follow-through

Handle: `concept://open_and_workset_followthrough`

Compact-tool open/workset path that resolves session handles, expands concept and text-fragment targets, assembles supporting reads and likely tests, and enforces workset budget limits.

Aliases: `compact open/workset`, `follow-through reads`

### Core Members

- `prism_mcp::compact_tools::open`
- `prism_mcp::compact_tools::workset`

### Related Concepts

- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- part of: `compact discovery and opening flow` (`concept://compact_discovery_and_opening`)

### Evidence

- open.rs resolves handle targets, concept follow-through, text-fragment reads, related handles, and suggested next actions.
- workset.rs builds bounded worksets with primary targets, supporting reads, likely tests, follow-up handles, and JSON-size budgeting.

## outcome event and replay memory

Handle: `concept://outcome_event_and_replay_memory`

prism-memory modules that store outcome events, index them by anchors and tasks, and replay prior validation or failure history for resume flows.

Aliases: `outcome replay memory`, `task replay memory`

### Core Members

- `prism_memory::outcome`
- `prism_memory::outcome_query`
- `prism_memory::types`

### Related Concepts

- part of: `memory outcomes and session history` (`concept://memory_outcomes_and_session_history`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `outcome.rs` indexes and reanchors outcome events, while `outcome_query.rs` defines the replay/query filter surface over those events.
- This is the event-history side of the memory subsystem, distinct from general episodic or semantic recall.

### Risk Hint

- If outcome replay drifts, later agents lose the failure and validation trail they need to resume work safely.

## parse, resolution, and reanchor flow

Handle: `concept://parse_resolution_and_reanchor_flow`

prism-core modules that parse files, resolve static edges, and infer lineage-preserving reanchors across file moves and symbol changes during refresh.

Aliases: `parse and resolution flow`, `reanchor flow`

### Core Members

- `prism_core::parse_pipeline`
- `prism_core::resolution`
- `prism_core::reanchor`

### Related Concepts

- depends on: `language adapter family` (`concept://language_adapter_family`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- part of: `core indexing pipeline` (`concept://core_indexing_pipeline`)

### Evidence

- `parse_pipeline.rs` produces parse jobs, `resolution.rs` resolves edges from unresolved parser outputs, and `reanchor.rs` preserves identity across moves and edits.
- These modules are the structural-truth half of index refresh after snapshot loading.

### Risk Hint

- If this flow drifts, PRISM can rebuild a graph that looks complete but loses identity continuity or edge truth.

## parser contract and fingerprint utilities

Handle: `concept://parser_contract_and_fingerprint_utilities`

Shared prism-parser contract that defines parse inputs/results, language adapter behavior, intent extraction, document naming, and stable fingerprint helpers reused by every adapter crate.

Aliases: `parser contract`, `adapter contract`, `fingerprint utilities`

### Core Members

- `prism_parser::LanguageAdapter`
- `prism_parser::ParseInput`
- `prism_parser::ParseResult`
- `prism_parser::fingerprint_from_parts`

### Supporting Members

- `prism_parser::document_name`
- `prism_parser::extract_intent_targets`

### Related Concepts

- depended on by: `Markdown heading and intent adapter` (`concept://markdown_heading_and_intent_adapter`)
- depended on by: `Python tree-sitter adapter pipeline` (`concept://python_tree_sitter_adapter_pipeline`)
- depended on by: `Rust tree-sitter adapter pipeline` (`concept://rust_tree_sitter_adapter_pipeline`)
- depended on by: `code language adapters` (`concept://code_language_adapters`)
- depended on by: `document and config adapters` (`concept://document_and_config_adapters`)
- depended on by: `structured config value adapters` (`concept://structured_config_value_adapters`)
- part of: `language adapter family` (`concept://language_adapter_family`)

### Evidence

- prism-parser centralizes LanguageAdapter, ParseInput, ParseResult, fingerprint helpers, document path/name helpers, and intent-target extraction reused across adapter crates.
- Both code adapters and document/config adapters import this crate for the common parse contract and stable shape/fingerprint behavior.

## persistence and history layer

Handle: `concept://persistence_and_history`

SQLite-backed graph and temporal persistence layer that stores structural state, history snapshots, memory projections, and other durable PRISM artifacts.

Aliases: `storage layer`, `sqlite store`, `history store`

### Core Members

- `prism_store`
- `prism_store::sqlite::SqliteStore`
- `prism_history`
- `prism_history::store::HistoryStore`

### Related Concepts

- depended on by: `co-change and validation projection indexes` (`concept://cochange_and_validation_projection_indexes`)
- depended on by: `indexer orchestration and snapshot loading` (`concept://indexer_orchestration_and_snapshot_loading`)
- depended on by: `memory system` (`concept://memory_system`)
- depended on by: `projection and query layer` (`concept://projection_and_query_layer`)
- depended on by: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)
- depended on by: `workspace session runtime` (`concept://workspace_session_runtime`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `SQLite and graph persistence` (`concept://sqlite_and_graph_persistence`)
- has part: `history snapshot and resolution` (`concept://history_snapshot_and_resolution`)
- has part: `memory projection persistence` (`concept://memory_projection_persistence`)
- part of: `PRISM architecture` (`concept://prism_architecture`)

### Evidence

- prism-store owns graph persistence, memory projection persistence, memory store support, and sqlite IO/schema modules.
- prism-history owns resolver, snapshot, and store primitives for temporal identity and lineage snapshots.

### Risk Hint

- Schema or snapshot drift here can invalidate indexing, replay, query correctness, and knowledge hydration.

## persistence_split

Handle: `concept://persistence_split`

The target three-plane persistence model that separates repo-published `.prism` truth, shared mutable backend state, and process-local cache/materializations.

Aliases: `three state planes`, `three-plane persistence`, `persistence split`

### Core Members

- `prism::document::docs::PERSISTENCE_STATE_CLASSIFICATION_md::three_state_planes`
- `prism::document::docs::PERSISTENCE_STATE_CLASSIFICATION_md::boundary_guidance`

### Evidence

- docs/PERSISTENCE_STATE_CLASSIFICATION.md defines the three state planes and states that a shared database complements `.prism`; it does not replace it.
- The same document distinguishes repo-published knowledge, shared runtime continuity, and process-local cache so derived views and snapshots do not become a second semantic authority.

### Risk Hint

- If this split collapses, PRISM recreates dual truth between published repo knowledge, shared runtime state, and derived snapshots or overlays.

## plan and coordination IR

Handle: `concept://plan_and_coordination_ir`

prism-ir modules that define plan graphs, blockers, validation refs, claims, capabilities, review states, and other shared coordination schema used above the raw graph layer.

Aliases: `coordination ir`, `plan schema ir`

### Core Members

- `prism_ir::plans`
- `prism_ir::coordination`

### Related Concepts

- often used with: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `structural IR and identity model` (`concept://structural_ir`)

### Evidence

- `prism-ir/src/lib.rs` re-exports plans and coordination as the shared typed schema for runtime plan and review state.
- These modules are the contract between low-level storage and the higher coordination runtime/query layers.

### Risk Hint

- If these shared types drift, coordination state and plan projections diverge across crates.

## plan and repo-layout publication

Handle: `concept://plan_and_repo_layout_publication`

prism-core modules that govern published plan material, repo layout paths, and supporting helpers used when durable artifacts are written back into the workspace.

Aliases: `plan publication`, `repo layout publication`

### Core Members

- `prism_core::published_plans`
- `prism_core::layout`
- `prism_core::util`
- `prism_core::indexer_support`

### Related Concepts

- depends on: `repo publication guards` (`concept://repo_publication_guards`)
- often used with: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- published_plans, layout, util, and indexer_support together capture the file-system and helper layer around durable PRISM artifacts in the workspace.
- This is adjacent to publication guards but narrower: it handles where and how published plan-related artifacts land.

### Risk Hint

- Path/layout mistakes here can make valid durable artifacts invisible or misplaced without changing their semantic contents.

## plan completion and insight queries

Handle: `concept://plan_completion_and_insight_queries`

prism-query modules that evaluate plan completion, blockers, recommendations, and higher-level plan insights beyond raw runtime overlays.

Aliases: `plan insight queries`, `completion queries`

### Core Members

- `prism_query::plan_completion`
- `prism_query::plan_insights`

### Related Concepts

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `query coordination and plan views` (`concept://query_coordination_and_plan_views`)

### Evidence

- `crates/prism-query/src/lib.rs` keeps `plan_completion` and `plan_insights` separate from raw coordination/runtime access.
- These modules are the judgment-oriented read path that turns plan state into blockers, completion checks, and recommendations.

### Risk Hint

- If these heuristics drift, agents may act on misleading plan readiness or blocker interpretations.

## plan-graph compatibility and runtime overlays

Handle: `concept://plan_graph_compatibility_and_runtime_overlays`

prism-coordination modules that translate between coordination snapshots and plan graphs and expose runtime overlay state above the canonical store.

Aliases: `plan graph compat`, `runtime overlays`

### Core Members

- `prism_coordination::compat`
- `prism_coordination::runtime`

### Related Concepts

- part of: `coordination state model` (`concept://coordination_state_model`)

### Evidence

- `compat.rs` maps coordination snapshots to plan graphs and overlays, while `runtime.rs` exposes a mutable runtime facade over coordination state.
- This is the projection-and-overlay half of the coordination state model.

### Risk Hint

- Drift here breaks the translation between canonical coordination state and plan-runtime views used elsewhere in the repo.

## principal identity and mutation provenance

Handle: `concept://principal_identity_and_mutation_provenance`

Cross-crate principal-authentication and provenance-stamping layer that turns authenticated local principals into durable event actors and execution-context snapshots for every authoritative mutation.

Aliases: `authenticated principal identity`, `mutation provenance`, `authenticated mutation provenance`

### Core Members

- `prism_core::principal_registry::AuthenticatedPrincipal`
- `prism_ir::principal::PrincipalActor`
- `prism_ir::events::EventExecutionContext`
- `prism_mcp::mutation_provenance::MutationProvenance`

### Supporting Members

- `prism_mcp::host_mutations::mutation_provenance`

### Related Concepts

- depended on by: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- depended on by: `coordination mutation, lease, and policy helpers` (`concept://coordination_mutation_and_policy_helpers`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)

### Evidence

- The principal-identity cutover snapshots authority id, principal id, principal kind/name, credential id, session id, request id, and workspace execution context into authoritative mutation events.
- Bridge-adopted local principals are attribution and coordination identities; durable audit truth comes from event actors and execution-context snapshots rather than ambient `prism_session` state.

### Risk Hint

- If principal stamping drifts, audit history, lease ownership, and authenticated mutation attribution become untrustworthy even when writes still succeed.

## PRISM architecture

Handle: `concept://prism_architecture`

Top-level monorepo architecture spanning semantic IR, language parsing, workspace indexing, persistence/history, memory/projections/query, coordination/curation, and MCP-facing product surfaces.

Aliases: `repo architecture`, `crate architecture`, `overall architecture`

### Core Members

- `prism_core`
- `prism_ir`
- `prism_query`
- `prism_mcp`
- `prism_store`

### Supporting Members

- `prism::document::docs::SPEC_md::1_crate_architecture`
- `prism::document::docs::VALIDATION_md::7_validation_pipeline`
- `prism::document::docs::PRISM_VAULT_HARBOR_md::7_1_prism_code_cognition`

### Related Concepts

- has part: `CLI surface` (`concept://cli_surface`)
- has part: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- has part: `agent inference and curation` (`concept://agent_inference_and_curation`)
- has part: `compact tool surface` (`concept://compact_tool_surface`)
- has part: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- has part: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- has part: `dashboard surface` (`concept://dashboard_surface`)
- has part: `language adapter family` (`concept://language_adapter_family`)
- has part: `memory system` (`concept://memory_system`)
- has part: `persistence and history layer` (`concept://persistence_and_history`)
- has part: `projection and query layer` (`concept://projection_and_query_layer`)
- has part: `structural IR and identity model` (`concept://structural_ir`)
- has part: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)
- has part: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)

### Evidence

- Workspace members are split into focused crates for IR, core indexing, store, history, memory, projections, query, coordination, curator, MCP, and language adapters.
- docs/SPEC.md frames the crate architecture while prism-mcp and prism-cli provide the operator-facing product surfaces.

### Risk Hint

- Refresh this concept when crate ownership changes or subsystem boundaries are extracted into new crates.

## projection and query layer

Handle: `concept://projection_and_query_layer`

Derived indexes and read APIs that turn graph, history, memory, and coordination state into semantic discovery, impact, and programmable query results.

Aliases: `projection layer`, `query layer`, `semantic read surface`

### Core Members

- `prism_projections`
- `prism_projections::projections::ProjectionIndex`
- `prism_projections::intent::IntentIndex`
- `prism_query`
- `prism_js`

### Supporting Members

- `prism::document::docs::AGENT_COMPRESSION_LAYER_md::target_default_agent_path`
- `prism::document::docs::AGENT_COMPRESSION_LAYER_md::compact_primary_tools`

### Related Concepts

- depended on by: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`)
- depended on by: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- depended on by: `compact discovery and opening flow` (`concept://compact_discovery_and_opening`)
- depended on by: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`)
- depended on by: `compact tool surface` (`concept://compact_tool_surface`)
- depended on by: `concept and expand decode runtime` (`concept://concept_and_expand_decode_runtime`)
- depended on by: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- depended on by: `curator rule synthesis and proposal types` (`concept://curator_rule_synthesis_and_proposal_types`)
- depended on by: `locate ranking and text-candidate flow` (`concept://locate_ranking_and_text_candidate_flow`)
- depended on by: `open and workset follow-through` (`concept://open_and_workset_followthrough`)
- depended on by: `query execution and semantic-context serving` (`concept://query_execution_and_semantic_context_serving`)
- depended on by: `validation feedback and metrics loop` (`concept://validation_feedback_and_metrics_loop`)
- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- depends on: `memory system` (`concept://memory_system`)
- depends on: `persistence and history layer` (`concept://persistence_and_history`)
- has part: `JavaScript query ABI` (`concept://javascript_query_abi`)
- has part: `query coordination and plan views` (`concept://query_coordination_and_plan_views`)
- has part: `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`)
- has part: `semantic projection indexes` (`concept://semantic_projection_indexes`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-projections derives concepts, concept relations, intent, co-change, and validation-oriented projection indexes.
- prism-query aggregates store, history, memory, and coordination into the programmable Prism surface that prism-js documents for MCP consumers.

### Risk Hint

- Ranking or projection drift usually first appears as low-quality first hops and missing context rather than obvious crashes.

## published knowledge and memory event logs

Handle: `concept://published_knowledge_and_memory_event_logs`

prism-core modules that persist published knowledge artifacts and append memory event logs used to hydrate durable repo knowledge and memory views.

Aliases: `published knowledge logs`, `memory event logs`

### Core Members

- `prism_core::published_knowledge`
- `prism_core::memory_events`

### Related Concepts

- depends on: `repo publication guards` (`concept://repo_publication_guards`)
- often used with: `memory system` (`concept://memory_system`)
- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)

### Evidence

- `crates/prism-core/src/lib.rs` declares `published_knowledge` and `memory_events` as dedicated persistence modules adjacent to concept event streams.
- These modules are the durable log layer for published knowledge state and memory-side publication artifacts.

### Risk Hint

- Bugs here can make durable knowledge hydration or memory replay silently diverge from the underlying repo event history.

## Python tree-sitter adapter pipeline

Handle: `concept://python_tree_sitter_adapter_pipeline`

Python adapter modules that parse Python source with tree-sitter and combine syntax extraction and import/path normalization into PRISM nodes, fingerprints, unresolved calls, imports, and intents.

Aliases: `Python adapter pipeline`, `Python parsing adapter`

### Core Members

- `prism_lang_python::parser`
- `prism_lang_python::syntax`
- `prism_lang_python::paths`

### Related Concepts

- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`)
- part of: `code language adapters` (`concept://code_language_adapters`)

### Evidence

- The Python crate mirrors the Rust split into parser.rs, syntax.rs, and paths.rs, with parser.rs driving tree-sitter parsing and helper modules resolving Python-specific declarations, imports, and dotted references.
- This is the concrete Python-specific half of the broader code language adapter family.

## query coordination and plan views

Handle: `concept://query_coordination_and_plan_views`

prism-query modules that project coordination state, plan runtime overlays, intent, and shared query types into the programmable read surface.

Aliases: `coordination query views`, `plan query views`

### Core Members

- `prism_query::coordination`
- `prism_query::plan_runtime`
- `prism_query::intent`
- `prism_query::types`

### Related Concepts

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- has part: `coordination and plan-runtime queries` (`concept://coordination_and_plan_runtime_queries`)
- has part: `plan completion and insight queries` (`concept://plan_completion_and_insight_queries`)
- part of: `projection and query layer` (`concept://projection_and_query_layer`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- coordination.rs and plan_runtime.rs bridge continuity state into the query surface, while intent.rs and types.rs support the surfaced view model.
- This is the coordination-aware half of prism-query that complements symbol/source/change contexts.

### Risk Hint

- Drift here yields coherent symbol lookup but broken task, plan, or intent projections for agents and dashboards.

## query execution and semantic-context serving

Handle: `concept://query_execution_and_semantic_context_serving`

prism-mcp modules that execute PRISM reads, normalize diagnostics, and serve semantic context bundles over the live MCP runtime.

Aliases: `query runtime serving`, `semantic context serving`

### Core Members

- `prism_mcp::query_runtime`
- `prism_mcp::semantic_contexts`
- `prism_mcp::query_helpers`
- `prism_mcp::query_errors`

### Related Concepts

- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- part of: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`)

### Evidence

- `crates/prism-mcp/src/lib.rs` groups `query_runtime`, `semantic_contexts`, `query_helpers`, and `query_errors` together around the live read path.
- These modules are the execution core for serving bounded query results and semantic context views.

### Risk Hint

- If this path degrades, the MCP server returns confusing or weakly grounded reads even though the underlying Prism object remains healthy.

## query symbol and change contexts

Handle: `concept://query_symbol_and_change_contexts`

prism-query modules that resolve symbols, source slices, impact, and outcome/change context into high-signal read bundles for agent work.

Aliases: `symbol contexts`, `change contexts`, `read bundles`

### Core Members

- `prism_query::symbol`
- `prism_query::source`
- `prism_query::impact`
- `prism_query::outcomes`

### Related Concepts

- depends on: `semantic projection indexes` (`concept://semantic_projection_indexes`)
- has part: `impact and outcome queries` (`concept://impact_and_outcome_queries`)
- has part: `symbol, source, and relation queries` (`concept://symbol_source_and_relation_queries`)
- part of: `projection and query layer` (`concept://projection_and_query_layer`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- symbol.rs, source.rs, impact.rs, and outcomes.rs are the modules behind focused symbol reads, source windows, blast radius, failures, and recent change context.
- This slice is the local-context and recent-change half of prism-query rather than the coordination/runtime half.

### Risk Hint

- If these bundles regress, agents lose the short path from a symbol to its relevant context even while raw search still works.

## recall signal and text scoring

Handle: `concept://recall_signal_and_text_scoring`

prism-memory modules that compute anchor overlap, recency, trust, token/substring/embedding text signals, and final scored recall ordering across memory entries.

Aliases: `memory scoring core`, `text recall scoring`

### Core Members

- `prism_memory::recall`
- `prism_memory::text`
- `prism_memory::common`

### Related Concepts

- depended on by: `semantic memory backend runtime` (`concept://semantic_memory_backend_runtime`)
- depended on by: `structural memory feature model` (`concept://structural_memory_feature_model`)
- part of: `memory recall and scoring` (`concept://memory_recall_and_scoring`)

### Evidence

- `recall.rs`, `text.rs`, and `common.rs` define the scoring signals, tokenization/embedding helpers, and ranking utilities used across memory recall paths.
- These modules are the low-level ranking substrate below higher memory-module routing.

### Risk Hint

- If these scoring signals drift, every memory recall surface becomes plausibly ranked but practically misleading.

## repo publication guards

Handle: `concept://repo_publication_guards`

prism-core publication and validation modules that govern which memory, concept, relation, and plan artifacts are durable enough to publish into repo knowledge.

Aliases: `publication guards`, `repo knowledge validators`

### Core Members

- `prism_core::published_knowledge`
- `prism_core::concept_events`
- `prism_core::concept_relation_events`
- `prism_core::memory_events`
- `prism_core::published_plans`

### Supporting Members

- `prism::document::docs::CONCEPT_MAINTENANCE_md::3_1_concepts_are_evidence_backed_semantic_packets`
- `prism::document::docs::CONCEPT_MAINTENANCE_md::15_integration_with_concept_to_concept_edges`

### Related Concepts

- depended on by: `plan and repo-layout publication` (`concept://plan_and_repo_layout_publication`)
- depended on by: `published knowledge and memory event logs` (`concept://published_knowledge_and_memory_event_logs`)
- part of: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- published_knowledge.rs validates repo-scoped memory, concept, and concept-relation publication constraints such as trust, evidence, core-member count, and provenance.
- The surrounding event modules capture the durable artifacts that later projection layers hydrate into repo knowledge.

### Risk Hint

- Weakening these guards makes the entire repo concept layer easier to pollute with low-quality durable knowledge.

## resource schemas and host-resource serving

Handle: `concept://resource_schemas_and_host_resource_serving`

prism-mcp modules that publish resource payload schemas, capability resources, and host-backed resource reads over the MCP surface.

Aliases: `resource serving`, `host resources`

### Core Members

- `prism_mcp::resources`
- `prism_mcp::host_resources`
- `prism_mcp::resource_schemas`
- `prism_mcp::capabilities_resource`

### Related Concepts

- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- part of: `MCP query and resource serving` (`concept://mcp_query_and_resource_serving`)

### Evidence

- `crates/prism-mcp/src/lib.rs` keeps `resources`, `host_resources`, `resource_schemas`, and `capabilities_resource` as the explicit resource-serving side of the server.
- These modules expose repo and runtime state as inspectable MCP resources rather than tool calls.

### Risk Hint

- Schema or host-resource drift here breaks discoverability and resource-level trust for later agents.

## Rust tree-sitter adapter pipeline

Handle: `concept://rust_tree_sitter_adapter_pipeline`

Rust adapter modules that parse Rust source with tree-sitter and map syntax and path-resolution into PRISM nodes, fingerprints, unresolved calls, impls, and imports.

Aliases: `Rust adapter pipeline`, `Rust parsing adapter`

### Core Members

- `prism_lang_rust::parser`
- `prism_lang_rust::syntax`
- `prism_lang_rust::paths`

### Related Concepts

- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`)
- part of: `code language adapters` (`concept://code_language_adapters`)

### Evidence

- The Rust crate splits into parser.rs, syntax.rs, and paths.rs, with parser.rs orchestrating tree-sitter parsing while syntax/path helpers build nodes, spans, edges, and canonical symbol paths.
- This is the concrete Rust-specific half of the broader code language adapter family.

## semantic and structural memory models

Handle: `concept://semantic_and_structural_memory_models`

Memory modules that derive structural features, manage structural recall, and integrate semantic matching backends for deeper retrieval beyond simple text scoring.

Aliases: `semantic memory`, `structural memory models`

### Core Members

- `prism_memory::structural`
- `prism_memory::structural_features`
- `prism_memory::semantic`

### Related Concepts

- depended on by: `session and episodic memory store` (`concept://session_and_episodic_memory_store`)
- has part: `semantic memory backend runtime` (`concept://semantic_memory_backend_runtime`)
- has part: `structural memory feature model` (`concept://structural_memory_feature_model`)
- often used with: `memory recall and scoring` (`concept://memory_recall_and_scoring`)
- part of: `memory system` (`concept://memory_system`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- structural.rs and structural_features.rs own derived feature pathways while semantic/ owns embedding-backed matching behavior.
- This slice represents the richer retrieval models layered on top of basic recall and outcome history.

### Risk Hint

- Model or feature drift here can produce subtle retrieval changes that look plausible but are hard to validate by eye.

## semantic memory backend runtime

Handle: `concept://semantic_memory_backend_runtime`

prism-memory semantic modules that configure semantic backends, compute local and remote semantic signals, and rank semantic memory matches across embeddings and lexical bridges.

Aliases: `semantic memory backend`, `semantic recall runtime`

### Core Members

- `prism_memory::semantic`
- `prism_memory::semantic::runtime`
- `prism_memory::semantic::config`

### Supporting Members

- `prism_memory::semantic::openai`

### Related Concepts

- depends on: `recall signal and text scoring` (`concept://recall_signal_and_text_scoring`)
- part of: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`)

### Evidence

- `semantic/mod.rs` combines lexical, alias, and semantic signals over an entry store, while `semantic::runtime` and `semantic::config` select and drive backend behavior.
- This is the embedding-aware half of model-backed memory recall.

### Risk Hint

- Backend drift here can make semantic recall feel intelligent while silently collapsing recall precision or calibration.

## semantic projection indexes

Handle: `concept://semantic_projection_indexes`

prism-projections modules that materialize derived concept, relation, intent, and projection indexes from lower-level events and snapshots.

Aliases: `projection indexes`, `derived indexes`

### Core Members

- `prism_projections::projections`
- `prism_projections::concepts`
- `prism_projections::concept_relations`
- `prism_projections::intent`
- `prism_projections::types`

### Related Concepts

- depended on by: `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`)
- depended on by: `symbol, source, and relation queries` (`concept://symbol_source_and_relation_queries`)
- depends on: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- has part: `co-change and validation projection indexes` (`concept://cochange_and_validation_projection_indexes`)
- has part: `concept, relation, and intent projections` (`concept://concept_relation_and_intent_projections`)
- often used with: `concept and relation event streams` (`concept://concept_and_relation_event_streams`)
- part of: `projection and query layer` (`concept://projection_and_query_layer`)

### Evidence

- prism-projections keeps projections, concepts, concept_relations, intent, and types as the materialization layer for derived semantic indexes.
- This is the concrete internal slice behind the higher-level projection and query layer concept.

### Risk Hint

- Projection drift here changes what the rest of PRISM believes is important without changing the underlying raw graph.

## server surface and runtime health views

Handle: `concept://server_surface_and_runtime_health_views`

prism-mcp modules that expose top-level server capabilities, feature flags, runtime status, and diagnostics for the live server surface.

Aliases: `server health views`, `runtime health surface`

### Core Members

- `prism_mcp::server_surface`
- `prism_mcp::runtime_state`
- `prism_mcp::features`
- `prism_mcp::diagnostics`

### Related Concepts

- part of: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `crates/prism-mcp/src/lib.rs` groups `server_surface`, `runtime_state`, `features`, and `diagnostics` around the server-level presentation of runtime health and capabilities.
- These modules provide the health/status side of lifecycle management rather than process spawning itself.

### Risk Hint

- If this surface drifts, operators and agents can misread server capability or health even while the daemon is running.

## session and episodic memory store

Handle: `concept://session_and_episodic_memory_store`

prism-memory modules that assemble the session-scoped memory composite, persist episodic entries, and snapshot memory state across workspace reloads.

Aliases: `session memory store`, `episodic memory store`

### Core Members

- `prism_memory::session`
- `prism_memory::episodic`
- `prism_memory::entry_store`

### Related Concepts

- depends on: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`)
- part of: `memory outcomes and session history` (`concept://memory_outcomes_and_session_history`)

### Evidence

- `session.rs` composes episodic, structural, and semantic memory into the session runtime, while `episodic.rs` and `entry_store.rs` provide the concrete persisted entry path.
- This slice governs session-level continuity rather than semantic scoring or outcome replay.

### Risk Hint

- If this store layer drifts, memory may appear to exist but fail to survive reloads or route correctly across kinds.

## session context and hosted mutation runtime

Handle: `concept://session_state_and_mutation_runtime`

prism-mcp modules that hold per-session runtime context, execute authenticated hosted mutations, and shape follow-up runtime/read-model views such as task context and heartbeat guidance.

Aliases: `hosted mutation runtime`, `session context runtime`, `session view runtime`

### Core Members

- `prism_mcp::session_state::SessionState`
- `prism_mcp::host_mutations::QueryHost::store_outcome_without_refresh_authenticated`
- `prism_mcp::runtime_views::runtime_status`

### Related Concepts

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)

### Evidence

- `session_state`, hosted mutation entrypoints, and runtime/read-model views still form the execution half of the MCP mutation path.
- After the authority cutover, this layer keeps session context for handles, task context, and post-mutation guidance rather than acting as ambient coordination authority.

### Risk Hint

- Breakage here causes authenticated mutations to lose session context, return misleading follow-up views, or surface stale heartbeat guidance.

## SQLite and graph persistence

Handle: `concept://sqlite_and_graph_persistence`

prism-store modules that own the SQLite backend, graph snapshots, and persist batches for the durable structural store behind PRISM sessions.

Aliases: `store persistence`, `sqlite graph store`

### Core Members

- `prism_store::sqlite`
- `prism_store::graph`
- `prism_store::store`

### Related Concepts

- depends on: `structural IR and identity model` (`concept://structural_ir`)
- part of: `persistence and history layer` (`concept://persistence_and_history`)

### Evidence

- `prism-store/src/lib.rs` groups sqlite, graph, and store as the main persistence surface for graph state and persist batches.
- These modules are the durable structural backbone distinct from memory-specific or history-specific persistence helpers.

### Risk Hint

- Corruption or schema drift here cascades into indexing, reload, and query correctness.

## structural IR and identity model

Handle: `concept://structural_ir`

Authoritative semantic schema for nodes, edges, anchors, events, and stable identities that every higher PRISM layer depends on.

Aliases: `core ir`, `ir layer`, `identity model`

### Core Members

- `prism_ir`
- `prism_ir::graph::NodeId`
- `prism_ir::anchor::AnchorRef`
- `prism_ir::events::EventMeta`

### Supporting Members

- `prism::document::docs::SPEC_md::1_crate_architecture`

### Related Concepts

- depended on by: `SQLite and graph persistence` (`concept://sqlite_and_graph_persistence`)
- depended on by: `code language adapters` (`concept://code_language_adapters`)
- depended on by: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- depended on by: `coordination state model` (`concept://coordination_state_model`)
- depended on by: `core indexing pipeline` (`concept://core_indexing_pipeline`)
- depended on by: `document and config adapters` (`concept://document_and_config_adapters`)
- depended on by: `history snapshot and resolution` (`concept://history_snapshot_and_resolution`)
- depended on by: `language adapter family` (`concept://language_adapter_family`)
- depended on by: `parse, resolution, and reanchor flow` (`concept://parse_resolution_and_reanchor_flow`)
- depended on by: `persistence and history layer` (`concept://persistence_and_history`)
- depended on by: `principal identity and mutation provenance` (`concept://principal_identity_and_mutation_provenance`)
- depended on by: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)
- has part: `anchor, change, and lineage IR` (`concept://anchor_change_and_lineage_ir`)
- has part: `graph, identity, and parse IR` (`concept://graph_identity_and_parse_ir`)
- has part: `plan and coordination IR` (`concept://plan_and_coordination_ir`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- validated by: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- prism-ir re-exports graph, anchor, history, identity, plans, parse, and primitive types as the shared schema layer.
- Downstream crates prism-parser, prism-history, prism-memory, prism-query, prism-core, and prism-coordination all depend directly on prism-ir.

### Risk Hint

- Changes here propagate into parsing, storage, query serialization, coordination, and MCP contracts.

## structural memory feature model

Handle: `concept://structural_memory_feature_model`

prism-memory modules that derive structural tags and rule features from memory entries and use them to rank structural recall results.

Aliases: `structural memory features`, `structural recall model`

### Core Members

- `prism_memory::structural`
- `prism_memory::structural_features`

### Related Concepts

- depends on: `recall signal and text scoring` (`concept://recall_signal_and_text_scoring`)
- part of: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`)

### Evidence

- `structural.rs` ranks structural memories using derived features, and `structural_features.rs` extracts tags, rule kinds, and promoted-rule evidence from memory entries.
- This is the rule- and invariant-oriented half of model-backed memory recall.

### Risk Hint

- If these features drift, structural memories can be retrieved confidently for the wrong architectural rule or invariant.

## structured config value adapters

Handle: `concept://structured_config_value_adapters`

JSON, TOML, and YAML adapter modules that parse structured configuration documents into document/key trees, record stable shape fingerprints, and mine intent targets from configuration values.

Aliases: `config adapters`, `JSON TOML YAML adapters`

### Core Members

- `prism_lang_json::adapter`
- `prism_lang_toml`
- `prism_lang_yaml`

### Related Concepts

- depends on: `parser contract and fingerprint utilities` (`concept://parser_contract_and_fingerprint_utilities`)
- part of: `document and config adapters` (`concept://document_and_config_adapters`)

### Evidence

- The JSON adapter centers on adapter.rs, while TOML and YAML expose single-module implementations that all walk nested values, emit document/key nodes, and attach intent targets from configuration content.
- These formats share a structural key/value traversal style that differs from the markdown document-hierarchy adapter.

## symbol, source, and relation queries

Handle: `concept://symbol_source_and_relation_queries`

prism-query modules that resolve symbols, source excerpts, relation views, and read-oriented helper logic for direct code navigation.

Aliases: `symbol queries`, `source queries`

### Core Members

- `prism_query::symbol`
- `prism_query::source`
- `prism_query::common`
- `prism_query::types`

### Related Concepts

- depends on: `semantic projection indexes` (`concept://semantic_projection_indexes`)
- part of: `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`)

### Evidence

- `crates/prism-query/src/lib.rs` groups `symbol`, `source`, `common`, and `types` as the bounded read/navigation side of the query crate.
- These modules power relation inspection and source slicing before higher impact or coordination reasoning kicks in.

### Risk Hint

- If this path drifts, agents lose trustworthy symbol lookup and bounded source context even when deeper projections still exist.

## task-brief and coordination summary views

Handle: `concept://task_brief_and_coordination_summary_views`

Compact-tool coordination briefing path that condenses plan/task status, blockers, claims, conflicts, recent outcomes, likely validations, and next reads into bounded coordination summaries.

Aliases: `task brief runtime`, `coordination summary views`

### Core Members

- `prism_mcp::compact_tools::task_brief`
- `prism_coordination::queries`

### Related Concepts

- depends on: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- part of: `compact expansion and concept views` (`concept://compact_expansion_and_concept_views`)

### Evidence

- task_brief.rs composes blockers, claims, conflicts, plan graph state, journal outcomes, validation recipes, and next-read recommendations into the compact task brief result.
- Its data path is specifically coordination-oriented and therefore distinct from generic concept/expand decoding.

## validation and dogfooding loop

Handle: `concept://validation_and_dogfooding`

Cross-cutting validation pipeline for structural truth, lineage, memory anchoring, projections, coordination, and MCP/query behavior, plus direct dogfooding feedback capture.

Aliases: `validation pipeline`, `dogfooding loop`, `trust gates`

### Core Members

- `prism_core`
- `prism_projections`
- `prism_mcp`
- `prism::document::docs::VALIDATION_md::7_validation_pipeline`

### Supporting Members

- `prism::document::docs::CONCEPT_MAINTENANCE_md::3_1_concepts_are_evidence_backed_semantic_packets`

### Related Concepts

- has part: `validation feedback and metrics loop` (`concept://validation_feedback_and_metrics_loop`)
- has part: `validation policy and release gates` (`concept://validation_policy_and_release_gates`)
- part of: `PRISM architecture` (`concept://prism_architecture`)
- validates: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- validates: `MCP runtime surface` (`concept://mcp_runtime_surface`)
- validates: `compact tool surface` (`concept://compact_tool_surface`)
- validates: `concept and publication pipeline` (`concept://concept_and_publication_pipeline`)
- validates: `coordination and plan runtime` (`concept://coordination_and_plan_runtime`)
- validates: `coordination mutation, lease, and policy helpers` (`concept://coordination_mutation_and_policy_helpers`)
- validates: `coordination operations and policy` (`concept://coordination_operations_and_policy`)
- validates: `curator backend execution and prompting` (`concept://curator_backend_execution_and_prompting`)
- validates: `curator execution flow` (`concept://curator_execution_flow`)
- validates: `dashboard surface` (`concept://dashboard_surface`)
- validates: `inferred edge runtime and session store` (`concept://inferred_edge_runtime_and_session_store`)
- validates: `memory outcomes and session history` (`concept://memory_outcomes_and_session_history`)
- validates: `memory refresh and patch outcome recording` (`concept://memory_refresh_and_patch_outcome_recording`)
- validates: `memory system` (`concept://memory_system`)
- validates: `mutation argument and schema surface` (`concept://mutation_argument_and_schema_surface`)
- validates: `outcome event and replay memory` (`concept://outcome_event_and_replay_memory`)
- validates: `plan and repo-layout publication` (`concept://plan_and_repo_layout_publication`)
- validates: `projection and query layer` (`concept://projection_and_query_layer`)
- validates: `query coordination and plan views` (`concept://query_coordination_and_plan_views`)
- validates: `query symbol and change contexts` (`concept://query_symbol_and_change_contexts`)
- validates: `repo publication guards` (`concept://repo_publication_guards`)
- validates: `semantic and structural memory models` (`concept://semantic_and_structural_memory_models`)
- validates: `server surface and runtime health views` (`concept://server_surface_and_runtime_health_views`)
- validates: `structural IR and identity model` (`concept://structural_ir`)

### Evidence

- docs/VALIDATION.md defines layered validation across structural truth, lineage, memory, projections, inference, coordination, and MCP/query surfaces.
- prism-core owns validation_feedback support and prism-mcp exposes validation feedback mutation and validation-oriented resources.

### Risk Hint

- Without explicit feedback capture, PRISM can look coherent while shipping stale, noisy, or weakly grounded reasoning surfaces.

## validation feedback and metrics loop

Handle: `concept://validation_feedback_and_metrics_loop`

Runtime and spec elements that capture validation feedback, materialize validation checks, and define the metrics/dashboard loop used to calibrate PRISM over time.

Aliases: `validation feedback capture`, `validation scorecards`

### Core Members

- `prism_core::validation_feedback`
- `prism_mcp::host_mutations`
- `prism_projections::projections`
- `prism::document::docs::VALIDATION_md::10_metrics_dashboard`
- `prism::document::docs::VALIDATION_md::12_human_feedback_loop`

### Related Concepts

- depends on: `MCP authenticated mutation host` (`concept://mcp_mutation_and_session_host`)
- depends on: `projection and query layer` (`concept://projection_and_query_layer`)
- often used with: `dashboard surface` (`concept://dashboard_surface`)
- part of: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `prism-core` owns append-only validation feedback records, `prism-mcp` exposes the mutation path, and `prism-projections::projections` maintains validation checks/deltas.
- `docs/VALIDATION.md` defines both the metrics dashboard and human feedback loop as first-class parts of the product, not incidental testing notes.

### Risk Hint

- Without this loop, PRISM can accumulate plausible but uncalibrated behavior with no durable correction pressure.

## validation policy and release gates

Handle: `concept://validation_policy_and_release_gates`

Core validation spec headings that define layered validation principles, the evaluation pipeline, the PRISM-first validation plan, and risk-tiered release gates.

Aliases: `validation spec`, `trust gate policy`

### Core Members

- `prism::document::docs::VALIDATION_md::3_validation_principles`
- `prism::document::docs::VALIDATION_md::7_validation_pipeline`
- `prism::document::docs::VALIDATION_md::8_prism_validation_plan_implement_first`
- `prism::document::docs::VALIDATION_md::11_release_gates_and_trust_gates`

### Related Concepts

- part of: `validation and dogfooding loop` (`concept://validation_and_dogfooding`)

### Evidence

- `docs/VALIDATION.md` separates principles, pipeline shape, PRISM-first implementation plan, and trust gates into distinct top-level headings.
- Together these headings define the policy contract that the repo uses to judge whether a surface is safe enough for exploration, planning, or mutation guidance.

### Risk Hint

- If this policy layer drifts from implementation, agents may over-trust surfaces that were meant to remain exploratory.

## validation_pipeline

Handle: `concept://validation_pipeline`

The cross-crate path that derives likely validations from impact, promotes them into MCP views, and compresses them into compact follow-through.

Aliases: `validation`, `validation recipe`, `checks`, `likely tests`

### Core Members

- `prism_query::impact::Prism::task_validation_recipe`
- `prism_projections::projections::ProjectionIndex::validation_checks_for_lineages`
- `prism_mcp::views::promoted_validation_checks`
- `prism_mcp::compact_followups::compact_validation_checks`

### Supporting Members

- `prism_mcp::views::task_validation_recipe_view`
- `prism_mcp::compact_tools::expand::structured_config_validation_checks`
- `prism_js::api_types::ChangeImpactView::validation_checks`
- `prism_js::api_types::TaskRiskView::validation_checks`

### Likely Tests

- `prism_coordination::tests::validation_policy_requires_approved_artifact_checks`
- `prism_mcp::compact_followups::tests::compact_validation_checks_trim_shell_chains`

### Evidence

- Prism::task_validation_recipe assembles checks, scored checks, related nodes, co-change neighbors, and recent failures from task blast radius.
- ProjectionIndex::validation_checks_for_lineages provides the lineage-backed validation signal that later surfaces through MCP views and risk summaries.
- promoted_validation_checks and compact_validation_checks turn the same validation recipe into promoted memory-backed guidance and compact staged follow-through.

### Risk Hint

- Validation drift usually appears as missing checks or stale command suggestions after impact or projection logic changes.

## workspace indexing and refresh

Handle: `concept://workspace_indexing_and_refresh`

prism-core orchestration that watches the workspace, runs parse and resolution pipelines, reanchors history and memory, and maintains the live PRISM session state.

Aliases: `indexing pipeline`, `refresh pipeline`, `workspace session`

### Core Members

- `prism_core`
- `prism_core::indexer::WorkspaceIndexer`
- `prism_parser`
- `prism_store`
- `prism_history`

### Supporting Members

- `prism::document::docs::SPEC_md::1_crate_architecture`

### Related Concepts

- depended on by: `CLI command and parse surface` (`concept://cli_command_and_parse_surface`)
- depended on by: `CLI surface` (`concept://cli_surface`)
- depends on: `language adapter family` (`concept://language_adapter_family`)
- depends on: `persistence and history layer` (`concept://persistence_and_history`)
- depends on: `structural IR and identity model` (`concept://structural_ir`)
- has part: `core indexing pipeline` (`concept://core_indexing_pipeline`)
- has part: `workspace session runtime` (`concept://workspace_session_runtime`)
- part of: `PRISM architecture` (`concept://prism_architecture`)

### Evidence

- prism-core owns indexer, parse_pipeline, resolution, reanchor, watch, session, memory_refresh, and patch_outcomes modules.
- Its dependency surface spans parser, language crates, store, history, memory, projections, query, coordination, curator, and agent layers.

### Risk Hint

- This integration hotspot is where refresh, watch, and patch-observation regressions usually surface first.

## workspace session refresh runtime

Handle: `concept://workspace_session_refresh_runtime`

prism-core modules that maintain the live workspace session, track filesystem dirtiness, and run refresh cycles through file watching and guarded session state.

Aliases: `session refresh runtime`, `fs refresh runtime`

### Core Members

- `prism_core::session`
- `prism_core::watch`

### Related Concepts

- part of: `workspace session runtime` (`concept://workspace_session_runtime`)

### Evidence

- `session.rs` owns `WorkspaceSession` and refresh-state bookkeeping, while `watch.rs` drives filesystem-triggered refreshes and guarded snapshot replacement.
- This is the live runtime half of workspace session handling.

### Risk Hint

- If this runtime drifts, PRISM may look live while serving stale snapshots or racing refresh state.

## workspace session runtime

Handle: `concept://workspace_session_runtime`

Session-oriented prism-core modules that hydrate WorkspaceSession state, refresh memory snapshots, and record patch outcomes around the live workspace.

Aliases: `workspace session`, `session refresh runtime`

### Core Members

- `prism_core::session`
- `prism_core::memory_refresh`
- `prism_core::patch_outcomes`

### Supporting Members

- `prism_core::indexer::WorkspaceIndexer`

### Related Concepts

- depended on by: `MCP runtime lifecycle` (`concept://mcp_runtime_lifecycle`)
- depends on: `memory system` (`concept://memory_system`)
- depends on: `persistence and history layer` (`concept://persistence_and_history`)
- has part: `memory refresh and patch outcome recording` (`concept://memory_refresh_and_patch_outcome_recording`)
- has part: `workspace session refresh runtime` (`concept://workspace_session_refresh_runtime`)
- part of: `workspace indexing and refresh` (`concept://workspace_indexing_and_refresh`)

### Evidence

- prism-core exposes WorkspaceSession from session.rs and pairs it with memory_refresh and patch_outcomes to keep the live session synchronized with persisted state and observed changes.
- This slice is distinct from raw indexing because it manages hydrated runtime state after the graph exists.

### Risk Hint

- Session reload drift here leads to stale runtime views and incorrect downstream mutation/query behavior.

