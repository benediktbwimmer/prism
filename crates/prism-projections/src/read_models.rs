use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionClass {
    Published,
    Serving,
    AdHoc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionAuthorityPlane {
    PublishedRepo,
    SharedRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFreshnessState {
    Current,
    Pending,
    Stale,
    Recovery,
    Deferred,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionMaterializationState {
    Materialized,
    Partial,
    Deferred,
    KnownUnmaterialized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionReadModel {
    pub name: String,
    pub projection_class: ProjectionClass,
    pub authority_planes: Vec<ProjectionAuthorityPlane>,
    pub freshness: ProjectionFreshnessState,
    pub materialization: ProjectionMaterializationState,
    pub entry_count: usize,
}

impl ProjectionReadModel {
    pub fn serving(
        name: impl Into<String>,
        authority_planes: Vec<ProjectionAuthorityPlane>,
        freshness: ProjectionFreshnessState,
        materialization: ProjectionMaterializationState,
        entry_count: usize,
    ) -> Self {
        Self {
            name: name.into(),
            projection_class: ProjectionClass::Serving,
            authority_planes,
            freshness,
            materialization,
            entry_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionScopeReadModel {
    pub scope: String,
    pub projection_class: ProjectionClass,
    pub authority_planes: Vec<ProjectionAuthorityPlane>,
    pub freshness: ProjectionFreshnessState,
    pub materialization: ProjectionMaterializationState,
    pub concept_count: usize,
    pub relation_count: usize,
    pub contract_count: usize,
    pub co_change_lineage_count: usize,
    pub validation_lineage_count: usize,
    pub read_models: Vec<ProjectionReadModel>,
}

impl ProjectionScopeReadModel {
    #[allow(clippy::too_many_arguments)]
    pub fn serving(
        scope: impl Into<String>,
        authority_planes: Vec<ProjectionAuthorityPlane>,
        freshness: ProjectionFreshnessState,
        materialization: ProjectionMaterializationState,
        concept_count: usize,
        relation_count: usize,
        contract_count: usize,
        co_change_lineage_count: usize,
        validation_lineage_count: usize,
        read_models: Vec<ProjectionReadModel>,
    ) -> Self {
        Self {
            scope: scope.into(),
            projection_class: ProjectionClass::Serving,
            authority_planes,
            freshness,
            materialization,
            concept_count,
            relation_count,
            contract_count,
            co_change_lineage_count,
            validation_lineage_count,
            read_models,
        }
    }
}
