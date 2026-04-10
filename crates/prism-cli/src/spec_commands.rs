use std::fmt::Write as _;

use anyhow::Result;
use prism_core::{SpecQueryEngine, SpecQueryLookup, WorkspaceSession, WorkspaceSpecSurface};

use crate::cli::SpecsCommand;

pub(crate) fn handle_specs_command(
    session: &WorkspaceSession,
    command: SpecsCommand,
) -> Result<()> {
    match command {
        SpecsCommand::List => {
            let rendered = with_spec_query_engine(session, |engine| {
                let entries = engine.list_specs()?;
                Ok(render_spec_list(&entries))
            })?;
            print!("{rendered}");
        }
        SpecsCommand::Show { spec_id } => {
            let rendered = with_spec_query_engine(session, |engine| {
                Ok(match engine.spec(&spec_id)? {
                    SpecQueryLookup::Found(view) => Some(render_spec_document(&view)),
                    SpecQueryLookup::NotFound => None,
                })
            })?;
            match rendered {
                Some(rendered) => print!("{rendered}"),
                None => eprintln!("no spec matched `{spec_id}`"),
            }
        }
        SpecsCommand::SyncBrief { spec_id } => {
            let rendered = with_spec_query_engine(session, |engine| {
                Ok(match engine.sync_brief(&spec_id)? {
                    SpecQueryLookup::Found(view) => Some(render_spec_sync_brief(&view)),
                    SpecQueryLookup::NotFound => None,
                })
            })?;
            match rendered {
                Some(rendered) => print!("{rendered}"),
                None => eprintln!("no spec matched `{spec_id}`"),
            }
        }
        SpecsCommand::Coverage { spec_id } => {
            let rendered = with_spec_query_engine(session, |engine| {
                Ok(match engine.coverage(&spec_id)? {
                    SpecQueryLookup::Found(view) => Some(render_spec_coverage(&view)),
                    SpecQueryLookup::NotFound => None,
                })
            })?;
            match rendered {
                Some(rendered) => print!("{rendered}"),
                None => eprintln!("no spec matched `{spec_id}`"),
            }
        }
        SpecsCommand::SyncProvenance { spec_id } => {
            let rendered = with_spec_query_engine(session, |engine| {
                Ok(match engine.sync_provenance(&spec_id)? {
                    SpecQueryLookup::Found(view) => Some(render_spec_sync_provenance(&view)),
                    SpecQueryLookup::NotFound => None,
                })
            })?;
            match rendered {
                Some(rendered) => print!("{rendered}"),
                None => eprintln!("no spec matched `{spec_id}`"),
            }
        }
    }
    Ok(())
}

fn with_spec_query_engine<T, F>(session: &WorkspaceSession, f: F) -> Result<T>
where
    F: FnOnce(&dyn SpecQueryEngine) -> Result<T>,
{
    let surface = WorkspaceSpecSurface::new(session.root());
    surface.with_query_engine(Some(session.prism().coordination_snapshot_v2()), f)
}

fn render_spec_list(entries: &[prism_core::SpecListEntry]) -> String {
    if entries.is_empty() {
        return "no specs\n".to_string();
    }
    let mut rendered = String::new();
    for entry in entries {
        let overall = entry.overall_status.as_deref().unwrap_or("unknown");
        let _ = writeln!(
            rendered,
            "{}  title={} declared={} overall={} created={} source={}",
            entry.spec_id,
            entry.title,
            entry.declared_status,
            overall,
            entry.created,
            entry.source_path
        );
    }
    rendered
}

fn render_spec_document(view: &prism_core::SpecDocumentView) -> String {
    let mut rendered = String::new();
    let overall = view
        .status
        .as_ref()
        .map(|status| status.overall_status.as_str())
        .unwrap_or("unknown");
    let checklist_posture = view
        .status
        .as_ref()
        .map(|status| format!("{:?}", status.checklist_posture).to_lowercase())
        .unwrap_or_else(|| "unknown".to_string());
    let dependency_posture = view
        .status
        .as_ref()
        .map(|status| format!("{:?}", status.dependency_posture).to_lowercase())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = writeln!(rendered, "spec_id: {}", view.record.spec_id);
    let _ = writeln!(rendered, "title: {}", view.record.title);
    let _ = writeln!(rendered, "source_path: {}", view.record.source_path);
    let _ = writeln!(rendered, "declared_status: {}", view.record.declared_status);
    let _ = writeln!(rendered, "overall_status: {overall}");
    let _ = writeln!(rendered, "checklist_posture: {checklist_posture}");
    let _ = writeln!(rendered, "dependency_posture: {dependency_posture}");
    let _ = writeln!(rendered, "created: {}", view.record.created);
    let _ = writeln!(rendered, "content_digest: {}", view.record.content_digest);
    let _ = writeln!(
        rendered,
        "git_revision: {}",
        view.record.git_revision.as_deref().unwrap_or("<none>")
    );
    let _ = writeln!(rendered);
    let _ = writeln!(rendered, "body:");
    let _ = writeln!(rendered, "{}", view.record.body.trim_end());
    rendered
}

fn render_spec_sync_brief(view: &prism_core::SpecSyncBriefView) -> String {
    let mut rendered = String::new();
    let _ = writeln!(rendered, "spec_id: {}", view.spec.record.spec_id);
    let _ = writeln!(rendered, "title: {}", view.spec.record.title);
    let _ = writeln!(rendered, "required_checklist_items:");
    if view.required_checklist_items.is_empty() {
        let _ = writeln!(rendered, "  <none>");
    } else {
        for item in &view.required_checklist_items {
            let _ = writeln!(
                rendered,
                "  {} checked={} section_path={} line={}",
                item.item.item_id,
                item.item.checked,
                item.item.section_path.join(" > "),
                item.item.line_number
            );
        }
    }
    let _ = writeln!(rendered, "coverage:");
    if view.coverage.is_empty() {
        let _ = writeln!(rendered, "  <none>");
    } else {
        for record in &view.coverage {
            let _ = writeln!(
                rendered,
                "  {} kind={} coordination_ref={}",
                record.checklist_item_id,
                record.coverage_kind,
                record.coordination_ref.as_deref().unwrap_or("<none>")
            );
        }
    }
    let _ = writeln!(rendered, "linked_coordination_refs:");
    if view.linked_coordination_refs.is_empty() {
        let _ = writeln!(rendered, "  <none>");
    } else {
        for record in &view.linked_coordination_refs {
            let covered = if record.covered_checklist_items.is_empty() {
                "<none>".to_string()
            } else {
                record.covered_checklist_items.join(",")
            };
            let _ = writeln!(
                rendered,
                "  {} kind={} source_revision={} covered_items={}",
                record.target_coordination_ref,
                record.sync_kind,
                record.source_revision.as_deref().unwrap_or("<none>"),
                covered
            );
        }
    }
    rendered
}

fn render_spec_coverage(view: &prism_core::SpecCoverageView) -> String {
    let mut rendered = String::new();
    let _ = writeln!(rendered, "spec_id: {}", view.spec_id);
    if view.records.is_empty() {
        let _ = writeln!(rendered, "<none>");
        return rendered;
    }
    for record in &view.records {
        let _ = writeln!(
            rendered,
            "{} kind={} coordination_ref={}",
            record.checklist_item_id,
            record.coverage_kind,
            record.coordination_ref.as_deref().unwrap_or("<none>")
        );
    }
    rendered
}

fn render_spec_sync_provenance(view: &prism_core::SpecSyncProvenanceView) -> String {
    let mut rendered = String::new();
    let _ = writeln!(rendered, "spec_id: {}", view.spec_id);
    if view.records.is_empty() {
        let _ = writeln!(rendered, "<none>");
        return rendered;
    }
    for record in &view.records {
        let covered = if record.covered_checklist_items.is_empty() {
            "<none>".to_string()
        } else {
            record.covered_checklist_items.join(",")
        };
        let _ = writeln!(
            rendered,
            "{} kind={} source_revision={} covered_items={}",
            record.target_coordination_ref,
            record.sync_kind,
            record.source_revision.as_deref().unwrap_or("<none>"),
            covered
        );
    }
    rendered
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prism_coordination::{CoordinationSpecRef, CoordinationTaskSpecRef, TaskCreateInput};
    use prism_core::index_workspace_session;
    use prism_ir::{CoordinationTaskStatus, EventActor, EventId, PlanStatus, WorkspaceRevision};
    use prism_query::{NativeSpecPlanCreateInput, NativeSpecTaskCreateInput};

    use super::{
        render_spec_coverage, render_spec_document, render_spec_list, render_spec_sync_brief,
        render_spec_sync_provenance, with_spec_query_engine,
    };

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn spec_cli_renderers_surface_native_spec_views() {
        let root = temp_workspace("cli-specs");
        fs::create_dir_all(root.join(".prism/specs")).unwrap();
        fs::write(
            root.join(".prism/specs/2026-04-09-alpha.md"),
            "---\n\
id: spec:alpha\n\
title: Alpha\n\
status: in_progress\n\
created: 2026-04-09\n\
---\n\
\n\
- [ ] implement core flow <!-- id: item-1 -->\n\
- [ ] validate rollout <!-- id: item-2 -->\n",
        )
        .unwrap();

        let session = index_workspace_session(&root).unwrap();
        let prism = session.prism();
        let plan = prism
            .create_native_plan_from_spec_transaction(
                prism_ir::EventMeta {
                    id: EventId::new("coord:plan:cli-spec-query"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                    execution_context: None,
                },
                NativeSpecPlanCreateInput {
                    title: "Ship alpha".into(),
                    goal: "Ship alpha".into(),
                    status: Some(PlanStatus::Active),
                    policy: None,
                    scheduling: None,
                    spec_ref: CoordinationSpecRef {
                        spec_id: "spec:alpha".into(),
                        source_path: ".prism/specs/2026-04-09-alpha.md".into(),
                        source_revision: Some("rev-plan".into()),
                    },
                },
            )
            .unwrap();
        prism
            .create_native_task_from_spec_transaction(
                prism_ir::EventMeta {
                    id: EventId::new("coord:task:cli-spec-query"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                    execution_context: None,
                },
                NativeSpecTaskCreateInput {
                    task: TaskCreateInput {
                        plan_id: plan.plan_id,
                        title: "Implement alpha".into(),
                        status: Some(CoordinationTaskStatus::Ready),
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        spec_refs: Vec::new(),
                        artifact_requirements: Vec::new(),
                        review_requirements: Vec::new(),
                        base_revision: WorkspaceRevision::default(),
                    },
                    spec_ref: CoordinationTaskSpecRef {
                        spec_id: "spec:alpha".into(),
                        source_path: ".prism/specs/2026-04-09-alpha.md".into(),
                        source_revision: Some("rev-task".into()),
                        sync_kind: "task".into(),
                        covered_checklist_items: vec!["spec:alpha::checklist::item-1".into()],
                        covered_sections: Vec::new(),
                    },
                },
            )
            .unwrap();

        let (list, show, brief, coverage, provenance) =
            with_spec_query_engine(&session, |engine| {
                let list = render_spec_list(&engine.list_specs()?);
                let show = match engine.spec("spec:alpha")? {
                    prism_core::SpecQueryLookup::Found(view) => render_spec_document(&view),
                    prism_core::SpecQueryLookup::NotFound => String::new(),
                };
                let brief = match engine.sync_brief("spec:alpha")? {
                    prism_core::SpecQueryLookup::Found(view) => render_spec_sync_brief(&view),
                    prism_core::SpecQueryLookup::NotFound => String::new(),
                };
                let coverage = match engine.coverage("spec:alpha")? {
                    prism_core::SpecQueryLookup::Found(view) => render_spec_coverage(&view),
                    prism_core::SpecQueryLookup::NotFound => String::new(),
                };
                let provenance = match engine.sync_provenance("spec:alpha")? {
                    prism_core::SpecQueryLookup::Found(view) => render_spec_sync_provenance(&view),
                    prism_core::SpecQueryLookup::NotFound => String::new(),
                };
                Ok((list, show, brief, coverage, provenance))
            })
            .unwrap();

        assert!(list.contains("spec:alpha"));
        assert!(show.contains("title: Alpha"));
        assert!(show.contains("overall_status: in_progress"));
        assert!(brief.contains("required_checklist_items:"));
        assert!(brief.contains("spec:alpha::checklist::item-1"));
        assert!(coverage.contains("kind=represented"));
        assert!(provenance.contains("kind=task"));
        assert!(provenance.contains("source_revision=rev-task"));
    }

    fn temp_workspace(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-cli-{label}-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init", "-b", "main"]);
        run_git(&root, &["config", "user.name", "PRISM Test"]);
        run_git(&root, &["config", "user.email", "prism@example.com"]);
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        root
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
