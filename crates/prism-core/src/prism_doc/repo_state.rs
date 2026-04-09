use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_coordination::{
    CanonicalPlanRecord, CanonicalTaskRecord, CoordinationDependencyRecord, CoordinationSnapshotV2,
};
use prism_ir::{
    CoordinationTaskStatus, DerivedPlanStatus, EffectiveTaskStatus, GitExecutionStatus, NodeRef,
    NodeRefKind, PlanId, PlanKind, PlanScope, PlanStatus, TaskId, TaskLifecycleStatus,
};
use prism_memory::{MemoryEntry, MemoryEvent, MemoryEventKind};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::memory_events::load_repo_memory_events;
use crate::published_plans::{
    load_authoritative_coordination_plan_state, HydratedCoordinationPlanState,
};

use super::{anchor_label, write_generated_file, PrismDocFileSync};

const STATE_PROJECTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RepoStateSummary {
    pub(super) memory_count: usize,
    pub(super) plan_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct RepoStateCatalog {
    memories: Vec<PublishedMemoryRecord>,
    memory_events: Vec<MemoryEvent>,
    plans: Vec<PublishedPlanDoc>,
}

impl RepoStateCatalog {
    pub(super) fn load(
        root: &Path,
        plan_state_override: Option<HydratedCoordinationPlanState>,
    ) -> Result<Self> {
        let memory_events = load_repo_memory_events(root)?;
        let plan_state = match plan_state_override {
            Some(state) => Some(state),
            None => load_authoritative_coordination_plan_state(root)?,
        };
        let mut plans = plan_state
            .map(|state| published_plan_docs(&state))
            .transpose()?
            .unwrap_or_default();
        plans.sort_by(|left, right| {
            left.bucket
                .sort_key()
                .cmp(&right.bucket.sort_key())
                .then_with(|| {
                    left.plan
                        .title
                        .to_ascii_lowercase()
                        .cmp(&right.plan.title.to_ascii_lowercase())
                })
                .then_with(|| left.plan.id.0.cmp(&right.plan.id.0))
        });

        Ok(Self {
            memories: project_memories(&memory_events),
            memory_events,
            plans,
        })
    }

    pub(super) fn summary(&self) -> RepoStateSummary {
        RepoStateSummary {
            memory_count: self.memories.len(),
            plan_count: self.plans.len(),
        }
    }
}

#[derive(Debug, Clone)]
struct ProjectionMetadata {
    source_head: String,
    source_logical_timestamp: Option<u64>,
    source_snapshot: String,
}

impl ProjectionMetadata {
    fn from_sources<T: Serialize>(
        sources: &T,
        source_logical_timestamp: Option<u64>,
        source_snapshot: String,
    ) -> Self {
        let canonical = serde_jcs::to_vec(sources).expect("state projection stamp should encode");
        Self {
            source_head: format!("sha256:{:x}", Sha256::digest(canonical)),
            source_logical_timestamp,
            source_snapshot,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct PublishedMemoryRecord {
    entry: MemoryEntry,
    latest_event_id: String,
    latest_recorded_at: u64,
    event_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct PublishedPlanDoc {
    plan: CanonicalPlanRecord,
    status: PlanStatus,
    derived_status: DerivedPlanStatus,
    child_plans: Vec<CanonicalPlanRecord>,
    tasks: Vec<PublishedTaskDoc>,
    dependencies: Vec<CoordinationDependencyRecord>,
    bucket: PlanDocBucket,
    doc_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct PublishedTaskDoc {
    task: CanonicalTaskRecord,
    effective_status: EffectiveTaskStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum PlanDocBucket {
    Active,
    Archived,
}

impl PlanDocBucket {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Archived => "Archived",
        }
    }

    fn sort_key(self) -> u8 {
        match self {
            Self::Active => 0,
            Self::Archived => 1,
        }
    }
}

pub(super) fn export_repo_state_docs(
    output_root: &Path,
    catalog: &RepoStateCatalog,
) -> Result<Vec<PrismDocFileSync>> {
    let prism_docs_dir = output_root.join("docs").join("prism");
    let plan_docs_dir = prism_docs_dir.join("plans");
    fs::create_dir_all(&plan_docs_dir)?;

    let mut files = Vec::new();
    files.push(write_generated_file(
        prism_docs_dir.join("memory.md"),
        render_memory_doc(catalog),
    )?);
    files.push(write_generated_file(
        plan_docs_dir.join("index.md"),
        render_plan_index_doc(catalog),
    )?);
    let changes_doc = prism_docs_dir.join("changes.md");
    if changes_doc.exists() {
        fs::remove_file(changes_doc)?;
    }

    let mut expected_plan_docs = BTreeSet::new();
    for plan in &catalog.plans {
        expected_plan_docs.insert(plan.doc_path.clone());
        files.push(write_generated_file(
            output_root.join(&plan.doc_path),
            render_plan_doc(plan),
        )?);
    }
    remove_stale_plan_docs(output_root, &expected_plan_docs)?;

    Ok(files)
}

pub(super) fn render_published_plan_markdown(
    snapshot: &CoordinationSnapshotV2,
    plan_id: &PlanId,
    status: Option<PlanStatus>,
) -> Option<String> {
    build_published_plan_doc(snapshot, plan_id, status)
        .ok()
        .map(|plan| render_plan_doc_parts(&plan))
}

fn project_memories(events: &[MemoryEvent]) -> Vec<PublishedMemoryRecord> {
    let mut sorted = events.to_vec();
    sorted.sort_by(|left, right| {
        left.recorded_at
            .cmp(&right.recorded_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut current = HashMap::<String, MemoryEntry>::new();
    let mut latest = HashMap::<String, String>::new();
    let mut latest_recorded_at = HashMap::<String, u64>::new();
    let mut event_counts = HashMap::<String, usize>::new();
    for event in sorted {
        for superseded in &event.supersedes {
            current.remove(&superseded.0);
        }
        *event_counts.entry(event.memory_id.0.clone()).or_insert(0) += 1;
        latest.insert(event.memory_id.0.clone(), event.id.clone());
        latest_recorded_at.insert(event.memory_id.0.clone(), event.recorded_at);
        match event.action {
            MemoryEventKind::Stored | MemoryEventKind::Promoted | MemoryEventKind::Superseded => {
                if let Some(entry) = event.entry.clone() {
                    current.insert(event.memory_id.0.clone(), entry);
                }
            }
            MemoryEventKind::Retired => {
                current.remove(&event.memory_id.0);
            }
        }
    }

    let mut projected = current
        .into_iter()
        .map(|(id, entry)| PublishedMemoryRecord {
            entry,
            latest_event_id: latest.get(&id).cloned().unwrap_or_default(),
            latest_recorded_at: latest_recorded_at.get(&id).copied().unwrap_or_default(),
            event_count: event_counts.get(&id).copied().unwrap_or(0),
        })
        .collect::<Vec<_>>();
    projected.sort_by(|left, right| {
        left.entry
            .created_at
            .cmp(&right.entry.created_at)
            .then_with(|| left.entry.id.0.cmp(&right.entry.id.0))
    });
    projected
}

fn render_memory_doc(catalog: &RepoStateCatalog) -> String {
    let metadata = ProjectionMetadata::from_sources(
        &catalog.memory_events,
        catalog
            .memory_events
            .iter()
            .map(|event| event.recorded_at)
            .max(),
        format!(
            "{} active memories, {} memory events",
            catalog.memories.len(),
            catalog.memory_events.len()
        ),
    );
    let mut markdown = String::new();
    markdown.push_str("# PRISM Memory\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM memory events.\n");
    markdown.push_str("> Return to the concise entrypoint in `../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Active repo memories: {}\n",
        catalog.memories.len()
    ));
    markdown.push_str(&format!(
        "- Repo memory events logged: {}\n\n",
        catalog.memory_events.len()
    ));

    if catalog.memories.is_empty() {
        markdown.push_str("No active repo-scoped memories are currently published.\n");
        return markdown;
    }

    markdown.push_str("## Published Memories\n\n");
    for memory in &catalog.memories {
        markdown.push_str(&format!(
            "- `{}`: {}\n",
            memory.entry.id.0,
            first_line(&memory.entry.content)
        ));
    }
    markdown.push('\n');

    for memory in &catalog.memories {
        markdown.push_str(&format!("## {}\n\n", memory.entry.id.0));
        markdown.push_str(&format!(
            "Kind: {}  \nSource: {}  \nTrust: {:.2}  \nCreated at: `{}`\n\n",
            format_memory_kind(memory.entry.kind),
            format_memory_source(memory.entry.source),
            memory.entry.trust,
            memory.entry.created_at
        ));
        markdown.push_str(&memory.entry.content);
        markdown.push_str("\n\n");

        if !memory.entry.anchors.is_empty() {
            markdown.push_str("### Anchors\n\n");
            for anchor in &memory.entry.anchors {
                markdown.push_str("- `");
                markdown.push_str(&anchor_label(anchor));
                markdown.push_str("`\n");
            }
            markdown.push('\n');
        }

        if let Some(publication) = metadata_object_lines(&memory.entry.metadata, "publication") {
            markdown.push_str("### Publication\n\n");
            for line in publication {
                markdown.push_str("- ");
                markdown.push_str(&line);
                markdown.push('\n');
            }
            markdown.push('\n');
        }
        if let Some(provenance) = metadata_object_lines(&memory.entry.metadata, "provenance") {
            markdown.push_str("### Provenance\n\n");
            for line in provenance {
                markdown.push_str("- ");
                markdown.push_str(&line);
                markdown.push('\n');
            }
            markdown.push('\n');
        }

        markdown.push_str("### Event Summary\n\n");
        markdown.push_str(&format!(
            "- Latest event id: `{}`\n- Latest recorded at: `{}`\n- Event count: `{}`\n\n",
            memory.latest_event_id, memory.latest_recorded_at, memory.event_count
        ));
    }

    markdown
}

fn render_plan_index_doc(catalog: &RepoStateCatalog) -> String {
    let active_count = catalog
        .plans
        .iter()
        .filter(|plan| plan.bucket == PlanDocBucket::Active)
        .count();
    let archived_count = catalog.plans.len().saturating_sub(active_count);
    let metadata = ProjectionMetadata::from_sources(
        &catalog.plans,
        None,
        format!(
            "{} plans, {} active, {} archived",
            catalog.plans.len(),
            active_count,
            archived_count
        ),
    );
    let mut markdown = String::new();
    markdown.push_str("# PRISM Plans\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM published plan state.\n");
    markdown.push_str("> Return to the concise entrypoint in `../../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!("- Published plans: {}\n", catalog.plans.len()));
    markdown.push_str(&format!("- Active plans: {}\n", active_count));
    markdown.push_str(&format!("- Archived plans: {}\n\n", archived_count));

    if catalog.plans.is_empty() {
        markdown.push_str("No repo-scoped plans are currently published.\n");
        return markdown;
    }

    render_plan_bucket_section(&mut markdown, catalog, PlanDocBucket::Active);
    render_plan_bucket_section(&mut markdown, catalog, PlanDocBucket::Archived);
    markdown
}

fn published_plan_docs(state: &HydratedCoordinationPlanState) -> Result<Vec<PublishedPlanDoc>> {
    let status_by_plan = state
        .snapshot
        .plans
        .iter()
        .map(|plan| (plan.id.0.clone(), plan.status))
        .collect::<BTreeMap<_, _>>();
    state
        .canonical_snapshot_v2
        .plans
        .iter()
        .map(|plan| {
            build_published_plan_doc(
                &state.canonical_snapshot_v2,
                &plan.id,
                status_by_plan.get(plan.id.0.as_str()).copied(),
            )
        })
        .collect::<Result<Vec<_>>>()
}

fn build_published_plan_doc(
    snapshot: &CoordinationSnapshotV2,
    plan_id: &PlanId,
    status_override: Option<PlanStatus>,
) -> Result<PublishedPlanDoc> {
    let graph = snapshot.graph()?;
    let derivations = snapshot.derive_statuses()?;
    let plan = snapshot
        .plans
        .iter()
        .find(|plan| plan.id == *plan_id)
        .cloned()
        .expect("plan should exist in canonical snapshot");
    let derived_status = derivations
        .plan_state(plan_id)
        .map(|derived| derived.derived_status)
        .unwrap_or(DerivedPlanStatus::Pending);
    let child_plans = graph
        .children_of_plan(plan_id)
        .into_iter()
        .filter(|child| child.kind == NodeRefKind::Plan)
        .filter_map(|child| {
            snapshot
                .plans
                .iter()
                .find(|plan| plan.id.0 == child.id)
                .cloned()
        })
        .collect::<Vec<_>>();
    let descendant_task_ids = descendant_task_ids_for_plan(&graph, plan_id);
    let task_statuses = descendant_task_ids
        .iter()
        .filter_map(|task_id| {
            let task = snapshot
                .tasks
                .iter()
                .find(|task| task.id == *task_id)?
                .clone();
            let effective_status = derivations.task_state(task_id)?.effective_status;
            Some(PublishedTaskDoc {
                task,
                effective_status,
            })
        })
        .collect::<Vec<_>>();
    let subtree_nodes = subtree_node_refs(&graph, plan_id);
    let dependencies = snapshot
        .dependencies
        .iter()
        .filter(|dependency| {
            subtree_nodes.contains(&node_ref_key(&dependency.source))
                && subtree_nodes.contains(&node_ref_key(&dependency.target))
        })
        .cloned()
        .collect::<Vec<_>>();
    let status = status_override.unwrap_or_else(|| compatibility_plan_status(derived_status));
    let bucket = plan_bucket(status);
    Ok(PublishedPlanDoc {
        plan,
        status,
        derived_status,
        child_plans,
        tasks: task_statuses,
        dependencies,
        bucket,
        doc_path: plan_doc_path(bucket, &plan_id.0),
    })
}

fn render_plan_doc(plan: &PublishedPlanDoc) -> String {
    render_plan_doc_parts(plan)
}

fn render_plan_doc_parts(plan: &PublishedPlanDoc) -> String {
    let metadata = ProjectionMetadata::from_sources(
        plan,
        None,
        format!(
            "{} child plans, {} tasks, {} dependencies",
            plan.child_plans.len(),
            plan.tasks.len(),
            plan.dependencies.len()
        ),
    );
    let mut markdown = String::new();
    markdown.push_str("# ");
    markdown.push_str(&plan.plan.title);
    markdown.push_str("\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM plan state.\n");
    markdown.push_str("> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Plan id: `{}`\n- Status: `{}`\n- Derived status: `{}`\n- Kind: `{}`\n- Scope: `{}`\n- Child plans: `{}`\n- Descendant tasks: `{}`\n- Internal dependencies: `{}`\n\n",
        plan.plan.id.0,
        format_plan_status(plan.status),
        format_derived_plan_status(plan.derived_status),
        format_plan_kind(plan.plan.kind),
        format_plan_scope(plan.plan.scope),
        plan.child_plans.len(),
        plan.tasks.len(),
        plan.dependencies.len(),
    ));

    markdown.push_str("## Goal\n\n");
    markdown.push_str(&plan.plan.goal);
    markdown.push_str("\n\n");

    markdown.push_str("## Git Execution Policy\n\n");
    markdown.push_str(&format!(
        "- Start mode: `{}`\n- Completion mode: `{}`\n- Target branch: `{}`\n",
        format_git_execution_start_mode(plan.plan.policy.git_execution.start_mode),
        format_git_execution_completion_mode(plan.plan.policy.git_execution.completion_mode),
        plan.plan.policy.git_execution.target_branch,
    ));
    if let Some(target_ref) = plan.plan.policy.git_execution.target_ref.as_ref() {
        markdown.push_str(&format!("- Target ref: `{target_ref}`\n"));
    }
    markdown.push_str(&format!(
        "- Require task branch: `{}`\n- Max commits behind target: `{}`\n",
        plan.plan.policy.git_execution.require_task_branch,
        plan.plan.policy.git_execution.max_commits_behind_target,
    ));
    if let Some(max_fetch_age_seconds) = plan.plan.policy.git_execution.max_fetch_age_seconds {
        markdown.push_str(&format!(
            "- Max fetch age seconds: `{max_fetch_age_seconds}`\n"
        ));
    }
    markdown.push('\n');

    markdown.push_str("## Branch Snapshot Export\n\n");
    markdown.push_str(
        "- Authoritative coordination state: coordination authority backend (`SQLite` by default; Git shared refs when explicitly selected)\n",
    );
    markdown.push_str(
        "- Local hot cache: shared-runtime SQLite startup checkpoint and hydrated in-memory runtime\n",
    );
    markdown.push_str(
        "- Branch-local tracked `.prism/state/plans/**` export: disabled; plans no longer mirror into tracked repo snapshot state\n",
    );
    markdown.push_str(
        "- Manual markdown export path: `docs/prism/plans/**` only when `prism docs export --output-dir <dir>` is invoked explicitly\n\n",
    );

    if !plan.child_plans.is_empty() {
        markdown.push_str("## Child Plans\n\n");
        for child in &plan.child_plans {
            markdown.push_str(&format!(
                "- `{}`: {} (`{}`)\n",
                child.id.0,
                child.title,
                format_plan_kind(child.kind)
            ));
        }
        markdown.push('\n');
    }

    markdown.push_str("## Tasks\n\n");
    if plan.tasks.is_empty() {
        markdown.push_str("No canonical tasks are currently recorded for this plan.\n\n");
    } else {
        for task in &plan.tasks {
            render_plan_task(&mut markdown, task);
        }
    }

    markdown.push_str("## Dependencies\n\n");
    if plan.dependencies.is_empty() {
        markdown.push_str("No internal dependency edges are currently recorded for this plan.\n\n");
    } else {
        for dependency in &plan.dependencies {
            markdown.push_str(&format!(
                "- `{}` {} `{}`\n",
                format_node_ref(&dependency.source),
                dependency_arrow(),
                format_node_ref(&dependency.target),
            ));
        }
        markdown.push('\n');
    }

    markdown
}

fn render_plan_bucket_section(
    markdown: &mut String,
    catalog: &RepoStateCatalog,
    bucket: PlanDocBucket,
) {
    let plans = catalog
        .plans
        .iter()
        .filter(|plan| plan.bucket == bucket)
        .collect::<Vec<_>>();
    if plans.is_empty() {
        return;
    }
    markdown.push_str("## ");
    markdown.push_str(bucket.label());
    markdown.push_str(" Plans\n\n");
    for plan in plans {
        let relative = plan
            .doc_path
            .strip_prefix(Path::new("docs").join("prism").join("plans"))
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| plan.doc_path.to_string_lossy().replace('\\', "/"));
        markdown.push_str(&format!(
            "- [{}]({relative}): {} (`{}`)\n",
            plan.plan.title, plan.plan.goal, plan.plan.id.0
        ));
    }
    markdown.push('\n');
}

fn render_plan_task(markdown: &mut String, task: &PublishedTaskDoc) {
    markdown.push_str(&format!("### {}\n\n", task.task.title));
    markdown.push_str(&format!(
        "- Task id: `{}`\n- Effective status: `{}`\n- Lifecycle status: `{}`\n",
        task.task.id.0,
        format_effective_task_status(task.effective_status),
        format_task_lifecycle_status(task.task.lifecycle_status),
    ));
    markdown.push_str(&format!(
        "- Parent plan: `{}`\n- Estimated minutes: `{}`\n- Executor class: `{}`\n",
        task.task.parent_plan_id.0,
        task.task.estimated_minutes,
        format!("{:?}", task.task.executor.executor_class).to_ascii_lowercase()
    ));
    if let Some(summary) = task.task.summary.as_ref() {
        markdown.push_str("- Summary: ");
        markdown.push_str(summary);
        markdown.push('\n');
    }
    if let Some(priority) = task.task.priority {
        markdown.push_str(&format!("- Priority: `{priority}`\n"));
    }
    if let Some(assignee) = task.task.assignee.as_ref() {
        markdown.push_str(&format!("- Assignee: `{}`\n", assignee.0));
    }
    if let Some(session) = task.task.session.as_ref() {
        markdown.push_str(&format!("- Session: `{}`\n", session.0));
    }
    if let Some(worktree_id) = task.task.worktree_id.as_ref() {
        markdown.push_str(&format!("- Worktree: `{worktree_id}`\n"));
    }
    if let Some(branch_ref) = task.task.branch_ref.as_ref() {
        markdown.push_str(&format!("- Branch: `{branch_ref}`\n"));
    }
    markdown.push_str(&format!(
        "- Base revision graph version: `{}`\n",
        task.task.base_revision.graph_version
    ));
    markdown.push('\n');

    render_plan_binding(markdown, &task.task.bindings);
    if !task.task.acceptance.is_empty() {
        markdown.push_str("#### Acceptance\n\n");
        for criterion in &task.task.acceptance {
            markdown.push_str(&format!("- {}\n", criterion.label));
            for anchor in &criterion.anchors {
                markdown.push_str("- Anchor: `");
                markdown.push_str(&anchor_label(anchor));
                markdown.push_str("`\n");
            }
        }
        markdown.push('\n');
    }
    if !task.task.validation_refs.is_empty() {
        markdown.push_str("#### Validation Refs\n\n");
        for validation in &task.task.validation_refs {
            markdown.push_str("- `");
            markdown.push_str(&validation.id);
            markdown.push_str("`\n");
        }
        markdown.push('\n');
    }
    if !task.task.tags.is_empty() {
        markdown.push_str("#### Tags\n\n");
        for tag in &task.task.tags {
            markdown.push_str("- `");
            markdown.push_str(tag);
            markdown.push_str("`\n");
        }
        markdown.push('\n');
    }
    render_task_git_execution(markdown, &task.task);
}

fn descendant_task_ids_for_plan(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> Vec<TaskId> {
    fn collect(
        graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
        plan_id: &PlanId,
        task_ids: &mut Vec<TaskId>,
    ) {
        for child in graph.children_of_plan(plan_id) {
            match child.kind {
                NodeRefKind::Task => task_ids.push(TaskId::new(child.id)),
                NodeRefKind::Plan => collect(graph, &PlanId::new(child.id), task_ids),
            }
        }
    }

    let mut task_ids = Vec::new();
    collect(graph, plan_id, &mut task_ids);
    task_ids.sort_by(|left, right| left.0.cmp(&right.0));
    task_ids.dedup_by(|left, right| left == right);
    task_ids
}

fn subtree_node_refs(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> BTreeSet<(NodeRefKind, String)> {
    fn collect(
        graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
        plan_id: &PlanId,
        nodes: &mut BTreeSet<(NodeRefKind, String)>,
    ) {
        let plan_ref = node_ref_key(&NodeRef::plan(plan_id.clone()));
        if !nodes.insert(plan_ref) {
            return;
        }
        for child in graph.children_of_plan(plan_id) {
            nodes.insert(node_ref_key(&child));
            if child.kind == NodeRefKind::Plan {
                collect(graph, &PlanId::new(child.id), nodes);
            }
        }
    }

    let mut nodes = BTreeSet::new();
    collect(graph, plan_id, &mut nodes);
    nodes
}

fn render_task_git_execution(markdown: &mut String, task: &CanonicalTaskRecord) {
    let git_execution = &task.git_execution;
    if git_execution == &prism_coordination::TaskGitExecution::default() {
        return;
    }
    markdown.push_str("#### Git Execution\n\n");
    markdown.push_str(&format!(
        "- Status: `{}`\n",
        format_git_execution_status(git_execution.status)
    ));
    if let Some(status) = git_execution.pending_task_status {
        markdown.push_str(&format!(
            "- Pending task status: `{}`\n",
            format_coordination_task_status(status)
        ));
    }
    if let Some(source_ref) = git_execution.source_ref.as_ref() {
        markdown.push_str(&format!("- Source ref: `{source_ref}`\n"));
    }
    if let Some(target_ref) = git_execution.target_ref.as_ref() {
        markdown.push_str(&format!("- Target ref: `{target_ref}`\n"));
    }
    if let Some(publish_ref) = git_execution.publish_ref.as_ref() {
        markdown.push_str(&format!("- Publish ref: `{publish_ref}`\n"));
    }
    if let Some(target_branch) = git_execution.target_branch.as_ref() {
        markdown.push_str(&format!("- Target branch: `{target_branch}`\n"));
    }
    if let Some(review_artifact_ref) = git_execution.review_artifact_ref.as_ref() {
        markdown.push_str(&format!("- Review artifact ref: `{review_artifact_ref}`\n"));
    }
    markdown.push('\n');
}

fn format_node_ref(node_ref: &NodeRef) -> String {
    let kind = match node_ref.kind {
        NodeRefKind::Plan => "plan",
        NodeRefKind::Task => "task",
    };
    format!("{kind}:{}", node_ref.id)
}

fn node_ref_key(node_ref: &NodeRef) -> (NodeRefKind, String) {
    (node_ref.kind, node_ref.id.clone())
}

fn dependency_arrow() -> &'static str {
    "depends on"
}

fn render_plan_binding(markdown: &mut String, binding: &prism_ir::PlanBinding) {
    if binding.anchors.is_empty()
        && binding.concept_handles.is_empty()
        && binding.artifact_refs.is_empty()
        && binding.memory_refs.is_empty()
        && binding.outcome_refs.is_empty()
    {
        return;
    }
    markdown.push_str("#### Bindings\n\n");
    for anchor in &binding.anchors {
        markdown.push_str("- Anchor: `");
        markdown.push_str(&anchor_label(anchor));
        markdown.push_str("`\n");
    }
    for handle in &binding.concept_handles {
        markdown.push_str("- Concept: `");
        markdown.push_str(handle);
        markdown.push_str("`\n");
    }
    for reference in &binding.artifact_refs {
        markdown.push_str("- Artifact: `");
        markdown.push_str(reference);
        markdown.push_str("`\n");
    }
    for reference in &binding.memory_refs {
        markdown.push_str("- Memory: `");
        markdown.push_str(reference);
        markdown.push_str("`\n");
    }
    for reference in &binding.outcome_refs {
        markdown.push_str("- Outcome: `");
        markdown.push_str(reference);
        markdown.push_str("`\n");
    }
    markdown.push('\n');
}

fn remove_stale_plan_docs(root: &Path, expected_paths: &BTreeSet<PathBuf>) -> Result<()> {
    let plans_root = root.join("docs").join("prism").join("plans");
    for bucket in [PlanDocBucket::Active, PlanDocBucket::Archived] {
        let bucket_dir = plans_root.join(bucket.as_str());
        if !bucket_dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&bucket_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .expect("plan doc path should stay under repo root")
                .to_path_buf();
            if !expected_paths.contains(&relative) {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn plan_bucket(status: PlanStatus) -> PlanDocBucket {
    if status == PlanStatus::Archived {
        PlanDocBucket::Archived
    } else {
        PlanDocBucket::Active
    }
}

fn plan_doc_path(bucket: PlanDocBucket, plan_id: &str) -> PathBuf {
    PathBuf::from("docs")
        .join("prism")
        .join("plans")
        .join(bucket.as_str())
        .join(format!("{}.md", sanitize_plan_id(plan_id)))
}

fn sanitize_plan_id(plan_id: &str) -> String {
    plan_id
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '-',
        })
        .collect()
}

fn metadata_object_lines(metadata: &Value, key: &str) -> Option<Vec<String>> {
    let object = metadata.get(key)?.as_object()?;
    let mut lines = object
        .iter()
        .map(|(entry_key, value)| (entry_key.clone(), value_to_markdown(value)))
        .collect::<Vec<_>>();
    lines.sort_by(|left, right| left.0.cmp(&right.0));
    Some(
        lines
            .into_iter()
            .map(|(entry_key, value)| format!("{entry_key}: {value}"))
            .collect(),
    )
}

fn value_to_markdown(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(string) => format!("`{string}`"),
        Value::Array(items) => {
            if items.is_empty() {
                "[]".to_string()
            } else {
                items
                    .iter()
                    .map(value_to_markdown)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            format!("{{{}}}", keys.join(", "))
        }
    }
}

fn write_projection_metadata_section(markdown: &mut String, metadata: &ProjectionMetadata) {
    markdown.push_str("## Projection Metadata\n\n");
    markdown.push_str("- Projection class: `published`\n");
    markdown.push_str("- Authority planes: `published_repo`\n");
    markdown.push_str(&format!(
        "- Projection version: `{}`\n",
        STATE_PROJECTION_VERSION
    ));
    markdown.push_str(&format!("- Source head: `{}`\n", metadata.source_head));
    if let Some(timestamp) = metadata.source_logical_timestamp {
        markdown.push_str(&format!("- Source logical timestamp: `{timestamp}`\n"));
    } else {
        markdown.push_str("- Source logical timestamp: `unknown`\n");
    }
    markdown.push_str(&format!(
        "- Source snapshot: `{}`\n\n",
        metadata.source_snapshot
    ));
}

fn format_memory_kind(kind: prism_memory::MemoryKind) -> &'static str {
    match kind {
        prism_memory::MemoryKind::Episodic => "episodic",
        prism_memory::MemoryKind::Structural => "structural",
        prism_memory::MemoryKind::Semantic => "semantic",
    }
}

fn format_memory_source(source: prism_memory::MemorySource) -> &'static str {
    match source {
        prism_memory::MemorySource::Agent => "agent",
        prism_memory::MemorySource::User => "user",
        prism_memory::MemorySource::System => "system",
    }
}

fn format_plan_kind(kind: PlanKind) -> &'static str {
    match kind {
        PlanKind::TaskExecution => "task_execution",
        PlanKind::Investigation => "investigation",
        PlanKind::Refactor => "refactor",
        PlanKind::Migration => "migration",
        PlanKind::Release => "release",
        PlanKind::IncidentResponse => "incident_response",
        PlanKind::Maintenance => "maintenance",
        PlanKind::Custom => "custom",
    }
}

fn format_plan_scope(scope: PlanScope) -> &'static str {
    match scope {
        PlanScope::Local => "local",
        PlanScope::Session => "session",
        PlanScope::Repo => "repo",
    }
}

fn format_plan_status(status: PlanStatus) -> &'static str {
    match status {
        PlanStatus::Draft => "draft",
        PlanStatus::Active => "active",
        PlanStatus::Blocked => "blocked",
        PlanStatus::Completed => "completed",
        PlanStatus::Abandoned => "abandoned",
        PlanStatus::Archived => "archived",
    }
}

fn format_derived_plan_status(status: DerivedPlanStatus) -> &'static str {
    match status {
        DerivedPlanStatus::Pending => "pending",
        DerivedPlanStatus::Active => "active",
        DerivedPlanStatus::Blocked => "blocked",
        DerivedPlanStatus::BrokenDependency => "broken_dependency",
        DerivedPlanStatus::Completed => "completed",
        DerivedPlanStatus::Failed => "failed",
        DerivedPlanStatus::Abandoned => "abandoned",
        DerivedPlanStatus::Archived => "archived",
    }
}

fn format_effective_task_status(status: EffectiveTaskStatus) -> &'static str {
    match status {
        EffectiveTaskStatus::Pending => "pending",
        EffectiveTaskStatus::Active => "active",
        EffectiveTaskStatus::Blocked => "blocked",
        EffectiveTaskStatus::BrokenDependency => "broken_dependency",
        EffectiveTaskStatus::Completed => "completed",
        EffectiveTaskStatus::Failed => "failed",
        EffectiveTaskStatus::Abandoned => "abandoned",
    }
}

fn format_task_lifecycle_status(status: TaskLifecycleStatus) -> &'static str {
    match status {
        TaskLifecycleStatus::Pending => "pending",
        TaskLifecycleStatus::Active => "active",
        TaskLifecycleStatus::Completed => "completed",
        TaskLifecycleStatus::Failed => "failed",
        TaskLifecycleStatus::Abandoned => "abandoned",
    }
}

fn format_git_execution_status(status: GitExecutionStatus) -> &'static str {
    match status {
        GitExecutionStatus::NotStarted => "not_started",
        GitExecutionStatus::PreflightFailed => "preflight_failed",
        GitExecutionStatus::InProgress => "in_progress",
        GitExecutionStatus::PublishPending => "publish_pending",
        GitExecutionStatus::PublishFailed => "publish_failed",
        GitExecutionStatus::CoordinationPublished => "coordination_published",
    }
}

fn format_git_execution_start_mode(
    mode: prism_coordination::GitExecutionStartMode,
) -> &'static str {
    match mode {
        prism_coordination::GitExecutionStartMode::Off => "off",
        prism_coordination::GitExecutionStartMode::Require => "require",
    }
}

fn format_git_execution_completion_mode(
    mode: prism_coordination::GitExecutionCompletionMode,
) -> &'static str {
    match mode {
        prism_coordination::GitExecutionCompletionMode::Off => "off",
        prism_coordination::GitExecutionCompletionMode::Require => "require",
    }
}

fn format_coordination_task_status(status: CoordinationTaskStatus) -> &'static str {
    match status {
        CoordinationTaskStatus::Proposed => "proposed",
        CoordinationTaskStatus::Ready => "ready",
        CoordinationTaskStatus::InProgress => "in_progress",
        CoordinationTaskStatus::Blocked => "blocked",
        CoordinationTaskStatus::InReview => "in_review",
        CoordinationTaskStatus::Validating => "validating",
        CoordinationTaskStatus::Completed => "completed",
        CoordinationTaskStatus::Abandoned => "abandoned",
    }
}

fn first_line(content: &str) -> &str {
    content.lines().next().unwrap_or(content)
}

fn compatibility_plan_status(status: DerivedPlanStatus) -> PlanStatus {
    match status {
        DerivedPlanStatus::Pending => PlanStatus::Draft,
        DerivedPlanStatus::Active => PlanStatus::Active,
        DerivedPlanStatus::Blocked => PlanStatus::Blocked,
        DerivedPlanStatus::BrokenDependency => PlanStatus::Blocked,
        DerivedPlanStatus::Completed => PlanStatus::Completed,
        DerivedPlanStatus::Failed => PlanStatus::Blocked,
        DerivedPlanStatus::Abandoned => PlanStatus::Abandoned,
        DerivedPlanStatus::Archived => PlanStatus::Archived,
    }
}
