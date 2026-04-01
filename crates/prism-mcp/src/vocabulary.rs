#[derive(Debug, Clone, Copy)]
pub(crate) struct VocabularyValueSpec {
    pub(crate) value: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) description: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VocabularyCategorySpec {
    pub(crate) key: &'static str,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) values: &'static [VocabularyValueSpec],
}

const PRISM_SESSION_ACTIONS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "start_task",
        aliases: &[],
        description: "Start a local PRISM task journal for the current session.",
    },
    VocabularyValueSpec {
        value: "bind_coordination_task",
        aliases: &[],
        description: "Bind the current session to an existing coordination task.",
    },
    VocabularyValueSpec {
        value: "configure",
        aliases: &[],
        description: "Adjust session task, agent, or limit settings.",
    },
    VocabularyValueSpec {
        value: "finish_task",
        aliases: &[],
        description: "Mark the current or specified PRISM task as completed.",
    },
    VocabularyValueSpec {
        value: "abandon_task",
        aliases: &[],
        description: "Mark the current or specified PRISM task as abandoned.",
    },
];

const PRISM_MUTATE_ACTIONS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "outcome",
        aliases: &[],
        description: "Record a durable outcome event.",
    },
    VocabularyValueSpec {
        value: "memory",
        aliases: &[],
        description: "Store anchored memory.",
    },
    VocabularyValueSpec {
        value: "concept",
        aliases: &[],
        description: "Promote, update, or retire a concept packet.",
    },
    VocabularyValueSpec {
        value: "contract",
        aliases: &[],
        description: "Promote, update, retire, or maintain a contract packet.",
    },
    VocabularyValueSpec {
        value: "concept_relation",
        aliases: &[],
        description: "Upsert or retire a concept relation.",
    },
    VocabularyValueSpec {
        value: "validation_feedback",
        aliases: &[],
        description: "Record PRISM dogfooding validation feedback.",
    },
    VocabularyValueSpec {
        value: "session_repair",
        aliases: &[],
        description: "Apply a narrow safe repair to the current MCP session context.",
    },
    VocabularyValueSpec {
        value: "infer_edge",
        aliases: &[],
        description: "Record an inferred edge.",
    },
    VocabularyValueSpec {
        value: "coordination",
        aliases: &[],
        description: "Mutate coordination plans, tasks, nodes, edges, and handoffs.",
    },
    VocabularyValueSpec {
        value: "claim",
        aliases: &[],
        description: "Acquire, renew, or release a claim.",
    },
    VocabularyValueSpec {
        value: "artifact",
        aliases: &[],
        description: "Propose, supersede, or review an artifact.",
    },
    VocabularyValueSpec {
        value: "test_ran",
        aliases: &[],
        description: "Record a test run outcome.",
    },
    VocabularyValueSpec {
        value: "failure_observed",
        aliases: &[],
        description: "Record a failure observation.",
    },
    VocabularyValueSpec {
        value: "fix_validated",
        aliases: &[],
        description: "Record that a fix was validated.",
    },
    VocabularyValueSpec {
        value: "curator_apply_proposal",
        aliases: &[],
        description: "Apply a curator proposal.",
    },
    VocabularyValueSpec {
        value: "curator_promote_edge",
        aliases: &[],
        description: "Promote an inferred edge from a curator proposal.",
    },
    VocabularyValueSpec {
        value: "curator_promote_concept",
        aliases: &[],
        description: "Promote a concept from a curator proposal.",
    },
    VocabularyValueSpec {
        value: "curator_promote_memory",
        aliases: &[],
        description: "Promote memory from a curator proposal.",
    },
    VocabularyValueSpec {
        value: "curator_reject_proposal",
        aliases: &[],
        description: "Reject a curator proposal.",
    },
];

const COORDINATION_MUTATION_KINDS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "plan_create",
        aliases: &[],
        description: "Create a coordination plan.",
    },
    VocabularyValueSpec {
        value: "plan_update",
        aliases: &[],
        description: "Update a coordination plan.",
    },
    VocabularyValueSpec {
        value: "plan_archive",
        aliases: &[],
        description: "Archive a coordination plan.",
    },
    VocabularyValueSpec {
        value: "task_create",
        aliases: &[],
        description: "Create a coordination task.",
    },
    VocabularyValueSpec {
        value: "update",
        aliases: &[],
        description: "Update a coordination task or first-class plan node by id.",
    },
    VocabularyValueSpec {
        value: "plan_node_create",
        aliases: &[],
        description: "Create a first-class plan node.",
    },
    VocabularyValueSpec {
        value: "plan_edge_create",
        aliases: &[],
        description: "Create a first-class plan edge.",
    },
    VocabularyValueSpec {
        value: "plan_edge_delete",
        aliases: &[],
        description: "Delete a first-class plan edge.",
    },
    VocabularyValueSpec {
        value: "handoff",
        aliases: &[],
        description: "Request a task handoff.",
    },
    VocabularyValueSpec {
        value: "resume",
        aliases: &[],
        description: "Resume a stale or expired task lease held by the same principal.",
    },
    VocabularyValueSpec {
        value: "reclaim",
        aliases: &[],
        description: "Reclaim a stale or expired task lease from another principal.",
    },
    VocabularyValueSpec {
        value: "handoff_accept",
        aliases: &[],
        description: "Accept a task handoff.",
    },
];

const CLAIM_ACTIONS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "acquire",
        aliases: &[],
        description: "Acquire a claim.",
    },
    VocabularyValueSpec {
        value: "renew",
        aliases: &[],
        description: "Renew a claim.",
    },
    VocabularyValueSpec {
        value: "release",
        aliases: &[],
        description: "Release a claim.",
    },
];

const ARTIFACT_ACTIONS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "propose",
        aliases: &[],
        description: "Propose an artifact for review.",
    },
    VocabularyValueSpec {
        value: "supersede",
        aliases: &[],
        description: "Supersede an artifact.",
    },
    VocabularyValueSpec {
        value: "review",
        aliases: &[],
        description: "Review an artifact.",
    },
];

const CAPABILITIES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "observe",
        aliases: &["Observe"],
        description: "Read or inspect without claiming edit ownership.",
    },
    VocabularyValueSpec {
        value: "edit",
        aliases: &["Edit"],
        description: "Claim edit ownership for a target.",
    },
    VocabularyValueSpec {
        value: "review",
        aliases: &["Review"],
        description: "Review or approve work.",
    },
    VocabularyValueSpec {
        value: "validate",
        aliases: &["Validate"],
        description: "Run or assert validations.",
    },
    VocabularyValueSpec {
        value: "merge",
        aliases: &["Merge"],
        description: "Perform merge or completion authority.",
    },
];

const CLAIM_MODES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "advisory",
        aliases: &["Advisory"],
        description: "Non-exclusive claim used for awareness.",
    },
    VocabularyValueSpec {
        value: "soft_exclusive",
        aliases: &[
            "soft-exclusive",
            "softExclusive",
            "SoftExclusive",
            "softexclusive",
        ],
        description: "Prefer one editor, but allow overlap when necessary.",
    },
    VocabularyValueSpec {
        value: "hard_exclusive",
        aliases: &[
            "hard-exclusive",
            "hardExclusive",
            "HardExclusive",
            "hardexclusive",
        ],
        description: "Exclusive claim that should block overlapping edits.",
    },
];

const COORDINATION_TASK_STATUSES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "proposed",
        aliases: &[],
        description: "Draft task not yet ready to act on.",
    },
    VocabularyValueSpec {
        value: "ready",
        aliases: &["todo"],
        description: "Actionable task waiting to be worked.",
    },
    VocabularyValueSpec {
        value: "in_progress",
        aliases: &["in-progress", "inprogress"],
        description: "Task actively being worked.",
    },
    VocabularyValueSpec {
        value: "blocked",
        aliases: &[],
        description: "Task cannot proceed yet.",
    },
    VocabularyValueSpec {
        value: "in_review",
        aliases: &["in-review", "inreview"],
        description: "Task is waiting on review.",
    },
    VocabularyValueSpec {
        value: "validating",
        aliases: &[],
        description: "Task is waiting on validation or verification.",
    },
    VocabularyValueSpec {
        value: "completed",
        aliases: &[],
        description: "Task is complete.",
    },
    VocabularyValueSpec {
        value: "abandoned",
        aliases: &[],
        description: "Task was intentionally abandoned.",
    },
];

const PLAN_STATUSES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "draft",
        aliases: &[],
        description: "Plan is not active yet.",
    },
    VocabularyValueSpec {
        value: "active",
        aliases: &[],
        description: "Plan is active.",
    },
    VocabularyValueSpec {
        value: "blocked",
        aliases: &[],
        description: "Plan is blocked overall.",
    },
    VocabularyValueSpec {
        value: "completed",
        aliases: &[],
        description: "Plan is complete.",
    },
    VocabularyValueSpec {
        value: "abandoned",
        aliases: &[],
        description: "Plan was intentionally abandoned.",
    },
    VocabularyValueSpec {
        value: "archived",
        aliases: &[],
        description: "Plan is retained in published history but no longer active.",
    },
];

const PLAN_SCOPES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "local",
        aliases: &[],
        description: "Runtime-local state only.",
    },
    VocabularyValueSpec {
        value: "session",
        aliases: &[],
        description: "Persisted for the current clone or session context.",
    },
    VocabularyValueSpec {
        value: "repo",
        aliases: &[],
        description: "Published repo-wide state.",
    },
];

const PLAN_NODE_STATUSES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "proposed",
        aliases: &[],
        description: "Draft node not yet ready.",
    },
    VocabularyValueSpec {
        value: "ready",
        aliases: &["todo"],
        description: "Actionable node waiting to be worked.",
    },
    VocabularyValueSpec {
        value: "in_progress",
        aliases: &["in-progress", "inprogress"],
        description: "Node actively being worked.",
    },
    VocabularyValueSpec {
        value: "blocked",
        aliases: &[],
        description: "Node is blocked.",
    },
    VocabularyValueSpec {
        value: "waiting",
        aliases: &[],
        description: "Node is waiting on some external condition.",
    },
    VocabularyValueSpec {
        value: "in_review",
        aliases: &["in-review", "inreview"],
        description: "Node is in review.",
    },
    VocabularyValueSpec {
        value: "validating",
        aliases: &[],
        description: "Node is validating.",
    },
    VocabularyValueSpec {
        value: "completed",
        aliases: &[],
        description: "Node is complete.",
    },
    VocabularyValueSpec {
        value: "abandoned",
        aliases: &[],
        description: "Node was abandoned.",
    },
];

const PLAN_NODE_KINDS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "investigate",
        aliases: &[],
        description: "Investigate or gather evidence.",
    },
    VocabularyValueSpec {
        value: "decide",
        aliases: &[],
        description: "Make a decision or choose a direction.",
    },
    VocabularyValueSpec {
        value: "edit",
        aliases: &[],
        description: "Implement code or content changes.",
    },
    VocabularyValueSpec {
        value: "validate",
        aliases: &[],
        description: "Validate or verify work.",
    },
    VocabularyValueSpec {
        value: "review",
        aliases: &[],
        description: "Perform review work.",
    },
    VocabularyValueSpec {
        value: "handoff",
        aliases: &[],
        description: "Transfer responsibility or context.",
    },
    VocabularyValueSpec {
        value: "merge",
        aliases: &[],
        description: "Merge or finalize work.",
    },
    VocabularyValueSpec {
        value: "release",
        aliases: &[],
        description: "Release or publish work.",
    },
    VocabularyValueSpec {
        value: "note",
        aliases: &[],
        description: "Record a note or structural marker.",
    },
];

const PLAN_EDGE_KINDS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "depends_on",
        aliases: &["depends-on", "dependson"],
        description: "Execution dependency edge.",
    },
    VocabularyValueSpec {
        value: "blocks",
        aliases: &[],
        description: "Durable authored blocking edge.",
    },
    VocabularyValueSpec {
        value: "informs",
        aliases: &[],
        description: "Informational edge.",
    },
    VocabularyValueSpec {
        value: "validates",
        aliases: &[],
        description: "Validation relationship edge.",
    },
    VocabularyValueSpec {
        value: "handoff_to",
        aliases: &["handoff-to", "handoffto"],
        description: "Handoff relationship edge.",
    },
    VocabularyValueSpec {
        value: "child_of",
        aliases: &["child-of", "childof"],
        description: "Hierarchical grouping edge.",
    },
    VocabularyValueSpec {
        value: "related_to",
        aliases: &["related-to", "relatedto"],
        description: "Loose semantic relation edge.",
    },
];

const REVIEW_VERDICTS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "approved",
        aliases: &[],
        description: "Artifact was approved.",
    },
    VocabularyValueSpec {
        value: "changes_requested",
        aliases: &["changes-requested", "changesrequested"],
        description: "Artifact needs changes before approval.",
    },
    VocabularyValueSpec {
        value: "rejected",
        aliases: &[],
        description: "Artifact was rejected.",
    },
];

const ACCEPTANCE_EVIDENCE_POLICIES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "any",
        aliases: &[],
        description: "Any acceptable evidence can satisfy the criterion.",
    },
    VocabularyValueSpec {
        value: "all",
        aliases: &[],
        description: "All configured evidence classes must be satisfied.",
    },
    VocabularyValueSpec {
        value: "review_only",
        aliases: &["review-only", "reviewonly"],
        description: "Review evidence is required.",
    },
    VocabularyValueSpec {
        value: "validation_only",
        aliases: &["validation-only", "validationonly"],
        description: "Validation evidence is required.",
    },
    VocabularyValueSpec {
        value: "review_and_validation",
        aliases: &["review-and-validation", "reviewandvalidation"],
        description: "Both review and validation evidence are required.",
    },
];

const PRISM_LOCATE_TASK_INTENTS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "inspect",
        aliases: &["code", "read"],
        description: "Bias locate toward inspection or explanation targets.",
    },
    VocabularyValueSpec {
        value: "tests",
        aliases: &["test"],
        description: "Bias locate toward likely tests.",
    },
    VocabularyValueSpec {
        value: "docs",
        aliases: &["spec"],
        description: "Bias locate toward docs or spec surfaces.",
    },
];

const PRISM_OPEN_MODES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "focus",
        aliases: &[],
        description: "Open a bounded local focus block.",
    },
    VocabularyValueSpec {
        value: "edit",
        aliases: &[],
        description: "Open an edit-oriented slice.",
    },
    VocabularyValueSpec {
        value: "raw",
        aliases: &[],
        description: "Open the raw literal file window.",
    },
];

const PRISM_EXPAND_KINDS: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "diagnostics",
        aliases: &[],
        description: "Expand diagnostic detail for a handle.",
    },
    VocabularyValueSpec {
        value: "lineage",
        aliases: &[],
        description: "Expand lineage detail.",
    },
    VocabularyValueSpec {
        value: "neighbors",
        aliases: &[],
        description: "Expand neighboring handles.",
    },
    VocabularyValueSpec {
        value: "diff",
        aliases: &[],
        description: "Expand diff detail.",
    },
    VocabularyValueSpec {
        value: "health",
        aliases: &[],
        description: "Expand health detail.",
    },
    VocabularyValueSpec {
        value: "validation",
        aliases: &[],
        description: "Expand validation detail.",
    },
    VocabularyValueSpec {
        value: "impact",
        aliases: &[],
        description: "Expand impact detail.",
    },
    VocabularyValueSpec {
        value: "timeline",
        aliases: &[],
        description: "Expand timeline detail.",
    },
    VocabularyValueSpec {
        value: "memory",
        aliases: &[],
        description: "Expand memory detail.",
    },
    VocabularyValueSpec {
        value: "drift",
        aliases: &[],
        description: "Expand drift detail.",
    },
];

const PRISM_CONCEPT_LENSES: &[VocabularyValueSpec] = &[
    VocabularyValueSpec {
        value: "open",
        aliases: &[],
        description: "Decode a concept into a primary open target.",
    },
    VocabularyValueSpec {
        value: "workset",
        aliases: &[],
        description: "Decode a concept into an implementation workset.",
    },
    VocabularyValueSpec {
        value: "validation",
        aliases: &[],
        description: "Decode a concept into validation context.",
    },
    VocabularyValueSpec {
        value: "timeline",
        aliases: &[],
        description: "Decode a concept into recent timeline context.",
    },
    VocabularyValueSpec {
        value: "memory",
        aliases: &[],
        description: "Decode a concept into related memory.",
    },
];

const VOCABULARY_CATEGORIES: &[VocabularyCategorySpec] = &[
    VocabularyCategorySpec {
        key: "prismSessionAction",
        title: "PRISM Session Actions",
        description: "Top-level action values accepted by prism_session.",
        values: PRISM_SESSION_ACTIONS,
    },
    VocabularyCategorySpec {
        key: "prismMutateAction",
        title: "PRISM Mutate Actions",
        description: "Top-level action values accepted by prism_mutate.",
        values: PRISM_MUTATE_ACTIONS,
    },
    VocabularyCategorySpec {
        key: "coordinationMutationKind",
        title: "Coordination Mutation Kinds",
        description: "Nested kind values accepted by prism_mutate action coordination.",
        values: COORDINATION_MUTATION_KINDS,
    },
    VocabularyCategorySpec {
        key: "claimAction",
        title: "Claim Actions",
        description: "Nested action values accepted by prism_mutate action claim.",
        values: CLAIM_ACTIONS,
    },
    VocabularyCategorySpec {
        key: "artifactAction",
        title: "Artifact Actions",
        description: "Nested action values accepted by prism_mutate action artifact.",
        values: ARTIFACT_ACTIONS,
    },
    VocabularyCategorySpec {
        key: "capability",
        title: "Claim Capabilities",
        description: "Capability values accepted by claim and simulateClaim inputs.",
        values: CAPABILITIES,
    },
    VocabularyCategorySpec {
        key: "claimMode",
        title: "Claim Modes",
        description: "Claim mode values accepted by claim and policy inputs.",
        values: CLAIM_MODES,
    },
    VocabularyCategorySpec {
        key: "coordinationTaskStatus",
        title: "Coordination Task Statuses",
        description: "Canonical coordination task status values.",
        values: COORDINATION_TASK_STATUSES,
    },
    VocabularyCategorySpec {
        key: "planStatus",
        title: "Plan Statuses",
        description: "Canonical coordination plan status values.",
        values: PLAN_STATUSES,
    },
    VocabularyCategorySpec {
        key: "planScope",
        title: "Plan Scopes",
        description: "Canonical plan scope values.",
        values: PLAN_SCOPES,
    },
    VocabularyCategorySpec {
        key: "planNodeStatus",
        title: "Plan Node Statuses",
        description: "Canonical first-class plan node status values.",
        values: PLAN_NODE_STATUSES,
    },
    VocabularyCategorySpec {
        key: "planNodeKind",
        title: "Plan Node Kinds",
        description: "Canonical first-class plan node kind values.",
        values: PLAN_NODE_KINDS,
    },
    VocabularyCategorySpec {
        key: "planEdgeKind",
        title: "Plan Edge Kinds",
        description: "Canonical first-class plan edge kind values.",
        values: PLAN_EDGE_KINDS,
    },
    VocabularyCategorySpec {
        key: "reviewVerdict",
        title: "Review Verdicts",
        description: "Artifact review verdict values.",
        values: REVIEW_VERDICTS,
    },
    VocabularyCategorySpec {
        key: "acceptanceEvidencePolicy",
        title: "Acceptance Evidence Policies",
        description: "Evidence policy values for plan acceptance criteria.",
        values: ACCEPTANCE_EVIDENCE_POLICIES,
    },
    VocabularyCategorySpec {
        key: "prismLocateTaskIntent",
        title: "Locate Task Intents",
        description: "Task-intent values accepted by prism_locate.",
        values: PRISM_LOCATE_TASK_INTENTS,
    },
    VocabularyCategorySpec {
        key: "prismOpenMode",
        title: "Open Modes",
        description: "Mode values accepted by prism_open.",
        values: PRISM_OPEN_MODES,
    },
    VocabularyCategorySpec {
        key: "prismExpandKind",
        title: "Expand Kinds",
        description: "Kind values accepted by prism_expand.",
        values: PRISM_EXPAND_KINDS,
    },
    VocabularyCategorySpec {
        key: "prismConceptLens",
        title: "Concept Lenses",
        description: "Lens values accepted by prism_concept.",
        values: PRISM_CONCEPT_LENSES,
    },
];

pub(crate) fn vocabulary_categories() -> &'static [VocabularyCategorySpec] {
    VOCABULARY_CATEGORIES
}

pub(crate) fn vocabulary_category(key: &str) -> Option<&'static VocabularyCategorySpec> {
    VOCABULARY_CATEGORIES
        .iter()
        .find(|category| category.key == key)
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut prev = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut curr = vec![0; right_chars.len() + 1];
    for (i, left_char) in left.chars().enumerate() {
        curr[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let cost = usize::from(left_char != *right_char);
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[right_chars.len()]
}

pub(crate) fn suggested_vocabulary_value(key: &str, raw_value: &str) -> Option<&'static str> {
    let category = vocabulary_category(key)?;
    let normalized = normalize(raw_value);
    category
        .values
        .iter()
        .flat_map(|value| {
            std::iter::once((value.value, normalize(value.value))).chain(
                value
                    .aliases
                    .iter()
                    .map(move |alias| (value.value, normalize(alias))),
            )
        })
        .map(|(canonical, candidate)| (canonical, levenshtein(&normalized, &candidate)))
        .min_by_key(|(_, distance)| *distance)
        .and_then(|(canonical, distance)| (distance <= 3).then_some(canonical))
}

pub(crate) fn vocabulary_error(
    key: &str,
    label: &str,
    raw_value: &str,
    field_example: &str,
) -> String {
    let Some(category) = vocabulary_category(key) else {
        return format!("unknown {label} `{raw_value}`");
    };
    let allowed = category
        .values
        .iter()
        .map(|value| format!("`{}`", value.value))
        .collect::<Vec<_>>()
        .join(", ");
    let suggestion = suggested_vocabulary_value(key, raw_value)
        .map(|value| format!(" Did you mean `{value}`?"))
        .unwrap_or_default();
    format!(
        "unknown {label} `{raw_value}`. Allowed values: {allowed}.{suggestion} Minimal example: {field_example}. Inspect prism://vocab for the canonical value set."
    )
}
