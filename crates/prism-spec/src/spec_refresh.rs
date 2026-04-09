use std::path::Path;

use anyhow::Result;
use prism_coordination::CoordinationSnapshot;

use crate::{
    discover_spec_sources, parse_spec_sources, resolve_spec_root, SpecMaterializedReplaceRequest,
    SpecMaterializedStore, SpecMaterializedWriteResult, SpecParseDiagnostic, SpecRootResolution,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SpecMaterializationRefreshResult {
    pub root_resolution: SpecRootResolution,
    pub discovered_count: usize,
    pub diagnostics: Vec<SpecParseDiagnostic>,
    pub write_result: SpecMaterializedWriteResult,
}

pub fn refresh_spec_materialization<S>(
    store: &S,
    repo_root: &Path,
    coordination: Option<CoordinationSnapshot>,
) -> Result<SpecMaterializationRefreshResult>
where
    S: SpecMaterializedStore,
{
    let root_resolution = resolve_spec_root(repo_root)?;
    let discovered = discover_spec_sources(repo_root)?;
    let discovered_count = discovered.len();
    let parsed = parse_spec_sources(&discovered);
    let write_result = store.replace_materialization(SpecMaterializedReplaceRequest {
        parsed: parsed.parsed,
        coordination,
    })?;
    Ok(SpecMaterializationRefreshResult {
        root_resolution,
        discovered_count,
        diagnostics: parsed.diagnostics,
        write_result,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_coordination::{
        CoordinationPolicy, CoordinationSnapshot, CoordinationSpecRef, CoordinationTask,
        CoordinationTaskSpecRef, Plan, PlanScheduling, TaskGitExecution,
    };
    use prism_ir::{
        CoordinationTaskId, CoordinationTaskStatus, PlanId, PlanKind, PlanNodeKind, PlanScope,
        PlanStatus, WorkspaceRevision,
    };

    use crate::{refresh_spec_materialization, SpecMaterializedStore, SqliteSpecMaterializedStore};

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_repo(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("prism-spec-refresh-{label}-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        root
    }

    fn write_spec(root: &PathBuf, relative_path: &str, contents: &str) {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn refresh_spec_materialization_derives_sync_provenance_from_coordination_snapshot() {
        let root = temp_repo("sync-provenance");
        write_spec(
            &root,
            ".prism/specs/2026-04-09-a.md",
            "---\nid: spec:a\ntitle: Alpha\nstatus: in_progress\ncreated: 2026-04-09\n---\n\n- [ ] implement <!-- id: item-1 -->\n",
        );
        let store = SqliteSpecMaterializedStore::new(&root.join(".tmp/spec-materialized.db"));

        let refresh = refresh_spec_materialization(
            &store,
            &root,
            Some(CoordinationSnapshot {
                plans: vec![Plan {
                    id: PlanId::new("plan:alpha"),
                    goal: "Ship alpha".into(),
                    title: "Ship alpha".into(),
                    status: PlanStatus::Active,
                    policy: CoordinationPolicy::default(),
                    scope: PlanScope::Repo,
                    kind: PlanKind::TaskExecution,
                    revision: 1,
                    scheduling: PlanScheduling::default(),
                    tags: Vec::new(),
                    spec_refs: vec![CoordinationSpecRef {
                        spec_id: "spec:a".into(),
                        source_path: ".prism/specs/2026-04-09-a.md".into(),
                        source_revision: Some("abc123".into()),
                    }],
                    created_from: None,
                    metadata: serde_json::Value::Null,
                }],
                tasks: vec![CoordinationTask {
                    id: CoordinationTaskId::new("coord-task:alpha"),
                    plan: PlanId::new("plan:alpha"),
                    kind: PlanNodeKind::Edit,
                    title: "Implement alpha".into(),
                    summary: None,
                    status: CoordinationTaskStatus::Ready,
                    published_task_status: None,
                    assignee: None,
                    pending_handoff_to: None,
                    session: None,
                    lease_holder: None,
                    lease_started_at: None,
                    lease_refreshed_at: None,
                    lease_stale_at: None,
                    lease_expires_at: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    bindings: Default::default(),
                    depends_on: Vec::new(),
                    coordination_depends_on: Vec::new(),
                    integrated_depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    validation_refs: Vec::new(),
                    is_abstract: false,
                    base_revision: WorkspaceRevision::default(),
                    priority: None,
                    tags: Vec::new(),
                    spec_refs: vec![CoordinationTaskSpecRef {
                        spec_id: "spec:a".into(),
                        source_path: ".prism/specs/2026-04-09-a.md".into(),
                        source_revision: Some("def456".into()),
                        sync_kind: "task".into(),
                        covered_checklist_items: vec!["spec:a::checklist::item-1".into()],
                        covered_sections: Vec::new(),
                    }],
                    metadata: serde_json::Value::Null,
                    git_execution: TaskGitExecution::default(),
                }],
                claims: Vec::new(),
                artifacts: Vec::new(),
                reviews: Vec::new(),
                events: Vec::new(),
                next_plan: 2,
                next_task: 2,
                next_claim: 1,
                next_artifact: 1,
                next_review: 1,
            }),
        )
        .unwrap();

        assert_eq!(refresh.discovered_count, 1);
        assert!(refresh.diagnostics.is_empty());
        assert_eq!(
            refresh.write_result.metadata.sync_provenance_record_count,
            2
        );

        let sync_provenance = store.read_sync_provenance_records().unwrap();
        assert_eq!(sync_provenance.value.len(), 2);
        assert_eq!(
            sync_provenance.value[0].target_coordination_ref,
            "coord-task:alpha"
        );
        assert_eq!(sync_provenance.value[0].sync_kind, "task");
        assert_eq!(
            sync_provenance.value[0].covered_checklist_items,
            vec!["spec:a::checklist::item-1"]
        );
        assert_eq!(
            sync_provenance.value[1].target_coordination_ref,
            "plan:alpha"
        );
        assert_eq!(sync_provenance.value[1].sync_kind, "plan");
    }
}
