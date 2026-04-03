use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_coordination::CoordinationPolicy;
use prism_ir::{
    AcceptanceEvidencePolicy, CoordinationTaskStatus, GitExecutionStatus, PlanEdgeKind,
    PlanExecutionOverlay, PlanGraph, PlanKind, PlanNode, PlanNodeKind, PlanNodeStatus, PlanScope,
    PlanStatus,
};
use prism_memory::{MemoryEntry, MemoryEvent, MemoryEventKind, OutcomeEvent, OutcomeEvidence};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::memory_events::load_repo_memory_events;
use crate::published_plans::{
    load_hydrated_coordination_plan_state, load_repo_published_plan_index, PublishedPlanIndexEntry,
};
use crate::repo_patch_events::load_repo_patch_events;

use super::{anchor_label, write_generated_file, PrismDocFileSync};

const STATE_PROJECTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RepoStateSummary {
    pub(super) memory_count: usize,
    pub(super) plan_count: usize,
    pub(super) change_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct RepoStateCatalog {
    memories: Vec<PublishedMemoryRecord>,
    memory_events: Vec<MemoryEvent>,
    patch_events: Vec<OutcomeEvent>,
    plans: Vec<PublishedPlanDoc>,
}

impl RepoStateCatalog {
    pub(super) fn load(root: &Path) -> Result<Self> {
        let memory_events = load_repo_memory_events(root)?;
        let patch_events = load_repo_patch_events(root)?;
        let plan_state = load_hydrated_coordination_plan_state(root, None)?;
        let plan_index = load_repo_published_plan_index(root)?
            .into_iter()
            .map(|entry| (entry.plan_id.0.clone(), entry))
            .collect::<HashMap<_, _>>();
        let plan_policies = plan_state
            .as_ref()
            .map(|state| {
                state
                    .snapshot
                    .plans
                    .iter()
                    .map(|plan| (plan.id.0.clone(), plan.policy.clone()))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let execution_overlays = plan_state
            .as_ref()
            .map(|state| state.execution_overlays.clone())
            .unwrap_or_default();
        let mut plans = plan_state
            .map(|state| state.plan_graphs)
            .unwrap_or_default()
            .into_iter()
            .map(|graph| {
                let key = graph.id.0.clone();
                let index = plan_index.get(&key).cloned();
                let bucket = plan_bucket(index.as_ref(), graph.status);
                let overlays = execution_overlays
                    .get(key.as_str())
                    .cloned()
                    .unwrap_or_default();
                PublishedPlanDoc {
                    graph,
                    policy: plan_policies.get(&key).cloned().unwrap_or_default(),
                    index,
                    overlays,
                    bucket,
                    doc_path: plan_doc_path(bucket, &key),
                }
            })
            .collect::<Vec<_>>();
        plans.sort_by(|left, right| {
            left.bucket
                .sort_key()
                .cmp(&right.bucket.sort_key())
                .then_with(|| {
                    left.graph
                        .title
                        .to_ascii_lowercase()
                        .cmp(&right.graph.title.to_ascii_lowercase())
                })
                .then_with(|| left.graph.id.0.cmp(&right.graph.id.0))
        });

        Ok(Self {
            memories: project_memories(&memory_events),
            memory_events,
            patch_events,
            plans,
        })
    }

    pub(super) fn summary(&self) -> RepoStateSummary {
        RepoStateSummary {
            memory_count: self.memories.len(),
            plan_count: self.plans.len(),
            change_count: self.patch_events.len(),
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
    graph: PlanGraph,
    policy: CoordinationPolicy,
    index: Option<PublishedPlanIndexEntry>,
    overlays: Vec<PlanExecutionOverlay>,
    bucket: PlanDocBucket,
    doc_path: PathBuf,
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

pub(super) fn sync_repo_state_docs(
    root: &Path,
    catalog: &RepoStateCatalog,
) -> Result<Vec<PrismDocFileSync>> {
    let prism_docs_dir = root.join("docs").join("prism");
    let plan_docs_dir = prism_docs_dir.join("plans");
    fs::create_dir_all(&plan_docs_dir)?;

    let mut files = Vec::new();
    files.push(write_generated_file(
        prism_docs_dir.join("memory.md"),
        render_memory_doc(catalog),
    )?);
    files.push(write_generated_file(
        prism_docs_dir.join("changes.md"),
        render_changes_doc(catalog),
    )?);
    files.push(write_generated_file(
        plan_docs_dir.join("index.md"),
        render_plan_index_doc(catalog),
    )?);

    let mut expected_plan_docs = BTreeSet::new();
    for plan in &catalog.plans {
        expected_plan_docs.insert(plan.doc_path.clone());
        files.push(write_generated_file(
            root.join(&plan.doc_path),
            render_plan_doc(plan),
        )?);
    }
    remove_stale_plan_docs(root, &expected_plan_docs)?;

    Ok(files)
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

fn render_changes_doc(catalog: &RepoStateCatalog) -> String {
    let metadata = ProjectionMetadata::from_sources(
        &catalog.patch_events,
        catalog.patch_events.iter().map(|event| event.meta.ts).max(),
        format!(
            "{} published patch events, {} unique touched files",
            catalog.patch_events.len(),
            touched_file_counts(&catalog.patch_events).len()
        ),
    );
    let mut markdown = String::new();
    markdown.push_str("# PRISM Changes\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM patch events.\n");
    markdown.push_str("> Return to the concise entrypoint in `../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &metadata);

    let file_counts = touched_file_counts(&catalog.patch_events);
    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Published patch events: {}\n",
        catalog.patch_events.len()
    ));
    markdown.push_str(&format!(
        "- Unique files touched: {}\n\n",
        file_counts.len()
    ));

    if catalog.patch_events.is_empty() {
        markdown.push_str("No repo-scoped patch events are currently published.\n");
        return markdown;
    }

    markdown.push_str("## Most Touched Files\n\n");
    for (path, count) in file_counts.into_iter().take(20) {
        markdown.push_str(&format!("- `{path}`: `{count}` patch event(s)\n"));
    }
    markdown.push('\n');

    markdown.push_str("## Recent Published Patch Events\n\n");
    let mut recent = catalog.patch_events.clone();
    recent.sort_by(|left, right| {
        right
            .meta
            .ts
            .cmp(&left.meta.ts)
            .then_with(|| right.meta.id.0.cmp(&left.meta.id.0))
    });
    for event in recent.into_iter().take(25) {
        markdown.push_str(&format!("### {}\n\n", event.meta.id.0));
        markdown.push_str(&format!(
            "- Summary: {}\n- Result: `{}`\n- Recorded at: `{}`\n",
            event.summary,
            format_outcome_result(event.result),
            event.meta.ts
        ));
        if let Some(work_context) = event
            .meta
            .execution_context
            .as_ref()
            .and_then(|context| context.work_context.as_ref())
        {
            markdown.push_str(&format!(
                "- Work: `{}` ({})\n",
                work_context.title, work_context.work_id
            ));
        }
        let files = extract_file_paths(&event);
        if !files.is_empty() {
            markdown.push_str("- Files:\n");
            for path in files {
                markdown.push_str("  - `");
                markdown.push_str(&path);
                markdown.push_str("`\n");
            }
        }
        let evidence = format_outcome_evidence(&event.evidence);
        if !evidence.is_empty() {
            markdown.push_str("- Evidence:\n");
            for item in evidence {
                markdown.push_str("  - ");
                markdown.push_str(&item);
                markdown.push('\n');
            }
        }
        markdown.push('\n');
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

fn render_plan_doc(plan: &PublishedPlanDoc) -> String {
    let metadata = ProjectionMetadata::from_sources(
        &(
            plan.graph.clone(),
            plan.index.clone(),
            plan.overlays.clone(),
        ),
        None,
        format!(
            "{} nodes, {} edges, {} overlays",
            plan.graph.nodes.len(),
            plan.graph.edges.len(),
            plan.overlays.len()
        ),
    );
    let mut markdown = String::new();
    markdown.push_str("# ");
    markdown.push_str(&plan.graph.title);
    markdown.push_str("\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM plan state.\n");
    markdown.push_str("> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Plan id: `{}`\n- Status: `{}`\n- Kind: `{}`\n- Scope: `{}`\n- Revision: `{}`\n- Nodes: `{}`\n- Edges: `{}`\n\n",
        plan.graph.id.0,
        format_plan_status(plan.graph.status),
        format_plan_kind(plan.graph.kind),
        format_plan_scope(plan.graph.scope),
        plan.graph.revision,
        plan.graph.nodes.len(),
        plan.graph.edges.len()
    ));

    markdown.push_str("## Goal\n\n");
    markdown.push_str(&plan.graph.goal);
    markdown.push_str("\n\n");

    markdown.push_str("## Git Execution Policy\n\n");
    markdown.push_str(&format!(
        "- Start mode: `{}`\n- Completion mode: `{}`\n- Target branch: `{}`\n",
        format_git_execution_start_mode(plan.policy.git_execution.start_mode),
        format_git_execution_completion_mode(plan.policy.git_execution.completion_mode),
        plan.policy.git_execution.target_branch,
    ));
    if let Some(target_ref) = plan.policy.git_execution.target_ref.as_ref() {
        markdown.push_str(&format!("- Target ref: `{target_ref}`\n"));
    }
    markdown.push_str(&format!(
        "- Require task branch: `{}`\n- Max commits behind target: `{}`\n",
        plan.policy.git_execution.require_task_branch,
        plan.policy.git_execution.max_commits_behind_target,
    ));
    if let Some(max_fetch_age_seconds) = plan.policy.git_execution.max_fetch_age_seconds {
        markdown.push_str(&format!(
            "- Max fetch age seconds: `{max_fetch_age_seconds}`\n"
        ));
    }
    markdown.push('\n');

    markdown.push_str("## Source of Truth\n\n");
    markdown.push_str("- Index path: `.prism/plans/index.jsonl`\n");
    if let Some(index) = &plan.index {
        markdown.push_str("- Log path: `");
        markdown.push_str(&index.log_path);
        markdown.push_str("`\n\n");
    } else {
        markdown.push_str("- Log path: unavailable in the current projection\n\n");
    }

    if !plan.graph.root_nodes.is_empty() {
        markdown.push_str("## Root Nodes\n\n");
        for node_id in &plan.graph.root_nodes {
            markdown.push_str("- `");
            markdown.push_str(&node_id.0);
            markdown.push_str("`\n");
        }
        markdown.push('\n');
    }

    markdown.push_str("## Nodes\n\n");
    if plan.graph.nodes.is_empty() {
        markdown.push_str("No published plan nodes are currently recorded.\n\n");
    } else {
        for node in &plan.graph.nodes {
            render_plan_node(&mut markdown, node);
        }
    }

    markdown.push_str("## Edges\n\n");
    if plan.graph.edges.is_empty() {
        markdown.push_str("No published plan edges are currently recorded.\n\n");
    } else {
        for edge in &plan.graph.edges {
            markdown.push_str(&format!(
                "- `{}`: `{}` {} `{}`\n",
                edge.id.0,
                edge.from.0,
                format_plan_edge_kind(edge.kind),
                edge.to.0
            ));
            if let Some(summary) = edge.summary.as_ref() {
                markdown.push_str("  summary: ");
                markdown.push_str(summary);
                markdown.push('\n');
            }
        }
        markdown.push('\n');
    }

    if !plan.overlays.is_empty() {
        markdown.push_str("## Execution Overlays\n\n");
        for overlay in &plan.overlays {
            markdown.push_str(&format!("- Node: `{}`\n", overlay.node_id.0));
            if let Some(agent) = overlay.effective_assignee.as_ref() {
                markdown.push_str(&format!("  effective assignee: `{}`\n", agent.0));
            }
            if let Some(agent) = overlay.pending_handoff_to.as_ref() {
                markdown.push_str(&format!("  pending handoff to: `{}`\n", agent.0));
            }
            if let Some(node_id) = overlay.awaiting_handoff_from.as_ref() {
                markdown.push_str(&format!("  awaiting handoff from: `{}`\n", node_id.0));
            }
            if let Some(git_execution) = overlay.git_execution.as_ref() {
                markdown.push_str(&format!(
                    "  git execution status: `{}`\n",
                    format_git_execution_status(git_execution.status)
                ));
                if let Some(status) = git_execution.pending_task_status {
                    markdown.push_str(&format!(
                        "  pending task status: `{}`\n",
                        format_coordination_task_status(status)
                    ));
                }
                if let Some(source_ref) = git_execution.source_ref.as_ref() {
                    markdown.push_str(&format!("  source ref: `{}`\n", source_ref));
                }
                if let Some(target_ref) = git_execution.target_ref.as_ref() {
                    markdown.push_str(&format!("  target ref: `{}`\n", target_ref));
                }
                if let Some(publish_ref) = git_execution.publish_ref.as_ref() {
                    markdown.push_str(&format!("  publish ref: `{}`\n", publish_ref));
                }
            }
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
            plan.graph.title, plan.graph.goal, plan.graph.id.0
        ));
    }
    markdown.push('\n');
}

fn render_plan_node(markdown: &mut String, node: &PlanNode) {
    markdown.push_str(&format!("### {}\n\n", node.title));
    markdown.push_str(&format!(
        "- Node id: `{}`\n- Kind: `{}`\n- Status: `{}`\n",
        node.id.0,
        format_plan_node_kind(node.kind),
        format_plan_node_status(node.status)
    ));
    if let Some(summary) = node.summary.as_ref() {
        markdown.push_str("- Summary: ");
        markdown.push_str(summary);
        markdown.push('\n');
    }
    if let Some(priority) = node.priority {
        markdown.push_str(&format!("- Priority: `{priority}`\n"));
    }
    if node.is_abstract {
        markdown.push_str("- Abstract: `true`\n");
    }
    if let Some(assignee) = node.assignee.as_ref() {
        markdown.push_str(&format!("- Assignee: `{}`\n", assignee.0));
    }
    markdown.push('\n');

    render_plan_binding(markdown, &node.bindings);
    if !node.acceptance.is_empty() {
        markdown.push_str("#### Acceptance\n\n");
        for criterion in &node.acceptance {
            markdown.push_str(&format!(
                "- {} [{}]\n",
                criterion.label,
                format_acceptance_policy(criterion.evidence_policy)
            ));
            for anchor in &criterion.anchors {
                markdown.push_str("  anchor: `");
                markdown.push_str(&anchor_label(anchor));
                markdown.push_str("`\n");
            }
            for check in &criterion.required_checks {
                markdown.push_str("  check: `");
                markdown.push_str(&check.id);
                markdown.push_str("`\n");
            }
        }
        markdown.push('\n');
    }
    if !node.validation_refs.is_empty() {
        markdown.push_str("#### Validation Refs\n\n");
        for validation in &node.validation_refs {
            markdown.push_str("- `");
            markdown.push_str(&validation.id);
            markdown.push_str("`\n");
        }
        markdown.push('\n');
    }
    if !node.tags.is_empty() {
        markdown.push_str("#### Tags\n\n");
        for tag in &node.tags {
            markdown.push_str("- `");
            markdown.push_str(tag);
            markdown.push_str("`\n");
        }
        markdown.push('\n');
    }
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

fn plan_bucket(index: Option<&PublishedPlanIndexEntry>, status: PlanStatus) -> PlanDocBucket {
    if index
        .as_ref()
        .is_some_and(|entry| entry.log_path.contains("/archived/"))
        || status == PlanStatus::Archived
    {
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

fn touched_file_counts(events: &[OutcomeEvent]) -> Vec<(String, usize)> {
    let mut counts = HashMap::<String, usize>::new();
    for event in events {
        for path in extract_file_paths(event) {
            *counts.entry(path).or_insert(0) += 1;
        }
    }
    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    counts
}

fn extract_file_paths(event: &OutcomeEvent) -> Vec<String> {
    event.metadata["filePaths"]
        .as_array()
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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

fn format_outcome_result(result: prism_memory::OutcomeResult) -> &'static str {
    match result {
        prism_memory::OutcomeResult::Success => "success",
        prism_memory::OutcomeResult::Failure => "failure",
        prism_memory::OutcomeResult::Partial => "partial",
        prism_memory::OutcomeResult::Unknown => "unknown",
    }
}

fn format_outcome_evidence(evidence: &[OutcomeEvidence]) -> Vec<String> {
    evidence
        .iter()
        .map(|item| match item {
            OutcomeEvidence::Commit { sha } => format!("commit `{sha}`"),
            OutcomeEvidence::Test { name, passed } => {
                format!("test `{name}` (passed: `{passed}`)")
            }
            OutcomeEvidence::Build { target, passed } => {
                format!("build `{target}` (passed: `{passed}`)")
            }
            OutcomeEvidence::Command { argv, passed } => {
                format!("command `{}` (passed: `{passed}`)", argv.join(" "))
            }
            OutcomeEvidence::Reviewer { author } => format!("reviewer `{author}`"),
            OutcomeEvidence::Issue { id } => format!("issue `{id}`"),
            OutcomeEvidence::StackTrace { hash } => format!("stack trace `{hash}`"),
            OutcomeEvidence::DiffSummary { text } => text.clone(),
        })
        .collect()
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

fn format_plan_node_kind(kind: PlanNodeKind) -> &'static str {
    match kind {
        PlanNodeKind::Investigate => "investigate",
        PlanNodeKind::Decide => "decide",
        PlanNodeKind::Edit => "edit",
        PlanNodeKind::Validate => "validate",
        PlanNodeKind::Review => "review",
        PlanNodeKind::Handoff => "handoff",
        PlanNodeKind::Merge => "merge",
        PlanNodeKind::Release => "release",
        PlanNodeKind::Note => "note",
    }
}

fn format_plan_node_status(status: PlanNodeStatus) -> &'static str {
    match status {
        PlanNodeStatus::Proposed => "proposed",
        PlanNodeStatus::Ready => "ready",
        PlanNodeStatus::InProgress => "in_progress",
        PlanNodeStatus::Blocked => "blocked",
        PlanNodeStatus::Waiting => "waiting",
        PlanNodeStatus::InReview => "in_review",
        PlanNodeStatus::Validating => "validating",
        PlanNodeStatus::Completed => "completed",
        PlanNodeStatus::Abandoned => "abandoned",
    }
}

fn format_plan_edge_kind(kind: PlanEdgeKind) -> &'static str {
    match kind {
        PlanEdgeKind::DependsOn => "depends on",
        PlanEdgeKind::Blocks => "blocks",
        PlanEdgeKind::Informs => "informs",
        PlanEdgeKind::Validates => "validates",
        PlanEdgeKind::HandoffTo => "handoff to",
        PlanEdgeKind::ChildOf => "child of",
        PlanEdgeKind::RelatedTo => "related to",
    }
}

fn format_git_execution_status(status: GitExecutionStatus) -> &'static str {
    match status {
        GitExecutionStatus::NotStarted => "not_started",
        GitExecutionStatus::PreflightFailed => "preflight_failed",
        GitExecutionStatus::InProgress => "in_progress",
        GitExecutionStatus::PublishPending => "publish_pending",
        GitExecutionStatus::PublishFailed => "publish_failed",
        GitExecutionStatus::Published => "published",
    }
}

fn format_git_execution_start_mode(
    mode: prism_coordination::GitExecutionStartMode,
) -> &'static str {
    match mode {
        prism_coordination::GitExecutionStartMode::Off => "off",
        prism_coordination::GitExecutionStartMode::Require => "require",
        prism_coordination::GitExecutionStartMode::Auto => "auto",
    }
}

fn format_git_execution_completion_mode(
    mode: prism_coordination::GitExecutionCompletionMode,
) -> &'static str {
    match mode {
        prism_coordination::GitExecutionCompletionMode::Off => "off",
        prism_coordination::GitExecutionCompletionMode::Require => "require",
        prism_coordination::GitExecutionCompletionMode::Auto => "auto",
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

fn format_acceptance_policy(policy: AcceptanceEvidencePolicy) -> &'static str {
    match policy {
        AcceptanceEvidencePolicy::Any => "any",
        AcceptanceEvidencePolicy::All => "all",
        AcceptanceEvidencePolicy::ReviewOnly => "review_only",
        AcceptanceEvidencePolicy::ValidationOnly => "validation_only",
        AcceptanceEvidencePolicy::ReviewAndValidation => "review_and_validation",
    }
}

fn first_line(content: &str) -> &str {
    content.lines().next().unwrap_or(content)
}
