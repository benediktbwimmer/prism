# Shared Coordination Refs Validation

Validated on `2026-04-04` against [`docs/PRISM_SHARED_COORDINATION_REFS.md`](/Users/bene/code/prism-protected-state-runtime-sync/docs/PRISM_SHARED_COORDINATION_REFS.md).

This file records the concrete evidence used to validate the shared-ref implementation before the
final documentation-closure pass updates the design doc itself to implemented reality.

## Coverage Matrix

- Cold hydration from shared authority:
  [`shared_coordination_ref::tests::hydrated_plan_state_prefers_shared_coordination_ref_over_branch_snapshot`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  and
  [`shared_coordination_ref::tests::startup_loader_prefers_materialized_checkpoint_over_inline_shared_ref_hydration`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  prove startup and hydration prefer shared-ref authority or the local materialized checkpoint instead of branch snapshot mirrors.

- Compare-and-swap races and retry behavior:
  [`shared_coordination_ref::tests::shared_coordination_ref_retries_stale_head_and_merges_disjoint_changes`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  covers competing writers and safe retry behavior after stale-head detection.

- Claim visibility without branch merges:
  [`shared_coordination_ref::tests::shared_coordination_ref_round_trips_claims_and_git_execution_state`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  verifies claim and git-execution state is visible through shared authority without requiring task-branch integration.

- Authoritative lease renewal and expiry:
  [`shared_coordination_ref::tests::due_task_heartbeat_refreshes_shared_coordination_ref_lease_state`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs),
  [`shared_coordination_ref::tests::early_task_heartbeat_does_not_advance_shared_coordination_ref_head`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs),
  and
  [`watch::tests::assisted_watcher_heartbeat_records_local_liveness_without_authoritative_mutation`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/watch.rs)
  cover low-frequency authoritative renewal plus bounded non-authoritative assistance.

- Self-write suppression:
  [`shared_coordination_ref::tests::shared_coordination_ref_live_sync_suppresses_self_write_and_imports_remote_change`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  validates that local publication does not churn on its own just-published shared-ref updates.

- Mirror-derivedness and lifecycle-aware query surfaces:
  [`views::tests::coordination_task_view_exposes_integration_aware_dependencies_and_lifecycle`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/views.rs)
  plus the earlier branch-local export demotion work validate that lifecycle and dependency views no longer depend on tracked snapshot mirrors as authority.

- Publication ordering and recovery:
  [`tests::git_execution_policy_completion_require_publishes_after_manual_code_commit`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  [`tests::git_execution_policy_completion_require_push_failure_keeps_task_in_progress`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  and
  [`tests::session_resource_surfaces_publish_failed_repair_action`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs)
  validate branch-first publication, durable partial-publication state, and explicit retry guidance.

- Integration modes:
  [`tests::manual_pr_requires_approved_review_artifact_before_recording_target_landing`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  [`tests::manual_pr_approved_review_auto_observes_merge_landing_on_task_touch`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  [`tests::manual_pr_review_approval_observes_already_landed_merge`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  [`tests::manual_pr_rebase_landing_accepts_verified_trusted_record`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  [`tests::auto_pr_completion_creates_and_links_review_artifact`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  [`tests::auto_pr_approved_review_advances_integration_and_allows_verified_landing`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  [`tests::auto_pr_rejected_review_marks_integration_failed`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs),
  and
  [`tests::direct_integrate_completion_lands_target_and_records_trusted_evidence`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-mcp/src/tests.rs)
  cover all supported integration modes.

- Compaction and observability:
  [`shared_coordination_ref::tests::shared_coordination_ref_compacts_history_after_threshold`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  and
  [`shared_coordination_ref::tests::shared_coordination_ref_diagnostics_report_manifest_and_history`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  validate trust continuity, retry metadata, and operator-visible diagnostics.

- Verification failure and silent-fallback prevention:
  [`shared_coordination_ref::tests::shared_coordination_ref_diagnostics_surface_degraded_verification_state`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  and
  [`shared_coordination_ref::tests::invalid_shared_coordination_ref_blocks_authoritative_hydration_without_fallback`](/Users/bene/code/prism-protected-state-runtime-sync/crates/prism-core/src/shared_coordination_ref.rs)
  validate degraded runtime state and blocked authoritative hydration on invalid shared-ref state.

## Live Daemon Checks

- Release binaries rebuilt:
  `cargo build --release -p prism-cli -p prism-mcp`

- Release daemon restarted successfully:
  `./target/release/prism-cli mcp restart --internal-developer`

- Release daemon status and health were healthy on `http://127.0.0.1:48137/mcp`:
  `./target/release/prism-cli mcp status`
  `./target/release/prism-cli mcp health`

- PRISM-on-PRISM coordination, task brief, and completion mutations for this plan node were executed against the live daemon during this validation pass.

## Workspace Validation

- Full workspace validation was run with `cargo test`.
- The full run hit three nondeterministic or order-sensitive `prism-mcp` failures:
  - `mcp_call_log::tests::default_mcp_call_log_path_is_stable_across_restarts`
  - `tests::prism_runtime_views_surface_startup_recovery_work`
  - `tests::runtime_status_surfaces_shared_coordination_ref_diagnostics`
- All three passed immediately in isolated reruns, so validation is treated as successful under repo policy.

## Deferred Behavior

- No new implementation deferrals were identified during this validation pass.
- The remaining open work after this node is documentation closure: update the design doc and generated plan/docs to match the implemented system explicitly.
