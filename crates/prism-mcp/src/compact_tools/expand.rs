use super::open::compact_preview_for_symbol_view;
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
                            serde_json::to_value(crate::lineage_view(
                                prism.as_ref(),
                                target_symbol_id(&target)?,
                            )?)?
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
                            let lineage = target
                                .lineage_id
                                .as_ref()
                                .map(|value| LineageId::new(value.clone()));
                            serde_json::to_value(diff_for(
                                prism.as_ref(),
                                Some(target_symbol_id(&target)?),
                                lineage.as_ref(),
                                None,
                                None,
                                EXPAND_DIFF_LIMIT,
                            )?)?
                        }
                    }
                    AgentExpandKind::Validation => {
                        if is_text_fragment_target(&target) {
                            compact_text_fragment_validation(host, session.as_ref(), &target)?
                        } else {
                            let mut cache = crate::SemanticContextCache::default();
                            let validation = validation_context_view_cached(
                                prism.as_ref(),
                                session.as_ref(),
                                &mut cache,
                                target_symbol_id(&target)?,
                            )?;
                            let next_reads = if is_structured_config_target(target.kind) {
                                structured_symbol_followups(
                                    host,
                                    session.as_ref(),
                                    prism.as_ref(),
                                    &target,
                                    WORKSET_SUPPORTING_LIMIT,
                                )?
                            } else {
                                Vec::new()
                            };
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
                            let mut checks = compact_validation_checks(
                                &validation.validation_recipe.checks,
                                &validation.validation_recipe.scored_checks,
                                COMPACT_VALIDATION_CHECK_LIMIT,
                                COMPACT_VALIDATION_CHECK_MAX_CHARS,
                            );
                            if !next_reads.is_empty() {
                                checks.insert(
                                    0,
                                    "Confirm parent and sibling structured keys still agree with this entry."
                                        .to_string(),
                                );
                                checks.truncate(COMPACT_VALIDATION_CHECK_LIMIT);
                            }
                            json!({
                                "checks": checks,
                                "nextReads": next_reads,
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
                };

                Ok((
                    AgentExpandResultView {
                        handle: args.handle,
                        kind,
                        result,
                        remapped,
                        top_preview,
                    },
                    Vec::new(),
                ))
            },
        )
    }
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
