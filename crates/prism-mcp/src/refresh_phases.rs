use std::time::Duration;

use serde_json::{json, Value};

use crate::dashboard_events::MutationRun;
use crate::query_log::QueryRun;
use crate::{WorkspaceRefreshMetrics, WorkspaceRefreshReport};

fn duration_from_ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

pub(crate) fn accounted_runtime_sync_duration(report: &WorkspaceRefreshReport) -> Duration {
    if report.deferred && report.metrics == WorkspaceRefreshMetrics::default() {
        return Duration::ZERO;
    }
    let total_ms = u128::from(report.metrics.lock_wait_ms)
        + u128::from(report.metrics.fs_refresh_ms)
        + u128::from(report.metrics.plan_refresh_ms)
        + u128::from(report.metrics.build_indexer_ms)
        + u128::from(report.metrics.index_workspace_ms)
        + u128::from(report.metrics.publish_generation_ms)
        + u128::from(report.metrics.assisted_lease_ms)
        + u128::from(report.metrics.curator_enqueue_ms)
        + u128::from(report.metrics.attach_cold_query_backends_ms)
        + u128::from(report.metrics.finalize_refresh_state_ms)
        + u128::from(report.metrics.snapshot_revisions_ms)
        + u128::from(report.metrics.load_episodic_ms)
        + u128::from(report.metrics.load_inference_ms)
        + u128::from(report.metrics.load_coordination_ms);
    duration_from_ms(u64::try_from(total_ms).unwrap_or(u64::MAX))
}

fn should_emit_breakdown(report: &WorkspaceRefreshReport) -> bool {
    report.runtime_sync_used
        || report.deferred
        || report.metrics.lock_wait_ms > 0
        || report.metrics.lock_hold_ms > 0
        || report.metrics.fs_refresh_ms > 0
        || report.metrics.plan_refresh_ms > 0
        || report.metrics.build_indexer_ms > 0
        || report.metrics.index_workspace_ms > 0
        || report.metrics.publish_generation_ms > 0
        || report.metrics.assisted_lease_ms > 0
        || report.metrics.curator_enqueue_ms > 0
        || report.metrics.attach_cold_query_backends_ms > 0
        || report.metrics.finalize_refresh_state_ms > 0
        || report.metrics.snapshot_revisions_ms > 0
        || report.metrics.load_episodic_ms > 0
        || report.metrics.load_inference_ms > 0
        || report.metrics.load_coordination_ms > 0
        || report.episodic_reloaded
        || report.inference_reloaded
        || report.coordination_reloaded
}

fn phase_specs(
    report: &WorkspaceRefreshReport,
) -> Vec<(&'static str, Value, Duration, bool, Option<String>)> {
    if !should_emit_breakdown(report) {
        return Vec::new();
    }
    if report.deferred && report.metrics == WorkspaceRefreshMetrics::default() {
        return vec![(
            "runtimeSync.deferred",
            json!({ "refreshPath": report.refresh_path }),
            Duration::ZERO,
            true,
            None,
        )];
    }
    vec![
        (
            "runtimeSync.waitLock",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.lock_wait_ms),
            true,
            None,
        ),
        (
            "runtimeSync.refreshFs",
            json!({
                "refreshPath": report.refresh_path,
                "workspaceReloaded": report.metrics.workspace_reloaded,
            }),
            duration_from_ms(report.metrics.fs_refresh_ms),
            true,
            None,
        ),
        (
            "runtimeSync.planRefresh",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.plan_refresh_ms),
            true,
            None,
        ),
        (
            "runtimeSync.buildIndexer",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.build_indexer_ms),
            true,
            None,
        ),
        (
            "runtimeSync.indexWorkspace",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.index_workspace_ms),
            true,
            None,
        ),
        (
            "runtimeSync.publishGeneration",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.publish_generation_ms),
            true,
            None,
        ),
        (
            "runtimeSync.assistedLease",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.assisted_lease_ms),
            true,
            None,
        ),
        (
            "runtimeSync.enqueueCurator",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.curator_enqueue_ms),
            true,
            None,
        ),
        (
            "runtimeSync.attachColdQueryBackends",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.attach_cold_query_backends_ms),
            true,
            None,
        ),
        (
            "runtimeSync.finalizeRefreshState",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.finalize_refresh_state_ms),
            true,
            None,
        ),
        (
            "runtimeSync.snapshotRevisions",
            json!({ "refreshPath": report.refresh_path }),
            duration_from_ms(report.metrics.snapshot_revisions_ms),
            true,
            None,
        ),
        (
            "runtimeSync.reloadEpisodic",
            json!({
                "refreshPath": report.refresh_path,
                "reloaded": report.episodic_reloaded,
                "loadedBytes": report.metrics.loaded_bytes,
                "replayVolume": report.metrics.replay_volume,
            }),
            duration_from_ms(report.metrics.load_episodic_ms),
            true,
            None,
        ),
        (
            "runtimeSync.reloadInference",
            json!({
                "refreshPath": report.refresh_path,
                "reloaded": report.inference_reloaded,
                "loadedBytes": report.metrics.loaded_bytes,
                "replayVolume": report.metrics.replay_volume,
            }),
            duration_from_ms(report.metrics.load_inference_ms),
            true,
            None,
        ),
        (
            "runtimeSync.reloadCoordination",
            json!({
                "refreshPath": report.refresh_path,
                "reloaded": report.coordination_reloaded,
                "loadedBytes": report.metrics.loaded_bytes,
                "replayVolume": report.metrics.replay_volume,
            }),
            duration_from_ms(report.metrics.load_coordination_ms),
            true,
            None,
        ),
    ]
}

pub(crate) fn record_query_runtime_sync_phases(
    query_run: &QueryRun,
    report: &WorkspaceRefreshReport,
) {
    for (operation, args, duration, success, error) in phase_specs(report) {
        query_run.record_phase(operation, &args, duration, success, error);
    }
}

pub(crate) fn record_query_runtime_sync_gap_phase(
    query_run: &QueryRun,
    report: &WorkspaceRefreshReport,
    total_duration: Duration,
) -> Duration {
    let accounted_duration = accounted_runtime_sync_duration(report);
    let unattributed_duration = total_duration.saturating_sub(accounted_duration);
    if !unattributed_duration.is_zero() {
        query_run.record_phase(
            "runtimeSync.unattributed",
            &json!({
                "refreshPath": report.refresh_path,
                "accountedMs": accounted_duration.as_millis(),
                "totalMs": total_duration.as_millis(),
                "lockHoldMs": report.metrics.lock_hold_ms,
            }),
            unattributed_duration,
            true,
            None,
        );
    }
    unattributed_duration
}

pub(crate) fn record_mutation_runtime_sync_phases(
    run: &MutationRun,
    report: &WorkspaceRefreshReport,
) {
    for (operation, args, duration, success, error) in phase_specs(report) {
        run.record_phase(operation, &args, duration, success, error);
    }
}

pub(crate) fn record_resource_runtime_sync_phases(report: &WorkspaceRefreshReport) {
    for (operation, args, duration, success, error) in phase_specs(report) {
        crate::resource_trace::record_phase(operation, &args, duration, success, error);
    }
}

#[cfg(test)]
mod tests {
    use super::accounted_runtime_sync_duration;
    use crate::{WorkspaceRefreshMetrics, WorkspaceRefreshReport};

    #[test]
    fn accounted_runtime_sync_duration_sums_non_overlapping_refresh_metrics() {
        let report = WorkspaceRefreshReport {
            refresh_path: "none",
            runtime_sync_used: true,
            deferred: false,
            episodic_reloaded: false,
            inference_reloaded: false,
            coordination_reloaded: false,
            metrics: WorkspaceRefreshMetrics {
                lock_wait_ms: 7,
                fs_refresh_ms: 11,
                plan_refresh_ms: 13,
                build_indexer_ms: 17,
                index_workspace_ms: 19,
                publish_generation_ms: 23,
                assisted_lease_ms: 29,
                curator_enqueue_ms: 31,
                attach_cold_query_backends_ms: 37,
                finalize_refresh_state_ms: 41,
                snapshot_revisions_ms: 43,
                load_episodic_ms: 47,
                load_inference_ms: 53,
                load_coordination_ms: 59,
                ..WorkspaceRefreshMetrics::default()
            },
        };

        assert_eq!(
            accounted_runtime_sync_duration(&report).as_millis(),
            430
        );
    }
}
