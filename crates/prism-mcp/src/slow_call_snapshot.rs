use prism_core::WorkspaceSession;
use serde_json::{json, Value};

use crate::dashboard_events::DashboardState;

pub(crate) const SLOW_CALL_SNAPSHOT_THRESHOLD_MS: u64 = 1_000;
const SLOW_CALL_ACTIVE_LIMIT: usize = 5;

pub(crate) fn attach_slow_call_snapshot(
    metadata: &mut Value,
    duration_ms: u64,
    dashboard: &DashboardState,
    workspace: Option<&WorkspaceSession>,
) {
    if duration_ms < SLOW_CALL_SNAPSHOT_THRESHOLD_MS {
        return;
    }

    let active_operations = dashboard.active_operations();
    let active_snapshot = active_operations
        .iter()
        .take(SLOW_CALL_ACTIVE_LIMIT)
        .map(|operation| {
            json!({
                "id": operation.id,
                "kind": operation.kind,
                "label": operation.label,
                "phase": operation.phase,
                "status": operation.status,
                "taskId": operation.task_id,
                "sessionId": operation.session_id,
                "touched": operation.touched,
                "error": operation.error,
            })
        })
        .collect::<Vec<_>>();

    let workspace_snapshot = workspace.map(|workspace| {
        let last_refresh = workspace.last_refresh();
        let materialization = workspace.workspace_materialization_summary();
        json!({
            "root": workspace.root().display().to_string(),
            "fsObservedRevision": workspace.observed_fs_revision(),
            "fsAppliedRevision": workspace.applied_fs_revision(),
            "fsDirty": workspace.observed_fs_revision() != workspace.applied_fs_revision(),
            "lastRefresh": last_refresh.as_ref().map(|refresh| json!({
                "path": refresh.path,
                "timestamp": refresh.timestamp,
                "durationMs": refresh.duration_ms,
                "workspaceRevision": refresh.workspace_revision,
                "loadedBytes": refresh.loaded_bytes,
                "replayVolume": refresh.replay_volume,
                "fullRebuildCount": refresh.full_rebuild_count,
                "workspaceReloaded": refresh.workspace_reloaded,
                "changedFiles": refresh.changed_files,
                "removedFiles": refresh.removed_files,
                "changedDirectories": refresh.changed_directories,
                "changedPackages": refresh.changed_packages,
            })),
            "materialization": {
                "knownFiles": materialization.known_files,
                "knownDirectories": materialization.known_directories,
                "materializedFiles": materialization.materialized_files,
                "materializedNodes": materialization.materialized_nodes,
                "materializedEdges": materialization.materialized_edges,
                "depth": materialization.depth(),
                "boundaryCount": materialization.boundaries.len(),
            },
        })
    });

    metadata["slowCallSnapshot"] = json!({
        "thresholdMs": SLOW_CALL_SNAPSHOT_THRESHOLD_MS,
        "activeOperationCount": active_operations.len(),
        "activeOperations": active_snapshot,
        "workspace": workspace_snapshot,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_call_snapshot_respects_threshold() {
        let dashboard = DashboardState::default();
        let mut fast = json!({});
        attach_slow_call_snapshot(
            &mut fast,
            SLOW_CALL_SNAPSHOT_THRESHOLD_MS - 1,
            &dashboard,
            None,
        );
        assert!(fast.get("slowCallSnapshot").is_none());

        let mut slow = json!({});
        attach_slow_call_snapshot(&mut slow, SLOW_CALL_SNAPSHOT_THRESHOLD_MS, &dashboard, None);
        assert_eq!(
            slow["slowCallSnapshot"]["thresholdMs"],
            json!(SLOW_CALL_SNAPSHOT_THRESHOLD_MS)
        );
        assert_eq!(slow["slowCallSnapshot"]["activeOperationCount"], json!(0));
    }
}
