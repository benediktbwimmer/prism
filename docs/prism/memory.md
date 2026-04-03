# PRISM Memory

> Generated from repo-scoped PRISM memory events.
> Return to the concise entrypoint in `../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:fec6a17f82107cea483754a083acd19f2dcad566a95246cf6b3387c01434b2d5`
- Source logical timestamp: `1774903041`
- Source snapshot: `27 active memories, 27 memory events`

## Overview

- Active repo memories: 27
- Repo memory events logged: 27

## Published Memories

- `structural:9`: Curator-promoted structural memories should carry structuralRule metadata and structural recall should derive rule kinds plus evidence strength so promoted rules outrank generic notes for focused repo recall.
- `structural:10`: Text-fragment gather/open nextAction should branch on the related-handle surface: semantic promotion should point to prism_workset/prism_open on the semantic handle, while document-only follow-ups should keep the generic slice guidance.
- `memory:21`: Curator context already loads focused episodic memories into ctx.memories, but synthesized proposals still ignore that input; curator improvement can target synthesis and query visibility without changing memory-store plumbing.
- `structural:11`: Semantic recall stays local-first by default. The remote OpenAI path is opt-in via the `openai-embeddings` cargo feature plus `PRISM_SEMANTIC_BACKEND=openai` and an API key (`PRISM_OPENAI_API_KEY` or `OPENAI_API_KEY`); when unavailable, semantic recall falls back to the local hashed+alias baseline.
- `structural:12`: Curator memory promotion now carries CandidateMemoryEvidence.memory_ids end to end: strong episodic memories synthesize structural proposals with source memory ids, promoted repo memories keep those ids in metadata.evidence.memoryIds, and the promoted memory event records them in promotedFrom.
- `semantic:1`: Current Prism already compresses multi-artifact meaning through target-scoped views such as worksets and validation recipes, but those groupings do not yet have durable concept identity. A semantic codec should therefore be a first-class concept layer built on projections/query surfaces rather than a rename of existing per-target bundles.
- `structural:13`: Compact staged results can stay token-light and more parallel-friendly by emitting suggestedActions with tool/handle/mode-kind fields, plus promotedHandle on gather/open when an exact text hit lifts to a stronger semantic target.
- `memory:24`: Compact Perception Lenses v1 surfaces impact, timeline, and memory through prism_expand handles, and prism_task_brief is the compact coordination entrypoint for blockers, outcomes, validations, and next reads.
- `structural:14`: When a PRISM MCP tool is public, keep server_surface, tool_schemas, schema_examples, prism-js API docs, and tool-count tests in sync; partial exposure creates misleading surface drift.
- `structural:15`: Semantic memory stays local-first by default. Remote OpenAI embeddings are optional behind SemanticMemoryConfig and PRISM_SEMANTIC_BACKEND=openai, and recall must fall back cleanly to local alias-plus-hash scoring when the runtime is unavailable.
- `memory:26`: AGENTS.md now instructs agents to use prism_concept for broad repo-native nouns before falling back to symbol or text search, and to capture strong concept candidates as anchored episodic memory until Prism exposes an explicit concept-pack promotion mutation.
- `memory:28`: Repo concept packets are now canonicalized through `.prism/concepts/events.jsonl`; workspace load/reload overlays replayed concept events onto the derived concept layer, and concept mutations update both the repo log and the cached projection snapshot so the live daemon sees curated concepts immediately.
- `memory:34`: Repo-persisted memory already carries scope, provenance, evidence hooks, and promotion/supersession metadata, but repo-persisted concepts are still thinner: they are portable and reviewable, yet they lack first-class provenance fields, staleness/supersession controls, and a local/session/repo promotion ladder.
- `memory:67`: The repo now has a durable root architecture concept at concept://prism_architecture with subsystem children for IR, adapters, persistence/history, indexing/refresh, memory, coordination, projections/query, concept publication, inference/curation, MCP, compact tools, dashboard, and CLI surfaces.
- `memory:68`: Validation is a cross-cutting architectural concern in this repo rather than a test-only sidecar; the architecture concept graph explicitly attaches validated_by edges from structural, memory, coordination, projection/query, concept-publication, MCP, compact-tool, and dashboard concepts to concept://validation_and_dogfooding.
- `memory:69`: The architecture graph now has module-level children under the major subsystem concepts: workspace_indexing_and_refresh splits into core_indexing_pipeline and workspace_session_runtime; concept_and_publication_pipeline now contains repo_publication_guards; mcp_runtime_surface splits into mcp_query_and_resource_serving, mcp_mutation_and_session_host, and mcp_runtime_lifecycle; memory_system splits into memory_recall_and_scoring, memory_outcomes_and_session_history, and semantic_and_structural_memory_models; coordination_and_plan_runtime splits into coordination_state_model and coordination_operations_and_policy.
- `memory:70`: The read-side architecture now separates semantic_projection_indexes, query_symbol_and_change_contexts, query_coordination_and_plan_views, and javascript_query_abi beneath concept://projection_and_query_layer, which means later agents can distinguish projection materialization, local read bundles, coordination query overlays, and JS-facing ABI concerns without reopening those crates.
- `memory:73`: The language-adapter subsystem now has a stable two-way internal split between code adapters (`concept://code_language_adapters`) and document/config adapters (`concept://document_and_config_adapters`), while the curation/publication side is split between curator execution (`concept://curator_execution_flow`) and filesystem publication/layout (`concept://plan_and_repo_layout_publication`). Future architecture reasoning should reuse those four concepts instead of reopening crates to rediscover the same boundaries.
- `memory:74`: The repo-wide architecture hierarchy is now materially balanced at the top level: every direct child of `concept://prism_architecture` has at least one internal child concept, and most major subsystems now split into reusable module-level packets. Important newer boundaries are the IR trio (`concept://graph_identity_and_parse_ir`, `concept://anchor_change_and_lineage_ir`, `concept://plan_and_coordination_ir`), the persistence trio (`concept://sqlite_and_graph_persistence`, `concept://memory_projection_persistence`, `concept://history_snapshot_and_resolution`), the compact-tool pair (`concept://compact_discovery_and_opening`, `concept://compact_expansion_and_concept_views`), the CLI pair, the dashboard pair, the validation pair, and the inference branch `concept://inferred_edge_runtime_and_session_store`. Future sessions should traverse these concept packets before reopening crate facades.
- `memory:75`: The architecture map now has a third layer inside the densest branches. `semantic_projection_indexes` splits into packet materialization (`concept://concept_relation_and_intent_projections`) and derived co-change/validation indexes (`concept://cochange_and_validation_projection_indexes`); the query layer splits symbol/source, impact/outcome, coordination/runtime, and plan-insight reads; the MCP runtime splits query execution, resource serving, mutation/schema contracts, session-state mutation runtime, daemon/process lifecycle, and server-health views; and the concept-publication pipeline now exposes explicit concept/relation event streams plus published-knowledge/memory logs. Future architecture reasoning should descend through those child concepts before reopening crate facades.
- `memory:78`: Architecture split refined and verified: prism-memory now has durable child branches for recall/scoring, outcome-and-session history, and semantic/structural models; prism-core splits workspace refresh into core indexing versus workspace session runtime; prism-coordination splits into state model versus operations/policy, each with two internal child concepts. Later broad reasoning about memory, workspace refresh, or coordination should start from these concept branches instead of reopening crate roots.
- `memory:81`: Architecture hierarchy deepened in three broad areas. The adapter stack now has a shared parser-contract layer plus concrete Rust, Python, Markdown, and structured-config sub-pipelines. The curator flow now splits into backend execution/prompting versus rule synthesis/proposal typing. The compact tool surface now splits into locate/text-candidate ranking, open/workset follow-through, concept/expand decode, and task-brief coordination summaries. Future architecture reasoning should traverse those concepts before reopening the adapter, curator, or compact-tool code directly.
- `memory:01kmwfk9m5skznfys67588df6j`: Repo-persisted Prism entities that can be created by independent runtimes must use globally unique sortable IDs instead of process-local counters or raw timestamps. Semantic NodeIds stay path-based and session handles stay runtime-local; durable shared identities such as memories, plans, coordination tasks, claims, artifacts, reviews, lineage records, curator jobs, inferred edges, observed events, sessions, and emitted outcome or memory events should use the shared sortable ID helper.
- `memory:01kmwpc4w3rp0pfy6pce3w3816`: Parallel prism-mcp tests should not derive temporary workspace directories from wall-clock timestamps alone. A live refactor run surfaced SQLite `database is locked` failures when extracted test modules ran concurrently; switching `temp_workspace` to `new_sortable_token()` removes the naming-collision path and makes the shared test harness safer under parallel execution.
- `memory:01kn07e726myjcgr1rsj33nf6j`: PRISM daemon health depends on keeping authoritative runtime state, bounded serving projections, and cold analytical evidence separate. Symbol-level co-change is analytical evidence, not hot runtime history: persisting unbounded pairwise co-change or hydrating it into HistoryStore makes cache growth, startup memory, and refresh lock contention explode. Co-change should stay bounded in serving projections, while any colder evidence must remain off the daemon hot path.
- `memory:01kn07e79gh5je3z1zk8zgg3qf`: Native plan-node completion must honor successful validation outcomes correlated directly to the node id, because MCP commonly uses a native node id as the current task while recording `test_ran` and `fix_validated` events with no anchors. If completion logic only accepts anchor-matched evidence or real coordination-task ids, `plan_node_update` can reject completion even after the exact required validations were run and recorded.
- `memory:01kn07e7gtqccwq2yj9fhy2r1k`: After the co-change rewrite, the next major daemon-memory risks are eager curator hydration and oversized PatchApplied outcomes, with full history-event hydration still contributing. The live cache showed a single curator snapshot row of about 29 MB JSON that is always loaded into CuratorHandle state, and PatchApplied events dominated outcome_event_log because they embed large `metadata.changedSymbols` arrays. Future daemon-memory work should target those surfaces before chasing SQLite mapping effects.

## structural:9

Kind: structural  
Source: user  
Trust: 0.50  
Created at: `1774703989`

Curator-promoted structural memories should carry structuralRule metadata and structural recall should derive rule kinds plus evidence strength so promoted rules outrank generic notes for focused repo recall.

### Anchors

- `lineage:lineage:5723`
- `node:prism_memory:prism_memory::structural_features::derive_structural_features:function`

### Event Summary

- Latest event id: `memory-event:1774703989129467000`
- Latest recorded at: `1774703989`
- Event count: `1`

## structural:10

Kind: structural  
Source: agent  
Trust: 0.92  
Created at: `1774704609`

Text-fragment gather/open nextAction should branch on the related-handle surface: semantic promotion should point to prism_workset/prism_open on the semantic handle, while document-only follow-ups should keep the generic slice guidance.

### Anchors

- `lineage:lineage:10421`
- `node:prism_mcp:prism_mcp::compact_tools::text_fragments::text_fragment_staged_next_action:function`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774704609494777000`
- Latest recorded at: `1774704609`
- Event count: `1`

## memory:21

Kind: episodic  
Source: agent  
Trust: 0.76  
Created at: `1774705077`

Curator context already loads focused episodic memories into ctx.memories, but synthesized proposals still ignore that input; curator improvement can target synthesis and query visibility without changing memory-store plumbing.

### Anchors

- `lineage:lineage:1434`
- `lineage:lineage:1832`
- `node:prism_core:prism_core::curator_support::build_curator_context:function`
- `node:prism_curator:prism_curator::synthesis::synthesize_curator_run:function`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774705077914438000`
- Latest recorded at: `1774705077`
- Event count: `1`

## structural:11

Kind: structural  
Source: user  
Trust: 0.50  
Created at: `1774705324`

Semantic recall stays local-first by default. The remote OpenAI path is opt-in via the `openai-embeddings` cargo feature plus `PRISM_SEMANTIC_BACKEND=openai` and an API key (`PRISM_OPENAI_API_KEY` or `OPENAI_API_KEY`); when unavailable, semantic recall falls back to the local hashed+alias baseline.

### Anchors

- `lineage:lineage:10432`
- `node:prism_memory:prism_memory::semantic::config::SemanticMemoryConfig:struct`

### Event Summary

- Latest event id: `memory-event:1774705324513657000`
- Latest recorded at: `1774705324`
- Event count: `1`

## structural:12

Kind: structural  
Source: agent  
Trust: 0.90  
Created at: `1774705615`

Curator memory promotion now carries CandidateMemoryEvidence.memory_ids end to end: strong episodic memories synthesize structural proposals with source memory ids, promoted repo memories keep those ids in metadata.evidence.memoryIds, and the promoted memory event records them in promotedFrom.

### Anchors

- `lineage:lineage:1832`
- `lineage:lineage:3686`
- `lineage:lineage:3898`
- `node:prism_curator:prism_curator::synthesis::synthesize_curator_run:function`
- `node:prism_mcp:prism_mcp::host_mutations::QueryHost::promote_curator_memory:method`
- `node:prism_mcp:prism_mcp::memory_metadata::curator_memory_metadata:function`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774705615956861000`
- Latest recorded at: `1774705615`
- Event count: `1`

## semantic:1

Kind: semantic  
Source: agent  
Trust: 0.50  
Created at: `1774705920`

Current Prism already compresses multi-artifact meaning through target-scoped views such as worksets and validation recipes, but those groupings do not yet have durable concept identity. A semantic codec should therefore be a first-class concept layer built on projections/query surfaces rather than a rename of existing per-target bundles.

### Anchors

- `lineage:lineage:6032`
- `lineage:lineage:9863`
- `node:prism_mcp:prism_mcp::compact_tools::workset::workset_context_for_target:function`
- `node:prism_query:prism_query::impact::Prism::validation_recipe:method`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774705920524766000`
- Latest recorded at: `1774705920`
- Event count: `1`

## structural:13

Kind: structural  
Source: agent  
Trust: 0.93  
Created at: `1774706623`

Compact staged results can stay token-light and more parallel-friendly by emitting suggestedActions with tool/handle/mode-kind fields, plus promotedHandle on gather/open when an exact text hit lifts to a stronger semantic target.

### Anchors

- `lineage:lineage:10532`
- `lineage:lineage:10547`
- `node:prism_js:prism_js::api_types::AgentSuggestedActionView:struct`
- `node:prism_mcp:prism_mcp::compact_tools::open::compact_open_suggested_actions:function`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774706623212175000`
- Latest recorded at: `1774706623`
- Event count: `1`

## memory:24

Kind: episodic  
Source: agent  
Trust: 0.93  
Created at: `1774706915`

Compact Perception Lenses v1 surfaces impact, timeline, and memory through prism_expand handles, and prism_task_brief is the compact coordination entrypoint for blockers, outcomes, validations, and next reads.

### Anchors

- `lineage:lineage:10635`
- `lineage:lineage:9787`
- `node:prism_mcp:prism_mcp::server_surface::PrismMcpServer::prism_expand:method`
- `node:prism_mcp:prism_mcp::server_surface::PrismMcpServer::prism_task_brief:method`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774706915722273000`
- Latest recorded at: `1774706915`
- Event count: `1`

## structural:14

Kind: structural  
Source: agent  
Trust: 0.93  
Created at: `1774707283`

When a PRISM MCP tool is public, keep server_surface, tool_schemas, schema_examples, prism-js API docs, and tool-count tests in sync; partial exposure creates misleading surface drift.

### Anchors

- `lineage:lineage:10635`
- `node:prism_mcp:prism_mcp::server_surface::PrismMcpServer::prism_task_brief:method`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774707283369544000`
- Latest recorded at: `1774707283`
- Event count: `1`

## structural:15

Kind: structural  
Source: agent  
Trust: 0.92  
Created at: `1774707283`

Semantic memory stays local-first by default. Remote OpenAI embeddings are optional behind SemanticMemoryConfig and PRISM_SEMANTIC_BACKEND=openai, and recall must fall back cleanly to local alias-plus-hash scoring when the runtime is unavailable.

### Anchors

- `lineage:lineage:10432`
- `node:prism_memory:prism_memory::semantic::config::SemanticMemoryConfig:struct`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774707283449869000`
- Latest recorded at: `1774707283`
- Event count: `1`

## memory:26

Kind: episodic  
Source: agent  
Trust: 0.88  
Created at: `1774707738`

AGENTS.md now instructs agents to use prism_concept for broad repo-native nouns before falling back to symbol or text search, and to capture strong concept candidates as anchored episodic memory until Prism exposes an explicit concept-pack promotion mutation.

### Anchors

- `lineage:lineage:10768`
- `lineage:lineage:29`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774707738730027000`
- Latest recorded at: `1774707738`
- Event count: `1`

## memory:28

Kind: episodic  
Source: agent  
Trust: 0.90  
Created at: `1774709179`

Repo concept packets are now canonicalized through `.prism/concepts/events.jsonl`; workspace load/reload overlays replayed concept events onto the derived concept layer, and concept mutations update both the repo log and the cached projection snapshot so the live daemon sees curated concepts immediately.

### Anchors

- `lineage:lineage:10786`
- `lineage:lineage:10792`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774709179302009000`
- Latest recorded at: `1774709179`
- Event count: `1`

## memory:34

Kind: episodic  
Source: agent  
Trust: 0.95  
Created at: `1774710027`

Repo-persisted memory already carries scope, provenance, evidence hooks, and promotion/supersession metadata, but repo-persisted concepts are still thinner: they are portable and reviewable, yet they lack first-class provenance fields, staleness/supersession controls, and a local/session/repo promotion ladder.

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774710027990289000`
- Latest recorded at: `1774710027`
- Event count: `1`

## memory:67

Kind: episodic  
Source: agent  
Trust: 0.94  
Created at: `1774733544`

The repo now has a durable root architecture concept at concept://prism_architecture with subsystem children for IR, adapters, persistence/history, indexing/refresh, memory, coordination, projections/query, concept publication, inference/curation, MCP, compact tools, dashboard, and CLI surfaces.

### Anchors

- `lineage:lineage:1533`
- `lineage:lineage:3894`
- `lineage:lineage:6074`
- `node:prism_core:prism_core:module`
- `node:prism_mcp:prism_mcp:module`
- `node:prism_query:prism_query:module`

### Publication

- lastReviewedAt: 1774733544
- publishedAt: 1774733544
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774733544609150000`
- Latest recorded at: `1774733544`
- Event count: `1`

## memory:68

Kind: episodic  
Source: agent  
Trust: 0.92  
Created at: `1774733554`

Validation is a cross-cutting architectural concern in this repo rather than a test-only sidecar; the architecture concept graph explicitly attaches validated_by edges from structural, memory, coordination, projection/query, concept-publication, MCP, compact-tool, and dashboard concepts to concept://validation_and_dogfooding.

### Anchors

- `lineage:lineage:3894`
- `lineage:lineage:5913`
- `lineage:lineage:6954`
- `node:prism:prism::document::docs::VALIDATION_md::7_validation_pipeline:markdown-heading`
- `node:prism_mcp:prism_mcp:module`
- `node:prism_projections:prism_projections:module`

### Publication

- lastReviewedAt: 1774733554
- publishedAt: 1774733554
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774733554300706000`
- Latest recorded at: `1774733554`
- Event count: `1`

## memory:69

Kind: episodic  
Source: agent  
Trust: 0.95  
Created at: `1774734024`

The architecture graph now has module-level children under the major subsystem concepts: workspace_indexing_and_refresh splits into core_indexing_pipeline and workspace_session_runtime; concept_and_publication_pipeline now contains repo_publication_guards; mcp_runtime_surface splits into mcp_query_and_resource_serving, mcp_mutation_and_session_host, and mcp_runtime_lifecycle; memory_system splits into memory_recall_and_scoring, memory_outcomes_and_session_history, and semantic_and_structural_memory_models; coordination_and_plan_runtime splits into coordination_state_model and coordination_operations_and_policy.

### Anchors

- `lineage:lineage:1096`
- `lineage:lineage:1533`
- `lineage:lineage:3894`
- `lineage:lineage:5611`
- `node:prism_coordination:prism_coordination:module`
- `node:prism_core:prism_core:module`
- `node:prism_mcp:prism_mcp:module`
- `node:prism_memory:prism_memory:module`

### Publication

- lastReviewedAt: 1774734024
- publishedAt: 1774734024
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774734024110743000`
- Latest recorded at: `1774734024`
- Event count: `1`

## memory:70

Kind: episodic  
Source: agent  
Trust: 0.94  
Created at: `1774734024`

The read-side architecture now separates semantic_projection_indexes, query_symbol_and_change_contexts, query_coordination_and_plan_views, and javascript_query_abi beneath concept://projection_and_query_layer, which means later agents can distinguish projection materialization, local read bundles, coordination query overlays, and JS-facing ABI concerns without reopening those crates.

### Anchors

- `lineage:lineage:2891`
- `lineage:lineage:5913`
- `lineage:lineage:6074`
- `node:prism_js:prism_js:module`
- `node:prism_projections:prism_projections:module`
- `node:prism_query:prism_query:module`

### Publication

- lastReviewedAt: 1774734024
- publishedAt: 1774734024
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774734024194651000`
- Latest recorded at: `1774734024`
- Event count: `1`

## memory:73

Kind: episodic  
Source: agent  
Trust: 0.90  
Created at: `1774734270`

The language-adapter subsystem now has a stable two-way internal split between code adapters (`concept://code_language_adapters`) and document/config adapters (`concept://document_and_config_adapters`), while the curation/publication side is split between curator execution (`concept://curator_execution_flow`) and filesystem publication/layout (`concept://plan_and_repo_layout_publication`). Future architecture reasoning should reuse those four concepts instead of reopening crates to rediscover the same boundaries.

### Anchors

- `lineage:lineage:11594`
- `lineage:lineage:1818`
- `lineage:lineage:5873`
- `node:prism_core:prism_core::published_plans:module`
- `node:prism_curator:prism_curator:module`
- `node:prism_parser:prism_parser:module`

### Publication

- lastReviewedAt: 1774734270
- publishedAt: 1774734270
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774734270073409000`
- Latest recorded at: `1774734270`
- Event count: `1`

## memory:74

Kind: episodic  
Source: agent  
Trust: 0.92  
Created at: `1774734719`

The repo-wide architecture hierarchy is now materially balanced at the top level: every direct child of `concept://prism_architecture` has at least one internal child concept, and most major subsystems now split into reusable module-level packets. Important newer boundaries are the IR trio (`concept://graph_identity_and_parse_ir`, `concept://anchor_change_and_lineage_ir`, `concept://plan_and_coordination_ir`), the persistence trio (`concept://sqlite_and_graph_persistence`, `concept://memory_projection_persistence`, `concept://history_snapshot_and_resolution`), the compact-tool pair (`concept://compact_discovery_and_opening`, `concept://compact_expansion_and_concept_views`), the CLI pair, the dashboard pair, the validation pair, and the inference branch `concept://inferred_edge_runtime_and_session_store`. Future sessions should traverse these concept packets before reopening crate facades.

### Anchors

- `lineage:lineage:1683`
- `lineage:lineage:2211`
- `lineage:lineage:3566`
- `lineage:lineage:6426`
- `lineage:lineage:877`
- `lineage:lineage:951`
- `lineage:lineage:9774`
- `node:prism_agent:prism_agent:module`
- `node:prism_cli:prism_cli::main:function`
- `node:prism_core:prism_core::validation_feedback:module`
- `node:prism_ir:prism_ir:module`
- `node:prism_mcp:prism_mcp::compact_tools:module`
- `node:prism_mcp:prism_mcp::dashboard_router:module`
- `node:prism_store:prism_store:module`

### Publication

- lastReviewedAt: 1774734719
- publishedAt: 1774734719
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774734719845440000`
- Latest recorded at: `1774734719`
- Event count: `1`

## memory:75

Kind: episodic  
Source: agent  
Trust: 0.92  
Created at: `1774735144`

The architecture map now has a third layer inside the densest branches. `semantic_projection_indexes` splits into packet materialization (`concept://concept_relation_and_intent_projections`) and derived co-change/validation indexes (`concept://cochange_and_validation_projection_indexes`); the query layer splits symbol/source, impact/outcome, coordination/runtime, and plan-insight reads; the MCP runtime splits query execution, resource serving, mutation/schema contracts, session-state mutation runtime, daemon/process lifecycle, and server-health views; and the concept-publication pipeline now exposes explicit concept/relation event streams plus published-knowledge/memory logs. Future architecture reasoning should descend through those child concepts before reopening crate facades.

### Anchors

- `lineage:lineage:10673`
- `lineage:lineage:10790`
- `lineage:lineage:12779`
- `lineage:lineage:3460`
- `lineage:lineage:4145`
- `lineage:lineage:5310`
- `lineage:lineage:6149`
- `node:prism_core:prism_core::concept_events:module`
- `node:prism_mcp:prism_mcp::daemon_mode:module`
- `node:prism_mcp:prism_mcp::query_runtime:module`
- `node:prism_mcp:prism_mcp::tool_args:module`
- `node:prism_projections:prism_projections::concepts:module`
- `node:prism_query:prism_query::plan_completion:module`
- `node:prism_query:prism_query::symbol:module`

### Publication

- lastReviewedAt: 1774735144
- publishedAt: 1774735144
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774735144889720000`
- Latest recorded at: `1774735144`
- Event count: `1`

## memory:78

Kind: episodic  
Source: agent  
Trust: 0.90  
Created at: `1774735787`

Architecture split refined and verified: prism-memory now has durable child branches for recall/scoring, outcome-and-session history, and semantic/structural models; prism-core splits workspace refresh into core indexing versus workspace session runtime; prism-coordination splits into state model versus operations/policy, each with two internal child concepts. Later broad reasoning about memory, workspace refresh, or coordination should start from these concept branches instead of reopening crate roots.

### Anchors

- `lineage:lineage:1139`
- `lineage:lineage:1596`
- `lineage:lineage:5681`
- `node:prism_coordination:prism_coordination::state:module`
- `node:prism_core:prism_core::session:module`
- `node:prism_memory:prism_memory::session:module`

### Publication

- lastReviewedAt: 1774735787
- publishedAt: 1774735787
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774735787389338000`
- Latest recorded at: `1774735787`
- Event count: `1`

## memory:81

Kind: episodic  
Source: agent  
Trust: 0.92  
Created at: `1774736197`

Architecture hierarchy deepened in three broad areas. The adapter stack now has a shared parser-contract layer plus concrete Rust, Python, Markdown, and structured-config sub-pipelines. The curator flow now splits into backend execution/prompting versus rule synthesis/proposal typing. The compact tool surface now splits into locate/text-candidate ranking, open/workset follow-through, concept/expand decode, and task-brief coordination summaries. Future architecture reasoning should traverse those concepts before reopening the adapter, curator, or compact-tool code directly.

### Anchors

- `lineage:lineage:1818`
- `lineage:lineage:5873`
- `lineage:lineage:9774`
- `node:prism_curator:prism_curator:module`
- `node:prism_mcp:prism_mcp::compact_tools:module`
- `node:prism_parser:prism_parser:module`

### Publication

- lastReviewedAt: 1774736197
- publishedAt: 1774736197
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:1774736197305668000`
- Latest recorded at: `1774736197`
- Event count: `1`

## memory:01kmwfk9m5skznfys67588df6j

Kind: episodic  
Source: agent  
Trust: 0.93  
Created at: `1774777378`

Repo-persisted Prism entities that can be created by independent runtimes must use globally unique sortable IDs instead of process-local counters or raw timestamps. Semantic NodeIds stay path-based and session handles stay runtime-local; durable shared identities such as memories, plans, coordination tasks, claims, artifacts, reviews, lineage records, curator jobs, inferred edges, observed events, sessions, and emitted outcome or memory events should use the shared sortable ID helper.

### Anchors

- `lineage:lineage:16166`
- `node:prism_ir:prism_ir::durable_ids::new_prefixed_id:function`

### Publication

- lastReviewedAt: 1774777378
- publishedAt: 1774777378
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:01kmwfk9m5pgpa4sf6hm8mnfdw`
- Latest recorded at: `1774777378`
- Event count: `1`

## memory:01kmwpc4w3rp0pfy6pce3w3816

Kind: episodic  
Source: agent  
Trust: 0.88  
Created at: `1774784484`

Parallel prism-mcp tests should not derive temporary workspace directories from wall-clock timestamps alone. A live refactor run surfaced SQLite `database is locked` failures when extracted test modules ran concurrently; switching `temp_workspace` to `new_sortable_token()` removes the naming-collision path and makes the shared test harness safer under parallel execution.

### Anchors

- `lineage:lineage:01kmwnhy16pe16x0d5w32x0nsm`
- `node:prism_mcp:prism_mcp::tests_support::temp_workspace:function`

### Publication

- lastReviewedAt: 1774784484
- publishedAt: 1774784484
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:01kmwpc4w33tjveeqr3q1sbp84`
- Latest recorded at: `1774784484`
- Event count: `1`

## memory:01kn07e726myjcgr1rsj33nf6j

Kind: episodic  
Source: agent  
Trust: 0.95  
Created at: `1774903041`

PRISM daemon health depends on keeping authoritative runtime state, bounded serving projections, and cold analytical evidence separate. Symbol-level co-change is analytical evidence, not hot runtime history: persisting unbounded pairwise co-change or hydrating it into HistoryStore makes cache growth, startup memory, and refresh lock contention explode. Co-change should stay bounded in serving projections, while any colder evidence must remain off the daemon hot path.

### Anchors

- `file:104`
- `file:231`
- `file:260`
- `file:317`

### Publication

- lastReviewedAt: 1774903041
- publishedAt: 1774903041
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:01kn07e726z4zb1j640w19dx9b`
- Latest recorded at: `1774903041`
- Event count: `1`

## memory:01kn07e79gh5je3z1zk8zgg3qf

Kind: episodic  
Source: agent  
Trust: 0.93  
Created at: `1774903041`

Native plan-node completion must honor successful validation outcomes correlated directly to the node id, because MCP commonly uses a native node id as the current task while recording `test_ran` and `fix_validated` events with no anchors. If completion logic only accepts anchor-matched evidence or real coordination-task ids, `plan_node_update` can reject completion even after the exact required validations were run and recorded.

### Anchors

- `file:164`
- `file:187`
- `file:241`

### Publication

- lastReviewedAt: 1774903041
- publishedAt: 1774903041
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:01kn07e79g04xxxwr9c11mramm`
- Latest recorded at: `1774903041`
- Event count: `1`

## memory:01kn07e7gtqccwq2yj9fhy2r1k

Kind: episodic  
Source: agent  
Trust: 0.90  
Created at: `1774903041`

After the co-change rewrite, the next major daemon-memory risks are eager curator hydration and oversized PatchApplied outcomes, with full history-event hydration still contributing. The live cache showed a single curator snapshot row of about 29 MB JSON that is always loaded into CuratorHandle state, and PatchApplied events dominated outcome_event_log because they embed large `metadata.changedSymbols` arrays. Future daemon-memory work should target those surfaces before chasing SQLite mapping effects.

### Anchors

- `file:260`
- `file:72`
- `file:75`
- `file:81`

### Publication

- lastReviewedAt: 1774903041
- publishedAt: 1774903041
- status: `active`

### Provenance

- kind: `manual_memory`
- origin: `manual_store`

### Event Summary

- Latest event id: `memory-event:01kn07e7gt5061cfzeagyks7gd`
- Latest recorded at: `1774903041`
- Event count: `1`

