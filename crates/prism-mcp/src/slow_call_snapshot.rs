use prism_core::WorkspaceSession;
use serde_json::{json, Value};

pub(crate) const SLOW_CALL_SNAPSHOT_THRESHOLD_MS: u64 = 1_000;

pub(crate) fn attach_slow_call_snapshot(
    metadata: &mut Value,
    duration_ms: u64,
    workspace: Option<&WorkspaceSession>,
) {
    if duration_ms < SLOW_CALL_SNAPSHOT_THRESHOLD_MS {
        return;
    }

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
        "workspace": workspace_snapshot,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_call_snapshot_respects_threshold() {
        let mut fast = json!({});
        attach_slow_call_snapshot(&mut fast, SLOW_CALL_SNAPSHOT_THRESHOLD_MS - 1, None);
        assert!(fast.get("slowCallSnapshot").is_none());

        let mut slow = json!({});
        attach_slow_call_snapshot(&mut slow, SLOW_CALL_SNAPSHOT_THRESHOLD_MS, None);
        assert_eq!(
            slow["slowCallSnapshot"]["thresholdMs"],
            json!(SLOW_CALL_SNAPSHOT_THRESHOLD_MS)
        );
    }
}
