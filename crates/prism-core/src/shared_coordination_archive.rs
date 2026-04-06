use std::collections::BTreeMap;

use prism_coordination::{
    Artifact, ArtifactReview, CoordinationEvent, CoordinationSnapshot, CoordinationTask, Plan,
    RuntimeDescriptor, WorkClaim,
};
use prism_ir::{ArtifactStatus, ClaimStatus, CoordinationTaskStatus, PlanStatus};
use serde::{Deserialize, Serialize};

#[cfg(not(test))]
const SHARED_COORDINATION_HOT_TERMINAL_PLAN_LIMIT: usize = 16;
#[cfg(test)]
const SHARED_COORDINATION_HOT_TERMINAL_PLAN_LIMIT: usize = 2;
#[cfg(not(test))]
const SHARED_COORDINATION_HOT_TERMINAL_TASK_LIMIT: usize = 64;
#[cfg(test)]
const SHARED_COORDINATION_HOT_TERMINAL_TASK_LIMIT: usize = 2;
#[cfg(not(test))]
const SHARED_COORDINATION_HOT_TERMINAL_CLAIM_LIMIT: usize = 64;
#[cfg(test)]
const SHARED_COORDINATION_HOT_TERMINAL_CLAIM_LIMIT: usize = 2;
#[cfg(not(test))]
const SHARED_COORDINATION_HOT_STALE_RUNTIME_LIMIT: usize = 16;
#[cfg(test)]
const SHARED_COORDINATION_HOT_STALE_RUNTIME_LIMIT: usize = 1;
#[cfg(not(test))]
const SHARED_COORDINATION_HOT_RUNTIME_STALE_AFTER_SECS: u64 = 60 * 60;
#[cfg(test)]
const SHARED_COORDINATION_HOT_RUNTIME_STALE_AFTER_SECS: u64 = 60;
#[cfg(not(test))]
const SHARED_COORDINATION_HOT_EVENT_LIMIT: usize = 128;
#[cfg(test)]
const SHARED_COORDINATION_HOT_EVENT_LIMIT: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedCoordinationArchiveSummary {
    pub archived_plan_count: usize,
    pub archived_task_count: usize,
    pub archived_claim_count: usize,
    pub archived_artifact_count: usize,
    pub archived_review_count: usize,
    pub archived_event_count: usize,
    pub archived_runtime_descriptor_count: usize,
}

impl SharedCoordinationArchiveSummary {
    pub(crate) fn has_archived_records(&self) -> bool {
        self.archived_plan_count > 0
            || self.archived_task_count > 0
            || self.archived_claim_count > 0
            || self.archived_artifact_count > 0
            || self.archived_review_count > 0
            || self.archived_event_count > 0
            || self.archived_runtime_descriptor_count > 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SharedCoordinationArchivePartition {
    pub(crate) hot_snapshot: CoordinationSnapshot,
    pub(crate) hot_runtime_descriptors: Vec<RuntimeDescriptor>,
    pub(crate) archive_snapshot: CoordinationSnapshot,
    pub(crate) archive_runtime_descriptors: Vec<RuntimeDescriptor>,
    pub(crate) summary: SharedCoordinationArchiveSummary,
}

pub(crate) fn partition_shared_coordination_hot_state(
    snapshot: &CoordinationSnapshot,
    runtime_descriptors: &[RuntimeDescriptor],
    now: u64,
) -> SharedCoordinationArchivePartition {
    let task_activity = task_activity_by_id(snapshot);
    let claim_activity = claim_activity_by_id(snapshot);
    let active_artifact_task_ids = snapshot
        .artifacts
        .iter()
        .filter(|artifact| is_hot_artifact_status(artifact.status))
        .map(|artifact| artifact.task.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    let nonterminal_plan_ids = snapshot
        .plans
        .iter()
        .filter(|plan| !is_terminal_plan_status(plan.status))
        .map(|plan| plan.id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    let mut hot_task_ids = snapshot
        .tasks
        .iter()
        .filter(|task| {
            !is_terminal_task_status(task.status)
                || nonterminal_plan_ids.contains(task.plan.0.as_str())
                || active_artifact_task_ids.contains(task.id.0.as_str())
        })
        .map(|task| task.id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    for task_id in recent_ids(
        snapshot
            .tasks
            .iter()
            .filter(|task| !hot_task_ids.contains(task.id.0.as_str())),
        |task| task.id.0.as_str(),
        |task| {
            task_activity
                .get(task.id.0.as_str())
                .copied()
                .unwrap_or_default()
        },
        SHARED_COORDINATION_HOT_TERMINAL_TASK_LIMIT,
    ) {
        hot_task_ids.insert(task_id);
    }

    let plan_activity = plan_activity_by_id(snapshot, &task_activity);
    let mut hot_plan_ids = snapshot
        .plans
        .iter()
        .filter(|plan| {
            !is_terminal_plan_status(plan.status)
                || snapshot
                    .tasks
                    .iter()
                    .any(|task| task.plan == plan.id && hot_task_ids.contains(task.id.0.as_str()))
        })
        .map(|plan| plan.id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    for plan_id in recent_ids(
        snapshot
            .plans
            .iter()
            .filter(|plan| !hot_plan_ids.contains(plan.id.0.as_str())),
        |plan| plan.id.0.as_str(),
        |plan| {
            plan_activity
                .get(plan.id.0.as_str())
                .copied()
                .unwrap_or_default()
        },
        SHARED_COORDINATION_HOT_TERMINAL_PLAN_LIMIT,
    ) {
        hot_plan_ids.insert(plan_id);
    }
    for task in &snapshot.tasks {
        if hot_task_ids.contains(task.id.0.as_str()) {
            hot_plan_ids.insert(task.plan.0.to_string());
        }
    }
    for task in &snapshot.tasks {
        if hot_plan_ids.contains(task.plan.0.as_str()) {
            hot_task_ids.insert(task.id.0.to_string());
        }
    }

    let mut hot_claim_ids = snapshot
        .claims
        .iter()
        .filter(|claim| {
            !is_terminal_claim_status(claim.status)
                || claim
                    .task
                    .as_ref()
                    .is_some_and(|task| hot_task_ids.contains(task.0.as_str()))
        })
        .map(|claim| claim.id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for claim_id in recent_ids(
        snapshot
            .claims
            .iter()
            .filter(|claim| !hot_claim_ids.contains(claim.id.0.as_str())),
        |claim| claim.id.0.as_str(),
        |claim| {
            claim_activity
                .get(claim.id.0.as_str())
                .copied()
                .unwrap_or_default()
        },
        SHARED_COORDINATION_HOT_TERMINAL_CLAIM_LIMIT,
    ) {
        hot_claim_ids.insert(claim_id);
    }

    let mut hot_runtime_ids = runtime_descriptors
        .iter()
        .filter(|descriptor| {
            descriptor
                .last_seen_at
                .saturating_add(SHARED_COORDINATION_HOT_RUNTIME_STALE_AFTER_SECS)
                >= now
        })
        .map(|descriptor| descriptor.runtime_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for runtime_id in recent_ids(
        runtime_descriptors
            .iter()
            .filter(|descriptor| !hot_runtime_ids.contains(descriptor.runtime_id.as_str())),
        |descriptor| descriptor.runtime_id.as_str(),
        |descriptor| descriptor.last_seen_at,
        SHARED_COORDINATION_HOT_STALE_RUNTIME_LIMIT,
    ) {
        hot_runtime_ids.insert(runtime_id);
    }

    let hot_artifact_ids = snapshot
        .artifacts
        .iter()
        .filter(|artifact| {
            hot_task_ids.contains(artifact.task.0.as_str())
                || is_hot_artifact_status(artifact.status)
        })
        .map(|artifact| artifact.id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    let hot_review_ids = snapshot
        .reviews
        .iter()
        .filter(|review| hot_artifact_ids.contains(review.artifact.0.as_str()))
        .map(|review| review.id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    let recent_event_ids = recent_ids(
        snapshot.events.iter(),
        |event| event.meta.id.0.as_str(),
        |event| event.meta.ts,
        SHARED_COORDINATION_HOT_EVENT_LIMIT,
    );
    let hot_event_ids = snapshot
        .events
        .iter()
        .filter(|event| {
            recent_event_ids.contains(event.meta.id.0.as_str())
                || event
                    .plan
                    .as_ref()
                    .is_some_and(|plan| hot_plan_ids.contains(plan.0.as_str()))
                || event
                    .task
                    .as_ref()
                    .is_some_and(|task| hot_task_ids.contains(task.0.as_str()))
                || event
                    .claim
                    .as_ref()
                    .is_some_and(|claim| hot_claim_ids.contains(claim.0.as_str()))
                || event
                    .artifact
                    .as_ref()
                    .is_some_and(|artifact| hot_artifact_ids.contains(artifact.0.as_str()))
                || event
                    .review
                    .as_ref()
                    .is_some_and(|review| hot_review_ids.contains(review.0.as_str()))
        })
        .map(|event| event.meta.id.0.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    let hot_snapshot = CoordinationSnapshot {
        plans: sort_plans(select_records(snapshot.plans.iter(), |plan| {
            hot_plan_ids.contains(plan.id.0.as_str())
        })),
        tasks: sort_tasks(select_records(snapshot.tasks.iter(), |task| {
            hot_task_ids.contains(task.id.0.as_str())
        })),
        claims: sort_claims(select_records(snapshot.claims.iter(), |claim| {
            hot_claim_ids.contains(claim.id.0.as_str())
        })),
        artifacts: sort_artifacts(select_records(snapshot.artifacts.iter(), |artifact| {
            hot_artifact_ids.contains(artifact.id.0.as_str())
        })),
        reviews: sort_reviews(select_records(snapshot.reviews.iter(), |review| {
            hot_review_ids.contains(review.id.0.as_str())
        })),
        events: sort_events(select_records(snapshot.events.iter(), |event| {
            hot_event_ids.contains(event.meta.id.0.as_str())
        })),
        next_plan: snapshot.next_plan,
        next_task: snapshot.next_task,
        next_claim: snapshot.next_claim,
        next_artifact: snapshot.next_artifact,
        next_review: snapshot.next_review,
    };

    let archive_snapshot = CoordinationSnapshot {
        plans: sort_plans(select_records(snapshot.plans.iter(), |plan| {
            !hot_plan_ids.contains(plan.id.0.as_str())
        })),
        tasks: sort_tasks(select_records(snapshot.tasks.iter(), |task| {
            !hot_task_ids.contains(task.id.0.as_str())
        })),
        claims: sort_claims(select_records(snapshot.claims.iter(), |claim| {
            !hot_claim_ids.contains(claim.id.0.as_str())
        })),
        artifacts: sort_artifacts(select_records(snapshot.artifacts.iter(), |artifact| {
            !hot_artifact_ids.contains(artifact.id.0.as_str())
        })),
        reviews: sort_reviews(select_records(snapshot.reviews.iter(), |review| {
            !hot_review_ids.contains(review.id.0.as_str())
        })),
        events: sort_events(select_records(snapshot.events.iter(), |event| {
            !hot_event_ids.contains(event.meta.id.0.as_str())
        })),
        next_plan: snapshot.next_plan,
        next_task: snapshot.next_task,
        next_claim: snapshot.next_claim,
        next_artifact: snapshot.next_artifact,
        next_review: snapshot.next_review,
    };

    let hot_runtime_descriptors =
        sort_runtime_descriptors(select_records(runtime_descriptors.iter(), |descriptor| {
            hot_runtime_ids.contains(descriptor.runtime_id.as_str())
        }));
    let archive_runtime_descriptors =
        sort_runtime_descriptors(select_records(runtime_descriptors.iter(), |descriptor| {
            !hot_runtime_ids.contains(descriptor.runtime_id.as_str())
        }));

    let summary = SharedCoordinationArchiveSummary {
        archived_plan_count: archive_snapshot.plans.len(),
        archived_task_count: archive_snapshot.tasks.len(),
        archived_claim_count: archive_snapshot.claims.len(),
        archived_artifact_count: archive_snapshot.artifacts.len(),
        archived_review_count: archive_snapshot.reviews.len(),
        archived_event_count: archive_snapshot.events.len(),
        archived_runtime_descriptor_count: archive_runtime_descriptors.len(),
    };

    SharedCoordinationArchivePartition {
        hot_snapshot,
        hot_runtime_descriptors,
        archive_snapshot,
        archive_runtime_descriptors,
        summary,
    }
}

fn recent_ids<'a, T, I, FId, FTs>(
    items: I,
    id_for: FId,
    ts_for: FTs,
    limit: usize,
) -> std::collections::BTreeSet<String>
where
    I: IntoIterator<Item = &'a T>,
    T: 'a,
    FId: Fn(&T) -> &str,
    FTs: Fn(&T) -> u64,
{
    let mut ranked = items
        .into_iter()
        .map(|item| (ts_for(item), id_for(item).to_string()))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked.into_iter().take(limit).map(|(_, id)| id).collect()
}

fn task_activity_by_id(snapshot: &CoordinationSnapshot) -> BTreeMap<String, u64> {
    let mut activity = BTreeMap::new();
    for task in &snapshot.tasks {
        let mut ts = 0;
        for candidate in [
            task.lease_started_at,
            task.lease_refreshed_at,
            task.lease_stale_at,
            task.lease_expires_at,
            task.git_execution
                .last_preflight
                .as_ref()
                .map(|preflight| preflight.checked_at),
        ]
        .into_iter()
        .flatten()
        {
            ts = ts.max(candidate);
        }
        activity.insert(task.id.0.to_string(), ts);
    }
    for event in &snapshot.events {
        if let Some(task) = &event.task {
            activity
                .entry(task.0.to_string())
                .and_modify(|ts| *ts = (*ts).max(event.meta.ts))
                .or_insert(event.meta.ts);
        }
    }
    activity
}

fn claim_activity_by_id(snapshot: &CoordinationSnapshot) -> BTreeMap<String, u64> {
    let mut activity = BTreeMap::new();
    for claim in &snapshot.claims {
        let mut ts = claim.since.max(claim.expires_at);
        for candidate in [claim.refreshed_at, claim.stale_at].into_iter().flatten() {
            ts = ts.max(candidate);
        }
        activity.insert(claim.id.0.to_string(), ts);
    }
    for event in &snapshot.events {
        if let Some(claim) = &event.claim {
            activity
                .entry(claim.0.to_string())
                .and_modify(|ts| *ts = (*ts).max(event.meta.ts))
                .or_insert(event.meta.ts);
        }
    }
    activity
}

fn plan_activity_by_id(
    snapshot: &CoordinationSnapshot,
    task_activity: &BTreeMap<String, u64>,
) -> BTreeMap<String, u64> {
    let mut activity = BTreeMap::new();
    for plan in &snapshot.plans {
        activity.insert(plan.id.0.to_string(), 0);
    }
    for task in &snapshot.tasks {
        if let Some(ts) = task_activity.get(task.id.0.as_str()) {
            activity
                .entry(task.plan.0.to_string())
                .and_modify(|current| *current = (*current).max(*ts))
                .or_insert(*ts);
        }
    }
    for event in &snapshot.events {
        if let Some(plan) = &event.plan {
            activity
                .entry(plan.0.to_string())
                .and_modify(|ts| *ts = (*ts).max(event.meta.ts))
                .or_insert(event.meta.ts);
        }
    }
    activity
}

fn select_records<'a, T: Clone, I, F>(items: I, include: F) -> Vec<T>
where
    I: IntoIterator<Item = &'a T>,
    F: Fn(&T) -> bool,
    T: 'a,
{
    items
        .into_iter()
        .filter(|item| include(item))
        .cloned()
        .collect()
}

fn is_terminal_plan_status(status: PlanStatus) -> bool {
    matches!(
        status,
        PlanStatus::Completed | PlanStatus::Abandoned | PlanStatus::Archived
    )
}

fn is_terminal_task_status(status: CoordinationTaskStatus) -> bool {
    matches!(
        status,
        CoordinationTaskStatus::Completed | CoordinationTaskStatus::Abandoned
    )
}

fn is_terminal_claim_status(status: ClaimStatus) -> bool {
    matches!(status, ClaimStatus::Released | ClaimStatus::Expired)
}

fn is_hot_artifact_status(status: ArtifactStatus) -> bool {
    matches!(status, ArtifactStatus::Proposed | ArtifactStatus::InReview)
}

fn sort_plans(mut plans: Vec<Plan>) -> Vec<Plan> {
    plans.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    plans
}

fn sort_tasks(mut tasks: Vec<CoordinationTask>) -> Vec<CoordinationTask> {
    tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    tasks
}

fn sort_claims(mut claims: Vec<WorkClaim>) -> Vec<WorkClaim> {
    claims.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    claims
}

fn sort_artifacts(mut artifacts: Vec<Artifact>) -> Vec<Artifact> {
    artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    artifacts
}

fn sort_reviews(mut reviews: Vec<ArtifactReview>) -> Vec<ArtifactReview> {
    reviews.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    reviews
}

fn sort_events(mut events: Vec<CoordinationEvent>) -> Vec<CoordinationEvent> {
    events.sort_by(|left, right| left.meta.id.0.cmp(&right.meta.id.0));
    events
}

fn sort_runtime_descriptors(mut descriptors: Vec<RuntimeDescriptor>) -> Vec<RuntimeDescriptor> {
    descriptors.sort_by(|left, right| {
        left.worktree_id
            .cmp(&right.worktree_id)
            .then_with(|| left.runtime_id.cmp(&right.runtime_id))
    });
    descriptors
}
