use std::thread;

use anyhow::{anyhow, Result};
use prism_ir::{AnchorRef, EdgeKind, NodeId};
use prism_js::{
    ChangeImpactView, DiscoveryBundleView, EditContextView, FocusedBlockView, OwnerCandidateView,
    OwnerHintView, ReadContextView, RecentChangeContextView, RelationsView, SourceExcerptView,
    SourceLocationView, SourceSliceView, SymbolView, ValidationContextView, ValidationRecipeView,
};
use prism_query::{EditSliceOptions, Prism, SourceExcerptOptions, Symbol};

use crate::{
    change_impact_view, merge_node_ids, merge_promoted_checks, node_id_view,
    promoted_summary_texts, promoted_validation_checks, validation_recipe_view, SessionState,
};

const CANDIDATE_EXCERPT_OPTIONS: SourceExcerptOptions = SourceExcerptOptions {
    context_lines: 0,
    max_lines: 4,
    max_chars: 240,
};

const FOCUSED_BLOCK_EXCERPT_OPTIONS: SourceExcerptOptions = SourceExcerptOptions {
    context_lines: 0,
    max_lines: 12,
    max_chars: 640,
};

const CONTEXT_BLOCK_OPTIONS: EditSliceOptions = EditSliceOptions {
    before_lines: 1,
    after_lines: 1,
    max_lines: 8,
    max_chars: 320,
};

pub(crate) const CONTEXT_BLOCK_LIMIT: usize = 3;

pub(crate) fn symbol_view(prism: &Prism, symbol: &Symbol<'_>) -> Result<SymbolView> {
    symbol_view_with_owner_hint(prism, symbol, None)
}

pub(crate) fn symbol_view_without_excerpt(
    prism: &Prism,
    symbol: &Symbol<'_>,
) -> Result<SymbolView> {
    symbol_view_with_excerpt(prism, symbol, None, false)
}

pub(crate) fn symbol_view_with_owner_hint(
    prism: &Prism,
    symbol: &Symbol<'_>,
    owner_hint: Option<OwnerHintView>,
) -> Result<SymbolView> {
    symbol_view_with_excerpt(prism, symbol, owner_hint, true)
}

fn symbol_view_with_excerpt(
    prism: &Prism,
    symbol: &Symbol<'_>,
    owner_hint: Option<OwnerHintView>,
    include_source_excerpt: bool,
) -> Result<SymbolView> {
    let node = symbol.node();
    Ok(SymbolView {
        id: node_id_view(symbol.id().clone()),
        name: symbol.name().to_owned(),
        kind: node.kind,
        signature: symbol.signature(),
        file_path: prism
            .graph()
            .file_path(node.file)
            .map(|path| path.to_string_lossy().into_owned()),
        span: node.span,
        location: symbol.location().map(source_location_view),
        language: node.language,
        lineage_id: prism
            .lineage_of(symbol.id())
            .map(|lineage| lineage.0.to_string()),
        source_excerpt: include_source_excerpt
            .then(|| symbol.excerpt(SourceExcerptOptions::default()))
            .flatten()
            .map(source_excerpt_view),
        owner_hint,
    })
}

pub(crate) fn symbol_views_for_ids(prism: &Prism, ids: Vec<NodeId>) -> Result<Vec<SymbolView>> {
    ids.into_iter()
        .map(|id| symbol_for(prism, &id).and_then(|symbol| symbol_view(prism, &symbol)))
        .collect()
}

pub(crate) fn symbol_views_for_ids_without_excerpt(
    prism: &Prism,
    ids: Vec<NodeId>,
) -> Result<Vec<SymbolView>> {
    ids.into_iter()
        .map(|id| {
            symbol_for(prism, &id).and_then(|symbol| symbol_view_without_excerpt(prism, &symbol))
        })
        .collect()
}

pub(crate) fn symbol_for<'a>(prism: &'a Prism, id: &NodeId) -> Result<Symbol<'a>> {
    prism
        .symbol_by_id(id)
        .ok_or_else(|| anyhow!("unknown symbol `{}`", id.path))
}

pub(crate) fn source_excerpt_for_symbol(
    symbol: &Symbol<'_>,
    options: SourceExcerptOptions,
) -> Option<SourceExcerptView> {
    symbol.excerpt(options).map(source_excerpt_view)
}

pub(crate) fn edit_slice_for_symbol(
    symbol: &Symbol<'_>,
    options: EditSliceOptions,
) -> Option<SourceSliceView> {
    symbol.edit_slice(options).map(source_slice_view)
}

pub(crate) fn focused_block_for_symbol(
    prism: &Prism,
    symbol: &Symbol<'_>,
    options: EditSliceOptions,
) -> Result<FocusedBlockView> {
    let symbol_view = symbol_view_without_excerpt(prism, symbol)?;
    let fallback_max_lines = options
        .max_lines
        .max(FOCUSED_BLOCK_EXCERPT_OPTIONS.max_lines);
    let fallback_max_chars = options
        .max_chars
        .max(FOCUSED_BLOCK_EXCERPT_OPTIONS.max_chars);
    let slice = edit_slice_for_symbol(symbol, options);
    let excerpt = if slice.is_none() {
        source_excerpt_for_symbol(
            symbol,
            SourceExcerptOptions {
                context_lines: 0,
                max_lines: fallback_max_lines,
                max_chars: fallback_max_chars,
            },
        )
    } else {
        None
    };
    let strategy = if slice.is_some() {
        "edit_slice"
    } else if excerpt.is_some() {
        "excerpt_fallback"
    } else {
        "symbol_only"
    };
    Ok(FocusedBlockView {
        symbol: symbol_view,
        slice,
        excerpt,
        strategy: strategy.to_string(),
    })
}

pub(crate) fn focused_blocks_for_symbol_views(
    prism: &Prism,
    symbols: &[SymbolView],
    limit: usize,
) -> Result<Vec<FocusedBlockView>> {
    let symbols = symbols.iter().take(limit).collect::<Vec<_>>();
    let worker_count = focused_block_worker_count(symbols.len());
    if worker_count <= 1 {
        return symbols
            .into_iter()
            .map(|symbol| focused_block_from_symbol_view(prism, symbol))
            .collect();
    }

    let chunk_size = symbols.len().div_ceil(worker_count);
    thread::scope(|scope| -> Result<Vec<FocusedBlockView>> {
        let mut tasks = Vec::new();
        for chunk in symbols.chunks(chunk_size) {
            tasks.push(scope.spawn(move || -> Result<Vec<FocusedBlockView>> {
                chunk
                    .iter()
                    .map(|symbol| focused_block_from_symbol_view(prism, symbol))
                    .collect()
            }));
        }

        let mut focused_blocks = Vec::with_capacity(symbols.len());
        for task in tasks {
            focused_blocks.extend(
                task.join()
                    .expect("focused-block worker panicked while expanding symbols")?,
            );
        }
        Ok(focused_blocks)
    })
}

pub(crate) fn context_target_block(prism: &Prism, target: &NodeId) -> Result<FocusedBlockView> {
    let symbol = symbol_for(prism, target)?;
    focused_block_for_symbol(prism, &symbol, CONTEXT_BLOCK_OPTIONS)
}

fn focused_block_from_symbol_view(prism: &Prism, symbol: &SymbolView) -> Result<FocusedBlockView> {
    let id = NodeId::new(
        symbol.id.crate_name.clone(),
        symbol.id.path.clone(),
        symbol.kind,
    );
    let symbol = symbol_for(prism, &id)?;
    focused_block_for_symbol(prism, &symbol, CONTEXT_BLOCK_OPTIONS)
}

fn focused_block_worker_count(symbol_count: usize) -> usize {
    if symbol_count < 2 {
        return 1;
    }
    thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(symbol_count)
}

pub(crate) fn compact_owner_candidate_excerpts(
    prism: &Prism,
    candidates: &mut [OwnerCandidateView],
) -> Result<()> {
    for candidate in candidates {
        compact_symbol_excerpt(prism, &mut candidate.symbol)?;
    }
    Ok(())
}

pub(crate) fn compact_read_context_candidate_excerpts(
    prism: &Prism,
    context: &mut ReadContextView,
) -> Result<()> {
    compact_owner_candidate_excerpts(prism, &mut context.suggested_reads)?;
    compact_owner_candidate_excerpts(prism, &mut context.tests)?;
    Ok(())
}

pub(crate) fn compact_edit_context_candidate_excerpts(
    prism: &Prism,
    context: &mut EditContextView,
) -> Result<()> {
    compact_owner_candidate_excerpts(prism, &mut context.suggested_reads)?;
    compact_owner_candidate_excerpts(prism, &mut context.write_paths)?;
    compact_owner_candidate_excerpts(prism, &mut context.tests)?;
    Ok(())
}

pub(crate) fn compact_validation_context_candidate_excerpts(
    prism: &Prism,
    context: &mut ValidationContextView,
) -> Result<()> {
    compact_owner_candidate_excerpts(prism, &mut context.tests)?;
    Ok(())
}

pub(crate) fn compact_recent_change_context_candidate_excerpts(
    _prism: &Prism,
    _context: &mut RecentChangeContextView,
) -> Result<()> {
    Ok(())
}

pub(crate) fn compact_discovery_bundle_candidate_excerpts(
    prism: &Prism,
    bundle: &mut DiscoveryBundleView,
) -> Result<()> {
    compact_owner_candidate_excerpts(prism, &mut bundle.suggested_reads)?;
    compact_read_context_candidate_excerpts(prism, &mut bundle.read_context)?;
    compact_edit_context_candidate_excerpts(prism, &mut bundle.edit_context)?;
    compact_validation_context_candidate_excerpts(prism, &mut bundle.validation_context)?;
    compact_recent_change_context_candidate_excerpts(prism, &mut bundle.recent_change_context)?;
    Ok(())
}

fn source_location_view(location: prism_query::SourceLocation) -> SourceLocationView {
    SourceLocationView {
        start_line: location.start_line,
        start_column: location.start_column,
        end_line: location.end_line,
        end_column: location.end_column,
    }
}

fn compact_symbol_excerpt(prism: &Prism, symbol: &mut SymbolView) -> Result<()> {
    let id = NodeId::new(
        symbol.id.crate_name.clone(),
        symbol.id.path.clone(),
        symbol.kind,
    );
    symbol.source_excerpt = symbol_for(prism, &id)?
        .excerpt(CANDIDATE_EXCERPT_OPTIONS)
        .map(source_excerpt_view);
    Ok(())
}

fn source_excerpt_view(excerpt: prism_query::SourceExcerpt) -> SourceExcerptView {
    SourceExcerptView {
        text: excerpt.text,
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        truncated: excerpt.truncated,
    }
}

fn source_slice_view(slice: prism_query::EditSlice) -> SourceSliceView {
    SourceSliceView {
        text: slice.text,
        start_line: slice.start_line,
        end_line: slice.end_line,
        focus: source_location_view(slice.focus),
        relative_focus: source_location_view(slice.relative_focus),
        truncated: slice.truncated,
    }
}

pub(crate) fn relations_view(
    prism: &Prism,
    session: &SessionState,
    id: &NodeId,
) -> Result<RelationsView> {
    let relations = symbol_for(prism, id)?.relations();
    Ok(RelationsView {
        contains: symbol_views_for_ids(
            prism,
            prism
                .graph()
                .edges_from(id, Some(EdgeKind::Contains))
                .into_iter()
                .map(|edge| edge.target.clone())
                .collect(),
        )?,
        callers: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.incoming_calls,
                session
                    .inferred_edges
                    .edges_to(id, Some(EdgeKind::Calls))
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
        )?,
        callees: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_calls,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Calls))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        references: symbol_views_for_ids(
            prism,
            merge_node_ids(
                prism
                    .graph()
                    .edges_from(id, Some(EdgeKind::References))
                    .into_iter()
                    .map(|edge| edge.target.clone())
                    .collect(),
                prism
                    .graph()
                    .edges_to(id, Some(EdgeKind::References))
                    .into_iter()
                    .map(|edge| edge.source.clone()),
            ),
        )?,
        imports: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_imports,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Imports))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        implements: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_implements,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Implements))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        specifies: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_specifies,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Specifies))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        specified_by: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.incoming_specifies,
                session
                    .inferred_edges
                    .edges_to(id, Some(EdgeKind::Specifies))
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
        )?,
        validates: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_validates,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Validates))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        validated_by: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.incoming_validates,
                session
                    .inferred_edges
                    .edges_to(id, Some(EdgeKind::Validates))
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
        )?,
        related: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_related,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::RelatedTo))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        related_by: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.incoming_related,
                session
                    .inferred_edges
                    .edges_to(id, Some(EdgeKind::RelatedTo))
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
        )?,
    })
}

pub(crate) fn blast_radius_view(
    prism: &Prism,
    session: &SessionState,
    id: &NodeId,
) -> ChangeImpactView {
    let mut impact = prism.blast_radius(id);
    for record in session.inferred_edges.edges_from(id, None) {
        impact.direct_nodes.push(record.edge.target);
    }
    for record in session.inferred_edges.edges_to(id, None) {
        impact.direct_nodes.push(record.edge.source);
    }
    impact.direct_nodes = merge_node_ids(impact.direct_nodes, std::iter::empty());
    let promoted_summaries = promoted_summary_texts(session, prism, &[AnchorRef::Node(id.clone())]);
    let mut view = change_impact_view(impact);
    view.promoted_summaries = promoted_summaries;
    view
}

pub(crate) fn validation_recipe_view_with(
    prism: &Prism,
    session: &SessionState,
    id: &NodeId,
) -> ValidationRecipeView {
    let mut recipe = prism.validation_recipe(id);
    merge_promoted_checks(
        &mut recipe.scored_checks,
        promoted_validation_checks(session, prism, &[AnchorRef::Node(id.clone())]),
    );
    recipe.checks = recipe
        .scored_checks
        .iter()
        .map(|check| check.label.clone())
        .collect::<Vec<_>>();
    recipe.checks.sort();
    recipe.checks.dedup();
    recipe.related_nodes = merge_node_ids(
        recipe.related_nodes,
        session
            .inferred_edges
            .edges_from(id, None)
            .into_iter()
            .map(|record| record.edge.target)
            .chain(
                session
                    .inferred_edges
                    .edges_to(id, None)
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
    );
    validation_recipe_view(recipe)
}
