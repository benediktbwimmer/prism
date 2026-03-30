mod common;
mod concept_relations;
mod concepts;
mod contracts;
mod intent;
mod projections;
#[cfg(test)]
mod tests;
mod types;

pub use crate::concept_relations::concept_relations_from_events;
pub use crate::concepts::{
    canonical_concept_handle, concept_from_event, curated_concepts_from_events,
};
pub use crate::contracts::{
    canonical_contract_handle, contract_from_event, curated_contracts_from_events,
};
pub use crate::intent::IntentIndex;
pub use crate::projections::{
    co_change_deltas_for_events, validation_deltas_for_event, ProjectionIndex,
    MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE,
};
pub use crate::types::{
    CoChangeDelta, CoChangeRecord, ConceptDecodeLens, ConceptEvent, ConceptEventAction,
    ConceptEventPatch, ConceptHealth, ConceptHealthSignals, ConceptHealthStatus, ConceptPacket,
    ConceptProvenance, ConceptPublication, ConceptPublicationStatus, ConceptRelation,
    ConceptRelationEvent, ConceptRelationEventAction, ConceptRelationKind, ConceptResolution,
    ConceptScope, ContractCompatibility, ContractEvent, ContractEventAction, ContractEventPatch,
    ContractGuarantee, ContractGuaranteeStrength, ContractKind, ContractPacket, ContractProvenance,
    ContractPublication, ContractPublicationStatus, ContractResolution, ContractScope,
    ContractStability, ContractStatus, ContractTarget, ContractValidation, IntentDriftRecord,
    IntentSpecProjection, ProjectionSnapshot, ValidationCheck, ValidationDelta,
};
