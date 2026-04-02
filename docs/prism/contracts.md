# PRISM Contracts

> Generated from repo-scoped PRISM contract knowledge.
> Return to the concise entrypoint in `../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:331e68edf71fbdb6a06a7b24c390ab85dd4e102d09a20086ac199072dd4fa6c7`
- Source logical timestamp: `1775113302`
- Source snapshot: `95` concepts, `206` relations, `8` contracts

## Overview

- Active repo contracts: 8
- Active repo concepts: 95
- Active repo relations: 206

## Published Contracts

- `compact tools surface` (`contract://compact_tools_surface`): PRISM exposes locate, open, workset, and expand as the staged compact default path for bounded repo interaction, with handle-centered results and conservative follow-through.
- `coordination runtime continuity` (`contract://coordination_runtime_continuity`): PRISM coordination state remains a live runtime authority that persists with explicit repo/worktree/session context, preserves lineage-scoped intent across renames and reloads, and hydrates published plan state through the backend facade instead of ad hoc snapshot ownership.
- `facade modularity rule` (`contract://facade_modularity_rule`): `main.rs` and `lib.rs` stay thin facades for wiring, module boundaries, and public surface curation rather than accumulating substantive domain logic.
- `javascript query ABI` (`contract://javascript_query_abi`): The stable prism.* JavaScript query surface stays synchronized across runtime prelude behavior, API-reference documentation, and pre-execution typechecking so agents can script PRISM without reverse-engineering host internals.
- `persistence split` (`contract://persistence_split`): PRISM preserves a three-plane persistence model where repo-published `.prism` knowledge, shared mutable runtime state, and process-local cache remain distinct authorities.
- `repo published knowledge replay` (`contract://repo_published_knowledge_replay`): Repo-scoped published knowledge is committed as append-only event logs under .prism and hydrates the live runtime from those logs on reload instead of treating hydrated packets or snapshots as authored truth.
- `typed MCP schema surface` (`contract://typed_mcp_schema_surface`): PRISM MCP exposes bindable root tool schemas, exact action-specific schema resources, and actionable validation repair so clients can call mutation and session tools safely without guessing payload shapes.
- `workspace refresh runtime` (`contract://workspace_refresh_runtime`): PRISM keeps live workspace serving on an incremental refresh path: filesystem changes mark dirty regions, request paths avoid full persisted reloads in steady state, and persisted auxiliary state reloads only when freshness requires it.

## compact tools surface

Handle: `contract://compact_tools_surface`

PRISM exposes locate, open, workset, and expand as the staged compact default path for bounded repo interaction, with handle-centered results and conservative follow-through.

Kind: interface  
Status: active  
Stability: internal

Aliases: `compact tools contract`, `staged compact surface`, `locate/open/workset/expand contract`

### Subject

Anchors:
- `file:21`
- `file:187`
Concept Handles:
- `concept://compact_tools`

### Guarantees

- `staged_compact_entrypoints`: Locate, open, workset, and expand remain the staged default entrypoints for bounded PRISM repo interaction before falling back to richer raw query surfaces. (scope: agent_default_path) [hard]
  evidence ref: `agents-staged-default-path`
  evidence ref: `server-compact-entrypoints`
- `compact_results_stay_bounded_and_handle_centered`: Compact tool results stay bounded, favor semantic handles, and avoid broad raw payloads as the default response shape. (scope: payload_budget) [hard]
  evidence ref: `compact-tools-concept`
  evidence ref: `compact-open-related-handles`
- `follow_through_stays_next_action_oriented`: Compact follow-through prefers conservative next reads, likely tests, and nearby handles over exhaustive context dumps. (scope: follow_through) [soft]
  evidence ref: `compact-tools-concept`
  evidence ref: `compact-workset-follow-through`

### Assumptions

- Agents may still use prism_query when the compact staged surface cannot express the needed read precisely.
- Deeper or broader context may require an explicit expand or open retry instead of widening compact defaults.

### Consumers

#### Target 1

Concept Handles:
- `concept://mcp_runtime_surface`
- `concept://prism_architecture`

### Validations

- `cargo test -p prism-mcp compact_locate_promotes_numbered_markdown_headings_to_semantic_handles`: Locate should promote semantic handles instead of leaving markdown headings as raw text hits.
  anchor: `node:prism_mcp:prism_mcp::tests::compact_locate_promotes_numbered_markdown_headings_to_semantic_handles:function`
- `cargo test -p prism-mcp compact_open_returns_compact_related_handles`: Open should return compact related handles for bounded follow-through.
  anchor: `node:prism_mcp:prism_mcp::tests::compact_open_returns_compact_related_handles:function`
- `cargo test -p prism-mcp compact_open_edit_returns_enough_body_context_to_start_editing`: Open edit mode should return enough bounded context to begin editing without a broad reread.
  anchor: `node:prism_mcp:prism_mcp::tests::compact_open_edit_returns_enough_body_context_to_start_editing:function`
- `cargo test -p prism-mcp compact_workset_prioritizes_contract_consumers_and_validation_targets`: Workset should surface nearby contract consumers and validation targets instead of generic unrelated reads.
  anchor: `node:prism_mcp:prism_mcp::tests::compact_workset_prioritizes_contract_consumers_and_validation_targets:function`

### Compatibility

#### Additive

- Adding new compact helpers or follow-up hints without widening existing result shapes by default.

#### Risky

- Returning much larger payloads, bypassing semantic handles, or changing the staged default path so agents have to rediscover raw surfaces first.

#### Breaking

- Removing the staged compact entrypoints or turning them into broad unbounded transport wrappers rather than bounded guidance surfaces.

### Evidence

- AGENTS.md defines the default agent path as PRISM-first and staged.
- The compact-tools concept packet and server surface treat locate/open/workset/expand as the staged agent-facing surface.
- The compact-tools test cluster validates semantic handles, bounded related handles, edit-sized context, and contract-aware workset follow-through.

## coordination runtime continuity

Handle: `contract://coordination_runtime_continuity`

PRISM coordination state remains a live runtime authority that persists with explicit repo/worktree/session context, preserves lineage-scoped intent across renames and reloads, and hydrates published plan state through the backend facade instead of ad hoc snapshot ownership.

Kind: lifecycle  
Status: active  
Stability: internal

Aliases: `coordination continuity`, `plan runtime continuity`, `coordination persistence contract`

### Subject

Anchors:
- `file:283`
- `file:275`
- `file:164`
- `file:90`
Concept Handles:
- `concept://coordination_and_plan_runtime`
- `concept://mcp_mutation_and_session_host`
- `concept://persistence_split`

### Guarantees

- `coordination_persistence_records_explicit_runtime_context`: Persisted coordination state records explicit repo, worktree, session, and instance context so one runtime can host multiple worktree contexts without smearing authoritative coordination history across them. (scope: persistence_context) [hard]
  evidence ref: `optimizations-worktree-contexts`
  evidence ref: `coordination-session-scope-tests`
- `coordination_mutations_use_live_runtime_authority`: Coordination mutations operate against the live runtime state and do not require a persisted reload hop before valid mutations become authoritative. (scope: live_runtime_authority) [hard]
  evidence ref: `spec-coordination-ownership`
  evidence ref: `live-runtime-coordination-tests`
- `lineage_scoped_coordination_survives_reload_and_repo_motion`: Claims, tasks, and related coordination intent continue to resolve through lineage and hydrated plan state across rename and reload flows instead of depending on stale runtime-only node ids. (scope: reload_and_rename_continuity) [hard]
  evidence ref: `coordination-rename-reload-tests`
  evidence ref: `coordination-backend-hydration`

### Assumptions

- Published plan state and shared coordination read models are persisted through the coordination backend facade rather than bypassing it with ad hoc side channels.
- Runtime overlays may be rebuilt, but authored coordination intent remains tied to explicit context and lineage-aware anchors.

### Consumers

#### Target 1

Concept Handles:
- `concept://mcp_runtime_surface`
- `concept://workspace_session_runtime`
- `concept://compact_expansion_and_concept_views`

### Validations

- `cargo test -p prism-core coordination_persistence_backend_wraps_store_and_repo_published_plans`: The coordination backend hydrates stored runtime state together with repo-published plan state.
  anchor: `node:prism_core:prism_core::tests::coordination_persistence_backend_wraps_store_and_repo_published_plans:function`
- `cargo test -p prism-core coordination_mutations_use_live_runtime_state_without_forcing_persisted_reload`: Coordination mutations use live runtime authority without forcing a persisted reload.
  anchor: `node:prism_core:prism_core::tests::coordination_mutations_use_live_runtime_state_without_forcing_persisted_reload:function`
- `cargo test -p prism-core reload_preserves_coordination_claim_resolution_through_rename`: Claims and tasks survive rename and reload by rebinding through lineage-scoped coordination anchors.
  anchor: `node:prism_core:prism_core::tests::reload_preserves_coordination_claim_resolution_through_rename:function`
- `cargo test -p prism-mcp workspace_coordination_persistence_records_mcp_session_scope`: MCP coordination persistence records the active session context in authoritative persistence.
  anchor: `node:prism_mcp:prism_mcp::tests::workspace_coordination_persistence_records_mcp_session_scope:function`
- `cargo test -p prism-mcp rejected_coordination_mutations_keep_mcp_session_scope_in_authoritative_persistence`: Rejected coordination mutations still preserve the originating MCP session scope in authoritative persistence.
  anchor: `node:prism_mcp:prism_mcp::tests::rejected_coordination_mutations_keep_mcp_session_scope_in_authoritative_persistence:function`

### Compatibility

#### Additive

- Adding new coordination read models or runtime overlays is compatible when explicit persistence context and lineage-aware authored intent remain authoritative.

#### Risky

- Dropping explicit session or worktree context, or forcing mutations through stale persisted snapshots before they become authoritative, is risky for multi-context correctness.

#### Breaking

- Making runtime-only node ids or snapshot materializations the sole source of coordination truth breaks continuity across reloads, renames, and multi-worktree serving.

#### Migrating

- Shared runtime materializations may evolve, but the backend facade and lineage-aware authored intent remain the continuity contract.

### Evidence

- docs/SPEC.md assigns shared plans, tasks, claims, artifacts, and coordination event state to prism-coordination as an owned subsystem.
- docs/OPTIMIZATIONS.md requires explicit repo/worktree/branch/session/instance identity and states that one MCP server can host multiple worktree contexts.
- The coordination persistence, live-runtime, and rename/reload tests anchor the continuity guarantees in executable behavior across core and MCP layers.

## facade modularity rule

Handle: `contract://facade_modularity_rule`

`main.rs` and `lib.rs` stay thin facades for wiring, module boundaries, and public surface curation rather than accumulating substantive domain logic.

Kind: operational  
Status: active  
Stability: internal

Aliases: `facade-only entrypoints`, `lib/main facade rule`, `modularity contract`

### Subject

Anchors:
- `file:21`
Concept Handles:
- `concept://facade_modularity_rule`

### Guarantees

- `facade_only_entrypoints`: `main.rs` and `lib.rs` remain facade-oriented files that wire entrypoints, declare modules, and expose intentional public APIs instead of owning core business, parsing, coordination, storage, or domain logic. (scope: crate_entrypoints) [hard]
  evidence ref: `agents-architectural-rule`
- `substantive_logic_moves_to_owned_modules`: When a feature introduces substantive behavior, the implementation moves into dedicated narrowly owned submodules rather than extending facade files. (scope: module_ownership) [hard]
  evidence ref: `agents-modularity-expectations`

### Assumptions

- Rare exceptions are documented explicitly instead of silently widening facade files.
- Generated or bootstrap-only wiring may remain shallow in facade files when it does not become the semantic owner.

### Consumers

#### Target 1

Concept Handles:
- `concept://prism_architecture`

#### Target 2

Concept Handles:
- `concept://mcp_runtime_surface`

### Validations

- `review:agential-facade-boundary`: Review changes to `main.rs` and `lib.rs` against the AGENTS architectural rule before treating them as intentional.
  anchor: `file:21`

### Compatibility

#### Additive

- Adding thin module declarations, re-exports, or bootstrap wiring inside facade files is compatible when ownership remains elsewhere.

#### Risky

- Growing parsing, coordination, storage, or domain logic directly inside `main.rs` or `lib.rs` is risky.

#### Breaking

- Treating facade files as the long-term semantic owner of subsystem logic breaks the repo's modularity contract.

### Evidence

- AGENTS.md states that `main.rs` and `lib.rs` files are facades only.
- AGENTS.md also requires narrowly scoped modules and moving substantive logic into dedicated submodules.

## javascript query ABI

Handle: `contract://javascript_query_abi`

The stable prism.* JavaScript query surface stays synchronized across runtime prelude behavior, API-reference documentation, and pre-execution typechecking so agents can script PRISM without reverse-engineering host internals.

Kind: interface  
Status: active  
Stability: internal

Aliases: `js query abi contract`, `typescript query surface`, `stable prism dot star ABI`

### Subject

Anchors:
- `file:21`
- `file:121`
- `file:119`
- `file:307`
Concept Handles:
- `concept://javascript_query_abi`
- `concept://mcp_runtime_surface`

### Guarantees

- `documented_methods_match_runtime_surface`: The generated API reference and declared prism.* method catalog stay aligned with the runtime prelude surface that query snippets actually execute against. (scope: runtime_docs_alignment) [hard]
  evidence ref: `agents-api-reference-guidance`
  evidence ref: `prism-js-docs-tests`
- `stable_prism_surface_typechecks_before_execution`: Misspelled stable prism.* methods, option keys, and result-property accesses fail at pre-execution typecheck time with repair guidance instead of degrading into opaque runtime failures. (scope: preflight_typecheck) [hard]
  evidence ref: `query-typecheck-docs`
  evidence ref: `query-typecheck-tests`
- `new_helpers_land_through_typed_surface_completion`: New stable helper methods are added through the typed query surface, runtime prelude, and docs together rather than existing only as undocumented host-side behavior. (scope: surface_evolution) [soft]
  evidence ref: `javascript-query-abi-concept`
  evidence ref: `contracts-helper-completion`

### Assumptions

- Read-only escape hatches like raw prism_query snippets still exist, but the stable prism.* ABI is the intended typed programmable surface.
- Generated docs and runtime prelude are rebuilt from the same source tree before release or restart verification.

### Consumers

#### Target 1

Concept Handles:
- `concept://mcp_runtime_surface`
- `concept://compact_tools`
- `concept://javascript_runtime_and_reference_bridge`

### Validations

- `cargo test -p prism-js api_reference_mentions_primary_tool`: The generated API reference includes the stable method declarations and type surface.
  anchor: `node:prism_js:prism_js::tests::api_reference_mentions_primary_tool:function`
- `cargo test -p prism-js prelude_exposes_global_prism`: The runtime prelude exposes the documented global prism surface and helper methods.
  anchor: `node:prism_js:prism_js::tests::prelude_exposes_global_prism:function`
- `cargo test -p prism-mcp prism_query_misspelled_method_names_suggest_repair`: Misspelled stable prism.* method names fail with did-you-mean repair guidance and API-reference hints.
  anchor: `node:prism_mcp:prism_mcp::tests::prism_query_misspelled_method_names_suggest_repair:function`
- `cargo test -p prism-mcp prism_query_rejects_unknown_option_keys_before_host_dispatch`: Unknown option keys on stable prism.* methods are rejected before host dispatch with typed repair data.
  anchor: `node:prism_mcp:prism_mcp::tests::prism_query_rejects_unknown_option_keys_before_host_dispatch:function`

### Compatibility

#### Additive

- Adding new documented prism.* helpers is compatible when they land through the typed surface, runtime prelude, and API reference together.

#### Risky

- Changing helper names, option keys, or docs without keeping runtime and typecheck behavior aligned is risky because query snippets become misleading or brittle.

#### Breaking

- Letting the documented prism.* surface diverge from the runtime prelude or removing pre-execution repair guidance breaks the JS query ABI contract.

### Evidence

- AGENTS.md tells agents to use prism://api-reference after capabilities and vocab so the typed query surface is a first-class workflow contract.
- prism-js separates runtime, query-surface declarations, and generated docs into one ABI-focused layer.
- The query typecheck path treats the stable prism.* surface as a pre-execution contract and returns query_typecheck_failed repair data when snippets violate it.

## persistence split

Handle: `contract://persistence_split`

PRISM preserves a three-plane persistence model where repo-published `.prism` knowledge, shared mutable runtime state, and process-local cache remain distinct authorities.

Kind: dependency boundary  
Status: active  
Stability: migrating

Aliases: `three-plane persistence`, `three state planes`, `persistence boundary contract`

### Subject

Anchors:
- `file:277`
Concept Handles:
- `concept://persistence_split`
- `concept://persistence_and_history`

### Guarantees

- `repo_published_truth_stays_in_prism`: Repo-quality concepts, memories, plans, and other published knowledge remain repo-owned truth under `.prism` and are not displaced by a shared runtime backend. (scope: repo_truth) [hard]
  evidence ref: `persistence-three-planes`
  evidence ref: `persistence-boundary-guidance`
- `shared_backend_holds_mutable_runtime_state`: A shared backend carries shared mutable runtime continuity such as claims, handoffs, live overlays, and session-bound coordination state, complementing rather than replacing repo-published truth. (scope: shared_runtime) [hard]
  evidence ref: `persistence-three-planes`
  evidence ref: `persistence-boundary-guidance`
- `derived_snapshots_never_become_sole_authority`: Snapshots, projections, compatibility read models, and other rebuildable materializations remain derived accelerators and must not become the only semantic write authority. (scope: derived_state) [hard]
  evidence ref: `persistence-classification`
  evidence ref: `persistence-transitional-caveats`

### Assumptions

- Repo, worktree, branch, session, and instance identity remain explicit instead of being inferred accidentally from storage layout.
- Some snapshot-shaped APIs may persist during migration, but new features do not make them the sole semantic write truth.

### Consumers

#### Target 1

Concept Handles:
- `concept://workspace_session_runtime`

#### Target 2

Concept Handles:
- `concept://mcp_runtime_surface`

#### Target 3

Concept Handles:
- `concept://memory_projection_persistence`

### Validations

- `cargo test -p prism-core coordination_persistence_backend_wraps_store_and_repo_published_plans`: Checks that coordination persistence respects both store-backed runtime state and repo-published plans.
  anchor: `file:90`
- `cargo test -p prism-core coordination_persistence_incrementally_updates_stored_read_models`: Checks that incremental coordination persistence updates derived read models rather than replacing authored truth.
  anchor: `file:90`
- `cargo test -p prism-core coordination_persistence_compacts_large_event_suffixes_into_optional_baseline`: Checks that compaction stays derived over authoritative event-backed persistence.
  anchor: `file:90`

### Compatibility

#### Additive

- Adding new authoritative event-backed state is additive when it preserves the three-plane split and explicit scope boundaries.

#### Risky

- Letting shared backend state replace repo-published truth or collapsing worktree/session scope into storage location is risky.

#### Breaking

- Making snapshots, projections, or compatibility read models the sole semantic write authority breaks the persistence split.

#### Migrating

- Snapshot-shaped transitional APIs may remain during migration, but new persistence work should converge toward authoritative event-backed or normalized state.

### Evidence

- docs/PERSISTENCE_STATE_CLASSIFICATION.md defines the three state planes and says the shared database complements `.prism`; it does not replace it.
- The same document classifies snapshots, projections, and compatibility read models as derived rather than authored write authority.

## repo published knowledge replay

Handle: `contract://repo_published_knowledge_replay`

Repo-scoped published knowledge is committed as append-only event logs under .prism and hydrates the live runtime from those logs on reload instead of treating hydrated packets or snapshots as authored truth.

Kind: lifecycle  
Status: active  
Stability: internal

Aliases: `repo knowledge replay`, `published knowledge event logs`, `repo-scoped knowledge reload`

### Subject

Anchors:
- `file:277`
- `file:283`
- `file:82`
Concept Handles:
- `concept://repo_publication_guards`
- `concept://published_knowledge_and_memory_event_logs`
- `concept://persistence_split`

### Guarantees

- `published_repo_knowledge_travels_by_event_log`: Repo-scoped published knowledge travels by committed append-only event logs under .prism rather than by hydrated packet or snapshot shape. (scope: repo_publication) [hard]
  evidence ref: `persistence-event-log-authority`
  evidence ref: `spec-repo-exported-concepts`
- `repo_memory_concepts_and_contracts_reload_from_committed_logs`: Repo memory, curated concepts, and curated contracts reload from committed .prism event logs when a workspace session is rebuilt. (scope: reload_hydration) [hard]
  evidence ref: `repo-roundtrip-tests`
  evidence ref: `published-knowledge-guards`
- `hydrated_views_remain_derived_over_published_logs`: Hydrated packets, recall indexes, and other replayed convenience views remain derived from the committed event logs and do not become a second authored authority. (scope: derived_views) [hard]
  evidence ref: `persistence-derived-views`
  evidence ref: `persistence-split-contract`

### Assumptions

- Repo-scoped artifacts satisfy publication guards before they are promoted into committed knowledge.
- Fresh sessions rebuild from the committed event logs together with current workspace indexing data.

### Consumers

#### Target 1

Concept Handles:
- `concept://mcp_runtime_surface`
- `concept://concept_and_publication_pipeline`
- `concept://workspace_session_runtime`

### Validations

- `cargo test -p prism-core repo_memory_events_round_trip_through_committed_jsonl_and_reload`: Repo memory events round-trip through committed JSONL and reload into a fresh workspace session.
  anchor: `node:prism_core:prism_core::tests::repo_memory_events_round_trip_through_committed_jsonl_and_reload:function`
- `cargo test -p prism-core repo_concept_events_round_trip_through_committed_jsonl_and_reload`: Repo concept events round-trip through committed JSONL and reload into a fresh workspace session.
  anchor: `node:prism_core:prism_core::tests::repo_concept_events_round_trip_through_committed_jsonl_and_reload:function`
- `cargo test -p prism-core repo_contract_events_round_trip_through_committed_jsonl_and_reload`: Repo contract events round-trip through committed JSONL and reload into a fresh workspace session.
  anchor: `node:prism_core:prism_core::tests::repo_contract_events_round_trip_through_committed_jsonl_and_reload:function`

### Compatibility

#### Additive

- Adding new repo-scoped published knowledge kinds is compatible when they publish through committed event logs and reload through the same event-backed path.

#### Risky

- Writing repo-scoped truth only into hydrated projections, snapshots, or runtime caches is risky because reload behavior can diverge from published history.

#### Breaking

- Treating hydrated packets, snapshots, or derived indexes as the sole authored form of repo-scoped knowledge breaks replay and clone portability.

#### Migrating

- Existing derived packet shapes may change across releases, but committed event logs remain the durable interchange contract.

### Evidence

- docs/PERSISTENCE_STATE_CLASSIFICATION.md classifies curated repo knowledge as published event logs and says hydrated packets are derived.
- docs/SPEC.md states that repo-exported concept events travel with the repo and hydrate the live projection layer on reload.
- published_knowledge.rs and the repo round-trip tests anchor the event-log publication and reload path for repo memory, concepts, and contracts.

## typed MCP schema surface

Handle: `contract://typed_mcp_schema_surface`

PRISM MCP exposes bindable root tool schemas, exact action-specific schema resources, and actionable validation repair so clients can call mutation and session tools safely without guessing payload shapes.

Kind: interface  
Status: active  
Stability: internal

Aliases: `MCP schema contract`, `typed tool schema surface`, `schema-guided mutation surface`

### Subject

Anchors:
- `file:21`
- `file:202`
- `file:201`
Concept Handles:
- `concept://mcp_runtime_surface`
- `concept://javascript_query_abi`

### Guarantees

- `tagged_tools_expose_bindable_root_schemas`: Tagged MCP tools such as prism_session and prism_mutate expose a bindable root action/input schema on tools/list instead of only opaque or action-erased payload blobs. (scope: tool_binding) [hard]
  evidence ref: `agents-tool-schema-guidance`
  evidence ref: `server-tool-input-schema-tests`
- `schema_resources_are_exact_payload_authority`: prism://tool-schemas and prism://schema/tool/{toolName}/action/{action} remain the exact JSON schema authority for non-trivial MCP payloads, including structured nested action payloads. (scope: schema_authority) [hard]
  evidence ref: `agents-tool-schema-guidance`
  evidence ref: `tool-schema-resources`
- `validation_failures_provide_actionable_repair`: Tool validation failures provide actionable repair guidance, including exact schema URIs and a minimal valid example, rather than leaving callers to reverse-engineer payload requirements manually. (scope: repair_path) [hard]
  evidence ref: `validate-tool-input-guidance`
  evidence ref: `minimal-valid-example-errors`

### Assumptions

- Clients either consult the schema resources directly or reuse validation errors as repair hints before retrying failed mutation calls.
- Alias-friendly shorthand may exist, but the schema resources remain the canonical source for the exact accepted shape.

### Consumers

#### Target 1

Concept Handles:
- `concept://mcp_runtime_surface`
- `concept://javascript_query_abi`
- `concept://compact_tools`

### Validations

- `cargo test -p prism-mcp mcp_server_lists_and_reads_tool_schema_resources`: The server lists bindable root tool schemas and exposes the exact tool schema resources.
  anchor: `node:prism_mcp:prism_mcp::tests::server_resources::mcp_server_lists_and_reads_tool_schema_resources:function`
- `cargo test -p prism-mcp prism_mutate_schema_expands_payload_shapes_for_structured_actions`: The mutate schema expands structured nested action payloads instead of leaving them opaque.
  anchor: `node:prism_mcp:prism_mcp::tests::prism_mutate_schema_expands_payload_shapes_for_structured_actions:function`
- `cargo test -p prism-mcp prism_mutate_coordination_rejects_missing_typed_payload_fields`: Mutation validation errors surface exact missing fields, schema paths, and a minimal valid example.
  anchor: `node:prism_mcp:prism_mcp::tests::server_tool_calls::prism_mutate_coordination_rejects_missing_typed_payload_fields:function`

### Compatibility

#### Additive

- Adding new typed actions, schema resources, or aliases is compatible when the root bindable action/input shape and exact schema resources remain available.

#### Risky

- Letting tools/list regress to untyped payload shells or letting schema resources drift from accepted payload behavior is risky for every MCP client.

#### Breaking

- Removing exact schema resources or forcing callers to infer nested payload shapes from trial and error breaks the MCP safety contract.

### Evidence

- AGENTS.md explicitly directs agents to use prism://tool-schemas, prism://schema/tool/{toolName}, and prism.tool(...) before hand-writing non-trivial mutation payloads.
- tool_schemas.rs and the server resources tests expose bindable root schemas plus exact action-specific schema resources.
- tool_args.rs and the server tool-call tests show validation failures returning schema URIs and minimal valid examples.

## workspace refresh runtime

Handle: `contract://workspace_refresh_runtime`

PRISM keeps live workspace serving on an incremental refresh path: filesystem changes mark dirty regions, request paths avoid full persisted reloads in steady state, and persisted auxiliary state reloads only when freshness requires it.

Kind: operational  
Status: active  
Stability: internal

Aliases: `workspace session refresh contract`, `incremental refresh runtime`, `live refresh path`

### Subject

Anchors:
- `file:281`
- `file:86`
- `file:93`
- `file:206`
Concept Handles:
- `concept://workspace_session_refresh_runtime`
- `concept://workspace_session_runtime`

### Guarantees

- `steady_state_requests_avoid_full_persisted_reload`: Normal query and mutation serving stays off the full persisted runtime reload path when the live runtime is current, using incremental refresh or targeted materialization checks instead. (scope: hot_path_refresh) [hard]
  evidence ref: `refresh-redesign-hot-path`
  evidence ref: `mcp-refresh-tests`
- `filesystem_changes_drive_dirty_region_refresh`: Filesystem-triggered refresh tracks dirty paths and updates the live workspace session through scoped refresh and guarded replacement rather than blind whole-workspace rebuilds for every edit. (scope: dirty_region_refresh) [hard]
  evidence ref: `session-refresh-runtime-concept`
  evidence ref: `watch-refresh-tests`
- `persisted_auxiliary_state_reloads_on_freshness_boundaries`: Persisted notes, inference, and other auxiliary materializations reload when freshness boundaries require it, while the live workspace session remains the serving authority between those reload points. (scope: materialization_reload) [conditional]
  evidence ref: `refresh-redesign-derived-state`
  evidence ref: `refresh-notes-reload`

### Assumptions

- Workspace identity and dirty-path tracking stay explicit so refresh work can be scoped correctly.
- Administrative or recovery flows may still trigger broader reload behavior outside the normal serving hot path.

### Consumers

#### Target 1

Concept Handles:
- `concept://mcp_runtime_surface`
- `concept://workspace_indexing_and_refresh`
- `concept://workspace_session_runtime`

### Validations

- `cargo test -p prism-mcp queries_skip_request_path_persisted_reload_when_runtime_is_current`: Request-path queries skip persisted reload work when the live runtime is already current.
  anchor: `node:prism_mcp:prism_mcp::tests::queries_skip_request_path_persisted_reload_when_runtime_is_current:function`
- `cargo test -p prism-mcp first_mutation_after_workspace_refresh_skips_persisted_reload`: The first mutation after a current workspace refresh stays off the persisted reload path.
  anchor: `node:prism_mcp:prism_mcp::tests::first_mutation_after_workspace_refresh_skips_persisted_reload:function`
- `cargo test -p prism-core fs_watch_refreshes_session_after_external_edit`: Filesystem watch refresh updates the live session after an external edit using dirty-path tracking.
  anchor: `node:prism_core:prism_core::tests::fs_watch_refreshes_session_after_external_edit:function`
- `cargo test -p prism-mcp refresh_workspace_reloads_updated_persisted_notes`: Persisted notes reload when the background refresh path detects updated persisted state.
  anchor: `node:prism_mcp:prism_mcp::tests::refresh_workspace_reloads_updated_persisted_notes:function`

### Compatibility

#### Additive

- Adding new scoped refresh checks or auxiliary materialization reloads is compatible when request hot paths still avoid full persisted reloads by default.

#### Risky

- Expanding normal request handling back onto full persisted reloads or dropping dirty-path scoping is risky for latency and freshness correctness.

#### Breaking

- Making full persisted reload part of normal steady-state serving or treating auxiliary reload materializations as the only live authority breaks the refresh runtime contract.

#### Migrating

- Recovery and admin-only reload paths may continue to exist while the normal serving path remains incremental.

### Evidence

- docs/REFRESH_RUNTIME_REDESIGN.md states that normal request handling must never call a full persisted reload of runtime state on the hot path.
- workspace session concepts and the session/watch modules anchor dirty-path tracking, guarded replacement, and live refresh behavior.
- The MCP refresh tests show request and mutation paths skipping persisted reload while still reloading persisted notes when freshness requires it.

