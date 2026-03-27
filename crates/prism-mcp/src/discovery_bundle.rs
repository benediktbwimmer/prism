use anyhow::Result;
use prism_ir::NodeId;
use prism_js::{
    CoChangeView, ConfidenceLabel, DiscoveryBundleView, EvidenceSourceKind,
    SpecDriftExplanationView, SpecImplementationClusterView, TrustSignalsView,
};
use prism_query::Prism;
use serde::de::DeserializeOwned;

use crate::{
    blast_radius_view, co_change_view, edit_context_view_cached, entrypoints_for, lineage_view,
    next_reads, read_context_view_cached, recent_change_context_view_cached, relations_view,
    symbol_for, symbol_suggested_queries, symbol_view, validation_context_view_cached,
    validation_recipe_view_with, where_used, SemanticContextCache, SessionState,
};

pub(crate) fn discovery_bundle_view(
    prism: &Prism,
    session: &SessionState,
    id: &NodeId,
) -> Result<DiscoveryBundleView> {
    let mut cache = SemanticContextCache::default();
    discovery_bundle_view_cached(prism, session, &mut cache, id)
}

pub(crate) fn discovery_bundle_view_cached(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    id: &NodeId,
) -> Result<DiscoveryBundleView> {
    let target = symbol_view(prism, &symbol_for(prism, id)?)?;
    let spec_drift: Option<SpecDriftExplanationView> =
        crate::spec_drift_explanation_view(prism, id)
            .ok()
            .map(convert_view)
            .transpose()?;
    let suggested_reads = spec_drift
        .as_ref()
        .map(|drift| drift.next_reads.clone())
        .filter(|reads| !reads.is_empty())
        .unwrap_or(next_reads(prism, id, crate::INSIGHT_LIMIT)?);
    let suggested_reads = if suggested_reads.is_empty() {
        cache_owner_views(prism, cache, id)?.all.clone()
    } else {
        suggested_reads
    };
    let read_context = read_context_view_cached(prism, session, cache, id)?;
    let edit_context = edit_context_view_cached(prism, session, cache, id)?;
    let validation_context = validation_context_view_cached(prism, session, cache, id)?;
    let recent_change_context = recent_change_context_view_cached(prism, session, cache, id)?;
    let entrypoints = entrypoints_for(prism, session, id, crate::INSIGHT_LIMIT)?;
    let where_used_direct = where_used(prism, session, id, Some("direct"), crate::INSIGHT_LIMIT)?;
    let where_used_behavioral =
        where_used(prism, session, id, Some("behavioral"), crate::INSIGHT_LIMIT)?;
    let suggested_queries = symbol_suggested_queries(id);
    let relations = relations_view(prism, session, id)?;
    let spec_cluster: Option<SpecImplementationClusterView> = crate::spec_cluster_view(prism, id)
        .ok()
        .map(convert_view)
        .transpose()?;
    let lineage = lineage_view(prism, id)?;
    let co_change_neighbors = prism
        .co_change_neighbors(id, 8)
        .into_iter()
        .map(co_change_view)
        .collect::<Vec<CoChangeView>>();
    let related_failures = prism.related_failures(id);
    let blast_radius = blast_radius_view(prism, session, id);
    let validation_recipe = validation_recipe_view_with(prism, session, id);

    let mut why = vec![
        "Suggested reads prioritize read-oriented owner paths for the target.".to_string(),
        "Read context groups direct links, tests, memory, and recent failures.".to_string(),
        "Edit context adds write paths, blast radius, and validations before mutation.".to_string(),
        "Validation and recent-change contexts package the testing and outcome history loops."
            .to_string(),
    ];
    if !entrypoints.is_empty() || !where_used_direct.is_empty() {
        why.push(
            "Entrypoints and where-used views show how the target is reached from the outside."
                .to_string(),
        );
    }
    if spec_cluster.is_some() || spec_drift.is_some() {
        why.push(
            "Spec clustering and drift views explain how this target maps to higher-level intent."
                .to_string(),
        );
    }
    if !related_failures.is_empty() {
        why.push(
            "Recent failures are included so discovery stays grounded in known regressions."
                .to_string(),
        );
    }
    let trust_signals = discovery_trust_signals(
        !read_context.direct_links.is_empty()
            || !entrypoints.is_empty()
            || !where_used_direct.is_empty()
            || !relations.specifies.is_empty()
            || !relations.specified_by.is_empty()
            || !relations.implements.is_empty()
            || !relations.validates.is_empty()
            || !relations.related.is_empty(),
        !suggested_reads.is_empty() || !where_used_behavioral.is_empty(),
        !read_context.related_memory.is_empty()
            || !edit_context.related_memory.is_empty()
            || !validation_context.related_memory.is_empty()
            || !recent_change_context.related_memory.is_empty()
            || !recent_change_context.promoted_summaries.is_empty(),
        !related_failures.is_empty()
            || !recent_change_context.recent_events.is_empty()
            || !validation_context.recent_failures.is_empty(),
    );

    Ok(DiscoveryBundleView {
        target,
        suggested_reads,
        read_context,
        edit_context,
        validation_context,
        recent_change_context,
        entrypoints,
        where_used_direct,
        where_used_behavioral,
        suggested_queries,
        relations,
        spec_cluster,
        spec_drift,
        lineage,
        co_change_neighbors,
        related_failures,
        blast_radius,
        validation_recipe,
        trust_signals,
        why,
    })
}

fn cache_owner_views(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    id: &NodeId,
) -> Result<crate::GroupedOwnerCandidateViews> {
    if let Some(value) = cache.owner_views.get(id) {
        return Ok(value.clone());
    }
    let value = crate::grouped_owner_views_for_target(prism, id, crate::INSIGHT_LIMIT)?;
    cache.owner_views.insert(id.clone(), value.clone());
    Ok(value)
}

fn convert_view<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: DeserializeOwned,
{
    Ok(serde_json::from_value(serde_json::to_value(value)?)?)
}

fn discovery_trust_signals(
    has_direct_graph: bool,
    has_inferred: bool,
    has_memory: bool,
    has_outcome: bool,
) -> TrustSignalsView {
    let mut evidence_sources = Vec::new();
    let mut why = Vec::new();
    if has_direct_graph {
        evidence_sources.push(EvidenceSourceKind::DirectGraph);
        why.push(
            "Direct graph links anchor the discovery bundle to indexed structural relations."
                .to_string(),
        );
    }
    if has_inferred {
        evidence_sources.push(EvidenceSourceKind::Inferred);
        why.push(
            "Behavioral owner ranking contributes inferred follow-up reads and usage paths."
                .to_string(),
        );
    }
    if has_memory {
        evidence_sources.push(EvidenceSourceKind::Memory);
        why.push("Anchored memory contributes recalled notes and promoted summaries.".to_string());
    }
    if has_outcome {
        evidence_sources.push(EvidenceSourceKind::Outcome);
        why.push(
            "Outcome history contributes recent failures, validations, or recorded events."
                .to_string(),
        );
    }
    let confidence_label = if has_direct_graph && (has_inferred || has_memory || has_outcome) {
        ConfidenceLabel::High
    } else if has_direct_graph
        || [has_inferred, has_memory, has_outcome]
            .into_iter()
            .filter(|value| *value)
            .count()
            >= 2
    {
        ConfidenceLabel::Medium
    } else {
        ConfidenceLabel::Low
    };
    if evidence_sources.is_empty() {
        why.push(
            "This bundle lacks direct corroborating evidence and should be treated as exploratory."
                .to_string(),
        );
    }
    TrustSignalsView {
        confidence_label,
        evidence_sources,
        why,
    }
}
