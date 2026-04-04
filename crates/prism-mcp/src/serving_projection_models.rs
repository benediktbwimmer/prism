use prism_js::RuntimeFreshnessView;
use prism_projections::{
    ProjectionAuthorityPlane, ProjectionFreshnessState, ProjectionMaterializationState,
    ProjectionReadModel, ProjectionScopeReadModel,
};
use prism_query::Prism;
use serde::Serialize;

pub(crate) fn runtime_projection_scopes(
    prism: &Prism,
    freshness: &RuntimeFreshnessView,
) -> Vec<ProjectionScopeReadModel> {
    let (co_change_lineage_count, validation_lineage_count) = prism.projection_lineage_counts();
    let concepts = prism.curated_concepts_snapshot();
    let relations = prism.concept_relations_snapshot();
    let contracts = prism.curated_contracts();
    let worktree_freshness = projection_freshness(freshness);
    let worktree_materialization = projection_materialization(freshness);

    vec![
        ProjectionScopeReadModel::serving(
            "repo",
            vec![ProjectionAuthorityPlane::PublishedRepo],
            ProjectionFreshnessState::Current,
            ProjectionMaterializationState::Materialized,
            scoped_count(&concepts, "repo", |concept| &concept.scope),
            scoped_count(&relations, "repo", |relation| &relation.scope),
            scoped_count(&contracts, "repo", |contract| &contract.scope),
            0,
            0,
            vec![
                ProjectionReadModel::serving(
                    "curated_concepts",
                    vec![ProjectionAuthorityPlane::PublishedRepo],
                    ProjectionFreshnessState::Current,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&concepts, "repo", |concept| &concept.scope),
                ),
                ProjectionReadModel::serving(
                    "concept_relations",
                    vec![ProjectionAuthorityPlane::PublishedRepo],
                    ProjectionFreshnessState::Current,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&relations, "repo", |relation| &relation.scope),
                ),
                ProjectionReadModel::serving(
                    "curated_contracts",
                    vec![ProjectionAuthorityPlane::PublishedRepo],
                    ProjectionFreshnessState::Current,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&contracts, "repo", |contract| &contract.scope),
                ),
            ],
        ),
        ProjectionScopeReadModel::serving(
            "worktree",
            vec![
                ProjectionAuthorityPlane::PublishedRepo,
                ProjectionAuthorityPlane::SharedRuntime,
            ],
            worktree_freshness,
            worktree_materialization,
            scoped_count(&concepts, "local", |concept| &concept.scope),
            scoped_count(&relations, "local", |relation| &relation.scope),
            scoped_count(&contracts, "local", |contract| &contract.scope),
            co_change_lineage_count,
            validation_lineage_count,
            vec![
                ProjectionReadModel::serving(
                    "curated_concepts",
                    vec![ProjectionAuthorityPlane::SharedRuntime],
                    worktree_freshness,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&concepts, "local", |concept| &concept.scope),
                ),
                ProjectionReadModel::serving(
                    "concept_relations",
                    vec![ProjectionAuthorityPlane::SharedRuntime],
                    worktree_freshness,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&relations, "local", |relation| &relation.scope),
                ),
                ProjectionReadModel::serving(
                    "curated_contracts",
                    vec![ProjectionAuthorityPlane::SharedRuntime],
                    worktree_freshness,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&contracts, "local", |contract| &contract.scope),
                ),
                ProjectionReadModel::serving(
                    "co_change",
                    vec![
                        ProjectionAuthorityPlane::PublishedRepo,
                        ProjectionAuthorityPlane::SharedRuntime,
                    ],
                    worktree_freshness,
                    worktree_materialization,
                    co_change_lineage_count,
                ),
                ProjectionReadModel::serving(
                    "validation",
                    vec![
                        ProjectionAuthorityPlane::PublishedRepo,
                        ProjectionAuthorityPlane::SharedRuntime,
                    ],
                    worktree_freshness,
                    worktree_materialization,
                    validation_lineage_count,
                ),
            ],
        ),
        ProjectionScopeReadModel::serving(
            "session",
            vec![ProjectionAuthorityPlane::SharedRuntime],
            ProjectionFreshnessState::Current,
            ProjectionMaterializationState::Materialized,
            scoped_count(&concepts, "session", |concept| &concept.scope),
            scoped_count(&relations, "session", |relation| &relation.scope),
            scoped_count(&contracts, "session", |contract| &contract.scope),
            0,
            0,
            vec![
                ProjectionReadModel::serving(
                    "curated_concepts",
                    vec![ProjectionAuthorityPlane::SharedRuntime],
                    ProjectionFreshnessState::Current,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&concepts, "session", |concept| &concept.scope),
                ),
                ProjectionReadModel::serving(
                    "concept_relations",
                    vec![ProjectionAuthorityPlane::SharedRuntime],
                    ProjectionFreshnessState::Current,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&relations, "session", |relation| &relation.scope),
                ),
                ProjectionReadModel::serving(
                    "curated_contracts",
                    vec![ProjectionAuthorityPlane::SharedRuntime],
                    ProjectionFreshnessState::Current,
                    ProjectionMaterializationState::Materialized,
                    scoped_count(&contracts, "session", |contract| &contract.scope),
                ),
            ],
        ),
    ]
}

fn scoped_count<T, S, U>(items: &[T], expected: &str, scope_of: S) -> usize
where
    S: Fn(&T) -> &U,
    U: Serialize + ?Sized,
{
    items
        .iter()
        .filter(|item| scope_label(scope_of(item)) == expected)
        .count()
}

fn scope_label<T: Serialize + ?Sized>(scope: &T) -> String {
    serde_json::to_value(scope)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn projection_freshness(freshness: &RuntimeFreshnessView) -> ProjectionFreshnessState {
    if freshness.last_refresh_path.as_deref() == Some("recovery") {
        return ProjectionFreshnessState::Recovery;
    }
    match freshness.status.as_str() {
        "current" => ProjectionFreshnessState::Current,
        "refresh-queued" => ProjectionFreshnessState::Pending,
        "deferred" => ProjectionFreshnessState::Deferred,
        "stale" => ProjectionFreshnessState::Stale,
        "unknown" => ProjectionFreshnessState::Unknown,
        _ => ProjectionFreshnessState::Unknown,
    }
}

fn projection_materialization(freshness: &RuntimeFreshnessView) -> ProjectionMaterializationState {
    if freshness.status == "deferred" {
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
    if freshness.fs_dirty || freshness.status == "stale" || freshness.status == "unknown" {
        ProjectionMaterializationState::Partial
    } else {
        ProjectionMaterializationState::Materialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_js::{
        RuntimeDomainFreshnessView, RuntimeMaterializationItemView, RuntimeMaterializationView,
    };

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
                    status: "current".to_string(),
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
    fn projection_materialization_marks_known_unmaterialized_domain() {
        let mut freshness = freshness("current");
        freshness.domains.push(RuntimeDomainFreshnessView {
            domain: "projections".to_string(),
            freshness: "pending".to_string(),
            materialization_depth: "known_unmaterialized".to_string(),
        });
        assert_eq!(
            projection_materialization(&freshness),
            ProjectionMaterializationState::KnownUnmaterialized
        );
    }

    #[test]
    fn projection_freshness_marks_recovery_from_refresh_path() {
        let mut freshness = freshness("stale");
        freshness.last_refresh_path = Some("recovery".to_string());
        assert_eq!(
            projection_freshness(&freshness),
            ProjectionFreshnessState::Recovery
        );
    }
}
