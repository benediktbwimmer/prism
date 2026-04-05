use std::collections::BTreeMap;

use prism_ir::{sortable_token_timestamp, CoordinationTaskId, PlanId};

use crate::{PlanActivity, Prism};

impl Prism {
    pub fn plan_activity(&self, plan_id: &PlanId) -> Option<PlanActivity> {
        self.plan_activity_index().remove(plan_id.0.as_str())
    }

    pub(crate) fn plan_activity_index(&self) -> BTreeMap<String, PlanActivity> {
        let snapshot = self.coordination_snapshot();
        let mut fallback_last_updated =
            BTreeMap::<String, (u64, Option<CoordinationTaskId>)>::new();
        let mut activity = snapshot
            .plans
            .iter()
            .map(|plan| {
                let mut entry = PlanActivity::default();
                observe_created_at(&mut entry, sortable_token_timestamp(plan.id.0.as_str()));
                observe_fallback_update(
                    &mut fallback_last_updated,
                    plan.id.0.as_str(),
                    sortable_token_timestamp(plan.id.0.as_str()),
                    None,
                );
                (plan.id.0.to_string(), entry)
            })
            .collect::<BTreeMap<_, _>>();
        let task_to_plan = snapshot
            .tasks
            .iter()
            .map(|task| (task.id.clone(), task.plan.clone()))
            .collect::<BTreeMap<_, _>>();
        let artifact_to_task = snapshot
            .artifacts
            .iter()
            .map(|artifact| (artifact.id.clone(), artifact.task.clone()))
            .collect::<BTreeMap<_, _>>();
        let claim_to_plan = snapshot
            .claims
            .iter()
            .filter_map(|claim| {
                let task_id = claim.task.as_ref()?;
                let plan_id = task_to_plan.get(task_id)?;
                Some((claim.id.clone(), plan_id.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let review_to_plan = snapshot
            .reviews
            .iter()
            .filter_map(|review| {
                let task_id = artifact_to_task.get(&review.artifact)?;
                let plan_id = task_to_plan.get(task_id)?;
                Some((review.id.clone(), plan_id.clone()))
            })
            .collect::<BTreeMap<_, _>>();

        for task in &snapshot.tasks {
            let Some(entry) = activity.get_mut(task.plan.0.as_str()) else {
                continue;
            };
            let created_at = sortable_token_timestamp(task.id.0.as_str());
            observe_created_at(entry, created_at);
            observe_fallback_update(
                &mut fallback_last_updated,
                task.plan.0.as_str(),
                created_at,
                Some(&task.id),
            );
            observe_fallback_update(
                &mut fallback_last_updated,
                task.plan.0.as_str(),
                task.lease_started_at,
                Some(&task.id),
            );
            observe_fallback_update(
                &mut fallback_last_updated,
                task.plan.0.as_str(),
                task.lease_refreshed_at,
                Some(&task.id),
            );
        }

        for claim in &snapshot.claims {
            let Some(task_id) = claim.task.as_ref() else {
                continue;
            };
            let Some(plan_id) = task_to_plan.get(task_id) else {
                continue;
            };
            let Some(entry) = activity.get_mut(plan_id.0.as_str()) else {
                continue;
            };
            let created_at = sortable_token_timestamp(claim.id.0.as_str()).or(Some(claim.since));
            observe_created_at(entry, created_at);
            observe_fallback_update(
                &mut fallback_last_updated,
                plan_id.0.as_str(),
                sortable_token_timestamp(claim.id.0.as_str()),
                Some(task_id),
            );
            observe_fallback_update(
                &mut fallback_last_updated,
                plan_id.0.as_str(),
                Some(claim.since),
                Some(task_id),
            );
            observe_fallback_update(
                &mut fallback_last_updated,
                plan_id.0.as_str(),
                claim.refreshed_at,
                Some(task_id),
            );
        }

        for artifact in &snapshot.artifacts {
            let Some(plan_id) = task_to_plan.get(&artifact.task) else {
                continue;
            };
            let Some(entry) = activity.get_mut(plan_id.0.as_str()) else {
                continue;
            };
            let created_at = sortable_token_timestamp(artifact.id.0.as_str());
            observe_created_at(entry, created_at);
            observe_fallback_update(
                &mut fallback_last_updated,
                plan_id.0.as_str(),
                created_at,
                Some(&artifact.task),
            );
        }

        for review in &snapshot.reviews {
            let Some(task_id) = artifact_to_task.get(&review.artifact) else {
                continue;
            };
            let Some(plan_id) = task_to_plan.get(task_id) else {
                continue;
            };
            let Some(entry) = activity.get_mut(plan_id.0.as_str()) else {
                continue;
            };
            let created_at =
                sortable_token_timestamp(review.id.0.as_str()).or(Some(review.meta.ts));
            observe_created_at(entry, created_at);
            observe_fallback_update(
                &mut fallback_last_updated,
                plan_id.0.as_str(),
                sortable_token_timestamp(review.id.0.as_str()),
                Some(task_id),
            );
            observe_fallback_update(
                &mut fallback_last_updated,
                plan_id.0.as_str(),
                Some(review.meta.ts),
                Some(task_id),
            );
        }

        for event in snapshot.events {
            let plan_id = event
                .plan
                .clone()
                .or_else(|| {
                    event
                        .task
                        .as_ref()
                        .and_then(|task_id| task_to_plan.get(task_id).cloned())
                })
                .or_else(|| {
                    event
                        .claim
                        .as_ref()
                        .and_then(|claim_id| claim_to_plan.get(claim_id).cloned())
                })
                .or_else(|| {
                    event.artifact.as_ref().and_then(|artifact_id| {
                        artifact_to_task
                            .get(artifact_id)
                            .and_then(|task_id| task_to_plan.get(task_id))
                            .cloned()
                    })
                })
                .or_else(|| {
                    event
                        .review
                        .as_ref()
                        .and_then(|review_id| review_to_plan.get(review_id).cloned())
                });
            let Some(plan_id) = plan_id else {
                continue;
            };
            let entry = activity.entry(plan_id.0.to_string()).or_default();
            entry.created_at = Some(match entry.created_at {
                Some(existing) => existing.min(event.meta.ts),
                None => event.meta.ts,
            });
            let replace_last = match entry.last_updated_at {
                Some(existing) => event.meta.ts >= existing,
                None => true,
            };
            if replace_last {
                entry.last_updated_at = Some(event.meta.ts);
                entry.last_event_kind = Some(event.kind);
                entry.last_event_summary = Some(event.summary);
                entry.last_event_task_id = event.task;
            }
        }

        for (plan_id, (ts, task_id)) in fallback_last_updated {
            let Some(entry) = activity.get_mut(plan_id.as_str()) else {
                continue;
            };
            if entry.last_updated_at.is_none() {
                entry.last_updated_at = Some(ts);
                entry.last_event_kind = None;
                entry.last_event_summary = None;
                entry.last_event_task_id = task_id;
            }
        }

        activity
    }
}

fn observe_created_at(entry: &mut PlanActivity, ts: Option<u64>) {
    let Some(ts) = ts else {
        return;
    };
    entry.created_at = Some(match entry.created_at {
        Some(existing) => existing.min(ts),
        None => ts,
    });
}

fn observe_fallback_update(
    fallback: &mut BTreeMap<String, (u64, Option<CoordinationTaskId>)>,
    plan_id: &str,
    ts: Option<u64>,
    task_id: Option<&CoordinationTaskId>,
) {
    let Some(ts) = ts else {
        return;
    };
    let replace = fallback
        .get(plan_id)
        .is_none_or(|(existing, _)| ts > *existing);
    if replace {
        fallback.insert(plan_id.to_string(), (ts, task_id.cloned()));
    }
}
