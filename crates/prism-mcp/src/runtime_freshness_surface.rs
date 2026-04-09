use prism_js::{RuntimeFreshnessView, RuntimeMaterializationView};
use prism_projections::{ProjectionFreshnessState, ProjectionMaterializationState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeFreshnessStatus {
    Current,
    RefreshQueued,
    Deferred,
    Stale,
    Unknown,
}

impl RuntimeFreshnessStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::RefreshQueued => "refresh-queued",
            Self::Deferred => "deferred",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
}

pub(crate) fn runtime_freshness_status_label(
    fs_dirty: bool,
    materialization: &RuntimeMaterializationView,
    last_refresh_path: Option<&str>,
) -> &'static str {
    classify_runtime_freshness(fs_dirty, materialization, last_refresh_path).as_str()
}

pub(crate) fn projection_freshness_state(
    freshness: &RuntimeFreshnessView,
) -> ProjectionFreshnessState {
    if freshness.last_refresh_path.as_deref() == Some("recovery") {
        return ProjectionFreshnessState::Recovery;
    }
    match parse_runtime_freshness_status(freshness.status.as_str()) {
        RuntimeFreshnessStatus::Current => ProjectionFreshnessState::Current,
        RuntimeFreshnessStatus::RefreshQueued => ProjectionFreshnessState::Pending,
        RuntimeFreshnessStatus::Deferred => ProjectionFreshnessState::Deferred,
        RuntimeFreshnessStatus::Stale => ProjectionFreshnessState::Stale,
        RuntimeFreshnessStatus::Unknown => ProjectionFreshnessState::Unknown,
    }
}

pub(crate) fn projection_materialization_state(
    freshness: &RuntimeFreshnessView,
) -> ProjectionMaterializationState {
    if parse_runtime_freshness_status(freshness.status.as_str()) == RuntimeFreshnessStatus::Deferred
    {
        return ProjectionMaterializationState::Deferred;
    }
    let projections_domain = freshness
        .domains
        .iter()
        .find(|domain| domain.domain == "projections");
    if projections_domain
        .is_some_and(|domain| domain.materialization_depth == "known_unmaterialized")
    {
        return ProjectionMaterializationState::KnownUnmaterialized;
    }
    match parse_runtime_freshness_status(freshness.status.as_str()) {
        RuntimeFreshnessStatus::Stale | RuntimeFreshnessStatus::Unknown if freshness.fs_dirty => {
            ProjectionMaterializationState::Partial
        }
        RuntimeFreshnessStatus::Stale | RuntimeFreshnessStatus::Unknown => {
            ProjectionMaterializationState::Partial
        }
        _ if freshness.fs_dirty => ProjectionMaterializationState::Partial,
        _ => ProjectionMaterializationState::Materialized,
    }
}

fn classify_runtime_freshness(
    fs_dirty: bool,
    materialization: &RuntimeMaterializationView,
    last_refresh_path: Option<&str>,
) -> RuntimeFreshnessStatus {
    if fs_dirty {
        return RuntimeFreshnessStatus::RefreshQueued;
    }
    if last_refresh_path == Some("deferred") {
        return RuntimeFreshnessStatus::Deferred;
    }
    let statuses = [
        materialization.workspace.status.as_str(),
        materialization.episodic.status.as_str(),
        materialization.inference.status.as_str(),
        materialization.coordination.status.as_str(),
    ];
    if statuses.contains(&"stale") {
        RuntimeFreshnessStatus::Stale
    } else if statuses.contains(&"unknown") {
        RuntimeFreshnessStatus::Unknown
    } else {
        RuntimeFreshnessStatus::Current
    }
}

fn parse_runtime_freshness_status(status: &str) -> RuntimeFreshnessStatus {
    match status {
        "current" => RuntimeFreshnessStatus::Current,
        "refresh-queued" => RuntimeFreshnessStatus::RefreshQueued,
        "deferred" => RuntimeFreshnessStatus::Deferred,
        "stale" => RuntimeFreshnessStatus::Stale,
        "unknown" => RuntimeFreshnessStatus::Unknown,
        _ => RuntimeFreshnessStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        projection_freshness_state, projection_materialization_state,
        runtime_freshness_status_label,
    };
    use prism_js::{
        RuntimeDomainFreshnessView, RuntimeFreshnessView, RuntimeMaterializationItemView,
        RuntimeMaterializationView,
    };
    use prism_projections::{ProjectionFreshnessState, ProjectionMaterializationState};

    fn freshness(status: &str) -> RuntimeFreshnessView {
        RuntimeFreshnessView {
            fs_observed_revision: 1,
            fs_applied_revision: 1,
            fs_dirty: false,
            generation_id: Some(1),
            parent_generation_id: None,
            committed_delta_sequence: None,
            last_refresh_path: None,
            last_refresh_timestamp: None,
            last_refresh_duration_ms: None,
            last_refresh_loaded_bytes: None,
            last_refresh_replay_volume: None,
            last_refresh_full_rebuild_count: None,
            last_refresh_workspace_reloaded: None,
            last_workspace_build_ms: None,
            last_daemon_ready_ms: None,
            materialization: RuntimeMaterializationView {
                workspace: RuntimeMaterializationItemView {
                    status: status.to_string(),
                    depth: "deep".to_string(),
                    loaded_revision: 1,
                    current_revision: Some(1),
                    coverage: None,
                    boundaries: Vec::new(),
                },
                episodic: RuntimeMaterializationItemView {
                    status: "current".to_string(),
                    depth: "deep".to_string(),
                    loaded_revision: 1,
                    current_revision: Some(1),
                    coverage: None,
                    boundaries: Vec::new(),
                },
                inference: RuntimeMaterializationItemView {
                    status: "current".to_string(),
                    depth: "deep".to_string(),
                    loaded_revision: 1,
                    current_revision: Some(1),
                    coverage: None,
                    boundaries: Vec::new(),
                },
                coordination: RuntimeMaterializationItemView {
                    status: "current".to_string(),
                    depth: "deep".to_string(),
                    loaded_revision: 1,
                    current_revision: Some(1),
                    coverage: None,
                    boundaries: Vec::new(),
                },
            },
            coordination_lag: None,
            domains: Vec::new(),
            active_command: None,
            active_queue_class: None,
            queue_depth: 0,
            queued_by_class: Vec::new(),
            status: status.to_string(),
            error: None,
        }
    }

    #[test]
    fn runtime_freshness_status_tracks_deferred_and_stale_materialization() {
        let materialization = freshness("current").materialization;
        assert_eq!(
            runtime_freshness_status_label(false, &materialization, Some("deferred")),
            "deferred"
        );

        let stale_materialization = freshness("current").materialization;
        let mut stale = stale_materialization.clone();
        stale.coordination.status = "stale".to_string();
        assert_eq!(runtime_freshness_status_label(false, &stale, None), "stale");
    }

    #[test]
    fn projection_freshness_state_marks_recovery_before_status_label() {
        let mut freshness = freshness("stale");
        freshness.last_refresh_path = Some("recovery".to_string());
        assert_eq!(
            projection_freshness_state(&freshness),
            ProjectionFreshnessState::Recovery
        );
    }

    #[test]
    fn projection_materialization_state_marks_known_unmaterialized_domain() {
        let mut freshness = freshness("current");
        freshness.domains.push(RuntimeDomainFreshnessView {
            domain: "projections".to_string(),
            freshness: "pending".to_string(),
            materialization_depth: "known_unmaterialized".to_string(),
        });
        assert_eq!(
            projection_materialization_state(&freshness),
            ProjectionMaterializationState::KnownUnmaterialized
        );
    }
}
