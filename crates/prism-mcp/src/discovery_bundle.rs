use anyhow::Result;
use prism_ir::NodeId;
use prism_js::CoChangeView;
use prism_query::Prism;

use crate::{
    blast_radius_view, co_change_view, edit_context_view, entrypoints_for, lineage_view,
    next_reads, owner_views_for_target, read_context_view, recent_change_context_view,
    relations_view, symbol_for, symbol_suggested_queries, symbol_view, validation_context_view,
    validation_recipe_view_with, where_used, DiscoveryBundleView, SessionState,
};

pub(crate) fn discovery_bundle_view(
    prism: &Prism,
    session: &SessionState,
    id: &NodeId,
) -> Result<DiscoveryBundleView> {
    let target = symbol_view(prism, &symbol_for(prism, id)?)?;
    let spec_drift = crate::spec_drift_explanation_view(prism, id).ok();
    let suggested_reads = spec_drift
        .as_ref()
        .map(|drift| drift.next_reads.clone())
        .filter(|reads| !reads.is_empty())
        .unwrap_or(next_reads(prism, id, crate::INSIGHT_LIMIT)?);
    let suggested_reads = if suggested_reads.is_empty() {
        owner_views_for_target(prism, id, None, crate::INSIGHT_LIMIT)?
    } else {
        suggested_reads
    };
    let read_context = read_context_view(prism, session, id)?;
    let edit_context = edit_context_view(prism, session, id)?;
    let validation_context = validation_context_view(prism, session, id)?;
    let recent_change_context = recent_change_context_view(prism, session, id)?;
    let entrypoints = entrypoints_for(prism, session, id, crate::INSIGHT_LIMIT)?;
    let where_used_direct = where_used(prism, session, id, Some("direct"), crate::INSIGHT_LIMIT)?;
    let where_used_behavioral =
        where_used(prism, session, id, Some("behavioral"), crate::INSIGHT_LIMIT)?;
    let suggested_queries = symbol_suggested_queries(id);
    let relations = relations_view(prism, session, id)?;
    let spec_cluster = crate::spec_cluster_view(prism, id).ok();
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
        why,
    })
}
