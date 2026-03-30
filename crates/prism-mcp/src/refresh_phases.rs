use std::time::Duration;

use serde_json::{json, Value};

use crate::dashboard_events::MutationRun;
use crate::query_log::QueryRun;
use crate::{WorkspaceRefreshMetrics, WorkspaceRefreshReport};

fn duration_from_ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

fn should_emit_breakdown(report: &WorkspaceRefreshReport) -> bool {
    report.runtime_sync_used
        || report.deferred
        || report.metrics.lock_wait_ms > 0
        || report.metrics.lock_hold_ms > 0
        || report.metrics.fs_refresh_ms > 0
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
