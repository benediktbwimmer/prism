use prism_ir::{AnchorRef, NodeId, PlanBinding, PlanGraph, PlanId, PlanNode};

use crate::common::{anchor_sort_key, sort_node_ids};
use crate::plan_runtime::NativePlanRuntimeState;
use crate::types::PlanProjection;
use crate::Prism;

impl Prism {
    pub(crate) fn hydrated_plan_projection_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        plan_id: &PlanId,
    ) -> Option<PlanProjection> {
        runtime.plan_projection(plan_id).map(|projection| PlanProjection {
            graph: self.hydrate_plan_graph(projection.graph),
            execution_overlays: projection.execution_overlays,
        })
    }

    pub(crate) fn hydrated_plan_projections_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
    ) -> Vec<PlanProjection> {
        runtime
            .plan_projections()
            .into_iter()
            .map(|projection| PlanProjection {
                graph: self.hydrate_plan_graph(projection.graph),
                execution_overlays: projection.execution_overlays,
            })
            .collect()
    }

    fn hydrate_plan_graph(&self, mut graph: PlanGraph) -> PlanGraph {
        for node in &mut graph.nodes {
            self.hydrate_plan_node(node);
        }
        graph
    }

    fn hydrate_plan_node(&self, node: &mut PlanNode) {
        node.bindings.anchors = self.hydrate_binding_anchors(&node.bindings);
    }

    fn hydrate_binding_anchors(&self, binding: &PlanBinding) -> Vec<AnchorRef> {
        let mut hydrated = binding.anchors.clone();
        let mut recovered = false;

        for anchor in &binding.anchors {
            if self.anchor_resolves(anchor) {
                if let AnchorRef::Lineage(lineage) = anchor {
                    hydrated.extend(
                        self.current_nodes_for_lineage(lineage)
                            .into_iter()
                            .map(AnchorRef::Node),
                    );
                }
                recovered = true;
                continue;
            }
            if let AnchorRef::Node(node) = anchor {
                if let Some(lineage) = self.recover_lineage_for_node(node) {
                    let current_nodes = self.current_nodes_for_lineage(&lineage);
                    if !current_nodes.is_empty() {
                        hydrated.push(AnchorRef::Lineage(lineage));
                        hydrated.extend(current_nodes.into_iter().map(AnchorRef::Node));
                        recovered = true;
                    }
                }
            }
        }

        if !recovered && !binding.concept_handles.is_empty() {
            let concept_nodes = self.binding_nodes_from_concepts(&binding.concept_handles);
            if !concept_nodes.is_empty() {
                hydrated.extend(concept_nodes.into_iter().map(AnchorRef::Node));
                recovered = true;
            }
        }

        if recovered {
            return self.expand_anchors(&hydrated);
        }

        hydrated.sort_by(anchor_sort_key);
        hydrated.dedup();
        hydrated
    }

    fn recover_lineage_for_node(&self, node: &NodeId) -> Option<prism_ir::LineageId> {
        self.lineage_of(node).or_else(|| {
            self.history
                .snapshot()
                .events
                .iter()
                .rev()
                .find(|event| event.before.contains(node) || event.after.contains(node))
                .map(|event| event.lineage.clone())
        })
    }

    fn anchor_resolves(&self, anchor: &AnchorRef) -> bool {
        !self
            .resolve_anchor_nodes(std::slice::from_ref(anchor))
            .is_empty()
    }

    fn binding_nodes_from_concepts(&self, handles: &[String]) -> Vec<NodeId> {
        let mut nodes = handles
            .iter()
            .filter_map(|handle| self.concept_by_handle(handle))
            .flat_map(|concept| {
                concept
                    .core_members
                    .into_iter()
                    .chain(concept.supporting_members)
            })
            .collect::<Vec<_>>();
        sort_node_ids(&mut nodes);
        nodes
    }
}
