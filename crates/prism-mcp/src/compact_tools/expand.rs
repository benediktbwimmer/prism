use prism_js::{EvidenceSourceKind, OwnerCandidateView};

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
                        if is_text_fragment_target(&target) {
                            compact_text_fragment_validation(host, session.as_ref(), &target)?
                        } else if is_structured_config_target(target.kind) {
                            compact_structured_config_validation_result(
                                host,
                                session.as_ref(),
                                prism.as_ref(),
                                &target,
                            )?
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
    let likely_tests = structured_config_likely_tests(
        session,
        target,
        owner_views_for_target(
            prism,
            target_symbol_id(target)?,
            Some("test"),
            WORKSET_TEST_LIMIT.saturating_mul(4),
        )?,
    );
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
