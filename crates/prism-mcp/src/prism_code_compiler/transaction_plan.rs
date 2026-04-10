use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde::Serialize;

use super::program_ir::{
    PrismProgramEffectId, PrismProgramIr, PrismProgramRegionControl, PrismProgramRegionId,
    PrismProgramSourceSpan,
};

pub(crate) type StructuredTransactionEffectId = usize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StructuredTransactionEffectMetadata {
    pub(crate) method_path: String,
    pub(crate) effect_id: Option<PrismProgramEffectId>,
    pub(crate) region_id: PrismProgramRegionId,
    pub(crate) region_lineage: Vec<PrismProgramRegionId>,
    pub(crate) span: Option<PrismProgramSourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum StructuredTransactionRegionMember {
    Region(PrismProgramRegionId),
    Effect(StructuredTransactionEffectId),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StructuredTransactionRegion {
    pub(crate) region_id: PrismProgramRegionId,
    pub(crate) parent_region_id: Option<PrismProgramRegionId>,
    pub(crate) control: PrismProgramRegionControl,
    pub(crate) span: PrismProgramSourceSpan,
    pub(crate) child_region_ids: Vec<PrismProgramRegionId>,
    pub(crate) effect_ids: Vec<StructuredTransactionEffectId>,
    pub(crate) members: Vec<StructuredTransactionRegionMember>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StructuredTransactionEffect<P> {
    pub(crate) id: StructuredTransactionEffectId,
    pub(crate) metadata: StructuredTransactionEffectMetadata,
    pub(crate) payload: P,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StructuredTransactionPlan<P> {
    pub(crate) root_region_id: PrismProgramRegionId,
    pub(crate) regions: Vec<StructuredTransactionRegion>,
    pub(crate) effects: Vec<StructuredTransactionEffect<P>>,
    pub(crate) effect_order: Vec<StructuredTransactionEffectId>,
    #[serde(skip)]
    region_indexes: HashMap<PrismProgramRegionId, usize>,
}

impl<P> StructuredTransactionPlan<P> {
    pub(crate) fn new(ir: &PrismProgramIr) -> Self {
        let root = &ir.regions[ir.root_region_id];
        let root_region = StructuredTransactionRegion {
            region_id: root.id,
            parent_region_id: root.parent,
            control: root.control.clone(),
            span: root.span.clone(),
            child_region_ids: Vec::new(),
            effect_ids: Vec::new(),
            members: Vec::new(),
        };
        let mut region_indexes = HashMap::new();
        region_indexes.insert(root.id, 0);
        Self {
            root_region_id: root.id,
            regions: vec![root_region],
            effects: Vec::new(),
            effect_order: Vec::new(),
            region_indexes,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub(crate) fn record_effect(
        &mut self,
        ir: &PrismProgramIr,
        metadata: StructuredTransactionEffectMetadata,
        payload: P,
    ) -> Result<StructuredTransactionEffectId> {
        let lineage = normalize_lineage(
            self.root_region_id,
            metadata.region_id,
            &metadata.region_lineage,
        );
        if lineage[0] != self.root_region_id {
            return Err(anyhow!(
                "structured transaction effect for `{}` does not descend from the transaction root region",
                metadata.method_path
            ));
        }
        for pair in lineage.windows(2) {
            self.ensure_child_region(ir, pair[0], pair[1])?;
        }
        let region_id = *lineage
            .last()
            .ok_or_else(|| anyhow!("structured transaction lineage should never be empty"))?;
        let effect_id = self.effects.len();
        self.effects.push(StructuredTransactionEffect {
            id: effect_id,
            metadata,
            payload,
        });
        self.effect_order.push(effect_id);
        let region_index = *self
            .region_indexes
            .get(&region_id)
            .ok_or_else(|| anyhow!("structured transaction region `{region_id}` missing"))?;
        let region = &mut self.regions[region_index];
        region.effect_ids.push(effect_id);
        region
            .members
            .push(StructuredTransactionRegionMember::Effect(effect_id));
        Ok(effect_id)
    }

    pub(crate) fn ordered_effect_ids(&self) -> Vec<StructuredTransactionEffectId> {
        self.effect_order.clone()
    }

    pub(crate) fn region(
        &self,
        region_id: PrismProgramRegionId,
    ) -> Option<&StructuredTransactionRegion> {
        self.region_indexes
            .get(&region_id)
            .and_then(|index| self.regions.get(*index))
    }

    fn ensure_child_region(
        &mut self,
        ir: &PrismProgramIr,
        parent_region_id: PrismProgramRegionId,
        child_region_id: PrismProgramRegionId,
    ) -> Result<()> {
        if self.region_indexes.contains_key(&child_region_id) {
            return Ok(());
        }
        let child = ir
            .regions
            .get(child_region_id)
            .ok_or_else(|| anyhow!("unknown PRISM Program IR region `{child_region_id}`"))?;
        let parent_index = *self
            .region_indexes
            .get(&parent_region_id)
            .ok_or_else(|| anyhow!("missing parent transaction region `{parent_region_id}`"))?;
        let region_index = self.regions.len();
        self.regions.push(StructuredTransactionRegion {
            region_id: child.id,
            parent_region_id: Some(parent_region_id),
            control: child.control.clone(),
            span: child.span.clone(),
            child_region_ids: Vec::new(),
            effect_ids: Vec::new(),
            members: Vec::new(),
        });
        self.region_indexes.insert(child.id, region_index);
        let parent = &mut self.regions[parent_index];
        parent.child_region_ids.push(child.id);
        parent
            .members
            .push(StructuredTransactionRegionMember::Region(child.id));
        Ok(())
    }
}

fn normalize_lineage(
    root_region_id: PrismProgramRegionId,
    region_id: PrismProgramRegionId,
    lineage: &[PrismProgramRegionId],
) -> Vec<PrismProgramRegionId> {
    if lineage.is_empty() {
        if region_id == root_region_id {
            return vec![root_region_id];
        }
        return vec![root_region_id, region_id];
    }
    let mut normalized = lineage.to_vec();
    if normalized[0] != root_region_id {
        normalized.insert(0, root_region_id);
    }
    normalized
}
