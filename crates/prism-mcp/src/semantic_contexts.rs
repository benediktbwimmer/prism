use anyhow::Result;
use prism_ir::{AnchorRef, NodeId, NodeKind};
use prism_js::{
    CoChangeView, EditContextView, ReadContextView, RecentChangeContextView, ScoredMemoryView,
    SuggestedQueryView, SymbolView, ValidationContextView,
};
use prism_memory::{MemoryModule, RecallQuery};
use prism_query::Prism;
use serde_json::json;

use crate::{
    blast_radius_view, co_change_view, lineage_view, owner_views_for_target,
    promoted_summary_texts, relations_view, scored_memory_view, symbol_for, symbol_view,
    validation_recipe_view_with, SessionState, INSIGHT_LIMIT,
};

const MEMORY_CONTEXT_LIMIT: usize = 5;
const FAILURE_CONTEXT_LIMIT: usize = 8;
const RECENT_EVENT_LIMIT: usize = 12;

pub(crate) fn read_context_view(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
) -> Result<ReadContextView> {
    let target_symbol = symbol_view(prism, &symbol_for(prism, target)?)?;
    let direct_links = direct_links(prism, session, target)?;
    let suggested_reads = owner_views_for_target(prism, target, Some("read"), INSIGHT_LIMIT)?;
    let tests = owner_views_for_target(prism, target, Some("test"), INSIGHT_LIMIT)?;
    let related_memory = related_memory(prism, session, target)?;
    let recent_failures = recent_failures(prism, target);
    let validation_recipe = validation_recipe_view_with(prism, session, target);

    let mut why = vec![
        "Direct links come from exact graph edges around the requested target.".to_string(),
        "Suggested reads are heuristic owner candidates scored from read-oriented paths, names, and excerpts.".to_string(),
    ];
    if !tests.is_empty() {
        why.push(
            "Test suggestions highlight validation owners that matched the same discovery terms."
                .to_string(),
        );
    }
    if !related_memory.is_empty() {
        why.push(
            "Related memory is recalled from session notes anchored to this target.".to_string(),
        );
    }

    Ok(ReadContextView {
        target: target_symbol,
        direct_links,
        suggested_reads,
        tests,
        related_memory,
        recent_failures,
        validation_recipe,
        why,
        suggested_queries: read_context_queries(target),
    })
}

pub(crate) fn edit_context_view(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
) -> Result<EditContextView> {
    let target_symbol = symbol_view(prism, &symbol_for(prism, target)?)?;
    let direct_links = direct_links(prism, session, target)?;
    let suggested_reads = owner_views_for_target(prism, target, Some("read"), INSIGHT_LIMIT)?;
    let mut write_paths = owner_views_for_target(prism, target, Some("write"), INSIGHT_LIMIT)?;
    if write_paths.is_empty() {
        write_paths = owner_views_for_target(prism, target, Some("persist"), INSIGHT_LIMIT)?;
    }
    if write_paths.is_empty() {
        write_paths = owner_views_for_target(prism, target, None, INSIGHT_LIMIT)?;
    }
    let tests = owner_views_for_target(prism, target, Some("test"), INSIGHT_LIMIT)?;
    let related_memory = related_memory(prism, session, target)?;
    let recent_failures = recent_failures(prism, target);
    let blast_radius = blast_radius_view(prism, session, target);
    let validation_recipe = validation_recipe_view_with(prism, session, target);

    let mut checklist = vec![
        "Read the direct links before editing to confirm the concrete code path.".to_string(),
        "Inspect write owners before changing behavior that persists or mutates state.".to_string(),
    ];
    if !validation_recipe.checks.is_empty() {
        checklist.push("Run the suggested validations after the edit.".to_string());
    }
    if !recent_failures.is_empty() {
        checklist.push(
            "Review recent failures first to avoid repeating a known regression.".to_string(),
        );
    }

    Ok(EditContextView {
        target: target_symbol,
        direct_links,
        suggested_reads,
        write_paths,
        tests,
        related_memory,
        recent_failures,
        blast_radius,
        validation_recipe,
        checklist,
        suggested_queries: edit_context_queries(target),
    })
}

pub(crate) fn validation_context_view(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
) -> Result<ValidationContextView> {
    let target_symbol = symbol_view(prism, &symbol_for(prism, target)?)?;
    let tests = owner_views_for_target(prism, target, Some("test"), INSIGHT_LIMIT)?;
    let related_memory = related_memory(prism, session, target)?;
    let recent_failures = recent_failures(prism, target);
    let blast_radius = blast_radius_view(prism, session, target);
    let validation_recipe = validation_recipe_view_with(prism, session, target);

    let mut why = vec![
        "Validation context combines the strongest test owners with PRISM's validation recipe."
            .to_string(),
        "Recent failures stay attached so the recommended checks reflect known regressions."
            .to_string(),
    ];
    if !blast_radius.direct_nodes.is_empty() {
        why.push(
            "Blast radius highlights the directly impacted nodes that should shape validation."
                .to_string(),
        );
    }

    Ok(ValidationContextView {
        target: target_symbol,
        tests,
        related_memory,
        recent_failures,
        blast_radius,
        validation_recipe,
        why,
        suggested_queries: validation_context_queries(target),
    })
}

pub(crate) fn recent_change_context_view(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
) -> Result<RecentChangeContextView> {
    let target_symbol = symbol_view(prism, &symbol_for(prism, target)?)?;
    let recent_events = prism.outcomes_for(&[AnchorRef::Node(target.clone())], RECENT_EVENT_LIMIT);
    let recent_failures = recent_failures(prism, target);
    let co_change_neighbors = prism
        .co_change_neighbors(target, 8)
        .into_iter()
        .map(co_change_view)
        .collect::<Vec<CoChangeView>>();
    let related_memory = related_memory(prism, session, target)?;
    let promoted_summaries =
        promoted_summary_texts(session, prism, &[AnchorRef::Node(target.clone())]);
    let lineage = lineage_view(prism, target)?;

    let mut why = vec![
        "Recent change context groups the latest recorded outcomes for this target.".to_string(),
        "Co-change neighbors show which nearby lineages tend to move with it.".to_string(),
    ];
    if lineage.is_some() {
        why.push(
            "Lineage is included so recent activity stays attached even when the symbol moved."
                .to_string(),
        );
    }
    if !related_memory.is_empty() {
        why.push(
            "Related memory carries earlier notes and preserved context into the recent-change view."
                .to_string(),
        );
    }

    Ok(RecentChangeContextView {
        target: target_symbol,
        recent_events,
        recent_failures,
        co_change_neighbors,
        related_memory,
        promoted_summaries,
        lineage,
        why,
        suggested_queries: recent_change_context_queries(target),
    })
}

pub(crate) fn read_context_queries(target: &NodeId) -> Vec<SuggestedQueryView> {
    let target_json = target_input_json(target);
    vec![
        SuggestedQueryView {
            label: "Read Context".to_string(),
            query: format!("return prism.readContext({target_json});"),
            why: "Fetch the semantic read bundle for this exact target.".to_string(),
        },
        SuggestedQueryView {
            label: "Next Reads".to_string(),
            query: format!("return prism.nextReads({target_json}, {{ limit: 5 }});"),
            why: "Ask PRISM for the next read-oriented candidates directly.".to_string(),
        },
        SuggestedQueryView {
            label: "Where Used".to_string(),
            query: format!(
                "return prism.whereUsed({target_json}, {{ mode: \"direct\", limit: 5 }});"
            ),
            why: "Inspect direct usages before jumping into a wider search.".to_string(),
        },
        SuggestedQueryView {
            label: "Validation Recipe".to_string(),
            query: format!("return prism.validationRecipe({target_json});"),
            why: "See the tests and checks most likely to validate a change here.".to_string(),
        },
    ]
}

pub(crate) fn edit_context_queries(target: &NodeId) -> Vec<SuggestedQueryView> {
    let target_json = target_input_json(target);
    vec![
        SuggestedQueryView {
            label: "Edit Context".to_string(),
            query: format!("return prism.editContext({target_json});"),
            why: "Fetch the edit-focused bundle with write paths, blast radius, and validations."
                .to_string(),
        },
        SuggestedQueryView {
            label: "Write Owners".to_string(),
            query: format!("return prism.owners({target_json}, {{ kind: \"write\", limit: 5 }});"),
            why: "Inspect write-oriented owners before making a mutation.".to_string(),
        },
        SuggestedQueryView {
            label: "Entry Points".to_string(),
            query: format!("return prism.entrypointsFor({target_json}, {{ limit: 5 }});"),
            why: "Find the reachable entrypoints before tracing an edit through the code path."
                .to_string(),
        },
        SuggestedQueryView {
            label: "Blast Radius".to_string(),
            query: format!("return prism.blastRadius({target_json});"),
            why: "Estimate connected impact before editing.".to_string(),
        },
    ]
}

pub(crate) fn validation_context_queries(target: &NodeId) -> Vec<SuggestedQueryView> {
    let target_json = target_input_json(target);
    vec![
        SuggestedQueryView {
            label: "Validation Context".to_string(),
            query: format!("return prism.validationContext({target_json});"),
            why: "Fetch the validation-focused bundle for this exact target.".to_string(),
        },
        SuggestedQueryView {
            label: "Validation Recipe".to_string(),
            query: format!("return prism.validationRecipe({target_json});"),
            why: "Inspect the checks PRISM thinks are most likely to validate a change."
                .to_string(),
        },
        SuggestedQueryView {
            label: "Test Owners".to_string(),
            query: format!("return prism.owners({target_json}, {{ kind: \"test\", limit: 5 }});"),
            why: "List the strongest test-oriented owner candidates only.".to_string(),
        },
    ]
}

pub(crate) fn recent_change_context_queries(target: &NodeId) -> Vec<SuggestedQueryView> {
    let target_json = target_input_json(target);
    vec![
        SuggestedQueryView {
            label: "Recent Change Context".to_string(),
            query: format!("return prism.recentChangeContext({target_json});"),
            why: "Fetch the recent outcome, co-change, and lineage bundle for this target."
                .to_string(),
        },
        SuggestedQueryView {
            label: "Recent Outcomes".to_string(),
            query: format!(
                "return prism.memory.outcomes({{ focus: [{target_json}], limit: 10 }});"
            ),
            why: "Inspect the latest recorded outcomes without reconstructing anchors.".to_string(),
        },
        SuggestedQueryView {
            label: "Co-Change Neighbors".to_string(),
            query: format!("return prism.coChangeNeighbors({target_json});"),
            why: "See which lineages tend to move with this target.".to_string(),
        },
    ]
}

pub(crate) fn search_queries(query: &str) -> Vec<SuggestedQueryView> {
    let query_json = serde_json::to_string(query).expect("query string should serialize");
    vec![
        SuggestedQueryView {
            label: "Direct Search".to_string(),
            query: format!("return prism.search({query_json}, {{ limit: 5 }});"),
            why: "Inspect a narrow direct symbol search first.".to_string(),
        },
        SuggestedQueryView {
            label: "Behavioral Search".to_string(),
            query: format!(
                "return prism.search({query_json}, {{ strategy: \"behavioral\", ownerKind: \"read\", limit: 5 }});"
            ),
            why: "Ask PRISM for read-oriented owners instead of exact noun matches.".to_string(),
        },
    ]
}

fn direct_links(prism: &Prism, session: &SessionState, target: &NodeId) -> Result<Vec<SymbolView>> {
    let relations = relations_view(prism, session, target)?;
    let mut links = Vec::new();
    push_unique_symbols(&mut links, &relations.specifies, INSIGHT_LIMIT);
    push_unique_symbols(&mut links, &relations.specified_by, INSIGHT_LIMIT);
    push_unique_symbols(&mut links, &relations.implements, INSIGHT_LIMIT);
    push_unique_symbols(&mut links, &relations.validates, INSIGHT_LIMIT);
    push_unique_symbols(&mut links, &relations.validated_by, INSIGHT_LIMIT);
    push_unique_symbols(&mut links, &relations.related, INSIGHT_LIMIT);
    push_unique_symbols(&mut links, &relations.related_by, INSIGHT_LIMIT);
    if links.is_empty() {
        push_unique_symbols(&mut links, &relations.callers, INSIGHT_LIMIT);
        push_unique_symbols(&mut links, &relations.callees, INSIGHT_LIMIT);
        push_unique_symbols(&mut links, &relations.references, INSIGHT_LIMIT);
    }
    Ok(links)
}

fn push_unique_symbols(target: &mut Vec<SymbolView>, candidates: &[SymbolView], limit: usize) {
    for candidate in candidates {
        if target.len() >= limit {
            break;
        }
        if target.iter().any(|existing| existing.id == candidate.id) {
            continue;
        }
        target.push(candidate.clone());
    }
}

fn related_memory(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
) -> Result<Vec<ScoredMemoryView>> {
    Ok(session
        .notes
        .recall(&RecallQuery {
            focus: prism.anchors_for(&[AnchorRef::Node(target.clone())]),
            text: None,
            limit: MEMORY_CONTEXT_LIMIT,
            kinds: None,
            since: None,
        })?
        .into_iter()
        .map(scored_memory_view)
        .collect())
}

fn recent_failures(prism: &Prism, target: &NodeId) -> Vec<prism_memory::OutcomeEvent> {
    let mut failures = prism.related_failures(target);
    failures.truncate(FAILURE_CONTEXT_LIMIT);
    failures
}

fn target_input_json(target: &NodeId) -> String {
    json!({
        "crateName": target.crate_name,
        "path": target.path,
        "kind": node_kind_label(target.kind),
    })
    .to_string()
}

fn node_kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Workspace => "workspace",
        NodeKind::Package => "package",
        NodeKind::Document => "document",
        NodeKind::Module => "module",
        NodeKind::Function => "function",
        NodeKind::Struct => "struct",
        NodeKind::Enum => "enum",
        NodeKind::Trait => "trait",
        NodeKind::Impl => "impl",
        NodeKind::Method => "method",
        NodeKind::Field => "field",
        NodeKind::TypeAlias => "type-alias",
        NodeKind::MarkdownHeading => "markdown-heading",
        NodeKind::JsonKey => "json-key",
        NodeKind::TomlKey => "toml-key",
        NodeKind::YamlKey => "yaml-key",
    }
}
