use prism_store::ProjectionMaterializationMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PersistedProjectionLoadPlan {
    pub(crate) load_history_events: bool,
    pub(crate) load_full_outcomes: bool,
    pub(crate) had_complete_derived_snapshot: bool,
}

impl PersistedProjectionLoadPlan {
    pub(crate) const fn disabled() -> Self {
        Self {
            load_history_events: false,
            load_full_outcomes: false,
            had_complete_derived_snapshot: false,
        }
    }
}

pub(crate) fn persisted_projection_load_plan(
    metadata: ProjectionMaterializationMetadata,
    hydrate_persisted_projections: bool,
    hydrate_persisted_co_change: bool,
) -> PersistedProjectionLoadPlan {
    let loads_co_change =
        (hydrate_persisted_projections || hydrate_persisted_co_change) && metadata.has_co_change;
    PersistedProjectionLoadPlan {
        load_history_events: !loads_co_change,
        load_full_outcomes: !metadata.has_derived_rows(),
        had_complete_derived_snapshot: metadata.has_complete_derived_snapshot(),
    }
}
