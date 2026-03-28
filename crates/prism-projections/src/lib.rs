mod common;
mod concepts;
mod intent;
mod projections;
#[cfg(test)]
mod tests;
mod types;

pub use crate::concepts::{canonical_concept_handle, curated_concepts_from_events};
pub use crate::intent::IntentIndex;
pub use crate::projections::{
    co_change_deltas_for_events, validation_deltas_for_event, ProjectionIndex,
    MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE,
};
pub use crate::types::{
    CoChangeDelta, CoChangeRecord, ConceptDecodeLens, ConceptEvent, ConceptEventAction,
    ConceptPacket, ConceptProvenance, ConceptPublication, ConceptPublicationStatus, ConceptScope,
    IntentDriftRecord, IntentSpecProjection, ProjectionSnapshot, ValidationCheck, ValidationDelta,
};
