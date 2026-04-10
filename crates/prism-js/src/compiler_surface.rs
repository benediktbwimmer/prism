use crate::query_surface::{PrismApiMethodSpec, PrismRecordArgBundle, PrismSurfaceTypeRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrismCompilerEffectKind {
    CoordinationRead,
    CoordinationWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrismCompilerMethodSpec {
    pub api: PrismApiMethodSpec,
    pub effect: PrismCompilerEffectKind,
    pub host_operation: Option<&'static str>,
}

const WORK_DECLARE_KEYS: &[&str] = &[
    "title",
    "kind",
    "summary",
    "parentWorkId",
    "parent_work_id",
    "coordinationTaskId",
    "coordination_task_id",
    "planId",
    "plan_id",
];
const CLAIM_ACQUIRE_KEYS: &[&str] = &[
    "anchors",
    "capability",
    "mode",
    "ttlSeconds",
    "ttl_seconds",
    "agent",
    "coordinationTaskId",
    "coordination_task_id",
];
const CLAIM_RENEW_KEYS: &[&str] = &["ttlSeconds", "ttl_seconds"];
const ARTIFACT_PROPOSE_KEYS: &[&str] = &[
    "taskId",
    "task_id",
    "artifactRequirementId",
    "artifact_requirement_id",
    "anchors",
    "diffRef",
    "diff_ref",
    "evidence",
    "requiredValidations",
    "required_validations",
    "validatedChecks",
    "validated_checks",
    "riskScore",
    "risk_score",
];
const ARTIFACT_REVIEW_KEYS: &[&str] = &[
    "reviewRequirementId",
    "review_requirement_id",
    "verdict",
    "summary",
    "requiredValidations",
    "required_validations",
    "validatedChecks",
    "validated_checks",
    "riskScore",
    "risk_score",
];
const COORDINATION_CREATE_PLAN_KEYS: &[&str] = &["title", "goal", "status", "policy", "scheduling"];
const PLAN_UPDATE_KEYS: &[&str] = &["title", "goal", "status", "policy", "scheduling"];
const PLAN_ADD_TASK_KEYS: &[&str] = &[
    "title",
    "status",
    "dependsOn",
    "depends_on",
    "assignee",
    "anchors",
    "acceptance",
    "artifactRequirements",
    "reviewRequirements",
];
const TASK_UPDATE_KEYS: &[&str] = &[
    "title",
    "status",
    "summary",
    "assignee",
    "priority",
    "dependsOn",
    "depends_on",
    "anchors",
    "acceptance",
    "validationRefs",
    "tags",
    "artifactRequirements",
    "reviewRequirements",
];
const TASK_COMPLETE_KEYS: &[&str] = &["title", "summary"];
const TASK_HANDOFF_KEYS: &[&str] = &["summary", "toAgent", "to_agent"];
const TASK_AGENT_KEYS: &[&str] = &["agent"];

const fn compiler_method(
    path: &'static str,
    declaration: Option<&'static str>,
    return_type: PrismSurfaceTypeRef,
    record_arg: Option<PrismRecordArgBundle>,
    effect: PrismCompilerEffectKind,
    host_operation: Option<&'static str>,
) -> PrismCompilerMethodSpec {
    PrismCompilerMethodSpec {
        api: PrismApiMethodSpec {
            path,
            declaration,
            return_type,
            record_arg,
        },
        effect,
        host_operation,
    }
}

pub fn prism_compiler_method_specs() -> &'static [PrismCompilerMethodSpec] {
    static SPECS: &[PrismCompilerMethodSpec] = &[
        compiler_method(
            "prism.work.declare",
            Some(
                "declare(input: { title: string; kind?: string; summary?: string; parentWorkId?: string; coordinationTaskId?: string; planId?: string }): unknown;",
            ),
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "workDeclare",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: WORK_DECLARE_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__declareWork"),
        ),
        compiler_method(
            "prism.claim.acquire",
            Some(
                "acquire(input: { anchors: AnchorRef[]; capability: string; mode?: string; ttlSeconds?: number; agent?: string; coordinationTaskId?: string }): ClaimView;",
            ),
            PrismSurfaceTypeRef::Named("ClaimView"),
            Some(PrismRecordArgBundle {
                bundle_name: "claimAcquire",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: CLAIM_ACQUIRE_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__claimAcquire"),
        ),
        compiler_method(
            "prism.claim.renew",
            Some("renew(claim: ClaimView | string, input?: { ttlSeconds?: number }): ClaimView;"),
            PrismSurfaceTypeRef::Named("ClaimView"),
            Some(PrismRecordArgBundle {
                bundle_name: "claimRenew",
                arg_name: "input",
                arg_index: 1,
                allowed_keys: CLAIM_RENEW_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__claimRenew"),
        ),
        compiler_method(
            "prism.claim.release",
            Some("release(claim: ClaimView | string): ClaimView;"),
            PrismSurfaceTypeRef::Named("ClaimView"),
            None,
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__claimRelease"),
        ),
        compiler_method(
            "prism.artifact.propose",
            Some(
                "propose(input: { taskId: string; artifactRequirementId?: string; anchors?: AnchorRef[]; diffRef?: string; evidence?: string[]; requiredValidations?: string[]; validatedChecks?: string[]; riskScore?: number }): ArtifactView;",
            ),
            PrismSurfaceTypeRef::Named("ArtifactView"),
            Some(PrismRecordArgBundle {
                bundle_name: "artifactPropose",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: ARTIFACT_PROPOSE_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__artifactPropose"),
        ),
        compiler_method(
            "prism.artifact.supersede",
            Some("supersede(artifact: ArtifactView | string): ArtifactView;"),
            PrismSurfaceTypeRef::Named("ArtifactView"),
            None,
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__artifactSupersede"),
        ),
        compiler_method(
            "prism.artifact.review",
            Some(
                "review(artifact: ArtifactView | string, input: { reviewRequirementId?: string; verdict: string; summary: string; requiredValidations?: string[]; validatedChecks?: string[]; riskScore?: number }): ArtifactView;",
            ),
            PrismSurfaceTypeRef::Named("ArtifactView"),
            Some(PrismRecordArgBundle {
                bundle_name: "artifactReview",
                arg_name: "input",
                arg_index: 1,
                allowed_keys: ARTIFACT_REVIEW_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__artifactReview"),
        ),
        compiler_method(
            "prism.coordination.createPlan",
            Some(
                "createPlan(input: { title: string; goal?: string; status?: \"draft\" | \"active\" | \"blocked\" | \"completed\" | \"abandoned\" | \"archived\" }): unknown;",
            ),
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "coordinationCreatePlan",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: COORDINATION_CREATE_PLAN_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationCreatePlan"),
        ),
        compiler_method(
            "prism.coordination.openPlan",
            Some("openPlan(planId: string): unknown;"),
            PrismSurfaceTypeRef::Unknown,
            None,
            PrismCompilerEffectKind::CoordinationRead,
            Some("__coordinationOpenPlan"),
        ),
        compiler_method(
            "prism.coordination.openTask",
            Some("openTask(taskId: string): unknown;"),
            PrismSurfaceTypeRef::Unknown,
            None,
            PrismCompilerEffectKind::CoordinationRead,
            Some("__coordinationOpenTask"),
        ),
        compiler_method(
            "plan.update",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "planUpdate",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: PLAN_UPDATE_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationPlanUpdate"),
        ),
        compiler_method(
            "plan.archive",
            None,
            PrismSurfaceTypeRef::Unknown,
            None,
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationPlanArchive"),
        ),
        compiler_method(
            "plan.addTask",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "planAddTask",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: PLAN_ADD_TASK_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationPlanAddTask"),
        ),
        compiler_method(
            "task.update",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "taskUpdate",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: TASK_UPDATE_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationTaskUpdate"),
        ),
        compiler_method(
            "task.complete",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "taskComplete",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: TASK_COMPLETE_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationTaskComplete"),
        ),
        compiler_method(
            "task.handoff",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "taskHandoff",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: TASK_HANDOFF_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationTaskHandoff"),
        ),
        compiler_method(
            "task.acceptHandoff",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "taskAcceptHandoff",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: TASK_AGENT_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationTaskAcceptHandoff"),
        ),
        compiler_method(
            "task.resume",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "taskResume",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: TASK_AGENT_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationTaskResume"),
        ),
        compiler_method(
            "task.reclaim",
            None,
            PrismSurfaceTypeRef::Unknown,
            Some(PrismRecordArgBundle {
                bundle_name: "taskReclaim",
                arg_name: "input",
                arg_index: 0,
                allowed_keys: TASK_AGENT_KEYS,
            }),
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationTaskReclaim"),
        ),
        compiler_method(
            "task.dependsOn",
            None,
            PrismSurfaceTypeRef::Unknown,
            None,
            PrismCompilerEffectKind::CoordinationWrite,
            Some("__coordinationTaskDependsOn"),
        ),
    ];
    SPECS
}

pub fn prism_compiler_method_spec(path: &str) -> Option<&'static PrismCompilerMethodSpec> {
    prism_compiler_method_specs()
        .iter()
        .find(|spec| spec.api.path == path)
}

pub fn prism_compiler_method_spec_by_host_operation(
    host_operation: &str,
) -> Option<&'static PrismCompilerMethodSpec> {
    prism_compiler_method_specs()
        .iter()
        .find(|spec| spec.host_operation == Some(host_operation))
}
