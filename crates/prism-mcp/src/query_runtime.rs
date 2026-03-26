use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use prism_ir::{AnchorRef, ArtifactId, CoordinationTaskId, EdgeKind, NodeId, PlanId};
use prism_js::{QueryDiagnostic, QueryEnvelope, ScoredMemoryView, SubgraphView, SymbolView};
use prism_memory::{MemoryModule, OutcomeRecallQuery, RecallQuery};
use prism_query::{Prism, Symbol};
use serde_json::{json, Value};

use crate::{
    artifact_risk_view, artifact_view, blast_radius_view, blocker_view, change_impact_view,
    claim_view, co_change_view, conflict_view, convert_anchors, convert_node_id,
    coordination_task_view, current_timestamp, drift_candidate_view, edge_kind_label, edge_view,
    js_runtime, lineage_view, merge_node_ids, merge_promoted_checks, parse_capability,
    parse_claim_mode, parse_event_actor, parse_memory_kind, parse_node_kind, parse_outcome_kind,
    parse_outcome_result, plan_view, policy_violation_record_view, promoted_memory_entries,
    promoted_summary_texts, promoted_validation_checks, relations_view, scored_memory_view,
    symbol_for, symbol_view, symbol_views_for_ids, task_intent_view, task_journal_view,
    task_risk_view, task_validation_recipe_view, validation_recipe_view_with, AnchorListArgs,
    CallGraphArgs, CoordinationTaskTargetArgs, CuratorJobArgs, CuratorJobsArgs,
    DEFAULT_CALL_GRAPH_DEPTH, DEFAULT_SEARCH_LIMIT, DEFAULT_TASK_JOURNAL_EVENT_LIMIT,
    DEFAULT_TASK_JOURNAL_MEMORY_LIMIT, LimitArgs, MemoryOutcomeArgs, MemoryRecallArgs,
    PendingReviewsArgs, PlanTargetArgs, PolicyViolationQueryArgs, QueryHost, QueryLanguage,
    SearchArgs, SimulateClaimArgs, SymbolQueryArgs, SymbolTargetArgs, TaskJournalArgs,
    TaskTargetArgs,
};

impl QueryHost {
    pub(crate) fn execute(&self, code: &str, language: QueryLanguage) -> Result<QueryEnvelope> {
        match language {
            QueryLanguage::Ts => self.execute_typescript(code),
        }
    }

    #[cfg(test)]
    pub(crate) fn symbol_query(&self, query: &str) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let result = serde_json::to_value(execution.best_symbol(query)?)?;
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    #[cfg(test)]
    pub(crate) fn search_query(&self, args: SearchArgs) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let result = serde_json::to_value(execution.search(args)?)?;
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    fn execute_typescript(&self, code: &str) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let source = format!(
            "(function() {{\n  try {{\n    const __prismUserQuery = () => {{\n{}\n    }};\n    const __prismResult = __prismUserQuery();\n    return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n  }} catch (error) {{\n    const __prismMessage = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : error && typeof error === \"object\" && \"message\" in error && error.message\n        ? String(error.message)\n        : String(error);\n    throw new Error(__prismMessage);\n  }}\n}})();\n",
            code
        );
        let transpiled = js_runtime::transpile_typescript(&source)?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let raw_result = self.worker.execute(transpiled, execution.clone())?;
        let mut result =
            serde_json::from_str(&raw_result).context("failed to decode query result JSON")?;
        let limits = self.session.limits();
        if raw_result.len() > limits.max_output_json_bytes {
            execution.push_diagnostic(
                "result_truncated",
                format!(
                    "Query output exceeded the {} byte session cap.",
                    limits.max_output_json_bytes
                ),
                Some(json!({
                    "applied": limits.max_output_json_bytes,
                    "observed": raw_result.len(),
                })),
            );
            result = Value::Null;
        }
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    pub(crate) fn co_change_neighbors_value(&self, id: &NodeId) -> Result<Value> {
        let prism = self.current_prism();
        serde_json::to_value(
            prism
                .co_change_neighbors(id, 8)
                .into_iter()
                .map(co_change_view)
                .collect::<Vec<_>>(),
        )
        .map_err(Into::into)
    }
}

#[derive(Clone)]
pub(crate) struct QueryExecution {
    host: QueryHost,
    prism: Arc<Prism>,
    diagnostics: Arc<Mutex<Vec<QueryDiagnostic>>>,
}

impl QueryExecution {
    pub(crate) fn new(host: QueryHost, prism: Arc<Prism>) -> Self {
        Self {
            host,
            prism,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn diagnostics(&self) -> Vec<QueryDiagnostic> {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .clone()
    }

    pub(crate) fn push_diagnostic(
        &self,
        code: &str,
        message: impl Into<String>,
        data: Option<Value>,
    ) {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .push(QueryDiagnostic {
                code: code.to_owned(),
                message: message.into(),
                data,
            });
    }

    pub(crate) fn dispatch_enveloped(&self, operation: &str, args_json: &str) -> String {
        match self.dispatch(operation, args_json) {
            Ok(value) => json!({ "ok": true, "value": value }).to_string(),
            Err(error) => json!({ "ok": false, "error": error.to_string() }).to_string(),
        }
    }

    pub(crate) fn dispatch(&self, operation: &str, args_json: &str) -> Result<Value> {
        let args = if args_json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(args_json).context("failed to parse host-call arguments")?
        };

        self.ensure_operation_enabled(operation)?;

        match operation {
            "symbol" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.best_symbol(&args.query)?)?)
            }
            "symbols" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.symbols(&args.query)?)?)
            }
            "search" => {
                let args: SearchArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.search(args)?)?)
            }
            "entrypoints" => Ok(serde_json::to_value(self.entrypoints()?)?),
            "plan" => {
                let args: PlanTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .coordination_plan(&PlanId::new(args.plan_id))
                        .map(plan_view),
                )?)
            }
            "coordinationTask" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .coordination_task(&CoordinationTaskId::new(args.task_id))
                        .map(coordination_task_view),
                )?)
            }
            "readyTasks" => {
                let args: PlanTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .ready_tasks(&PlanId::new(args.plan_id), current_timestamp())
                        .into_iter()
                        .map(coordination_task_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "claims" => {
                let args: AnchorListArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .claims(&convert_anchors(args.anchors)?, current_timestamp())
                        .into_iter()
                        .map(claim_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "conflicts" => {
                let args: AnchorListArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .conflicts(&convert_anchors(args.anchors)?, current_timestamp())
                        .into_iter()
                        .map(conflict_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "blockers" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let blockers = self.prism.blockers(
                    &CoordinationTaskId::new(args.task_id.clone()),
                    current_timestamp(),
                );
                if !blockers.is_empty() {
                    self.push_diagnostic(
                        "task_blocked",
                        format!(
                            "Coordination task `{}` currently has blockers.",
                            args.task_id
                        ),
                        Some(json!({ "taskId": args.task_id, "count": blockers.len() })),
                    );
                }
                if blockers
                    .iter()
                    .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision)
                {
                    self.push_diagnostic(
                        "stale_revision",
                        "The coordination task is based on a stale workspace revision.",
                        None,
                    );
                }
                if blockers.iter().any(|blocker| {
                    blocker.kind == prism_coordination::BlockerKind::ValidationRequired
                }) {
                    self.push_diagnostic(
                        "validation_required",
                        "The coordination task is missing required validations.",
                        None,
                    );
                }
                if blockers.iter().any(|blocker| {
                    blocker.kind == prism_coordination::BlockerKind::RiskReviewRequired
                        || blocker.kind == prism_coordination::BlockerKind::ArtifactStale
                }) {
                    self.push_diagnostic(
                        "task_risk_blocked",
                        "The coordination task is blocked by risk or stale artifact state.",
                        None,
                    );
                }
                Ok(serde_json::to_value(
                    blockers.into_iter().map(blocker_view).collect::<Vec<_>>(),
                )?)
            }
            "pendingReviews" => {
                let args: PendingReviewsArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .pending_reviews(
                            args.plan_id
                                .as_ref()
                                .map(|plan_id| PlanId::new(plan_id.clone()))
                                .as_ref(),
                        )
                        .into_iter()
                        .map(artifact_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "artifacts" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .artifacts(&CoordinationTaskId::new(args.task_id))
                        .into_iter()
                        .map(artifact_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "policyViolations" => {
                let args: PolicyViolationQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .policy_violations(
                            args.plan_id
                                .as_ref()
                                .map(|plan_id| PlanId::new(plan_id.clone()))
                                .as_ref(),
                            args.task_id
                                .as_ref()
                                .map(|task_id| CoordinationTaskId::new(task_id.clone()))
                                .as_ref(),
                            args.limit.unwrap_or(20),
                        )
                        .into_iter()
                        .map(policy_violation_record_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "taskBlastRadius" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism.task_blast_radius(&task_id).map(|impact| {
                        let anchors = self
                            .prism
                            .coordination_task(&task_id)
                            .map(|task| task.anchors)
                            .unwrap_or_default();
                        let mut view = change_impact_view(impact);
                        view.promoted_summaries = promoted_summary_texts(
                            self.host.session.as_ref(),
                            self.prism.as_ref(),
                            &anchors,
                        );
                        view
                    }),
                )?)
            }
            "taskValidationRecipe" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism
                        .task_validation_recipe(&task_id)
                        .map(|mut recipe| {
                            let anchors = self
                                .prism
                                .coordination_task(&task_id)
                                .map(|task| task.anchors)
                                .unwrap_or_default();
                            merge_promoted_checks(
                                &mut recipe.scored_checks,
                                promoted_validation_checks(
                                    self.host.session.as_ref(),
                                    self.prism.as_ref(),
                                    &anchors,
                                ),
                            );
                            recipe.checks = recipe
                                .scored_checks
                                .iter()
                                .map(|check| check.label.clone())
                                .collect::<Vec<_>>();
                            recipe.checks.sort();
                            recipe.checks.dedup();
                            task_validation_recipe_view(recipe)
                        }),
                )?)
            }
            "taskRisk" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism
                        .task_risk(&task_id, current_timestamp())
                        .map(|risk| {
                            let task = self.prism.coordination_task(&task_id);
                            let anchors = task
                                .as_ref()
                                .map(|task| task.anchors.clone())
                                .unwrap_or_default();
                            let promoted_summaries = promoted_summary_texts(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                            );
                            let promoted_risk_boost = promoted_memory_entries(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                                "risk_summary",
                            )
                            .into_iter()
                            .map(|entry| {
                                let severity_weight = match entry
                                    .metadata
                                    .get("severity")
                                    .and_then(Value::as_str)
                                    .unwrap_or("medium")
                                {
                                    "low" => 0.04,
                                    "high" => 0.12,
                                    _ => 0.08,
                                };
                                severity_weight * entry.trust.clamp(0.0, 1.0)
                            })
                            .sum::<f32>()
                            .min(0.25);
                            let boosted_risk_score =
                                (risk.risk_score + promoted_risk_boost).min(1.0);
                            let review_required = risk.review_required
                                || task
                                    .as_ref()
                                    .and_then(|task| self.prism.coordination_plan(&task.plan))
                                    .and_then(|plan| plan.policy.review_required_above_risk_score)
                                    .map(|threshold| boosted_risk_score >= threshold)
                                    .unwrap_or(false);
                            let mut view = task_risk_view(risk, promoted_summaries);
                            view.risk_score = boosted_risk_score;
                            view.review_required = review_required;
                            view
                        }),
                )?)
            }
            "artifactRisk" => {
                let artifact_id = args
                    .get("artifactId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("artifactId is required"))?;
                let artifact_id = ArtifactId::new(artifact_id.to_string());
                Ok(serde_json::to_value(
                    self.prism
                        .artifact_risk(&artifact_id, current_timestamp())
                        .map(|risk| {
                            let anchors = self
                                .prism
                                .coordination_snapshot()
                                .artifacts
                                .into_iter()
                                .find(|artifact| artifact.id == artifact_id)
                                .map(|artifact| artifact.anchors)
                                .unwrap_or_default();
                            let promoted_summaries = promoted_summary_texts(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                            );
                            let promoted_risk_boost = promoted_memory_entries(
                                self.host.session.as_ref(),
                                self.prism.as_ref(),
                                &anchors,
                                "risk_summary",
                            )
                            .into_iter()
                            .map(|entry| {
                                let severity_weight = match entry
                                    .metadata
                                    .get("severity")
                                    .and_then(Value::as_str)
                                    .unwrap_or("medium")
                                {
                                    "low" => 0.04,
                                    "high" => 0.12,
                                    _ => 0.08,
                                };
                                severity_weight * entry.trust.clamp(0.0, 1.0)
                            })
                            .sum::<f32>()
                            .min(0.25);
                            let mut view = artifact_risk_view(risk, promoted_summaries);
                            view.risk_score = (view.risk_score + promoted_risk_boost).min(1.0);
                            view
                        }),
                )?)
            }
            "taskIntent" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let task_id = CoordinationTaskId::new(args.task_id);
                Ok(serde_json::to_value(
                    self.prism.task_intent(&task_id).map(task_intent_view),
                )?)
            }
            "simulateClaim" => {
                let args: SimulateClaimArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .simulate_claim(
                            &self.host.session.session_id(),
                            &convert_anchors(args.anchors)?,
                            parse_capability(&args.capability)?,
                            args.mode.as_deref().map(parse_claim_mode).transpose()?,
                            args.task_id
                                .as_ref()
                                .map(|task_id| CoordinationTaskId::new(task_id.clone()))
                                .as_ref(),
                            current_timestamp(),
                        )
                        .into_iter()
                        .map(conflict_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "full" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(
                    symbol_for(self.prism.as_ref(), &id)?.full(),
                )?)
            }
            "relations" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(relations_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                )?)?)
            }
            "callGraph" => {
                let args: CallGraphArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.call_graph(args)?)?)
            }
            "lineage" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                let lineage = lineage_view(self.prism.as_ref(), &id)?;
                if lineage
                    .as_ref()
                    .is_some_and(|view| view.history.iter().any(|event| event.kind == "Ambiguous"))
                {
                    self.push_diagnostic(
                        "lineage_uncertain",
                        format!("Lineage for `{}` contains ambiguous history.", id.path),
                        Some(json!({ "id": id.path })),
                    );
                }
                Ok(serde_json::to_value(lineage)?)
            }
            "relatedFailures" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                serde_json::to_value(self.prism.related_failures(&id)).map_err(Into::into)
            }
            "coChangeNeighbors" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                self.host.co_change_neighbors_value(&id)
            }
            "blastRadius" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(blast_radius_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                ))?)
            }
            "validationRecipe" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(validation_recipe_view_with(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                ))?)
            }
            "specFor" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(symbol_views_for_ids(
                    self.prism.as_ref(),
                    self.prism.spec_for(&id),
                )?)?)
            }
            "implementationFor" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(symbol_views_for_ids(
                    self.prism.as_ref(),
                    self.prism.implementation_for(&id),
                )?)?)
            }
            "driftCandidates" => {
                let args: LimitArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .drift_candidates(args.limit.unwrap_or(10))
                        .into_iter()
                        .map(drift_candidate_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "resumeTask" => {
                let args: TaskTargetArgs = serde_json::from_value(args)?;
                serde_json::to_value(self.prism.resume_task(&args.task_id)).map_err(Into::into)
            }
            "taskJournal" => {
                let args: TaskJournalArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.task_journal(args)?)?)
            }
            "memoryRecall" => {
                let args: MemoryRecallArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.memory_recall(args)?)?)
            }
            "memoryOutcomes" => {
                let args: MemoryOutcomeArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.memory_outcomes(args)?)?)
            }
            "curatorJobs" => {
                let args: CuratorJobsArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.curator_jobs(args)?)?)
            }
            "curatorJob" => {
                let args: CuratorJobArgs = serde_json::from_value(args)?;
                let job = self.host.curator_job(&args.job_id)?;
                if job.is_none() {
                    self.push_diagnostic(
                        "anchor_unresolved",
                        format!("No curator job matched `{}`.", args.job_id),
                        Some(json!({ "jobId": args.job_id })),
                    );
                }
                Ok(serde_json::to_value(job)?)
            }
            "diagnostics" => Ok(serde_json::to_value(self.diagnostics())?),
            other => {
                self.push_diagnostic(
                    "unknown_method",
                    format!("Unknown Prism host operation `{other}`."),
                    Some(json!({ "operation": other })),
                );
                Err(anyhow!("unsupported host operation `{other}`"))
            }
        }
    }

    fn ensure_operation_enabled(&self, operation: &str) -> Result<()> {
        if let Some(group) = self.host.features.disabled_query_group(operation) {
            return Err(anyhow!(
                "coordination {group} queries are disabled by the PRISM MCP server feature flags"
            ));
        }
        Ok(())
    }

    pub(crate) fn best_symbol(&self, query: &str) -> Result<Option<SymbolView>> {
        let matches = self.symbols(query)?;
        if matches.is_empty() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No symbol matched `{query}`."),
                Some(json!({ "query": query })),
            );
            return Ok(None);
        }
        if matches.len() > 1 {
            self.push_diagnostic(
                "ambiguous_symbol",
                format!(
                    "`{query}` matched {} symbols; returning the first best match.",
                    matches.len()
                ),
                Some(json!({
                    "query": query,
                    "matches": matches
                        .iter()
                        .map(|symbol| symbol.id.path.to_string())
                        .collect::<Vec<_>>(),
                })),
            );
        }
        Ok(matches.into_iter().next())
    }

    pub(crate) fn search(&self, args: SearchArgs) -> Result<Vec<SymbolView>> {
        let _include_inferred = args.include_inferred.unwrap_or(true);
        let kind = args.kind.as_deref().map(parse_node_kind).transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);

        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Search limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut results = self
            .prism
            .search(
                &args.query,
                applied.saturating_add(1),
                kind,
                args.path.as_deref(),
            )
            .iter()
            .map(|symbol| symbol_view(self.prism.as_ref(), symbol))
            .collect::<Result<Vec<_>>>()?;

        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Search results for `{}` were truncated at {} entries.",
                    args.query, applied
                ),
                Some(json!({
                    "query": args.query,
                    "applied": applied,
                })),
            );
        }

        Ok(results)
    }

    pub(crate) fn entrypoints(&self) -> Result<Vec<SymbolView>> {
        let limits = self.host.session.limits();
        let mut results = self.symbols_from(self.prism.entrypoints())?;
        if results.len() > limits.max_result_nodes {
            results.truncate(limits.max_result_nodes);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Entrypoints were truncated at {} entries.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "applied": limits.max_result_nodes,
                })),
            );
        }
        Ok(results)
    }

    fn call_graph(&self, args: CallGraphArgs) -> Result<SubgraphView> {
        let limits = self.host.session.limits();
        let id = convert_node_id(args.id)?;
        let requested = args.depth.unwrap_or(DEFAULT_CALL_GRAPH_DEPTH);
        let applied = requested.min(limits.max_call_graph_depth);
        if requested > limits.max_call_graph_depth {
            self.push_diagnostic(
                "depth_limited",
                format!(
                    "Call-graph depth was capped at {} instead of {requested}.",
                    limits.max_call_graph_depth
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }
        let mut graph = symbol_for(self.prism.as_ref(), &id)?.call_graph(applied);
        let mut queue = vec![(id.clone(), 0usize)];
        let mut seen = std::collections::HashSet::from([id.clone()]);

        while let Some((current, depth)) = queue.pop() {
            if depth >= applied {
                continue;
            }
            for record in self
                .host
                .session
                .inferred_edges
                .edges_from(&current, Some(EdgeKind::Calls))
            {
                graph.edges.push(record.edge.clone());
                graph.nodes.push(record.edge.target.clone());
                if seen.insert(record.edge.target.clone()) {
                    queue.push((record.edge.target, depth + 1));
                }
            }
        }

        graph.nodes = merge_node_ids(graph.nodes, std::iter::empty());
        graph.edges.sort_by(|left, right| {
            left.source
                .path
                .cmp(&right.source.path)
                .then_with(|| left.target.path.cmp(&right.target.path))
                .then_with(|| edge_kind_label(left.kind).cmp(edge_kind_label(right.kind)))
        });
        graph.edges.dedup_by(|left, right| {
            left.kind == right.kind && left.source == right.source && left.target == right.target
        });
        if graph.nodes.len() > limits.max_result_nodes {
            let keep = graph
                .nodes
                .iter()
                .take(limits.max_result_nodes)
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            graph.nodes.truncate(limits.max_result_nodes);
            graph
                .edges
                .retain(|edge| keep.contains(&edge.source) && keep.contains(&edge.target));
            graph.truncated = true;
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Call graph for `{}` was truncated at {} nodes.",
                    id.path, limits.max_result_nodes
                ),
                Some(json!({
                    "query": id.path,
                    "applied": limits.max_result_nodes,
                })),
            );
        }
        graph.max_depth_reached = Some(applied);
        Ok(SubgraphView {
            nodes: symbol_views_for_ids(self.prism.as_ref(), graph.nodes)?,
            edges: graph.edges.into_iter().map(edge_view).collect(),
            truncated: graph.truncated,
            max_depth_reached: graph.max_depth_reached,
        })
    }

    fn memory_recall(&self, args: MemoryRecallArgs) -> Result<Vec<ScoredMemoryView>> {
        let requested = args.limit.unwrap_or(5);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Memory recall limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut focus = Vec::new();
        if let Some(ids) = args.focus {
            for id in ids {
                focus.push(AnchorRef::Node(convert_node_id(id)?));
            }
        }
        let focus = self.prism.anchors_for(&focus);
        let kinds = args
            .kinds
            .map(|kinds| {
                kinds
                    .into_iter()
                    .map(|kind| parse_memory_kind(&kind))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let results = self
            .host
            .session
            .notes
            .recall(&RecallQuery {
                focus,
                text: args.text,
                limit: applied,
                kinds,
                since: args.since,
            })?
            .into_iter()
            .map(scored_memory_view)
            .collect();
        Ok(results)
    }

    fn task_journal(&self, args: TaskJournalArgs) -> Result<prism_js::TaskJournalView> {
        let event_requested = args
            .event_limit
            .unwrap_or(DEFAULT_TASK_JOURNAL_EVENT_LIMIT);
        let memory_requested = args
            .memory_limit
            .unwrap_or(DEFAULT_TASK_JOURNAL_MEMORY_LIMIT);
        let limits = self.host.session.limits();
        let event_limit = event_requested.min(limits.max_result_nodes);
        let memory_limit = memory_requested.min(limits.max_result_nodes);

        if event_requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Task journal event limit was capped at {} instead of {event_requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": event_requested,
                    "applied": event_limit,
                    "field": "eventLimit",
                })),
            );
        }
        if memory_requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Task journal memory limit was capped at {} instead of {memory_requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": memory_requested,
                    "applied": memory_limit,
                    "field": "memoryLimit",
                })),
            );
        }

        let journal = task_journal_view(
            self.host.session.as_ref(),
            self.prism.as_ref(),
            &args.task_id,
            None,
            event_limit,
            memory_limit,
        )?;
        for diagnostic in &journal.diagnostics {
            self.push_diagnostic(
                &diagnostic.code,
                diagnostic.message.clone(),
                diagnostic.data.clone(),
            );
        }
        Ok(journal)
    }

    fn memory_outcomes(&self, args: MemoryOutcomeArgs) -> Result<Vec<prism_memory::OutcomeEvent>> {
        let requested = args.limit.unwrap_or(10);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Memory outcome query limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut focus = Vec::new();
        if let Some(ids) = args.focus {
            for id in ids {
                focus.push(AnchorRef::Node(convert_node_id(id)?));
            }
        }

        let kinds = args
            .kinds
            .map(|kinds| {
                kinds
                    .into_iter()
                    .map(|kind| parse_outcome_kind(&kind))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let result = args
            .result
            .as_deref()
            .map(parse_outcome_result)
            .transpose()?;
        let actor = args.actor.as_deref().map(parse_event_actor).transpose()?;

        Ok(self.prism.query_outcomes(&OutcomeRecallQuery {
            anchors: focus,
            task: args.task_id,
            kinds,
            result,
            actor,
            since: args.since,
            limit: applied,
        }))
    }

    fn symbols(&self, query: &str) -> Result<Vec<SymbolView>> {
        self.symbols_from(self.prism.symbol(query))
    }

    fn symbols_from<'a, I>(&self, symbols: I) -> Result<Vec<SymbolView>>
    where
        I: IntoIterator<Item = Symbol<'a>>,
    {
        symbols
            .into_iter()
            .map(|symbol| symbol_view(self.prism.as_ref(), &symbol))
            .collect()
    }
}
