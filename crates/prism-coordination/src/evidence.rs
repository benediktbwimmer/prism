use std::collections::BTreeSet;

use anyhow::{anyhow, Result};
use prism_ir::{ArtifactStatus, EventActor, PrincipalKind};

use crate::state::CoordinationState;
use crate::types::{
    Artifact, ArtifactRequirement, ArtifactReview, CoordinationTask, ReviewRequirement,
    ReviewerClass,
};

pub(crate) const LEGACY_ARTIFACT_REQUIREMENT_ID: &str = "__legacy_task_artifact__";
pub(crate) const LEGACY_REVIEW_REQUIREMENT_ID: &str = "__legacy_artifact_review__";

pub(crate) fn normalize_artifact_requirements(
    requirements: Vec<ArtifactRequirement>,
) -> Result<Vec<ArtifactRequirement>> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::with_capacity(requirements.len());
    for mut requirement in requirements {
        let id = requirement.client_artifact_requirement_id.trim().to_string();
        if id.is_empty() {
            return Err(anyhow!(
                "artifact requirements must include a non-empty client_artifact_requirement_id"
            ));
        }
        if !seen.insert(id.clone()) {
            return Err(anyhow!("duplicate artifact requirement `{id}`"));
        }
        requirement.client_artifact_requirement_id = id;
        requirement.evidence_types.sort();
        requirement.evidence_types.dedup();
        requirement.required_validations.sort();
        requirement.required_validations.dedup();
        normalized.push(requirement);
    }
    Ok(normalized)
}

pub(crate) fn normalize_review_requirements(
    artifact_requirements: &[ArtifactRequirement],
    review_requirements: Vec<ReviewRequirement>,
) -> Result<Vec<ReviewRequirement>> {
    let artifact_ids = artifact_requirements
        .iter()
        .map(|requirement| requirement.client_artifact_requirement_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::with_capacity(review_requirements.len());
    for mut requirement in review_requirements {
        let id = requirement.client_review_requirement_id.trim().to_string();
        if id.is_empty() {
            return Err(anyhow!(
                "review requirements must include a non-empty client_review_requirement_id"
            ));
        }
        if !seen.insert(id.clone()) {
            return Err(anyhow!("duplicate review requirement `{id}`"));
        }
        let artifact_ref = requirement.artifact_requirement_ref.trim().to_string();
        if artifact_ref.is_empty() {
            return Err(anyhow!(
                "review requirement `{id}` must reference an artifact requirement"
            ));
        }
        if !artifact_ids.contains(artifact_ref.as_str()) {
            return Err(anyhow!(
                "review requirement `{id}` references unknown artifact requirement `{artifact_ref}`"
            ));
        }
        requirement.client_review_requirement_id = id;
        requirement.artifact_requirement_ref = artifact_ref;
        requirement.allowed_reviewer_classes.sort();
        requirement.allowed_reviewer_classes.dedup();
        normalized.push(requirement);
    }
    Ok(normalized)
}

pub(crate) fn resolve_artifact_requirement_id(
    task: &CoordinationTask,
    artifact_requirement_id: Option<&str>,
) -> Result<String> {
    match artifact_requirement_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => {
            if task
                .artifact_requirements
                .iter()
                .any(|requirement| requirement.client_artifact_requirement_id == id)
            {
                Ok(id.to_string())
            } else {
                Err(anyhow!(
                    "task `{}` does not declare artifact requirement `{id}`",
                    task.id.0
                ))
            }
        }
        None if task.artifact_requirements.is_empty() => Ok(LEGACY_ARTIFACT_REQUIREMENT_ID.to_string()),
        None if task.artifact_requirements.len() == 1 => Ok(task.artifact_requirements[0]
            .client_artifact_requirement_id
            .clone()),
        None => Err(anyhow!(
            "task `{}` has multiple artifact requirements; artifact proposals must target one explicitly",
            task.id.0
        )),
    }
}

pub(crate) fn resolve_review_requirement_id(
    task: &CoordinationTask,
    artifact_requirement_id: &str,
    review_requirement_id: Option<&str>,
) -> Result<String> {
    match review_requirement_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => {
            let Some(requirement) = task
                .review_requirements
                .iter()
                .find(|requirement| requirement.client_review_requirement_id == id)
            else {
                return Err(anyhow!(
                    "task `{}` does not declare review requirement `{id}`",
                    task.id.0
                ));
            };
            if requirement.artifact_requirement_ref != artifact_requirement_id {
                return Err(anyhow!(
                    "review requirement `{id}` does not target artifact requirement `{artifact_requirement_id}`"
                ));
            }
            Ok(id.to_string())
        }
        None if task.review_requirements.is_empty() => Ok(LEGACY_REVIEW_REQUIREMENT_ID.to_string()),
        None => {
            let matches = task
                .review_requirements
                .iter()
                .filter(|requirement| requirement.artifact_requirement_ref == artifact_requirement_id)
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [requirement] => Ok(requirement.client_review_requirement_id.clone()),
                [] => Err(anyhow!(
                    "task `{}` does not declare a review requirement for artifact requirement `{artifact_requirement_id}`",
                    task.id.0
                )),
                _ => Err(anyhow!(
                    "task `{}` has multiple review requirements for artifact requirement `{artifact_requirement_id}`; reviews must target one explicitly",
                    task.id.0
                )),
            }
        }
    }
}

pub(crate) fn artifact_requirement_for_task<'a>(
    task: &'a CoordinationTask,
    artifact_requirement_id: &str,
) -> Option<&'a ArtifactRequirement> {
    task.artifact_requirements.iter().find(|requirement| {
        requirement.client_artifact_requirement_id == artifact_requirement_id
    })
}

pub(crate) fn review_requirement_for_task<'a>(
    task: &'a CoordinationTask,
    review_requirement_id: &str,
) -> Option<&'a ReviewRequirement> {
    task.review_requirements
        .iter()
        .find(|requirement| requirement.client_review_requirement_id == review_requirement_id)
}

pub(crate) fn artifacts_for_requirement_lineage<'a>(
    state: &'a CoordinationState,
    task: &CoordinationTask,
    artifact_requirement_id: &str,
) -> Vec<&'a Artifact> {
    let normalized_requirement_id = normalize_artifact_requirement_id_for_comparison(
        task,
        artifact_requirement_id,
    );
    let mut artifacts = state
        .artifacts
        .values()
        .filter(|artifact| artifact.task == task.id)
        .filter(|artifact| {
            normalize_artifact_requirement_id_for_comparison(task, &artifact.artifact_requirement_id)
                == normalized_requirement_id
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    artifacts
}

pub(crate) fn active_artifact_for_requirement<'a>(
    state: &'a CoordinationState,
    task: &CoordinationTask,
    artifact_requirement_id: &str,
) -> Option<&'a Artifact> {
    artifacts_for_requirement_lineage(state, task, artifact_requirement_id)
        .into_iter()
        .filter(|artifact| artifact.status != ArtifactStatus::Superseded)
        .max_by(|left, right| left.id.0.cmp(&right.id.0))
}

pub(crate) fn artifact_requirement_satisfied(
    state: &CoordinationState,
    task: &CoordinationTask,
    artifact_requirement_id: &str,
) -> bool {
    active_artifact_for_requirement(state, task, artifact_requirement_id).is_some()
}

pub(crate) fn review_requirement_satisfied(
    state: &CoordinationState,
    task: &CoordinationTask,
    review_requirement_id: &str,
) -> bool {
    let Some(review_requirement) = review_requirement_for_task(task, review_requirement_id) else {
        return false;
    };
    let Some(active_artifact) =
        active_artifact_for_requirement(state, task, &review_requirement.artifact_requirement_ref)
    else {
        return false;
    };
    if !matches!(active_artifact.status, ArtifactStatus::Approved | ArtifactStatus::Merged) {
        return false;
    }
    approved_reviews_for_requirement(state, active_artifact, review_requirement).len()
        >= usize::from(review_requirement.min_review_count.max(1))
}

pub(crate) fn unsatisfied_review_artifacts<'a>(
    state: &'a CoordinationState,
    task: &CoordinationTask,
) -> Vec<&'a Artifact> {
    let mut seen = BTreeSet::new();
    let mut artifacts = Vec::new();
    for requirement in &task.review_requirements {
        if review_requirement_satisfied(state, task, &requirement.client_review_requirement_id) {
            continue;
        }
        let Some(active_artifact) =
            active_artifact_for_requirement(state, task, &requirement.artifact_requirement_ref)
        else {
            continue;
        };
        if seen.insert(active_artifact.id.0.to_string()) {
            artifacts.push(active_artifact);
        }
    }
    artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    artifacts
}

pub(crate) fn reviewer_class(actor: &EventActor) -> ReviewerClass {
    match actor.canonical_identity_actor() {
        EventActor::User => ReviewerClass::Human,
        EventActor::Agent => ReviewerClass::Agent,
        EventActor::System => ReviewerClass::System,
        EventActor::CI => ReviewerClass::Ci,
        EventActor::GitAuthor { .. } => ReviewerClass::External,
        EventActor::Principal(principal) => match principal.kind.unwrap_or(PrincipalKind::External) {
            PrincipalKind::Human => ReviewerClass::Human,
            PrincipalKind::Service => ReviewerClass::Service,
            PrincipalKind::Agent => ReviewerClass::Agent,
            PrincipalKind::System => ReviewerClass::System,
            PrincipalKind::Ci => ReviewerClass::Ci,
            PrincipalKind::External => ReviewerClass::External,
        },
    }
}

pub(crate) fn reviewer_class_allowed(
    review_requirement: Option<&ReviewRequirement>,
    reviewer_class: ReviewerClass,
) -> bool {
    review_requirement.is_none_or(|requirement| {
        requirement.allowed_reviewer_classes.is_empty()
            || requirement
                .allowed_reviewer_classes
                .contains(&reviewer_class)
    })
}

fn approved_reviews_for_requirement<'a>(
    state: &'a CoordinationState,
    artifact: &Artifact,
    review_requirement: &ReviewRequirement,
) -> Vec<&'a ArtifactReview> {
    let mut reviews = state
        .reviews
        .values()
        .filter(|review| review.artifact == artifact.id)
        .filter(|review| {
            normalize_review_requirement_id_for_comparison(review.review_requirement_id.as_str())
                == review_requirement.client_review_requirement_id
        })
        .filter(|review| review.verdict == prism_ir::ReviewVerdict::Approved)
        .filter(|review| {
            reviewer_class_allowed(Some(review_requirement), review.reviewer_class.unwrap_or(ReviewerClass::External))
        })
        .collect::<Vec<_>>();
    reviews.sort_by(|left, right| {
        left.meta
            .ts
            .cmp(&right.meta.ts)
            .then_with(|| left.id.0.cmp(&right.id.0))
    });
    reviews
}

fn normalize_artifact_requirement_id_for_comparison(
    task: &CoordinationTask,
    artifact_requirement_id: &str,
) -> String {
    let trimmed = artifact_requirement_id.trim();
    if trimmed.is_empty() {
        if task.artifact_requirements.is_empty() {
            LEGACY_ARTIFACT_REQUIREMENT_ID.to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    }
}

fn normalize_review_requirement_id_for_comparison(review_requirement_id: &str) -> String {
    let trimmed = review_requirement_id.trim();
    if trimmed.is_empty() {
        LEGACY_REVIEW_REQUIREMENT_ID.to_string()
    } else {
        trimmed.to_string()
    }
}
