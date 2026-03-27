use std::collections::HashMap;

use anyhow::Result;
use prism_ir::{AnchorRef, NodeId, NodeKind};
use prism_js::{
    ChangeImpactView, CoChangeView, EditContextView, FocusedBlockView, LineageView, NodeIdView,
    ReadContextView, RecentChangeContextView, RelationsView, ScoredMemoryView, SuggestedQueryView,
    SymbolView, ValidationContextView, ValidationRecipeView,
};
use prism_memory::{MemoryModule, OutcomeEvent, RecallQuery};
use prism_query::Prism;
use serde_json::json;

use crate::{
    blast_radius_view, co_change_view, context_target_block, focused_blocks_for_symbol_views,
    grouped_owner_views_for_target, lineage_view, promoted_summary_texts, relations_view,
    scored_memory_view, symbol_for, symbol_view, GroupedOwnerCandidateViews, SessionState,
    CONTEXT_BLOCK_LIMIT, INSIGHT_LIMIT,
};

const MEMORY_CONTEXT_LIMIT: usize = 5;
const FAILURE_CONTEXT_LIMIT: usize = 8;
const RECENT_EVENT_LIMIT: usize = 12;

#[derive(Default)]
pub(crate) struct SemanticContextCache {
    pub(crate) target_symbols: HashMap<NodeId, SymbolView>,
    pub(crate) target_blocks: HashMap<NodeId, FocusedBlockView>,
    pub(crate) relations: HashMap<NodeId, RelationsView>,
    pub(crate) direct_links: HashMap<NodeId, Vec<SymbolView>>,
    pub(crate) owner_views: HashMap<NodeId, GroupedOwnerCandidateViews>,
    pub(crate) focused_blocks: HashMap<String, Vec<FocusedBlockView>>,
    pub(crate) related_memory: HashMap<NodeId, Vec<ScoredMemoryView>>,
    pub(crate) recent_failures: HashMap<NodeId, Vec<OutcomeEvent>>,
    pub(crate) blast_radius: HashMap<NodeId, ChangeImpactView>,
    pub(crate) validation_recipe: HashMap<NodeId, ValidationRecipeView>,
    pub(crate) recent_events: HashMap<NodeId, Vec<OutcomeEvent>>,
    pub(crate) co_change_neighbors: HashMap<NodeId, Vec<CoChangeView>>,
    pub(crate) promoted_summaries: HashMap<NodeId, Vec<String>>,
    pub(crate) lineage: HashMap<NodeId, Option<LineageView>>,
}

pub(crate) fn read_context_view_cached(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<ReadContextView> {
    let target_symbol = cached_target_symbol(prism, cache, target)?;
    let target_block = cached_target_block(prism, cache, target)?;
    let direct_links = cached_direct_links(prism, session, cache, target)?;
    let direct_link_blocks =
        cached_focused_blocks(prism, cache, &direct_links, CONTEXT_BLOCK_LIMIT)?;
    let owners = cached_owner_views(prism, cache, target)?;
    let suggested_reads = owners.read_path.clone();
    let tests = owners.tests.clone();
    let test_symbols = tests
        .iter()
        .map(|candidate| candidate.symbol.clone())
        .collect::<Vec<_>>();
    let test_blocks = cached_focused_blocks(prism, cache, &test_symbols, CONTEXT_BLOCK_LIMIT)?;
    let related_memory = cached_related_memory(prism, session, cache, target)?;
    let recent_failures = cached_recent_failures(prism, cache, target);
    let validation_recipe = cached_validation_recipe(prism, session, cache, target);

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
        target_block,
        direct_links,
        direct_link_blocks,
        suggested_reads,
        tests,
        test_blocks,
        related_memory,
        recent_failures,
        validation_recipe,
        why,
        suggested_queries: read_context_queries(target),
    })
}

pub(crate) fn edit_context_view_cached(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<EditContextView> {
    let target_symbol = cached_target_symbol(prism, cache, target)?;
    let target_block = cached_target_block(prism, cache, target)?;
    let direct_links = cached_direct_links(prism, session, cache, target)?;
    let direct_link_blocks =
        cached_focused_blocks(prism, cache, &direct_links, CONTEXT_BLOCK_LIMIT)?;
    let owners = cached_owner_views(prism, cache, target)?;
    let suggested_reads = owners.read_path.clone();
    let mut write_paths = owners.write_path.clone();
    if write_paths.is_empty() {
        write_paths = owners.persistence_path.clone();
    }
    if write_paths.is_empty() {
        write_paths = owners.all.clone();
    }
    let tests = owners.tests.clone();
    let write_path_symbols = write_paths
        .iter()
        .map(|candidate| candidate.symbol.clone())
        .collect::<Vec<_>>();
    let write_path_blocks =
        cached_focused_blocks(prism, cache, &write_path_symbols, CONTEXT_BLOCK_LIMIT)?;
    let test_symbols = tests
        .iter()
        .map(|candidate| candidate.symbol.clone())
        .collect::<Vec<_>>();
    let test_blocks = cached_focused_blocks(prism, cache, &test_symbols, CONTEXT_BLOCK_LIMIT)?;
    let related_memory = cached_related_memory(prism, session, cache, target)?;
    let recent_failures = cached_recent_failures(prism, cache, target);
    let blast_radius = cached_blast_radius(prism, session, cache, target);
    let validation_recipe = cached_validation_recipe(prism, session, cache, target);

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
        target_block,
        direct_links,
        direct_link_blocks,
        suggested_reads,
        write_paths,
        write_path_blocks,
        tests,
        test_blocks,
        related_memory,
        recent_failures,
        blast_radius,
        validation_recipe,
        checklist,
        suggested_queries: edit_context_queries(target),
    })
}

pub(crate) fn validation_context_view_cached(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<ValidationContextView> {
    let target_symbol = cached_target_symbol(prism, cache, target)?;
    let target_block = cached_target_block(prism, cache, target)?;
    let tests = cached_owner_views(prism, cache, target)?.tests.clone();
    let test_symbols = tests
        .iter()
        .map(|candidate| candidate.symbol.clone())
        .collect::<Vec<_>>();
    let test_blocks = cached_focused_blocks(prism, cache, &test_symbols, CONTEXT_BLOCK_LIMIT)?;
    let related_memory = cached_related_memory(prism, session, cache, target)?;
    let recent_failures = cached_recent_failures(prism, cache, target);
    let blast_radius = cached_blast_radius(prism, session, cache, target);
    let validation_recipe = cached_validation_recipe(prism, session, cache, target);

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
        target_block,
        tests,
        test_blocks,
        related_memory,
        recent_failures,
        blast_radius,
        validation_recipe,
        why,
        suggested_queries: validation_context_queries(target),
    })
}

pub(crate) fn recent_change_context_view_cached(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<RecentChangeContextView> {
    let target_symbol = cached_target_symbol(prism, cache, target)?;
    let recent_events = cached_recent_events(prism, cache, target);
    let recent_failures = cached_recent_failures(prism, cache, target);
    let co_change_neighbors = cached_co_change_neighbors(prism, cache, target);
    let related_memory = cached_related_memory(prism, session, cache, target)?;
    let promoted_summaries = cached_promoted_summaries(prism, session, cache, target);
    let lineage = cached_lineage(prism, cache, target)?;

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

fn cached_target_symbol(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<SymbolView> {
    if let Some(value) = cache.target_symbols.get(target) {
        return Ok(value.clone());
    }
    let value = symbol_view(prism, &symbol_for(prism, target)?)?;
    cache.target_symbols.insert(target.clone(), value.clone());
    Ok(value)
}

fn cached_target_block(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<FocusedBlockView> {
    if let Some(value) = cache.target_blocks.get(target) {
        return Ok(value.clone());
    }
    let value = context_target_block(prism, target)?;
    cache.target_blocks.insert(target.clone(), value.clone());
    Ok(value)
}

fn cached_relations(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<RelationsView> {
    if let Some(value) = cache.relations.get(target) {
        return Ok(value.clone());
    }
    let value = relations_view(prism, session, target)?;
    cache.relations.insert(target.clone(), value.clone());
    Ok(value)
}

fn cached_direct_links(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<Vec<SymbolView>> {
    if let Some(value) = cache.direct_links.get(target) {
        return Ok(value.clone());
    }
    let relations = cached_relations(prism, session, cache, target)?;
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
    cache.direct_links.insert(target.clone(), links.clone());
    Ok(links)
}

fn cached_owner_views(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<GroupedOwnerCandidateViews> {
    if let Some(value) = cache.owner_views.get(target) {
        return Ok(value.clone());
    }
    let value = grouped_owner_views_for_target(prism, target, INSIGHT_LIMIT)?;
    cache.owner_views.insert(target.clone(), value.clone());
    Ok(value)
}

fn cached_focused_blocks(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    symbols: &[SymbolView],
    limit: usize,
) -> Result<Vec<FocusedBlockView>> {
    let key = focused_blocks_key(symbols, limit);
    if let Some(value) = cache.focused_blocks.get(&key) {
        return Ok(value.clone());
    }
    let value = focused_blocks_for_symbol_views(prism, symbols, limit)?;
    cache.focused_blocks.insert(key, value.clone());
    Ok(value)
}

fn cached_related_memory(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<Vec<ScoredMemoryView>> {
    if let Some(value) = cache.related_memory.get(target) {
        return Ok(value.clone());
    }
    let value = session
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
        .collect::<Vec<_>>();
    cache.related_memory.insert(target.clone(), value.clone());
    Ok(value)
}

fn cached_recent_failures(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Vec<OutcomeEvent> {
    if let Some(value) = cache.recent_failures.get(target) {
        return value.clone();
    }
    let mut failures = prism.related_failures(target);
    failures.truncate(FAILURE_CONTEXT_LIMIT);
    cache
        .recent_failures
        .insert(target.clone(), failures.clone());
    failures
}

fn cached_blast_radius(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> ChangeImpactView {
    if let Some(value) = cache.blast_radius.get(target) {
        return value.clone();
    }
    let value = blast_radius_view(prism, session, target);
    cache.blast_radius.insert(target.clone(), value.clone());
    value
}

fn cached_validation_recipe(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> ValidationRecipeView {
    if let Some(value) = cache.validation_recipe.get(target) {
        return value.clone();
    }
    let blast_radius = cached_blast_radius(prism, session, cache, target);
    let value = ValidationRecipeView {
        target: NodeIdView {
            crate_name: target.crate_name.to_string(),
            path: target.path.to_string(),
            kind: target.kind,
        },
        checks: blast_radius.likely_validations.clone(),
        scored_checks: blast_radius.validation_checks.clone(),
        related_nodes: blast_radius.direct_nodes.clone(),
        co_change_neighbors: blast_radius.co_change_neighbors.clone(),
        recent_failures: blast_radius.risk_events.clone(),
    };
    cache
        .validation_recipe
        .insert(target.clone(), value.clone());
    value
}

fn cached_recent_events(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Vec<OutcomeEvent> {
    if let Some(value) = cache.recent_events.get(target) {
        return value.clone();
    }
    let value = prism.outcomes_for(&[AnchorRef::Node(target.clone())], RECENT_EVENT_LIMIT);
    cache.recent_events.insert(target.clone(), value.clone());
    value
}

fn cached_co_change_neighbors(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Vec<CoChangeView> {
    if let Some(value) = cache.co_change_neighbors.get(target) {
        return value.clone();
    }
    let value = prism
        .co_change_neighbors(target, 8)
        .into_iter()
        .map(co_change_view)
        .collect::<Vec<_>>();
    cache
        .co_change_neighbors
        .insert(target.clone(), value.clone());
    value
}

fn cached_promoted_summaries(
    prism: &Prism,
    session: &SessionState,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Vec<String> {
    if let Some(value) = cache.promoted_summaries.get(target) {
        return value.clone();
    }
    let value = promoted_summary_texts(session, prism, &[AnchorRef::Node(target.clone())]);
    cache
        .promoted_summaries
        .insert(target.clone(), value.clone());
    value
}

fn cached_lineage(
    prism: &Prism,
    cache: &mut SemanticContextCache,
    target: &NodeId,
) -> Result<Option<LineageView>> {
    if let Some(value) = cache.lineage.get(target) {
        return Ok(value.clone());
    }
    let value = lineage_view(prism, target)?;
    cache.lineage.insert(target.clone(), value.clone());
    Ok(value)
}

fn focused_blocks_key(symbols: &[SymbolView], limit: usize) -> String {
    let mut key = format!("{limit}:");
    for symbol in symbols {
        key.push_str(&symbol.id.crate_name);
        key.push(':');
        key.push_str(&symbol.id.kind.to_string());
        key.push(':');
        key.push_str(&symbol.id.path);
        key.push('|');
    }
    key
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
            label: "Focused Block".to_string(),
            query: format!("return prism.focusedBlock({target_json});"),
            why: "Jump straight to the exact local block around this target.".to_string(),
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
            label: "Focused Block".to_string(),
            query: format!("return prism.focusedBlock({target_json});"),
            why: "Show the exact local block to inspect before editing.".to_string(),
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
            label: "Focused Block".to_string(),
            query: format!("return prism.focusedBlock({target_json});"),
            why: "Expand this target into its exact local block before choosing validations."
                .to_string(),
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
