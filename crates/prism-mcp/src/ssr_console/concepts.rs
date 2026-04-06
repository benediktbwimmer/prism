use std::collections::{BTreeMap, BTreeSet, VecDeque};

use prism_js::ConceptPacketView;
use prism_query::Prism;

use crate::views::{concept_packet_view, ConceptVerbosity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConceptDirection {
    Inbound,
    Outbound,
    Both,
}

impl ConceptDirection {
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("both").trim().to_ascii_lowercase().as_str() {
            "inbound" | "in" => Self::Inbound,
            "outbound" | "out" => Self::Outbound,
            _ => Self::Both,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SsrConceptSlice {
    pub(crate) focus: ConceptPacketView,
    pub(crate) nodes: Vec<(String, String)>,
    pub(crate) edges: Vec<(String, String, String)>,
    pub(crate) depth: usize,
    pub(crate) direction: ConceptDirection,
    pub(crate) relation_filter: Option<String>,
}

pub(crate) fn concept_handle_to_slug(handle: &str) -> String {
    handle
        .strip_prefix("concept://")
        .unwrap_or(handle)
        .to_string()
}

pub(crate) fn concept_slug_to_handle(slug: &str) -> String {
    if slug.starts_with("concept://") {
        slug.to_string()
    } else {
        format!("concept://{slug}")
    }
}

pub(crate) fn build_concept_slice(
    prism: &Prism,
    handle: &str,
    depth: usize,
    direction: ConceptDirection,
    relation_filter: Option<&str>,
) -> Option<SsrConceptSlice> {
    let focus_packet = prism.concept_by_handle(handle)?;
    let focus = concept_packet_view(
        prism,
        focus_packet.clone(),
        ConceptVerbosity::Standard,
        false,
        None,
    );
    let normalized_filter = relation_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());

    let mut queue = VecDeque::from([(handle.to_string(), 0usize)]);
    let mut seen = BTreeSet::from([handle.to_string()]);
    let mut labels = BTreeMap::new();
    labels.insert(handle.to_string(), focus_packet.canonical_name.clone());
    let mut edges = BTreeSet::new();

    while let Some((current, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }
        for relation in prism.concept_relations_for_handle(&current) {
            let source = relation.source_handle.clone();
            let target = relation.target_handle.clone();
            let outgoing = source == current;
            let incoming = target == current;
            let direction_matches = match direction {
                ConceptDirection::Outbound => outgoing,
                ConceptDirection::Inbound => incoming,
                ConceptDirection::Both => outgoing || incoming,
            };
            if !direction_matches {
                continue;
            }
            let relation_label = format!("{:?}", relation.kind).to_ascii_lowercase();
            if let Some(filter) = &normalized_filter {
                if &relation_label != filter {
                    continue;
                }
            }
            let neighbor = if outgoing {
                target.clone()
            } else {
                source.clone()
            };
            if let Some(packet) = prism.concept_by_handle(&neighbor) {
                labels
                    .entry(neighbor.clone())
                    .or_insert(packet.canonical_name.clone());
            }
            edges.insert((source.clone(), target.clone(), relation_label.clone()));
            if seen.insert(neighbor.clone()) {
                queue.push_back((neighbor, current_depth + 1));
            }
        }
    }

    Some(SsrConceptSlice {
        focus,
        nodes: labels.into_iter().collect(),
        edges: edges.into_iter().collect(),
        depth,
        direction,
        relation_filter: normalized_filter,
    })
}
