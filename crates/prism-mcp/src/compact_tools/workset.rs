use prism_ir::{AnchorRef, NodeId};
use prism_js::AgentSuggestedActionView;

use super::concept::compact_concept_workset_result;
use super::suggested_actions::{
    dedupe_suggested_actions, suggested_expand_action, suggested_open_action,
};
use super::text_fragments::{
    compact_text_fragment_likely_tests, compact_text_fragment_supporting_reads, read_text_fragment,
};
use super::*;
use crate::compact_followups::workspace_scoped_path;
use crate::{
    concept_resolution_is_ambiguous, resolve_concepts_for_session, weak_concept_match_reason,
};

impl QueryHost {
    pub(crate) fn compact_workset(
        &self,
        session: Arc<SessionState>,
        args: PrismWorksetArgs,
    ) -> Result<AgentWorksetResultView> {
        let query_text = if let Some(handle) = args.handle.as_ref() {
            format!("prism_workset({handle})")
        } else if let Some(query) = args.query.as_ref() {
            format!("prism_workset({query})")
        } else {
            "prism_workset".to_string()
        };
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_workset",
            query_text,
            move |host, query_run| {
                let prism = host.current_prism();
                if let Some(handle) = args
                    .handle
                    .as_deref()
                    .filter(|handle| handle.starts_with("concept://"))
                {
                    let result =
                        compact_concept_workset_result(session.as_ref(), prism.as_ref(), handle)?;
                    return Ok((result, Vec::new()));
                }
                if let Some(query) = args.query.as_deref() {
                    if let Some(handle) = resolve_workset_query_concept_handle(
                        prism.as_ref(),
                        session.as_ref(),
                        query,
                    ) {
                        let mut result = compact_concept_workset_result(
                            session.as_ref(),
                            prism.as_ref(),
                            &handle,
                        )?;
                        result.remapped = true;
                        return Ok((result, Vec::new()));
                    }
                }
                let (target, remapped) = resolve_or_select_workset_target(
                    host,
                    Arc::clone(&session),
                    prism.as_ref(),
                    &args,
                    query_run,
                )?;
                let target_view = compact_target_from_session_target(session.as_ref(), &target);
                let workset =
                    workset_context_for_target(host, session.as_ref(), prism.as_ref(), &target)?;
                Ok((
                    budgeted_workset_result(
                        &target,
                        target_view,
                        workset.supporting_reads,
                        workset.likely_tests,
                        workset.why,
                        remapped,
                    )?,
                    Vec::new(),
                ))
            },
        )
    }
}

pub(super) fn budgeted_workset_result(
    target: &SessionHandleTarget,
    primary: AgentTargetHandleView,
    supporting_reads: Vec<AgentTargetHandleView>,
    likely_tests: Vec<AgentTargetHandleView>,
    why: String,
    remapped: bool,
) -> Result<AgentWorksetResultView> {
    let why = compact_workset_guidance(&why, &primary, &supporting_reads, &likely_tests);
    let followup_handle =
        result_primary_followup_handle(&primary, &supporting_reads, &likely_tests);
    let has_supporting_followup = !followup_handle
        .as_deref()
        .is_some_and(|handle| handle == primary.handle.as_str());
    let has_likely_tests = !likely_tests.is_empty();
    let suggested_actions = compact_workset_suggested_actions(
        target,
        &primary.handle,
        &followup_handle,
        likely_tests.first().map(|target| target.handle.as_str()),
    );
    budgeted_workset_result_with_followups(
        primary,
        supporting_reads,
        likely_tests,
        why,
        remapped,
        Some(compact_workset_next_action(
            target,
            has_supporting_followup,
            has_likely_tests,
        )),
        suggested_actions,
    )
}

pub(super) fn budgeted_workset_result_with_followups(
    primary: AgentTargetHandleView,
    supporting_reads: Vec<AgentTargetHandleView>,
    likely_tests: Vec<AgentTargetHandleView>,
    why: String,
    remapped: bool,
    next_action: Option<String>,
    suggested_actions: Vec<AgentSuggestedActionView>,
) -> Result<AgentWorksetResultView> {
    let mut result = AgentWorksetResultView {
        primary,
        supporting_reads,
        likely_tests,
        why: clamp_string(&why, WORKSET_WHY_MAX_CHARS),
        truncated: false,
        remapped,
        next_action,
        suggested_actions,
    };
    let mut trimmed = false;

    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES {
        trimmed |= strip_file_paths(&mut result.likely_tests);
    }
    while workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES && !result.likely_tests.is_empty() {
        result.likely_tests.pop();
        trimmed = true;
    }
    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES {
        let preserves_gap_summary = result.why.contains("Gap summary:");
        if !preserves_gap_summary {
            let tightened = clamp_string(&result.why, WORKSET_WHY_TIGHT_MAX_CHARS);
            if tightened != result.why {
                result.why = tightened;
                trimmed = true;
            }
        }
    }
    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES {
        let preserves_gap_summary = result.why.contains("Gap summary:");
        if !preserves_gap_summary {
            let tightened = clamp_string(&result.why, WORKSET_WHY_ULTRA_TIGHT_MAX_CHARS);
            if tightened != result.why {
                result.why = tightened;
                trimmed = true;
            }
        }
    }
    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES {
        trimmed |= strip_file_paths(&mut result.supporting_reads);
    }
    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES && result.primary.file_path.is_some() {
        result.primary.file_path = None;
        trimmed = true;
    }
    while workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES && result.supporting_reads.len() > 1
    {
        result.supporting_reads.pop();
        trimmed = true;
    }
    if workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES && result.next_action.is_some() {
        result.next_action = None;
        trimmed = true;
    }
    while workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES
        && result.suggested_actions.len() > 1
    {
        result.suggested_actions.pop();
        trimmed = true;
    }
    while workset_json_bytes(&result)? > WORKSET_MAX_JSON_BYTES
        && !result.supporting_reads.is_empty()
    {
        result.supporting_reads.pop();
        trimmed = true;
    }

    if trimmed {
        result.truncated = true;
    }
    Ok(result)
}

fn compact_workset_suggested_actions(
    target: &SessionHandleTarget,
    current_handle: &str,
    followup_handle: &Option<String>,
    likely_test_handle: Option<&str>,
) -> Vec<AgentSuggestedActionView> {
    let mut actions = Vec::new();
    actions.push(suggested_open_action(
        current_handle.to_string(),
        compact_workset_primary_open_mode(target),
    ));
    if let Some(handle) = followup_handle
        .as_deref()
        .filter(|handle| *handle != current_handle)
    {
        actions.push(suggested_open_action(
            handle.to_string(),
            AgentOpenMode::Focus,
        ));
    }
    if let Some(handle) = likely_test_handle.filter(|handle| *handle != current_handle) {
        actions.push(suggested_open_action(
            handle.to_string(),
            AgentOpenMode::Focus,
        ));
    }
    if is_structured_config_target(target.kind) {
        actions.push(suggested_expand_action(
            current_handle,
            AgentExpandKind::Validation,
        ));
    } else if is_text_fragment_target(target) {
        actions.push(suggested_expand_action(
            current_handle,
            AgentExpandKind::Neighbors,
        ));
    } else if is_spec_like_kind(target.kind)
        || target.file_path.as_deref().is_some_and(is_docs_path)
    {
        actions.push(suggested_expand_action(
            current_handle,
            AgentExpandKind::Drift,
        ));
    } else {
        actions.push(suggested_expand_action(
            current_handle,
            AgentExpandKind::Validation,
        ));
    }
    dedupe_suggested_actions(actions)
}

fn compact_workset_primary_open_mode(target: &SessionHandleTarget) -> AgentOpenMode {
    if is_spec_like_kind(target.kind) || target.file_path.as_deref().is_some_and(is_docs_path) {
        AgentOpenMode::Focus
    } else {
        AgentOpenMode::Edit
    }
}

fn compact_workset_guidance(
    why: &str,
    primary: &AgentTargetHandleView,
    supporting_reads: &[AgentTargetHandleView],
    likely_tests: &[AgentTargetHandleView],
) -> String {
    let preserve_leading_context = why.contains("Gap summary:")
        || why.contains("gap summary")
        || why.contains("Exact text hit");
    let mut parts = Vec::new();
    if preserve_leading_context {
        parts.push(why.to_string());
    }
    if let Some(target) = supporting_reads.first() {
        parts.push(format!(
            "Start with `{}`.",
            compact_workset_target_label(target)
        ));
    } else {
        parts.push(format!(
            "Start with `{}`.",
            compact_workset_target_label(primary)
        ));
    }
    if let Some(target) = likely_tests.first() {
        parts.push(format!(
            "Likely test: `{}`.",
            compact_workset_target_label(target)
        ));
    }
    if !preserve_leading_context {
        parts.push(why.to_string());
    }
    parts.join(" ")
}

fn compact_workset_target_label(target: &AgentTargetHandleView) -> &str {
    if !target.name.is_empty() && target.name != target.path {
        target.name.as_str()
    } else {
        target.path.as_str()
    }
}

fn compact_workset_next_action(
    target: &SessionHandleTarget,
    has_supporting_followup: bool,
    has_likely_tests: bool,
) -> String {
    if is_text_fragment_target(target) {
        if has_supporting_followup {
            "Use prism_open on the first supporting slice; if you need more room, reopen it in edit mode, or prism_expand `neighbors`.".to_string()
        } else if has_likely_tests {
            "Use prism_open on the first likely test, or prism_expand `neighbors`.".to_string()
        } else {
            "Use prism_open in edit mode on the primary slice, or prism_expand `neighbors`."
                .to_string()
        }
    } else if is_structured_config_target(target.kind) {
        if has_supporting_followup {
            "Use prism_open on the first same-file key, or prism_expand `validation`.".to_string()
        } else {
            "Use prism_open in edit mode on the primary target, or prism_expand `validation`."
                .to_string()
        }
    } else if is_spec_like_kind(target.kind)
        || target.file_path.as_deref().is_some_and(is_docs_path)
    {
        if has_supporting_followup {
            "Use prism_open on the first owner, or prism_expand `drift`.".to_string()
        } else {
            "Use prism_open on the primary target, or prism_expand `drift`.".to_string()
        }
    } else if has_supporting_followup && has_likely_tests {
        "Use prism_open on the first supporting read, then inspect the first likely test, or prism_expand `validation`.".to_string()
    } else if has_supporting_followup {
        "Use prism_open on the first supporting read, or prism_open in edit mode on the primary target.".to_string()
    } else if has_likely_tests {
        "Use prism_open on the first likely test, or prism_open in edit mode on the primary target."
            .to_string()
    } else {
        "Use prism_open in edit mode on the primary target, or prism_expand `validation`."
            .to_string()
    }
}

fn result_primary_followup_handle(
    primary: &AgentTargetHandleView,
    supporting_reads: &[AgentTargetHandleView],
    likely_tests: &[AgentTargetHandleView],
) -> Option<String> {
    supporting_reads
        .first()
        .or_else(|| likely_tests.first())
        .map(|handle| handle.handle.clone())
        .or_else(|| Some(primary.handle.clone()))
}

pub(super) fn compact_string_list(items: &[String], limit: usize, max_chars: usize) -> Vec<String> {
    let mut compact = Vec::<String>::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let item = clamp_string(item, max_chars);
        if compact.iter().any(|existing| existing == &item) {
            continue;
        }
        compact.push(item);
        if compact.len() >= limit {
            break;
        }
    }
    compact
}

fn workset_context_for_target(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<WorksetContext> {
    let mut context = if is_text_fragment_target(target) {
        compact_text_fragment_workset_context(host, session, target)?
    } else if is_spec_like_kind(target.kind)
        || target.file_path.as_deref().is_some_and(is_docs_path)
    {
        compact_spec_workset_context(host, session, prism, target)?
    } else if is_structured_config_target(target.kind) {
        let supporting_reads =
            structured_symbol_followups(host, session, prism, target, WORKSET_SUPPORTING_LIMIT)?;
        if !supporting_reads.is_empty() {
            WorksetContext {
                supporting_reads,
                likely_tests: Vec::new(),
                why: format!(
                    "{} Same-file structured follow-ups prioritized for config maintenance.",
                    workset_why(target)
                ),
            }
        } else {
            let supporting_reads = edit_ready_symbol_followups(
                host,
                session,
                prism,
                target,
                WORKSET_SUPPORTING_LIMIT,
            )?;
            WorksetContext {
                supporting_reads,
                likely_tests: owner_views_for_target(
                    prism,
                    target_symbol_id(target)?,
                    Some("test"),
                    WORKSET_TEST_LIMIT,
                )?
                .into_iter()
                .take(WORKSET_TEST_LIMIT)
                .map(|candidate| {
                    compact_target_view(
                        session,
                        &candidate.symbol,
                        target.query.as_deref(),
                        Some(candidate.why),
                    )
                })
                .collect(),
                why: format!(
                    "{} Same-file and graph-adjacent follow-ups are prioritized before broader read owners.",
                    workset_why(target)
                ),
            }
        }
    } else {
        let supporting_reads =
            edit_ready_symbol_followups(host, session, prism, target, WORKSET_SUPPORTING_LIMIT)?;
        WorksetContext {
            supporting_reads,
            likely_tests: owner_views_for_target(
                prism,
                target_symbol_id(target)?,
                Some("test"),
                WORKSET_TEST_LIMIT,
            )?
            .into_iter()
            .take(WORKSET_TEST_LIMIT)
            .map(|candidate| {
                compact_target_view(
                    session,
                    &candidate.symbol,
                    target.query.as_deref(),
                    Some(candidate.why),
                )
            })
            .collect(),
            why: format!(
                "{} Same-file and graph-adjacent follow-ups are prioritized before broader read owners.",
                workset_why(target)
            ),
        }
    };
    augment_contract_workset_context(session, prism, target, &mut context)?;
    Ok(context)
}

fn augment_contract_workset_context(
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
    context: &mut WorksetContext,
) -> Result<()> {
    let Ok(id) = target_symbol_id(target) else {
        return Ok(());
    };

    let mut contract_supporting = Vec::<AgentTargetHandleView>::new();
    let mut contract_tests = Vec::<AgentTargetHandleView>::new();
    let mut hint_lines = Vec::<String>::new();

    for packet in prism.contracts_for_target(id) {
        let subject_match = prism.contract_subject_matches_target(id, &packet);
        let consumer_match = prism.contract_consumer_matches_target(id, &packet);
        let mut anchored_validation_count = 0usize;
        let validation_labels = packet
            .validations
            .iter()
            .map(|validation| validation.id.clone())
            .collect::<Vec<_>>();

        if subject_match {
            for consumer in &packet.consumers {
                for node in prism.contract_target_nodes(consumer, WORKSET_SUPPORTING_LIMIT) {
                    if node == *id {
                        continue;
                    }
                    if let Some(view) = contract_target_handle_view(
                        session,
                        prism,
                        &node,
                        target.query.as_deref(),
                        format!("Known contract consumer recorded on `{}`.", packet.handle),
                    )? {
                        contract_supporting.push(view);
                    }
                }
            }
        }
        if consumer_match {
            for node in prism.contract_target_nodes(&packet.subject, WORKSET_SUPPORTING_LIMIT) {
                if node == *id {
                    continue;
                }
                if let Some(view) = contract_target_handle_view(
                    session,
                    prism,
                    &node,
                    target.query.as_deref(),
                    format!("Provider-side subject recorded on `{}`.", packet.handle),
                )? {
                    contract_supporting.push(view);
                }
            }
        }

        for validation in &packet.validations {
            for node in contract_validation_anchor_nodes(
                prism,
                validation.anchors.as_ref(),
                WORKSET_TEST_LIMIT,
            ) {
                if node == *id {
                    continue;
                }
                if let Some(view) = contract_target_handle_view(
                    session,
                    prism,
                    &node,
                    target.query.as_deref(),
                    format!("Validation anchor for contract `{}`.", packet.handle),
                )? {
                    anchored_validation_count += 1;
                    contract_tests.push(view);
                }
            }
        }

        if let Some(hint) = contract_workset_hint(
            &packet.handle,
            subject_match,
            consumer_match,
            anchored_validation_count,
            &validation_labels,
        ) {
            hint_lines.push(hint);
        }
    }

    if !contract_supporting.is_empty() {
        context.supporting_reads = merge_prioritized_targets(
            contract_supporting,
            std::mem::take(&mut context.supporting_reads),
            WORKSET_SUPPORTING_LIMIT,
        );
    }
    if !contract_tests.is_empty() {
        context.likely_tests = merge_prioritized_targets(
            contract_tests,
            std::mem::take(&mut context.likely_tests),
            WORKSET_TEST_LIMIT,
        );
    }
    if !hint_lines.is_empty() {
        context.why = clamp_string(
            &format!(
                "{} {}",
                context.why,
                compact_string_list(&hint_lines, 2, 72).join(" ")
            ),
            WORKSET_WHY_MAX_CHARS,
        );
    }
    Ok(())
}

fn contract_target_handle_view(
    session: &SessionState,
    prism: &Prism,
    node: &NodeId,
    query: Option<&str>,
    why: String,
) -> Result<Option<AgentTargetHandleView>> {
    let Ok(symbol) = symbol_for(prism, node) else {
        return Ok(None);
    };
    let symbol = symbol_view(prism, &symbol)?;
    Ok(Some(compact_target_view(
        session,
        &symbol,
        query,
        Some(why),
    )))
}

fn contract_validation_anchor_nodes(
    prism: &Prism,
    anchors: &[AnchorRef],
    limit: usize,
) -> Vec<NodeId> {
    let mut nodes = Vec::<NodeId>::new();
    for anchor in prism.anchors_for(anchors) {
        match anchor {
            AnchorRef::Node(node) => nodes.push(node),
            AnchorRef::Lineage(lineage) => nodes.extend(prism.current_nodes_for_lineage(&lineage)),
            AnchorRef::File { .. } | AnchorRef::Kind { .. } => {}
        }
    }
    nodes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.crate_name.cmp(&right.crate_name))
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });
    nodes.dedup();
    nodes.truncate(limit);
    nodes
}

fn merge_prioritized_targets(
    prioritized: Vec<AgentTargetHandleView>,
    existing: Vec<AgentTargetHandleView>,
    limit: usize,
) -> Vec<AgentTargetHandleView> {
    let mut merged = Vec::<AgentTargetHandleView>::new();
    let mut seen = HashSet::<String>::new();

    for target in prioritized.into_iter().chain(existing.into_iter()) {
        if !seen.insert(target.handle.clone()) {
            continue;
        }
        merged.push(target);
        if merged.len() >= limit {
            break;
        }
    }

    merged
}

fn contract_workset_hint(
    handle: &str,
    subject_match: bool,
    consumer_match: bool,
    anchored_validation_count: usize,
    validation_labels: &[String],
) -> Option<String> {
    let mut parts = Vec::<String>::new();
    if subject_match {
        parts.push(format!("`{handle}` adds known consumers."));
    }
    if consumer_match {
        parts.push(format!("`{handle}` links back to its provider."));
    }
    if anchored_validation_count > 0 {
        parts.push(format!(
            "`{handle}` anchors {anchored_validation_count} validation target(s)."
        ));
    } else if let Some(validation) = validation_labels.first() {
        parts.push(format!("`{handle}` expects `{validation}`."));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

pub(super) fn is_structured_config_target(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::JsonKey | NodeKind::TomlKey | NodeKind::YamlKey
    )
}

pub(super) fn structured_symbol_followups(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    if !is_structured_config_target(target.kind) {
        return Ok(Vec::new());
    }
    if is_text_fragment_target(target) {
        return compact_text_fragment_supporting_reads(host, session, target, limit);
    }
    let symbol_id = target_symbol_id(target)?;
    let symbol = symbol_for(prism, symbol_id)?;
    let current = symbol_view(prism, &symbol)?;
    let workspace_root = host.workspace_root();
    let current_file_path = current.file_path.as_deref().or(target.file_path.as_deref());
    let parent_id = structured_parent_symbol_id(symbol_id);
    let current_path = symbol_id.path.as_str();
    let mut followups = Vec::<AgentTargetHandleView>::new();
    let mut seen = HashSet::<String>::new();

    if let Some(parent_id) = parent_id.as_ref() {
        if let Ok(parent_symbol) = symbol_for(prism, parent_id) {
            let parent_view = symbol_view(prism, &parent_symbol)?;
            push_structured_followup(
                &mut followups,
                &mut seen,
                session,
                target,
                current_file_path,
                workspace_root,
                &parent_view,
                "Parent structured key in the same file.".to_string(),
                limit,
            );
        }
    }

    let scoped_file_path =
        current_file_path.map(|path| workspace_scoped_path(workspace_root, path));
    let mut queries = Vec::<String>::new();
    if let Some(parent_id) = parent_id.as_ref() {
        if let Some(parent_name) = parent_id.path.rsplit("::").next() {
            queries.push(parent_name.to_string());
        }
    }
    queries.push(current.name.clone());
    if let Some(query) = target.query.as_deref() {
        queries.push(query.to_string());
    }
    queries.sort();
    queries.dedup();
    for query in queries {
        for candidate in prism.search(
            &query,
            limit.saturating_mul(8).max(8),
            Some(target.kind),
            scoped_file_path.as_deref(),
        ) {
            let view = symbol_view(prism, &candidate)?;
            let Some(relationship) = structured_family_relationship(
                view.id.path.as_str(),
                current_path,
                parent_id.as_ref().map(|id| id.path.as_str()),
            ) else {
                continue;
            };
            let why = match relationship {
                StructuredRelationship::Sibling => {
                    "Sibling structured key in the same file.".to_string()
                }
                StructuredRelationship::Child => {
                    "Nested structured key in the same file.".to_string()
                }
            };
            push_structured_followup(
                &mut followups,
                &mut seen,
                session,
                target,
                current_file_path,
                workspace_root,
                &view,
                why,
                limit,
            );
            if followups.len() >= limit {
                return Ok(followups);
            }
        }
    }

    Ok(followups)
}

pub(super) fn edit_ready_symbol_followups(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    let symbol_id = target_symbol_id(target)?;
    let symbol = symbol_for(prism, symbol_id)?;
    let current = symbol_view(prism, &symbol)?;
    let relations = symbol.relations();
    let workspace_root = host.workspace_root();
    let current_file_path = current.file_path.as_deref().or(target.file_path.as_deref());
    let mut same_file_followups = Vec::<AgentTargetHandleView>::new();
    let mut cross_file_followups = Vec::<AgentTargetHandleView>::new();
    let mut seen = HashSet::<String>::from([target.id.path.to_string()]);

    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        relations.incoming_calls.into_iter().chain(
            session
                .inferred_edges
                .edges_to(symbol_id, Some(EdgeKind::Calls))
                .into_iter()
                .map(|record| record.edge.source),
        ),
        "Direct caller of this symbol.",
    )?;
    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        relations.outgoing_calls.into_iter().chain(
            session
                .inferred_edges
                .edges_from(symbol_id, Some(EdgeKind::Calls))
                .into_iter()
                .map(|record| record.edge.target),
        ),
        "Direct callee from this symbol.",
    )?;
    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        prism
            .graph()
            .edges_from(symbol_id, Some(EdgeKind::References))
            .into_iter()
            .map(|edge| edge.target.clone())
            .chain(
                prism
                    .graph()
                    .edges_to(symbol_id, Some(EdgeKind::References))
                    .into_iter()
                    .map(|edge| edge.source.clone()),
            )
            .chain(
                session
                    .inferred_edges
                    .edges_from(symbol_id, Some(EdgeKind::References))
                    .into_iter()
                    .map(|record| record.edge.target),
            )
            .chain(
                session
                    .inferred_edges
                    .edges_to(symbol_id, Some(EdgeKind::References))
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
        "Direct reference neighbor for this symbol.",
    )?;
    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        relations.outgoing_imports.into_iter().chain(
            session
                .inferred_edges
                .edges_from(symbol_id, Some(EdgeKind::Imports))
                .into_iter()
                .map(|record| record.edge.target),
        ),
        "Imported dependency used by this symbol.",
    )?;
    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        relations.outgoing_implements.into_iter().chain(
            session
                .inferred_edges
                .edges_from(symbol_id, Some(EdgeKind::Implements))
                .into_iter()
                .map(|record| record.edge.target),
        ),
        "Implementation surface linked to this symbol.",
    )?;
    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        relations
            .outgoing_specifies
            .into_iter()
            .chain(
                session
                    .inferred_edges
                    .edges_from(symbol_id, Some(EdgeKind::Specifies))
                    .into_iter()
                    .map(|record| record.edge.target),
            )
            .chain(
                relations.incoming_specifies.into_iter().chain(
                    session
                        .inferred_edges
                        .edges_to(symbol_id, Some(EdgeKind::Specifies))
                        .into_iter()
                        .map(|record| record.edge.source),
                ),
            ),
        "Specification-linked symbol.",
    )?;
    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        relations
            .outgoing_validates
            .into_iter()
            .chain(
                session
                    .inferred_edges
                    .edges_from(symbol_id, Some(EdgeKind::Validates))
                    .into_iter()
                    .map(|record| record.edge.target),
            )
            .chain(
                relations.incoming_validates.into_iter().chain(
                    session
                        .inferred_edges
                        .edges_to(symbol_id, Some(EdgeKind::Validates))
                        .into_iter()
                        .map(|record| record.edge.source),
                ),
            ),
        "Validation-linked symbol.",
    )?;
    push_edit_followups_for_ids(
        &mut same_file_followups,
        &mut cross_file_followups,
        &mut seen,
        session,
        target,
        current_file_path,
        workspace_root,
        prism,
        relations
            .outgoing_related
            .into_iter()
            .chain(
                session
                    .inferred_edges
                    .edges_from(symbol_id, Some(EdgeKind::RelatedTo))
                    .into_iter()
                    .map(|record| record.edge.target),
            )
            .chain(
                relations.incoming_related.into_iter().chain(
                    session
                        .inferred_edges
                        .edges_to(symbol_id, Some(EdgeKind::RelatedTo))
                        .into_iter()
                        .map(|record| record.edge.source),
                ),
            ),
        "Directly related symbol.",
    )?;

    for candidate in next_reads(prism, symbol_id, limit.saturating_mul(4).max(limit + 2))? {
        push_edit_followup(
            &mut same_file_followups,
            &mut cross_file_followups,
            &mut seen,
            session,
            target,
            current_file_path,
            workspace_root,
            &candidate.symbol,
            candidate.why,
        );
    }

    Ok(same_file_followups
        .into_iter()
        .chain(cross_file_followups)
        .take(limit)
        .collect())
}

fn structured_parent_symbol_id(id: &NodeId) -> Option<NodeId> {
    let (_, after_document) = id.path.split_once("::document::")?;
    let (_, structured) = after_document.split_once("::")?;
    structured.contains("::").then(|| {
        let (parent_path, _) = id.path.rsplit_once("::").expect("parent path");
        NodeId::new(id.crate_name.clone(), parent_path.to_string(), id.kind)
    })
}

#[derive(Debug, Clone, Copy)]
enum StructuredRelationship {
    Sibling,
    Child,
}

fn structured_family_relationship(
    candidate_path: &str,
    current_path: &str,
    parent_path: Option<&str>,
) -> Option<StructuredRelationship> {
    if candidate_path == current_path {
        return None;
    }
    if let Some(parent_path) = parent_path {
        if structured_direct_child_of(candidate_path, parent_path) {
            return Some(StructuredRelationship::Sibling);
        }
    }
    structured_direct_child_of(candidate_path, current_path)
        .then_some(StructuredRelationship::Child)
}

fn structured_direct_child_of(candidate_path: &str, parent_path: &str) -> bool {
    let Some(tail) = candidate_path.strip_prefix(&format!("{parent_path}::")) else {
        return false;
    };
    !tail.is_empty() && !tail.contains("::")
}

fn push_structured_followup(
    followups: &mut Vec<AgentTargetHandleView>,
    seen: &mut HashSet<String>,
    session: &SessionState,
    target: &SessionHandleTarget,
    current_file_path: Option<&str>,
    workspace_root: Option<&Path>,
    candidate: &SymbolView,
    why: String,
    limit: usize,
) {
    let Some(candidate_file_path) = candidate.file_path.as_deref() else {
        return;
    };
    if current_file_path
        .is_some_and(|expected| !same_workspace_file(workspace_root, expected, candidate_file_path))
    {
        return;
    }
    if !seen.insert(candidate.id.path.clone()) {
        return;
    }
    followups.push(compact_target_view(
        session,
        candidate,
        target.query.as_deref(),
        Some(why),
    ));
    if followups.len() > limit {
        followups.truncate(limit);
    }
}

fn push_edit_followups_for_ids<I>(
    same_file_followups: &mut Vec<AgentTargetHandleView>,
    cross_file_followups: &mut Vec<AgentTargetHandleView>,
    seen: &mut HashSet<String>,
    session: &SessionState,
    target: &SessionHandleTarget,
    current_file_path: Option<&str>,
    workspace_root: Option<&Path>,
    prism: &Prism,
    ids: I,
    why: &str,
) -> Result<()>
where
    I: IntoIterator<Item = NodeId>,
{
    for id in ids {
        let symbol = match symbol_for(prism, &id) {
            Ok(symbol) => symbol,
            Err(_) => continue,
        };
        let candidate = symbol_view(prism, &symbol)?;
        push_edit_followup(
            same_file_followups,
            cross_file_followups,
            seen,
            session,
            target,
            current_file_path,
            workspace_root,
            &candidate,
            why.to_string(),
        );
    }
    Ok(())
}

fn push_edit_followup(
    same_file_followups: &mut Vec<AgentTargetHandleView>,
    cross_file_followups: &mut Vec<AgentTargetHandleView>,
    seen: &mut HashSet<String>,
    session: &SessionState,
    target: &SessionHandleTarget,
    current_file_path: Option<&str>,
    workspace_root: Option<&Path>,
    candidate: &SymbolView,
    why: String,
) {
    if candidate.id.path == target.id.path || is_test_like_symbol(candidate) {
        return;
    }
    if !seen.insert(candidate.id.path.clone()) {
        return;
    }
    let followup = compact_target_view(session, candidate, target.query.as_deref(), Some(why));
    let same_file = current_file_path.is_some_and(|expected| {
        candidate
            .file_path
            .as_deref()
            .is_some_and(|path| same_workspace_file(workspace_root, expected, path))
    });
    if same_file {
        same_file_followups.push(followup);
    } else {
        cross_file_followups.push(followup);
    }
}

fn compact_text_fragment_workset_context(
    host: &QueryHost,
    session: &SessionState,
    target: &SessionHandleTarget,
) -> Result<WorksetContext> {
    let supporting_reads =
        compact_text_fragment_supporting_reads(host, session, target, WORKSET_SUPPORTING_LIMIT)?;
    Ok(WorksetContext {
        supporting_reads,
        likely_tests: compact_text_fragment_likely_tests(
            host,
            session,
            target,
            WORKSET_TEST_LIMIT,
        )?,
        why: format!(
            "{} Nearby semantic handles and likely tests are surfaced first for exact-hit follow-through.",
            workset_why(target)
        ),
    })
}

fn compact_spec_workset_context(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<WorksetContext> {
    let drift = spec_drift_explanation_view(prism, &target.id)?;
    let supporting_reads = prioritized_spec_supporting_reads(
        host,
        session,
        prism,
        target,
        &drift,
        WORKSET_SUPPORTING_LIMIT,
    )?;
    let likely_tests = prioritized_spec_test_reads(session, target, &drift, WORKSET_TEST_LIMIT);
    Ok(WorksetContext {
        supporting_reads,
        likely_tests,
        why: spec_workset_why(target, &drift.gaps, &drift.drift_reasons),
    })
}

#[derive(Debug, Clone)]
struct RankedCompactFollowup {
    symbol: SymbolView,
    why: String,
    score: i32,
    keep_without_overlap: bool,
}

#[derive(Debug, Clone)]
struct SpecIdentifierFollowup {
    symbol: SymbolView,
    term: String,
    exact_match: bool,
}

pub(super) fn prioritized_spec_supporting_reads(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    Ok(ranked_spec_followups(host, prism, target, drift)?
        .into_iter()
        .take(limit)
        .map(|candidate| {
            compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why),
            )
        })
        .collect())
}

fn prioritized_spec_test_reads(
    session: &SessionState,
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
    limit: usize,
) -> Vec<AgentTargetHandleView> {
    drift
        .cluster
        .tests
        .iter()
        .take(limit)
        .map(|candidate| {
            compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why.clone()),
            )
        })
        .collect()
}

fn spec_identifier_followups(
    host: &QueryHost,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Vec<SpecIdentifierFollowup>> {
    let symbol = symbol_for(prism, &target.id)?;
    let mut followups = Vec::<SpecIdentifierFollowup>::new();
    let mut seen = HashSet::<String>::new();
    for term in spec_body_identifier_terms(
        &spec_identifier_source_text(host, target, &symbol.full())?,
        SPEC_BODY_IDENTIFIER_LIMIT,
    ) {
        let normalized_term = normalize_locate_text(&term);
        let mut matched_any = false;
        for view in spec_identifier_symbol_matches(prism, &term, Some("src/"))?
            .into_iter()
            .chain(spec_identifier_symbol_matches(prism, &term, None)?)
        {
            if !seen.insert(view.id.path.clone()) {
                continue;
            }
            matched_any = true;
            let exact_match = spec_identifier_exact_match(&view, &normalized_term);
            followups.push(SpecIdentifierFollowup {
                symbol: view,
                term: term.clone(),
                exact_match,
            });
        }
        if matched_any {
            continue;
        }
        let outcome = search_text(
            host,
            SearchTextArgs {
                query: term.clone(),
                regex: Some(false),
                case_sensitive: Some(false),
                path: Some("src/".to_string()),
                glob: Some("**/*.rs".to_string()),
                limit: Some(SPEC_IDENTIFIER_TEXT_LIMIT),
                context_lines: Some(0),
            },
            SPEC_IDENTIFIER_SEARCH_LIMIT,
        )?;
        for matched in outcome.results {
            for view in spec_identifier_symbol_matches(prism, &term, Some(matched.path.as_str()))? {
                if !seen.insert(view.id.path.clone()) {
                    continue;
                }
                let exact_match = spec_identifier_exact_match(&view, &normalized_term);
                followups.push(SpecIdentifierFollowup {
                    symbol: view,
                    term: term.clone(),
                    exact_match,
                });
            }
        }
    }
    followups.sort_by(|left, right| {
        right
            .exact_match
            .cmp(&left.exact_match)
            .then_with(|| left.symbol.id.path.cmp(&right.symbol.id.path))
    });
    Ok(followups)
}

fn spec_identifier_symbol_matches(
    prism: &Prism,
    term: &str,
    path: Option<&str>,
) -> Result<Vec<SymbolView>> {
    let mut matches = Vec::<SymbolView>::new();
    let mut seen = HashSet::<String>::new();
    for kind in [
        NodeKind::Function,
        NodeKind::Method,
        NodeKind::Struct,
        NodeKind::Enum,
        NodeKind::Trait,
        NodeKind::Field,
        NodeKind::TypeAlias,
    ] {
        for symbol in prism.search(term, SPEC_IDENTIFIER_SEARCH_LIMIT, Some(kind), path) {
            let view = symbol_view(prism, &symbol)?;
            if !is_code_like_kind(view.kind) || is_test_like_symbol(&view) {
                continue;
            }
            if !seen.insert(view.id.path.clone()) {
                continue;
            }
            matches.push(view);
        }
    }
    Ok(matches)
}

fn spec_identifier_source_text(
    host: &QueryHost,
    target: &SessionHandleTarget,
    full: &str,
) -> Result<String> {
    if spec_body_identifier_terms(&full, 1).is_empty() {
        if let Some(file_path) = target.file_path.as_deref() {
            let excerpt = file_read(
                host,
                FileReadArgs {
                    path: file_path.to_string(),
                    start_line: None,
                    end_line: None,
                    max_chars: None,
                },
            )?;
            let start_line = target.start_line.or_else(|| {
                target
                    .query
                    .as_deref()
                    .and_then(|query| excerpt_start_line_for_query(excerpt.text.as_str(), query))
            });
            if let Some(start_line) = start_line {
                let end_line = target.end_line.unwrap_or(start_line).saturating_add(12);
                let excerpt =
                    read_text_fragment(host, target, start_line, end_line, RAW_OPEN_MAX_CHARS)?;
                if !excerpt.text.trim().is_empty() {
                    return Ok(excerpt.text);
                }
            }
            if let Some(query) = target.query.as_deref() {
                if let Some(section) = excerpt_section_for_query(excerpt.text.as_str(), query) {
                    return Ok(section);
                }
            }
            if !excerpt.text.trim().is_empty() {
                return Ok(excerpt.text);
            }
        }
    }
    Ok(full.to_string())
}

fn excerpt_start_line_for_query(text: &str, query: &str) -> Option<usize> {
    let query = normalize_locate_text(query);
    text.lines().enumerate().find_map(|(index, line)| {
        normalize_locate_text(line)
            .contains(query.as_str())
            .then_some(index + 1)
    })
}

fn excerpt_section_for_query(text: &str, query: &str) -> Option<String> {
    let start_line = excerpt_start_line_for_query(text, query)?;
    let section = text
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(16)
        .collect::<Vec<_>>()
        .join("\n");
    (!section.trim().is_empty()).then_some(section)
}

fn ranked_spec_followups(
    host: &QueryHost,
    prism: &Prism,
    target: &SessionHandleTarget,
    drift: &prism_js::SpecDriftExplanationView,
) -> Result<Vec<RankedCompactFollowup>> {
    let query_tokens = target
        .query
        .as_deref()
        .map(normalize_locate_text)
        .map(|query| locate_query_tokens(&query))
        .unwrap_or_default();
    let mut candidates = Vec::<RankedCompactFollowup>::new();
    let mut seen = HashSet::<String>::new();

    push_ranked_spec_identifier_followups(
        &mut candidates,
        &mut seen,
        spec_identifier_followups(host, prism, target)?.iter(),
        132,
        &query_tokens,
    );
    push_ranked_spec_symbols(
        &mut candidates,
        &mut seen,
        drift.cluster.implementations.iter(),
        "Implementation linked from the spec cluster.",
        140,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.write_path.iter(),
        120,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.read_path.iter(),
        110,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.cluster.persistence_path.iter(),
        100,
        &query_tokens,
        false,
    );
    push_ranked_spec_owners(
        &mut candidates,
        &mut seen,
        drift.next_reads.iter(),
        80,
        &query_tokens,
        false,
    );

    let has_token_overlap = candidates
        .iter()
        .any(|candidate| spec_followup_token_overlap(&candidate.symbol, &query_tokens) > 0);
    if has_token_overlap {
        candidates.retain(|candidate| {
            candidate.keep_without_overlap
                || spec_followup_token_overlap(&candidate.symbol, &query_tokens) > 0
        });
    }
    let has_non_test = candidates
        .iter()
        .any(|candidate| !is_test_like_symbol(&candidate.symbol));
    if has_non_test {
        candidates.retain(|candidate| !is_test_like_symbol(&candidate.symbol));
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.symbol.id.path.cmp(&right.symbol.id.path))
    });
    Ok(candidates)
}

fn push_ranked_spec_symbols<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    symbols: impl Iterator<Item = &'a SymbolView>,
    why: &str,
    source_weight: i32,
    query_tokens: &[String],
    keep_without_overlap: bool,
) {
    for symbol in symbols {
        if !seen.insert(symbol.id.path.clone()) {
            continue;
        }
        out.push(RankedCompactFollowup {
            symbol: symbol.clone(),
            why: why.to_string(),
            score: spec_followup_score(symbol, source_weight, query_tokens),
            keep_without_overlap,
        });
    }
}

fn push_ranked_spec_identifier_followups<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    candidates: impl Iterator<Item = &'a SpecIdentifierFollowup>,
    source_weight: i32,
    query_tokens: &[String],
) {
    for candidate in candidates {
        if !seen.insert(candidate.symbol.id.path.clone()) {
            continue;
        }
        let mut score = spec_followup_score(&candidate.symbol, source_weight, query_tokens);
        if candidate.exact_match {
            score += 48;
        }
        if matches!(candidate.symbol.kind, NodeKind::Function | NodeKind::Method) {
            score += 12;
        }
        out.push(RankedCompactFollowup {
            symbol: candidate.symbol.clone(),
            why: format!(
                "Identifier `{}` lifted from the spec body matched this implementation owner.",
                candidate.term
            ),
            score,
            keep_without_overlap: true,
        });
    }
}

fn push_ranked_spec_owners<'a>(
    out: &mut Vec<RankedCompactFollowup>,
    seen: &mut HashSet<String>,
    owners: impl Iterator<Item = &'a prism_js::OwnerCandidateView>,
    source_weight: i32,
    query_tokens: &[String],
    keep_without_overlap: bool,
) {
    for candidate in owners {
        if !seen.insert(candidate.symbol.id.path.clone()) {
            continue;
        }
        out.push(RankedCompactFollowup {
            symbol: candidate.symbol.clone(),
            why: candidate.why.clone(),
            score: spec_followup_score(&candidate.symbol, source_weight, query_tokens),
            keep_without_overlap,
        });
    }
}

fn spec_followup_score(symbol: &SymbolView, source_weight: i32, query_tokens: &[String]) -> i32 {
    let mut score = source_weight;
    let overlap = spec_followup_token_overlap(symbol, query_tokens) as i32;
    score += overlap * 26;
    if is_code_like_kind(symbol.kind) {
        score += 18;
    }
    if is_test_like_symbol(symbol) {
        score -= 80;
    }
    if matches!(symbol.kind, NodeKind::Module | NodeKind::Document) {
        score -= 20;
    }
    if symbol
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("/lib.rs"))
    {
        score -= 16;
    }
    score
}

fn spec_identifier_exact_match(symbol: &SymbolView, normalized_term: &str) -> bool {
    let name = normalize_locate_text(symbol.name.as_str());
    let path = normalize_locate_text(symbol.id.path.as_str());
    name == normalized_term
        || final_segment_normalized(symbol.id.path.as_str()) == normalized_term
        || path.ends_with(normalized_term)
}

fn spec_followup_token_overlap(symbol: &SymbolView, query_tokens: &[String]) -> usize {
    let name = normalize_locate_text(symbol.name.as_str());
    let path = normalize_locate_text(symbol.id.path.as_str());
    query_tokens
        .iter()
        .filter(|token| name.contains(token.as_str()) || path.contains(token.as_str()))
        .count()
}

pub(super) fn resolve_or_select_workset_target(
    host: &QueryHost,
    session: Arc<SessionState>,
    prism: &Prism,
    args: &PrismWorksetArgs,
    query_run: QueryRun,
) -> Result<(SessionHandleTarget, bool)> {
    if let Some(handle) = args.handle.as_deref() {
        return resolve_handle_target(host, session.as_ref(), prism, handle, Some("workset"));
    }
    let query = args
        .query
        .as_deref()
        .ok_or_else(|| anyhow!("prism_workset requires `handle` or `query`"))?;
    let execution = crate::QueryExecution::new(
        host.clone(),
        Arc::clone(&session),
        host.current_prism(),
        query_run,
    );
    let symbol = execution
        .search(SearchArgs {
            query: query.to_string(),
            limit: Some(1),
            kind: None,
            path: None,
            module: None,
            task_id: None,
            path_mode: None,
            strategy: Some("direct".to_string()),
            structured_path: None,
            top_level_only: None,
            prefer_callable_code: Some(true),
            prefer_editable_targets: Some(true),
            prefer_behavioral_owners: Some(true),
            owner_kind: None,
            include_inferred: Some(true),
        })?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no target matched `{query}`; rerun prism_locate first"))?;
    let handle_view = compact_target_view(session.as_ref(), &symbol, Some(query), None);
    resolve_handle_target(
        host,
        session.as_ref(),
        prism,
        &handle_view.handle,
        Some("workset"),
    )
}

fn resolve_workset_query_concept_handle(
    prism: &Prism,
    session: &SessionState,
    query: &str,
) -> Option<String> {
    if !query_prefers_concept_resolution(query) {
        return None;
    }
    let resolutions = resolve_concepts_for_session(prism, session, query, 2);
    let top = resolutions.first()?;
    if concept_resolution_is_ambiguous(&resolutions) {
        return None;
    }
    if weak_concept_match_reason(top.score).is_some() {
        return None;
    }
    Some(top.packet.handle.clone())
}

fn query_prefers_concept_resolution(query: &str) -> bool {
    let mut token_count = 0usize;
    let mut chars = query.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|ch| !ch.is_ascii_alphanumeric()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        token_count += 1;
        while chars.peek().is_some_and(|ch| ch.is_ascii_alphanumeric()) {
            chars.next();
        }
        if token_count >= 2 {
            return true;
        }
    }
    false
}
