use prism_ir::NodeId;
use prism_js::SuggestedQueryView;

use crate::{edit_context_queries, read_context_queries, search_queries, SearchAmbiguityView};

pub(crate) fn symbol_suggested_queries(target: &NodeId) -> Vec<SuggestedQueryView> {
    let mut suggestions = read_context_queries(target);
    suggestions.extend(edit_context_queries(target));
    dedupe_suggested_queries(suggestions)
}

pub(crate) fn search_suggested_queries(
    query: &str,
    top_target: Option<&NodeId>,
    ambiguity: Option<&SearchAmbiguityView>,
) -> Vec<SuggestedQueryView> {
    let mut suggestions = search_queries(query);
    if let Some(target) = top_target {
        suggestions.extend(read_context_queries(target));
        suggestions.extend(edit_context_queries(target));
    }
    if let Some(ambiguity) = ambiguity {
        suggestions.extend(ambiguity.suggested_queries.clone());
    }
    dedupe_suggested_queries(suggestions)
}

pub(crate) fn dedupe_suggested_queries(
    suggestions: Vec<SuggestedQueryView>,
) -> Vec<SuggestedQueryView> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(suggestions.len());
    for suggestion in suggestions {
        if seen.insert(suggestion.query.clone()) {
            deduped.push(suggestion);
        }
    }
    deduped
}
