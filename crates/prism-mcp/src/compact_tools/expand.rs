use prism_js::{EvidenceSourceKind, OwnerCandidateView};
use prism_memory::{MemoryModule, OutcomeEvent, OutcomeKind, RecallQuery};

use super::open::{compact_preview_for_structured_target, compact_preview_for_symbol_view};
use prism_js::AgentSuggestedActionView;

use super::suggested_actions::{
    dedupe_suggested_actions, suggested_expand_action, suggested_open_action,
    suggested_workset_action,
};
use super::text_fragments::{
    compact_text_fragment_diagnostics, compact_text_fragment_neighbors,
    compact_text_fragment_validation,
};
use super::workset::{
    compact_string_list, is_structured_config_target, prioritized_spec_supporting_reads,
    structured_symbol_followups,
};
use super::*;

impl QueryHost {
    pub(crate) fn compact_expand(
        &self,
        session: Arc<SessionState>,
        args: PrismExpandArgs,
    ) -> Result<AgentExpandResultView> {
        let kind = agent_expand_kind(&args.kind);
        let query_text = format!("prism_expand({}, {:?})", args.handle, kind);
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_expand",
            query_text,
            move |host, _query_run| {
                let prism = host.current_prism();
                let (target, remapped) =
                    resolve_handle_target(host, session.as_ref(), prism.as_ref(), &args.handle)?;
                let mut top_preview = None;
                let result = match kind {
                    AgentExpandKind::Diagnostics => {
                        if is_text_fragment_target(&target) {
                            compact_text_fragment_diagnostics(&target)
                        } else {
                            let symbol = symbol_for(prism.as_ref(), target_symbol_id(&target)?)?;
                            let symbol = symbol_view(prism.as_ref(), &symbol)?;
                            json!({
                                "query": target.query,
                                "whyShort": target.why_short,
                                "filePath": target.file_path,
                                "ownerHint": symbol.owner_hint,
                            })
                        }
                    }
                    AgentExpandKind::Lineage => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Lineage is only available for semantic symbol handles. Rerun prism_locate on a symbol target if you need lineage.",
                                "filePath": target.file_path,
                            })
                        } else {
                            compact_lineage_expand_result(prism.as_ref(), &target)?
                        }
                    }
                    AgentExpandKind::Neighbors => {
                        if is_text_fragment_target(&target) {
                            let (neighbors, preview) = compact_text_fragment_neighbors(
                                host,
                                session.as_ref(),
                                &target,
                                args.include_top_preview.unwrap_or(false),
                            )?;
                            top_preview = preview;
                            json!({ "neighbors": neighbors })
                        } else if is_structured_config_target(target.kind) {
                            let neighbors = structured_symbol_followups(
                                host,
                                session.as_ref(),
                                prism.as_ref(),
                                &target,
                                EXPAND_NEIGHBOR_LIMIT,
                            )?;
                            if args.include_top_preview.unwrap_or(false) {
                                top_preview = if let Some(candidate) = neighbors.first() {
                                    let (preview_target, _) = resolve_handle_target(
                                        host,
                                        session.as_ref(),
                                        prism.as_ref(),
                                        &candidate.handle,
                                    )?;
                                    compact_preview_for_structured_target(
                                        host,
                                        session.as_ref(),
                                        prism.as_ref(),
                                        &candidate.handle,
                                        &preview_target,
                                    )?
                                } else {
                                    None
                                };
                            }
                            json!({ "neighbors": neighbors })
                        } else {
                            let next_read_candidates =
                                next_reads(prism.as_ref(), target_symbol_id(&target)?, EXPAND_NEIGHBOR_LIMIT)?;
                            if args.include_top_preview.unwrap_or(false) {
                                top_preview = if let Some(candidate) = next_read_candidates.first() {
                                    let preview_handle = compact_target_view(
                                        &session,
                                        &candidate.symbol,
                                        target.query.as_deref(),
                                        Some(candidate.why.clone()),
                                    );
                                    compact_preview_for_symbol_view(
                                        prism.as_ref(),
                                        &preview_handle.handle,
                                        &candidate.symbol,
                                    )?
                                } else {
                                    None
                                };
                            }
                            let neighbors = next_read_candidates
                                .into_iter()
                                .take(EXPAND_NEIGHBOR_LIMIT)
                                .map(|candidate| {
                                    compact_target_view(
                                        &session,
                                        &candidate.symbol,
                                        target.query.as_deref(),
                                        Some(candidate.why),
                                    )
                                })
                                .collect::<Vec<_>>();
                            json!({ "neighbors": neighbors })
                        }
                    }
                    AgentExpandKind::Diff => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Diff expansion is only available for semantic symbol handles. Rerun prism_locate on a symbol target if you need a semantic diff.",
                                "filePath": target.file_path,
                            })
                        } else {
                            compact_diff_expand_result(prism.as_ref(), &target)?
                        }
                    }
                    AgentExpandKind::Validation => {
                        if is_structured_config_target(target.kind) {
                            compact_structured_config_validation_result(
                                host,
                                session.as_ref(),
                                prism.as_ref(),
                                &target,
                            )?
                        } else if is_text_fragment_target(&target) {
                            compact_text_fragment_validation(host, session.as_ref(), &target)?
                        } else {
                            let mut cache = crate::SemanticContextCache::default();
                            let validation = validation_context_view_cached(
                                prism.as_ref(),
                                session.as_ref(),
                                &mut cache,
                                target_symbol_id(&target)?,
                            )?;
                            let likely_tests = validation
                                .tests
                                .into_iter()
                                .take(WORKSET_TEST_LIMIT)
                                .map(|candidate| {
                                    compact_target_view(
                                        &session,
                                        &candidate.symbol,
                                        target.query.as_deref(),
                                        Some(candidate.why),
                                    )
                                })
                                .collect::<Vec<_>>();
                            let checks = compact_validation_checks(
                                &validation.validation_recipe.checks,
                                &validation.validation_recipe.scored_checks,
                                COMPACT_VALIDATION_CHECK_LIMIT,
                                COMPACT_VALIDATION_CHECK_MAX_CHARS,
                            );
                            json!({
                                "checks": checks,
                                "likelyTests": likely_tests,
                                "why": validation.why,
                            })
                        }
                    }
                    AgentExpandKind::Drift => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Drift expansion needs a semantic spec/doc handle. Rerun prism_locate on a heading or symbol target if you need drift details.",
                                "filePath": target.file_path,
                            })
                        } else {
                            compact_drift_expand_result(
                                host,
                                session.as_ref(),
                                prism.as_ref(),
                                &target,
                            )?
                        }
                    }
                    AgentExpandKind::Impact => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Impact expansion is only available for semantic symbol handles. Rerun prism_locate on a symbol target if you need blast-radius detail.",
                                "filePath": target.file_path,
                            })
                        } else {
                            compact_impact_expand_result(
                                session.as_ref(),
                                prism.as_ref(),
                                &target,
                            )?
                        }
                    }
                    AgentExpandKind::Timeline => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Timeline expansion is only available for semantic symbol handles. Rerun prism_locate on a symbol target if you need recent change history.",
                                "filePath": target.file_path,
                            })
                        } else {
                            compact_timeline_expand_result(prism.as_ref(), &target)?
                        }
                    }
                    AgentExpandKind::Memory => {
                        if is_text_fragment_target(&target) {
                            json!({
                                "note": "Memory expansion is only available for semantic symbol handles. Rerun prism_locate on a symbol target if you need anchored memory recall.",
                                "filePath": target.file_path,
                            })
                        } else {
                            compact_memory_expand_result(session.as_ref(), &target)?
                        }
                    }
                };
                let suggested_actions =
                    compact_expand_suggested_actions(kind, &args.handle, &target, &result);

                Ok((
                    AgentExpandResultView {
                        handle: args.handle,
                        kind,
                        result,
                        remapped,
                        top_preview,
                        next_action: Some(compact_expand_next_action(kind, &target)),
                        suggested_actions,
                    },
                    Vec::new(),
                ))
            },
        )
    }
}

fn compact_expand_next_action(kind: AgentExpandKind, target: &SessionHandleTarget) -> String {
    match kind {
        AgentExpandKind::Diagnostics => {
            if is_text_fragment_target(target) {
                "Use prism_open for the exact slice, or prism_workset for the staged bundle."
                    .to_string()
            } else if is_spec_like_kind(target.kind)
                || target.file_path.as_deref().is_some_and(is_docs_path)
            {
                "Use prism_workset for owners, or prism_open for the local section.".to_string()
            } else if is_structured_config_target(target.kind) {
                "Use prism_expand `validation` or `neighbors`.".to_string()
            } else {
                "Use prism_workset for reads/tests, or prism_open for local source.".to_string()
            }
        }
        AgentExpandKind::Lineage => {
            if is_text_fragment_target(target) {
                "Use prism_locate for a semantic symbol, or prism_workset to reach one.".to_string()
            } else {
                "Use prism_open for local source, or prism_query for full lineage detail."
                    .to_string()
            }
        }
        AgentExpandKind::Neighbors => {
            if is_text_fragment_target(target) {
                "Use prism_open on a neighbor, or prism_workset for the staged bundle.".to_string()
            } else if is_structured_config_target(target.kind) {
                "Use prism_open on a same-file key, or prism_expand `validation`.".to_string()
            } else if is_spec_like_kind(target.kind)
                || target.file_path.as_deref().is_some_and(is_docs_path)
            {
                "Use prism_open on an owner, or prism_expand `drift`.".to_string()
            } else {
                "Use prism_open on a neighbor, or prism_expand `validation`.".to_string()
            }
        }
        AgentExpandKind::Diff => {
            if is_text_fragment_target(target) {
                "Use prism_locate for a semantic symbol, or prism_open on this slice.".to_string()
            } else {
                "Use prism_open for local source, or prism_query for full diff history.".to_string()
            }
        }
        AgentExpandKind::Validation => {
            if is_text_fragment_target(target) {
                "Use prism_open on a supporting slice, or prism_workset for the staged bundle."
                    .to_string()
            } else if is_structured_config_target(target.kind) {
                "Use prism_open on a nextRead, or prism_expand `neighbors`.".to_string()
            } else {
                "Use prism_open on a likely test/read, or prism_workset for the staged bundle."
                    .to_string()
            }
        }
        AgentExpandKind::Drift => {
            "Use prism_open on a nextRead, or prism_workset for the full owner/test bundle."
                .to_string()
        }
        AgentExpandKind::Impact => {
            "Use prism_open on a likely touch target, or prism_expand `timeline` for recent changes."
                .to_string()
        }
        AgentExpandKind::Timeline => {
            "Use prism_open for local source, or prism_expand `memory` for recalled context."
                .to_string()
        }
        AgentExpandKind::Memory => {
            "Use prism_open on a matching code path, or prism_workset for the staged bundle."
                .to_string()
        }
    }
}

fn compact_expand_suggested_actions(
    kind: AgentExpandKind,
    current_handle: &str,
    target: &SessionHandleTarget,
    result: &Value,
) -> Vec<AgentSuggestedActionView> {
    let mut actions = Vec::new();
    let followup = match kind {
        AgentExpandKind::Neighbors => first_handle_in_result(result, "neighbors"),
        AgentExpandKind::Validation => first_handle_in_result(result, "nextReads")
            .or_else(|| first_handle_in_result(result, "likelyTests")),
        AgentExpandKind::Drift => first_handle_in_result(result, "nextReads"),
        _ => None,
    };

    if let Some(handle) = followup.clone() {
        actions.push(suggested_open_action(handle.handle, AgentOpenMode::Focus));
    }

    match kind {
        AgentExpandKind::Diagnostics => {
            actions.push(suggested_workset_action(current_handle));
            if !is_text_fragment_target(target) {
                actions.push(suggested_open_action(current_handle, AgentOpenMode::Focus));
            }
        }
        AgentExpandKind::Lineage | AgentExpandKind::Diff => {
            actions.push(suggested_open_action(current_handle, AgentOpenMode::Focus));
            actions.push(suggested_workset_action(current_handle));
        }
        AgentExpandKind::Neighbors => {
            if is_structured_config_target(target.kind) {
                actions.push(suggested_expand_action(
                    current_handle,
                    AgentExpandKind::Validation,
                ));
            } else if is_text_fragment_target(target) {
                actions.push(suggested_workset_action(current_handle));
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
        }
        AgentExpandKind::Validation => {
            if is_structured_config_target(target.kind) {
                actions.push(suggested_expand_action(
                    current_handle,
                    AgentExpandKind::Neighbors,
                ));
            } else if is_text_fragment_target(target) {
                actions.push(suggested_workset_action(current_handle));
            } else {
                actions.push(suggested_expand_action(
                    current_handle,
                    AgentExpandKind::Neighbors,
                ));
            }
        }
        AgentExpandKind::Drift => {
            actions.push(suggested_workset_action(current_handle));
        }
        AgentExpandKind::Impact => {
            actions.push(suggested_expand_action(
                current_handle,
                AgentExpandKind::Timeline,
            ));
            if let Some(handle) = followup {
                actions.push(suggested_open_action(handle.handle, AgentOpenMode::Focus));
            }
        }
        AgentExpandKind::Timeline => {
            actions.push(suggested_expand_action(
                current_handle,
                AgentExpandKind::Memory,
            ));
            actions.push(suggested_open_action(current_handle, AgentOpenMode::Focus));
        }
        AgentExpandKind::Memory => {
            actions.push(suggested_workset_action(current_handle));
            actions.push(suggested_open_action(current_handle, AgentOpenMode::Focus));
        }
    }

    dedupe_suggested_actions(actions)
}

fn first_handle_in_result(result: &Value, field: &str) -> Option<AgentTargetHandleView> {
    serde_json::from_value::<Vec<AgentTargetHandleView>>(result.get(field)?.clone())
        .ok()
        .and_then(|items| items.into_iter().next())
}

fn compact_lineage_expand_result(prism: &Prism, target: &SessionHandleTarget) -> Result<Value> {
    let Some(lineage) = crate::lineage_view(prism, target_symbol_id(target)?)? else {
        return Ok(json!({
            "summary": "No lineage history is currently recorded for this symbol.",
            "recentHistory": [],
            "uncertainty": [],
            "truncated": false,
        }));
    };
    let mut uncertainty = compact_string_list(
        &lineage.uncertainty,
        EXPAND_LINEAGE_UNCERTAINTY_LIMIT,
        EXPAND_LINEAGE_TEXT_MAX_CHARS,
    );
    let mut recent_history = lineage
        .history
        .iter()
        .rev()
        .take(EXPAND_LINEAGE_HISTORY_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    recent_history.reverse();
    let mut truncated = lineage.history.len() > recent_history.len()
        || lineage.uncertainty.len() > uncertainty.len();

    let mut result = compact_lineage_value(&lineage, &recent_history, &uncertainty, truncated);
    while expand_json_bytes(&result)? > EXPAND_LINEAGE_MAX_JSON_BYTES {
        if !recent_history.is_empty() {
            recent_history.remove(0);
            truncated = true;
            result = compact_lineage_value(&lineage, &recent_history, &uncertainty, truncated);
            continue;
        }
        if !uncertainty.is_empty() {
            uncertainty.pop();
            truncated = true;
            result = compact_lineage_value(&lineage, &recent_history, &uncertainty, truncated);
            continue;
        }
        break;
    }
    Ok(result)
}

fn compact_lineage_value(
    lineage: &prism_js::LineageView,
    recent_history: &[prism_js::LineageEventView],
    uncertainty: &[String],
    truncated: bool,
) -> Value {
    let recent_history = recent_history
        .iter()
        .map(compact_lineage_event_value)
        .collect::<Vec<_>>();
    json!({
        "lineageId": lineage.lineage_id,
        "currentPath": lineage.current.id.path,
        "currentKind": lineage.current.kind,
        "status": lineage.status,
        "summary": clamp_string(&lineage.summary, EXPAND_LINEAGE_TEXT_MAX_CHARS),
        "uncertainty": uncertainty,
        "recentHistory": recent_history,
        "truncated": truncated,
        "nextAction": truncated.then_some(
            "Use prism_query if you need the full lineage history or evidence details."
        ),
    })
}

fn compact_lineage_event_value(event: &prism_js::LineageEventView) -> Value {
    json!({
        "kind": event.kind,
        "summary": clamp_string(&event.summary, EXPAND_LINEAGE_TEXT_MAX_CHARS),
        "confidence": event.confidence,
        "evidence": compact_string_list(&event.evidence, EXPAND_LINEAGE_EVIDENCE_LIMIT, 32),
    })
}

fn compact_diff_expand_result(prism: &Prism, target: &SessionHandleTarget) -> Result<Value> {
    let lineage = target
        .lineage_id
        .as_ref()
        .map(|value| LineageId::new(value.clone()));
    let diff_result = diff_for(
        prism,
        Some(target_symbol_id(target)?),
        lineage.as_ref(),
        None,
        None,
        EXPAND_DIFF_LIMIT,
    )?;
    let total_diffs = diff_result.len();
    let mut recent_diffs = diff_result
        .into_iter()
        .take(EXPAND_COMPACT_DIFF_LIMIT)
        .collect::<Vec<_>>();
    let mut truncated = total_diffs > recent_diffs.len();
    let mut result = compact_diff_value(&recent_diffs, truncated);
    while expand_json_bytes(&result)? > EXPAND_DIFF_MAX_JSON_BYTES {
        if !recent_diffs.is_empty() {
            recent_diffs.pop();
            truncated = true;
            result = compact_diff_value(&recent_diffs, truncated);
            continue;
        }
        break;
    }
    Ok(result)
}

fn compact_diff_value(diffs: &[prism_js::DiffHunkView], truncated: bool) -> Value {
    let summary = if diffs.is_empty() {
        "No recent patch events are recorded for this symbol.".to_string()
    } else {
        format!(
            "{} recent patch event(s) touched this symbol or lineage.",
            diffs.len()
        )
    };
    let recent_diffs = diffs
        .iter()
        .map(compact_diff_hunk_value)
        .collect::<Vec<_>>();
    json!({
        "summary": summary,
        "recentDiffs": recent_diffs,
        "truncated": truncated,
        "nextAction": truncated.then_some(
            "Use prism_query if you need the full patch history or rich diff hunks."
        ),
    })
}

fn compact_diff_hunk_value(diff: &prism_js::DiffHunkView) -> Value {
    let symbol_path = diff
        .symbol
        .id
        .as_ref()
        .map(|id| id.path.clone())
        .unwrap_or_else(|| diff.symbol.name.clone());
    json!({
        "eventId": diff.event_id,
        "summary": clamp_string(&diff.summary, EXPAND_DIFF_TEXT_MAX_CHARS),
        "trigger": diff.trigger,
        "symbolPath": symbol_path,
        "symbolKind": diff.symbol.kind,
        "status": diff.symbol.status,
        "filePath": diff.symbol.file_path,
    })
}

fn compact_structured_config_validation_result(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Value> {
    let next_reads =
        structured_symbol_followups(host, session, prism, target, WORKSET_SUPPORTING_LIMIT)?;
    let likely_tests = if is_text_fragment_target(target) {
        Vec::new()
    } else {
        structured_config_likely_tests(
            session,
            target,
            owner_views_for_target(
                prism,
                target_symbol_id(target)?,
                Some("test"),
                WORKSET_TEST_LIMIT.saturating_mul(4),
            )?,
        )
    };
    Ok(json!({
        "checks": structured_config_validation_checks(),
        "nextReads": next_reads,
        "likelyTests": likely_tests,
        "why": [
            "Structured config validation prioritizes same-file semantic relatives before heuristic tests.",
            "Use the parent key and adjacent entries to confirm the edit preserves local config invariants.",
        ],
    }))
}

fn structured_config_validation_checks() -> Vec<String> {
    vec![
        "Confirm parent and sibling structured keys still agree with this entry.".to_string(),
        "Review adjacent same-file keys before falling back to parser or integration tests."
            .to_string(),
    ]
}

fn structured_config_likely_tests(
    session: &SessionState,
    target: &SessionHandleTarget,
    owners: Vec<OwnerCandidateView>,
) -> Vec<AgentTargetHandleView> {
    owners
        .into_iter()
        .filter(|candidate| {
            candidate
                .trust_signals
                .evidence_sources
                .iter()
                .any(|source| matches!(source, EvidenceSourceKind::DirectGraph))
        })
        .take(WORKSET_TEST_LIMIT)
        .map(|candidate| {
            compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why),
            )
        })
        .collect()
}

fn compact_drift_expand_result(
    host: &QueryHost,
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Value> {
    let drift = spec_drift_explanation_view(prism, &target.id)?;
    let drift_reasons = if drift.drift_reasons.is_empty() {
        if drift.gaps.is_empty() {
            compact_string_list(
                &drift.notes,
                EXPAND_DRIFT_LIST_LIMIT,
                EXPAND_DRIFT_TEXT_MAX_CHARS,
            )
        } else {
            compact_string_list(
                &drift.gaps,
                EXPAND_DRIFT_LIST_LIMIT,
                EXPAND_DRIFT_TEXT_MAX_CHARS,
            )
        }
    } else {
        compact_string_list(
            &drift.drift_reasons,
            EXPAND_DRIFT_LIST_LIMIT,
            EXPAND_DRIFT_TEXT_MAX_CHARS,
        )
    };
    let mut next_reads = prioritized_spec_supporting_reads(
        host,
        session,
        prism,
        target,
        &drift,
        EXPAND_DRIFT_NEXT_READ_LIMIT,
    )?;
    strip_file_paths(&mut next_reads);
    let mut result = json!({
        "driftReasons": drift_reasons,
        "expectations": compact_string_list(
            &drift.expectations,
            EXPAND_DRIFT_LIST_LIMIT,
            EXPAND_DRIFT_TEXT_MAX_CHARS,
        ),
        "gaps": compact_string_list(
            &drift.gaps,
            EXPAND_DRIFT_LIST_LIMIT,
            EXPAND_DRIFT_TEXT_MAX_CHARS,
        ),
        "nextReads": next_reads,
        "confidence": drift.trust_signals.confidence_label,
        "evidenceSources": drift.trust_signals.evidence_sources,
    });

    while drift_json_bytes(&result)? > EXPAND_DRIFT_MAX_JSON_BYTES {
        if next_reads.pop().is_some() {
            result["nextReads"] = serde_json::to_value(&next_reads)?;
            continue;
        }
        if let Some(expectations) = result
            .get_mut("expectations")
            .and_then(Value::as_array_mut)
            .filter(|items| !items.is_empty())
        {
            expectations.pop();
            continue;
        }
        if let Some(evidence_sources) = result
            .get_mut("evidenceSources")
            .and_then(Value::as_array_mut)
            .filter(|items| !items.is_empty())
        {
            evidence_sources.pop();
            continue;
        }
        break;
    }

    Ok(result)
}

fn compact_impact_expand_result(
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
) -> Result<Value> {
    let impact = crate::blast_radius_view(prism, session, target_symbol_id(target)?);
    let mut likely_touch = compact_touch_targets(
        session,
        prism,
        target,
        &impact.direct_nodes,
        EXPAND_IMPACT_TOUCH_LIMIT,
    )?;
    let mut cache = crate::SemanticContextCache::default();
    let validation =
        validation_context_view_cached(prism, session, &mut cache, target_symbol_id(target)?)?;
    let mut likely_tests = validation
        .tests
        .into_iter()
        .take(EXPAND_IMPACT_TEST_LIMIT)
        .map(|candidate| {
            compact_target_view(
                session,
                &candidate.symbol,
                target.query.as_deref(),
                Some(candidate.why),
            )
        })
        .collect::<Vec<_>>();
    let mut recent_failures = impact
        .risk_events
        .iter()
        .take(EXPAND_IMPACT_FAILURE_LIMIT)
        .map(|event| compact_outcome_summary_value(event, EXPAND_IMPACT_TEXT_MAX_CHARS))
        .collect::<Vec<_>>();
    let mut result = json!({
        "likelyTouch": likely_touch,
        "likelyTests": likely_tests,
        "recentFailures": recent_failures,
        "riskHint": compact_impact_risk_hint(&impact),
    });

    while expand_json_bytes(&result)? > EXPAND_IMPACT_MAX_JSON_BYTES {
        if strip_file_paths(&mut likely_touch) {
            result["likelyTouch"] = serde_json::to_value(&likely_touch)?;
            continue;
        }
        if strip_file_paths(&mut likely_tests) {
            result["likelyTests"] = serde_json::to_value(&likely_tests)?;
            continue;
        }
        if likely_touch.pop().is_some() {
            result["likelyTouch"] = serde_json::to_value(&likely_touch)?;
            continue;
        }
        if likely_tests.pop().is_some() {
            result["likelyTests"] = serde_json::to_value(&likely_tests)?;
            continue;
        }
        if recent_failures.pop().is_some() {
            result["recentFailures"] = Value::Array(recent_failures.clone());
            continue;
        }
        break;
    }

    Ok(result)
}

fn compact_touch_targets(
    session: &SessionState,
    prism: &Prism,
    target: &SessionHandleTarget,
    direct_nodes: &[prism_js::NodeIdView],
    limit: usize,
) -> Result<Vec<AgentTargetHandleView>> {
    let mut seen = HashSet::<String>::new();
    let mut targets = Vec::new();
    for node in direct_nodes {
        let node_id = NodeId::new(node.crate_name.clone(), node.path.clone(), node.kind);
        if node_id == target.id {
            continue;
        }
        let symbol = match symbol_for(prism, &node_id) {
            Ok(symbol) => symbol,
            Err(_) => continue,
        };
        let symbol = symbol_view(prism, &symbol)?;
        if is_test_like_symbol(&symbol) || !seen.insert(symbol.id.path.clone()) {
            continue;
        }
        targets.push(compact_target_view(
            session,
            &symbol,
            target.query.as_deref(),
            Some("Likely blast-radius follow-up.".to_string()),
        ));
        if targets.len() >= limit {
            break;
        }
    }
    Ok(targets)
}

fn compact_impact_risk_hint(impact: &prism_js::ChangeImpactView) -> String {
    let hint = if let Some(summary) = impact.promoted_summaries.first() {
        summary.clone()
    } else if !impact.risk_events.is_empty() {
        "Recent failures and recorded outcomes raise regression risk for this target.".to_string()
    } else if !impact.co_change_neighbors.is_empty() {
        "Co-change neighbors suggest this target tends to move with nearby lineages.".to_string()
    } else if !impact.likely_validations.is_empty() {
        format!(
            "Likely validations: {}.",
            impact
                .likely_validations
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        "Blast radius currently looks narrow and mostly local.".to_string()
    };
    clamp_string(&hint, EXPAND_IMPACT_TEXT_MAX_CHARS)
}

fn compact_timeline_expand_result(prism: &Prism, target: &SessionHandleTarget) -> Result<Value> {
    let anchors = compact_symbol_history_anchors(target);
    let mut recent_events = prism
        .outcomes_for(&anchors, EXPAND_TIMELINE_EVENT_LIMIT)
        .into_iter()
        .map(|event| compact_outcome_summary_value(&event, EXPAND_TIMELINE_TEXT_MAX_CHARS))
        .collect::<Vec<_>>();
    let lineage = target
        .lineage_id
        .as_ref()
        .map(|value| LineageId::new(value.clone()));
    let mut recent_patches = diff_for(
        prism,
        Some(target_symbol_id(target)?),
        lineage.as_ref(),
        None,
        None,
        EXPAND_TIMELINE_PATCH_LIMIT,
    )?
    .into_iter()
    .map(|diff| compact_diff_hunk_value(&diff))
    .collect::<Vec<_>>();
    let events = prism.outcomes_for(&anchors, 20);
    let mut result = json!({
        "recentEvents": recent_events,
        "recentPatches": recent_patches,
        "lastFailure": compact_last_outcome(&events, |event| {
            matches!(event.kind, OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved)
        }, EXPAND_TIMELINE_TEXT_MAX_CHARS),
        "lastValidation": compact_last_outcome(&events, |event| {
            matches!(
                event.kind,
                OutcomeKind::BuildRan | OutcomeKind::TestRan | OutcomeKind::FixValidated
            )
        }, EXPAND_TIMELINE_TEXT_MAX_CHARS),
    });

    while expand_json_bytes(&result)? > EXPAND_TIMELINE_MAX_JSON_BYTES {
        if recent_patches.pop().is_some() {
            result["recentPatches"] = Value::Array(recent_patches.clone());
            continue;
        }
        if recent_events.pop().is_some() {
            result["recentEvents"] = Value::Array(recent_events.clone());
            continue;
        }
        if result
            .get("lastValidation")
            .is_some_and(|value| !value.is_null())
        {
            result["lastValidation"] = Value::Null;
            continue;
        }
        break;
    }

    Ok(result)
}

fn compact_memory_expand_result(
    session: &SessionState,
    target: &SessionHandleTarget,
) -> Result<Value> {
    let recalled = session.notes.recall(&RecallQuery {
        focus: compact_symbol_history_anchors(target),
        text: target.query.clone().or_else(|| Some(target.name.clone())),
        limit: EXPAND_MEMORY_LIMIT,
        kinds: None,
        since: None,
    })?;
    let mut memories = recalled
        .into_iter()
        .map(|memory| {
            json!({
                "summary": clamp_string(&memory.entry.content, EXPAND_MEMORY_TEXT_MAX_CHARS),
                "kind": memory.entry.kind,
                "source": memory.entry.source,
                "trust": memory.entry.trust,
                "whyMatched": clamp_string(
                    memory
                        .explanation
                        .as_deref()
                        .unwrap_or("Matched on shared anchors and nearby task context."),
                    EXPAND_MEMORY_MATCH_MAX_CHARS,
                ),
            })
        })
        .collect::<Vec<_>>();
    let mut result = json!({ "memories": memories });
    while expand_json_bytes(&result)? > EXPAND_MEMORY_MAX_JSON_BYTES {
        if memories.pop().is_some() {
            result["memories"] = Value::Array(memories.clone());
            continue;
        }
        break;
    }
    Ok(result)
}

fn compact_symbol_history_anchors(target: &SessionHandleTarget) -> Vec<prism_ir::AnchorRef> {
    let mut anchors = vec![prism_ir::AnchorRef::Node(target.id.clone())];
    if let Some(lineage_id) = target.lineage_id.as_ref() {
        anchors.push(prism_ir::AnchorRef::Lineage(LineageId::new(
            lineage_id.clone(),
        )));
    }
    anchors
}

fn compact_last_outcome<F>(events: &[OutcomeEvent], predicate: F, max_chars: usize) -> Value
where
    F: Fn(&OutcomeEvent) -> bool,
{
    events
        .iter()
        .find(|event| predicate(event))
        .map(|event| compact_outcome_summary_value(event, max_chars))
        .unwrap_or(Value::Null)
}

fn compact_outcome_summary_value(event: &OutcomeEvent, max_chars: usize) -> Value {
    json!({
        "ts": event.meta.ts,
        "kind": event.kind,
        "result": event.result,
        "summary": clamp_string(&event.summary, max_chars),
    })
}

pub(super) fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "\"unknown\"".to_string())
        .trim_matches('"')
        .to_string()
}

#[cfg(test)]
mod tests {
    use prism_js::{ConfidenceLabel, NodeIdView, SourceExcerptView, SymbolView, TrustSignalsView};

    use super::*;

    #[test]
    fn structured_config_likely_tests_keeps_only_direct_graph_candidates() {
        let session = SessionState::new(
            Arc::new(prism_memory::SessionMemory::default()),
            Arc::new(prism_agent::InferenceStore::default()),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            prism_query::QueryLimits::default(),
        );
        let target = SessionHandleTarget {
            id: NodeId::new(
                "demo",
                "demo::document::Cargo_toml::workspace::dependencies",
                NodeKind::TomlKey,
            ),
            lineage_id: None,
            name: "dependencies".to_string(),
            kind: NodeKind::TomlKey,
            file_path: Some("Cargo.toml".to_string()),
            query: Some("workspace.dependencies".to_string()),
            why_short: "Structured key aligned with the exact text hit.".to_string(),
            start_line: None,
            end_line: None,
            start_column: None,
            end_column: None,
        };
        let direct = owner_candidate("demo::tests::direct", vec![EvidenceSourceKind::DirectGraph]);
        let inferred = owner_candidate("demo::tests::inferred", vec![EvidenceSourceKind::Inferred]);

        let likely_tests =
            structured_config_likely_tests(&session, &target, vec![inferred, direct.clone()]);

        assert_eq!(likely_tests.len(), 1);
        assert_eq!(likely_tests[0].path, direct.symbol.id.path);
    }

    fn owner_candidate(
        path: &str,
        evidence_sources: Vec<EvidenceSourceKind>,
    ) -> OwnerCandidateView {
        OwnerCandidateView {
            symbol: SymbolView {
                id: NodeIdView {
                    crate_name: "demo".to_string(),
                    path: path.to_string(),
                    kind: NodeKind::Function,
                },
                name: path.rsplit("::").next().unwrap_or(path).to_string(),
                kind: NodeKind::Function,
                signature: format!("function {path}"),
                file_path: Some("/workspace/src/tests.rs".to_string()),
                span: prism_ir::Span { start: 0, end: 0 },
                location: None,
                language: prism_ir::Language::Rust,
                lineage_id: None,
                source_excerpt: Some(SourceExcerptView {
                    text: "#[test] fn direct() {}".to_string(),
                    start_line: 1,
                    end_line: 1,
                    truncated: false,
                }),
                owner_hint: None,
            },
            kind: "test".to_string(),
            score: 10,
            matched_terms: vec!["dependencies".to_string()],
            why: "Test owner surfaced by PRISM.".to_string(),
            trust_signals: TrustSignalsView {
                confidence_label: ConfidenceLabel::High,
                evidence_sources,
                why: vec!["synthetic test".to_string()],
            },
        }
    }
}

pub(super) fn source_slice_view(slice: prism_query::EditSlice) -> SourceSliceView {
    SourceSliceView {
        text: slice.text,
        start_line: slice.start_line,
        end_line: slice.end_line,
        focus: SourceLocationView {
            start_line: slice.focus.start_line,
            start_column: slice.focus.start_column,
            end_line: slice.focus.end_line,
            end_column: slice.focus.end_column,
        },
        relative_focus: SourceLocationView {
            start_line: slice.relative_focus.start_line,
            start_column: slice.relative_focus.start_column,
            end_line: slice.relative_focus.end_line,
            end_column: slice.relative_focus.end_column,
        },
        truncated: slice.truncated,
    }
}

fn expand_json_bytes(result: &Value) -> Result<usize> {
    Ok(serde_json::to_vec(result)?.len())
}
