use std::thread;
use std::time::Instant;

use anyhow::Result;
use prism_ir::NodeId;
use prism_js::{ConfidenceLabel, DiscoveryBundleView, EvidenceSourceKind, TrustSignalsView};
use prism_query::Prism;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::{
    QueryRun, SemanticContextCache, SessionState, cached_blast_radius, cached_co_change_neighbors,
    cached_lineage, cached_owner_views, cached_recent_failures, cached_relations,
    cached_target_symbol, cached_validation_recipe, edit_context_view_cached, entrypoints_for,
    next_reads, prefetch_semantic_context_cache, read_context_view_cached,
    recent_change_context_view_cached, symbol_suggested_queries, validation_context_view_cached,
    where_used,
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
    discovery_bundle_view_cached_with_trace(prism, session, cache, None, id)
}

pub(crate) fn discovery_bundle_view_cached_with_trace(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    query_run: Option<QueryRun>,
    id: &NodeId,
) -> Result<DiscoveryBundleView> {
    record_bundle_phase(query_run.as_ref(), id, "discoveryBundle.prefetch", || {
        prefetch_semantic_context_cache(prism, session, cache, id)
    })?;
    let target = record_bundle_phase(query_run.as_ref(), id, "discoveryBundle.target", || {
        cached_target_symbol(prism, cache, id)
    })?;
    let (spec_drift, entrypoints, where_used_direct, where_used_behavioral, spec_cluster): (
        Option<prism_js::SpecDriftExplanationView>,
        Vec<prism_js::SymbolView>,
        Vec<prism_js::SymbolView>,
        Vec<prism_js::SymbolView>,
        Option<prism_js::SpecImplementationClusterView>,
    ) = thread::scope(|scope| -> Result<_> {
        let spec_drift_run = query_run.clone();
        let spec_drift_task = scope.spawn(move || {
            record_bundle_phase(
                spec_drift_run.as_ref(),
                id,
                "discoveryBundle.specDrift",
                || {
                    crate::spec_drift_explanation_view(prism, id)
                        .ok()
                        .map(convert_view)
                        .transpose()
                },
            )
        });
        let entrypoints_run = query_run.clone();
        let entrypoints_task = scope.spawn(move || {
            record_bundle_phase(
                entrypoints_run.as_ref(),
                id,
                "discoveryBundle.entrypointsFor",
                || entrypoints_for(prism, session, id, crate::INSIGHT_LIMIT),
            )
        });
        let where_used_direct_run = query_run.clone();
        let where_used_direct_task = scope.spawn(move || {
            record_bundle_phase(
                where_used_direct_run.as_ref(),
                id,
                "discoveryBundle.whereUsedDirect",
                || where_used(prism, session, id, Some("direct"), crate::INSIGHT_LIMIT),
            )
        });
        let where_used_behavioral_run = query_run.clone();
        let where_used_behavioral_task = scope.spawn(move || {
            record_bundle_phase(
                where_used_behavioral_run.as_ref(),
                id,
                "discoveryBundle.whereUsedBehavioral",
                || where_used(prism, session, id, Some("behavioral"), crate::INSIGHT_LIMIT),
            )
        });
        let spec_cluster_run = query_run.clone();
        let spec_cluster_task = scope.spawn(move || {
            record_bundle_phase(
                spec_cluster_run.as_ref(),
                id,
                "discoveryBundle.specCluster",
                || {
                    crate::spec_cluster_view(prism, id)
                        .ok()
                        .map(convert_view)
                        .transpose()
                },
            )
        });
        Ok((
            spec_drift_task
                .join()
                .expect("spec-drift bundle task panicked")?,
            entrypoints_task
                .join()
                .expect("entrypoints bundle task panicked")?,
            where_used_direct_task
                .join()
                .expect("direct where-used bundle task panicked")?,
            where_used_behavioral_task
                .join()
                .expect("behavioral where-used bundle task panicked")?,
            spec_cluster_task
                .join()
                .expect("spec-cluster bundle task panicked")?,
        ))
    })?;
    let suggested_reads = record_bundle_phase(
        query_run.as_ref(),
        id,
        "discoveryBundle.suggestedReads",
        || {
            Ok(spec_drift
                .as_ref()
                .map(|drift| drift.next_reads.clone())
                .filter(|reads| !reads.is_empty())
                .unwrap_or(next_reads(prism, id, crate::INSIGHT_LIMIT)?))
        },
    )?;
    let suggested_reads = if suggested_reads.is_empty() {
        cached_owner_views(prism, cache, id)?.all.clone()
    } else {
        suggested_reads
    };
    let read_context = record_bundle_phase(
        query_run.as_ref(),
        id,
        "discoveryBundle.readContext",
        || read_context_view_cached(prism, session, cache, id),
    )?;
    let edit_context = record_bundle_phase(
        query_run.as_ref(),
        id,
        "discoveryBundle.editContext",
        || edit_context_view_cached(prism, session, cache, id),
    )?;
    let validation_context = record_bundle_phase(
        query_run.as_ref(),
        id,
        "discoveryBundle.validationContext",
        || validation_context_view_cached(prism, session, cache, id),
    )?;
    let recent_change_context = record_bundle_phase(
        query_run.as_ref(),
        id,
        "discoveryBundle.recentChangeContext",
        || recent_change_context_view_cached(prism, session, cache, id),
    )?;
    let suggested_queries = symbol_suggested_queries(id);
    let (
        relations,
        lineage,
        co_change_neighbors,
        related_failures,
        blast_radius,
        validation_recipe,
    ) = record_bundle_phase(
        query_run.as_ref(),
        id,
        "discoveryBundle.sharedContext",
        || {
            Ok((
                cached_relations(prism, session, cache, id)?,
                cached_lineage(prism, cache, id)?,
                cached_co_change_neighbors(prism, cache, id),
                cached_recent_failures(prism, cache, id),
                cached_blast_radius(prism, session, cache, id),
                cached_validation_recipe(prism, session, cache, id),
            ))
        },
    )?;

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

fn record_bundle_phase<T>(
    query_run: Option<&QueryRun>,
    id: &NodeId,
    operation: &str,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let started = Instant::now();
    let result = work();
    if let Some(query_run) = query_run {
        query_run.record_phase(
            operation,
            &json!({
                "id": {
                    "crateName": id.crate_name.clone(),
                    "kind": id.kind,
                    "path": id.path.clone(),
                }
            }),
            started.elapsed(),
            result.is_ok(),
            result.as_ref().err().map(ToString::to_string),
        );
    }
    result
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
