mod common;
mod intent;
mod projections;
#[cfg(test)]
mod tests;
mod types;

pub use crate::intent::IntentIndex;
pub use crate::projections::{
    co_change_deltas_for_events, validation_deltas_for_event, ProjectionIndex,
};
pub use crate::types::{
    CoChangeDelta, CoChangeRecord, IntentDriftRecord, IntentSpecProjection, ProjectionSnapshot,
    ValidationCheck, ValidationDelta,
};
