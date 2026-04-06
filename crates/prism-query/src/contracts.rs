use prism_ir::{AnchorRef, NodeId};

use crate::common::{dedupe_node_ids, sort_node_ids};
use crate::{ContractPacket, ContractTarget, Prism};

impl Prism {
    pub fn contracts_for_target(&self, target: &NodeId) -> Vec<ContractPacket> {
        let mut contracts = self
            .projections
            .read()
            .expect("projection lock poisoned")
            .contract_packets()
            .iter()
            .filter(|packet| contract_matches_target(self, target, packet))
            .cloned()
            .collect::<Vec<_>>();
        contracts.sort_by(|left, right| left.handle.cmp(&right.handle));
        contracts.dedup_by(|left, right| left.handle == right.handle);
        contracts
    }

    pub fn contract_subject_matches_target(
        &self,
        target: &NodeId,
        packet: &ContractPacket,
    ) -> bool {
        contract_target_matches(self, target, &packet.subject)
    }

    pub fn contract_consumer_matches_target(
        &self,
        target: &NodeId,
        packet: &ContractPacket,
    ) -> bool {
        packet
            .consumers
            .iter()
            .any(|consumer| contract_target_matches(self, target, consumer))
    }

    pub fn contract_target_nodes(
        &self,
        contract_target: &ContractTarget,
        limit: usize,
    ) -> Vec<NodeId> {
        resolve_contract_target_nodes(self, contract_target, limit)
    }
}

fn contract_matches_target(prism: &Prism, target: &NodeId, packet: &ContractPacket) -> bool {
    prism.contract_subject_matches_target(target, packet)
        || prism.contract_consumer_matches_target(target, packet)
}

fn contract_target_matches(
    prism: &Prism,
    target: &NodeId,
    contract_target: &ContractTarget,
) -> bool {
    contract_target
        .anchors
        .iter()
        .any(|anchor| anchor_matches_target(prism, target, anchor))
        || contract_target
            .concept_handles
            .iter()
            .any(|handle| concept_handle_matches_target(prism, target, handle))
}

fn anchor_matches_target(prism: &Prism, target: &NodeId, anchor: &AnchorRef) -> bool {
    if anchor.matches_node_without_graph(target) {
        return true;
    }
    if anchor.requires_graph_resolution()
        && !prism
            .runtime_capabilities()
            .graph_backed_resolution_enabled()
    {
        return false;
    }
    match anchor {
        AnchorRef::Lineage(lineage) => prism.lineage_of(target).as_ref() == Some(lineage),
        AnchorRef::File(file_id) => prism
            .graph()
            .node(target)
            .is_some_and(|node| node.file == *file_id),
        AnchorRef::Node(_) | AnchorRef::Kind(_) => false,
    }
}

fn concept_handle_matches_target(prism: &Prism, target: &NodeId, handle: &str) -> bool {
    let Some(packet) = prism.concept_by_handle(handle) else {
        return false;
    };
    if packet.core_members.iter().any(|member| member == target)
        || packet
            .supporting_members
            .iter()
            .any(|member| member == target)
        || packet.likely_tests.iter().any(|member| member == target)
    {
        return true;
    }
    if !prism
        .runtime_capabilities()
        .graph_backed_resolution_enabled()
    {
        return false;
    }
    let Some(target_lineage) = prism.lineage_of(target) else {
        return false;
    };
    packet
        .core_member_lineages
        .iter()
        .chain(packet.supporting_member_lineages.iter())
        .chain(packet.likely_test_lineages.iter())
        .flatten()
        .any(|lineage| lineage == &target_lineage)
}

fn resolve_contract_target_nodes(
    prism: &Prism,
    contract_target: &ContractTarget,
    limit: usize,
) -> Vec<NodeId> {
    if limit == 0 {
        return Vec::new();
    }

    let mut nodes = Vec::<NodeId>::new();
    for anchor in &contract_target.anchors {
        match anchor {
            AnchorRef::Node(node) => nodes.push(node.clone()),
            AnchorRef::Lineage(lineage)
                if prism
                    .runtime_capabilities()
                    .graph_backed_resolution_enabled() =>
            {
                nodes.extend(prism.current_nodes_for_lineage(lineage))
            }
            AnchorRef::File(file_id) => {
                if !prism
                    .runtime_capabilities()
                    .graph_backed_resolution_enabled()
                {
                    continue;
                }
                nodes.extend(
                    prism
                        .graph()
                        .all_nodes()
                        .filter(|node| node.file == *file_id)
                        .take(limit)
                        .map(|node| node.id.clone()),
                );
            }
            AnchorRef::Kind(kind) => {
                if !prism
                    .runtime_capabilities()
                    .graph_backed_resolution_enabled()
                {
                    continue;
                }
                nodes.extend(
                    prism
                        .graph()
                        .all_nodes()
                        .filter(|node| node.id.kind == *kind)
                        .take(limit)
                        .map(|node| node.id.clone()),
                );
            }
            AnchorRef::Lineage(_) => {}
        }
    }

    for handle in &contract_target.concept_handles {
        let Some(packet) = prism.concept_by_handle(handle) else {
            continue;
        };
        nodes.extend(packet.core_members);
        nodes.extend(packet.supporting_members);
    }

    sort_node_ids(&mut nodes);
    let mut nodes = dedupe_node_ids(nodes);
    nodes.truncate(limit);
    nodes
}
