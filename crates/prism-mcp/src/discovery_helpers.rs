use std::collections::{HashSet, VecDeque};

use anyhow::{anyhow, Result};
use prism_ir::{EdgeKind, NodeId};
use prism_js::{OwnerCandidateView, SymbolView};
use prism_query::Prism;

use crate::{
    merge_node_ids, owner_symbol_views_for_target, owner_views_for_target, symbol_views_for_ids,
    SessionState,
};

pub(crate) fn next_reads(
    prism: &Prism,
    target: &NodeId,
    limit: usize,
) -> Result<Vec<OwnerCandidateView>> {
    let mut reads = owner_views_for_target(prism, target, Some("read"), limit)?;
    if reads.len() > limit {
        reads.truncate(limit);
    }
    Ok(reads)
}

pub(crate) fn where_used(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
    mode: Option<&str>,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    match mode.unwrap_or("direct") {
        "direct" => direct_where_used(prism, session, target, limit),
        "behavioral" => owner_symbol_views_for_target(prism, target, Some("read"), limit),
        other => Err(anyhow!("unknown whereUsed mode `{other}`")),
    }
}

pub(crate) fn entrypoints_for(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    let entrypoint_ids = prism
        .entrypoints()
        .into_iter()
        .map(|symbol| symbol.id().clone())
        .collect::<HashSet<_>>();

    if entrypoint_ids.contains(target) {
        return symbol_views_for_ids(prism, vec![target.clone()]);
    }

    let mut queue = VecDeque::from([target.clone()]);
    let mut seen = HashSet::from([target.clone()]);
    let mut entrypoints = Vec::new();

    while let Some(current) = queue.pop_front() {
        let callers = merge_node_ids(
            prism
                .graph()
                .edges_to(&current, Some(EdgeKind::Calls))
                .into_iter()
                .map(|edge| edge.source.clone())
                .collect(),
            session
                .inferred_edges
                .edges_to(&current, Some(EdgeKind::Calls))
                .into_iter()
                .map(|record| record.edge.source),
        );

        for caller in callers {
            if entrypoint_ids.contains(&caller)
                && !entrypoints
                    .iter()
                    .any(|existing: &NodeId| existing == &caller)
            {
                entrypoints.push(caller.clone());
                if entrypoints.len() >= limit {
                    return symbol_views_for_ids(prism, entrypoints);
                }
            }
            if seen.insert(caller.clone()) {
                queue.push_back(caller);
            }
        }
    }

    symbol_views_for_ids(prism, entrypoints)
}

fn direct_where_used(
    prism: &Prism,
    session: &SessionState,
    target: &NodeId,
    limit: usize,
) -> Result<Vec<SymbolView>> {
    let mut ids = Vec::new();
    for kind in [
        EdgeKind::Calls,
        EdgeKind::References,
        EdgeKind::Imports,
        EdgeKind::Implements,
        EdgeKind::Specifies,
        EdgeKind::Validates,
        EdgeKind::RelatedTo,
    ] {
        ids = merge_node_ids(
            ids,
            prism
                .graph()
                .edges_to(target, Some(kind))
                .into_iter()
                .map(|edge| edge.source.clone()),
        );
        ids = merge_node_ids(
            ids,
            session
                .inferred_edges
                .edges_to(target, Some(kind))
                .into_iter()
                .map(|record| record.edge.source),
        );
    }
    ids.truncate(limit);
    symbol_views_for_ids(prism, ids)
}
