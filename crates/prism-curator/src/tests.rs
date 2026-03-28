use std::fs;

use prism_ir::{
    AnchorRef, Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span, TaskId,
};

use crate::*;

fn sample_job() -> CuratorJob {
    CuratorJob {
        id: CuratorJobId("job:1".to_string()),
        trigger: CuratorTrigger::PostChange,
        task: Some(TaskId::new("task:1")),
        focus: vec![AnchorRef::Node(NodeId::new(
            "demo",
            "demo::alpha",
            NodeKind::Function,
        ))],
        budget: CuratorBudget::default(),
    }
}

fn sample_context() -> CuratorContext {
    CuratorContext {
        graph: CuratorGraphSlice {
            nodes: vec![Node {
                id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                name: "alpha".into(),
                kind: NodeKind::Function,
                file: prism_ir::FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            }],
            edges: vec![Edge {
                kind: EdgeKind::Calls,
                source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                target: NodeId::new("demo", "demo::beta", NodeKind::Function),
                origin: EdgeOrigin::Static,
                confidence: 1.0,
            }],
        },
        ..CuratorContext::default()
    }
}

#[test]
fn invocation_includes_typed_codex_options() {
    let mut config = CodexCliCuratorConfig::codex("codex", "/tmp/demo");
    config.model = Some("gpt-5.4".to_string());
    config.profile = Some("curator".to_string());
    config.sandbox = Some(CodexSandboxMode::WorkspaceWrite);
    config.approval_policy = Some(CodexApprovalPolicy::Never);
    config.reasoning_effort = Some(CodexReasoningEffort::High);
    config.execution_mode = CodexExecutionMode::FullAuto;
    config.add_dirs.push("/tmp/extra".into());
    config.skip_git_repo_check = true;
    config.ephemeral = true;
    config.enable_features.push("foo".to_string());
    config.disable_features.push("bar".to_string());

    let curator = CodexCliCurator::new(config);
    let invocation = curator
        .prepare_invocation(&sample_job(), &sample_context())
        .expect("invocation should prepare");

    assert_eq!(invocation.args[0], "exec");
    assert!(invocation
        .args
        .windows(2)
        .any(|pair| pair == ["-m", "gpt-5.4"]));
    assert!(invocation
        .args
        .windows(2)
        .any(|pair| pair == ["-p", "curator"]));
    assert!(invocation
        .args
        .windows(2)
        .any(|pair| pair == ["-s", "workspace-write"]));
    assert!(invocation.args.iter().any(|arg| arg == "--full-auto"));
    assert!(invocation
        .args
        .iter()
        .any(|arg| arg == "approval_policy=\"never\""));
    assert!(invocation
        .args
        .iter()
        .any(|arg| arg == "reasoning_effort=\"high\""));
    assert!(invocation
        .args
        .iter()
        .any(|arg| arg == "--skip-git-repo-check"));
    assert!(invocation.args.iter().any(|arg| arg == "--ephemeral"));
}

#[test]
fn bounded_context_respects_budget() {
    let mut context = sample_context();
    context.graph.nodes = (0..200)
        .map(|index| Node {
            id: NodeId::new("demo", format!("demo::node{index}"), NodeKind::Function),
            name: format!("node{index}").into(),
            kind: NodeKind::Function,
            file: prism_ir::FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        })
        .collect();

    let bounded = crate::support::bounded_context(
        &context,
        &CuratorBudget {
            max_context_nodes: 8,
            ..CuratorBudget::default()
        },
    );
    assert_eq!(bounded.graph.nodes.len(), 8);
    assert!(bounded.graph.edges.len() <= 32);
}

#[cfg(unix)]
#[test]
fn codex_backend_can_parse_structured_output() {
    use std::os::unix::fs::PermissionsExt;

    let script_dir = crate::support::unique_temp_dir("prism-curator-test").expect("temp dir");
    let script_path = script_dir.join("fake-codex.sh");
    fs::write(
        &script_path,
        r#"#!/bin/sh
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o|--output-last-message)
      out="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
cat >/dev/null
printf '%s' '{"proposals":[{"kind":"risk_summary","anchors":[],"summary":"watch beta","severity":"medium","evidence_events":[]}],"diagnostics":[]}' > "$out"
"#,
    )
    .expect("script should write");
    let mut permissions = fs::metadata(&script_path)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("permissions should set");

    let curator = CodexCliCurator::new(CodexCliCuratorConfig::codex(
        script_path.clone(),
        "/tmp/demo",
    ));
    let run = curator
        .run(&sample_job(), &sample_context())
        .expect("fake codex should run");

    assert_eq!(run.proposals.len(), 1);
    match &run.proposals[0] {
        CuratorProposal::RiskSummary(summary) => {
            assert_eq!(summary.summary, "watch beta");
            assert_eq!(summary.severity, "medium");
        }
        other => panic!("unexpected proposal: {other:?}"),
    }

    let _ = fs::remove_file(&script_path);
    let _ = fs::remove_dir_all(&script_dir);
}

#[test]
fn built_in_synthesis_emits_structural_and_semantic_memory() {
    let job = CuratorJob {
        trigger: CuratorTrigger::RepeatedFailure,
        ..sample_job()
    };
    let mut ctx = sample_context();
    ctx.outcomes = vec![
        prism_memory::OutcomeEvent {
            meta: prism_ir::EventMeta {
                id: prism_ir::EventId::new("outcome:1"),
                ts: 10,
                actor: prism_ir::EventActor::Agent,
                correlation: Some(TaskId::new("task:1")),
                causation: None,
            },
            anchors: job.focus.clone(),
            kind: prism_memory::OutcomeKind::FailureObserved,
            result: prism_memory::OutcomeResult::Failure,
            summary: "alpha failed under routing load".into(),
            evidence: vec![prism_memory::OutcomeEvidence::Test {
                name: "alpha_regression".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        },
        prism_memory::OutcomeEvent {
            meta: prism_ir::EventMeta {
                id: prism_ir::EventId::new("outcome:2"),
                ts: 11,
                actor: prism_ir::EventActor::Agent,
                correlation: Some(TaskId::new("task:1")),
                causation: None,
            },
            anchors: job.focus.clone(),
            kind: prism_memory::OutcomeKind::FailureObserved,
            result: prism_memory::OutcomeResult::Failure,
            summary: "alpha failed again after routing edits".into(),
            evidence: vec![prism_memory::OutcomeEvidence::Test {
                name: "alpha_regression".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        },
    ];
    ctx.projections.validation_checks = vec![prism_projections::ValidationCheck {
        label: "test:alpha_regression".into(),
        score: 0.9,
        last_seen: 11,
    }];

    let run = synthesize_curator_run(&job, &ctx);

    assert!(run.proposals.iter().any(|proposal| matches!(
        proposal,
        CuratorProposal::StructuralMemory(candidate)
            if candidate.content.contains("should run validation")
    )));
    assert!(run.proposals.iter().any(|proposal| matches!(
        proposal,
        CuratorProposal::SemanticMemory(candidate)
            if candidate.content.contains("Recent outcome context")
    )));
}

#[test]
fn curator_memory_proposals_round_trip_without_duplicate_kind_keys() {
    let proposal = CuratorProposal::StructuralMemory(CandidateMemory {
        anchors: vec![],
        kind: prism_memory::MemoryKind::Structural,
        content: "alpha owns the request path".to_string(),
        trust: 0.8,
        rationale: "captured from repeated co-change history".to_string(),
        category: Some("ownership".to_string()),
        evidence: CandidateMemoryEvidence::default(),
    });

    let json = serde_json::to_string(&proposal).expect("proposal should serialize");
    assert_eq!(json.matches("\"kind\":").count(), 1);
    assert!(json.contains("\"kind\":\"structural_memory\""));
    assert!(json.contains("\"memoryKind\":\"Structural\""));

    let decoded: CuratorProposal = serde_json::from_str(&json).expect("proposal should decode");
    assert_eq!(decoded, proposal);
}

#[test]
fn curator_memory_proposals_decode_legacy_duplicate_kind_shape() {
    let json = r#"{
        "kind": "structural_memory",
        "anchors": [],
        "kind": "Structural",
        "content": "alpha owns the request path",
        "trust": 0.8,
        "rationale": "captured from repeated co-change history",
        "category": "ownership",
        "evidence": {
            "event_ids": [],
            "validation_checks": [],
            "co_change_lineages": []
        }
    }"#;

    let decoded: CuratorProposal =
        serde_json::from_str(json).expect("legacy proposal should decode");
    assert!(matches!(
        decoded,
        CuratorProposal::StructuralMemory(CandidateMemory {
            kind: prism_memory::MemoryKind::Structural,
            ..
        })
    ));
}

#[test]
fn curator_concept_candidate_round_trips_with_tagged_kind() {
    let proposal = CuratorProposal::ConceptCandidate(CandidateConcept {
        recommended_operation: CandidateConceptOperation::Promote,
        canonical_name: "routing_cluster".to_string(),
        summary: "Hotspot-sized routing cluster.".to_string(),
        aliases: vec!["routing".to_string()],
        core_members: vec![
            NodeId::new("demo", "demo::alpha", NodeKind::Function),
            NodeId::new("demo", "demo::beta", NodeKind::Function),
        ],
        supporting_members: Vec::new(),
        likely_tests: Vec::new(),
        evidence: vec!["Observed in a hotspot edit.".to_string()],
        confidence: 0.78,
        rationale: "Curator saw repeated hotspot evidence.".to_string(),
    });

    let json = serde_json::to_string(&proposal).expect("proposal should serialize");
    assert!(json.contains("\"kind\":\"concept_candidate\""));
    assert!(json.contains("\"recommendedOperation\":\"promote\""));

    let decoded: CuratorProposal = serde_json::from_str(&json).expect("proposal should decode");
    assert_eq!(decoded, proposal);
}

#[test]
fn hotspot_synthesis_emits_concept_candidate_for_multi_node_hotspots() {
    let job = CuratorJob {
        trigger: CuratorTrigger::HotspotChanged,
        focus: vec![
            AnchorRef::Node(NodeId::new("demo", "demo::alpha_route", NodeKind::Function)),
            AnchorRef::Node(NodeId::new("demo", "demo::beta_route", NodeKind::Function)),
        ],
        ..sample_job()
    };
    let mut ctx = sample_context();
    ctx.graph.nodes = vec![
        Node {
            id: NodeId::new("demo", "demo::alpha_route", NodeKind::Function),
            name: "alpha_route".into(),
            kind: NodeKind::Function,
            file: prism_ir::FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        },
        Node {
            id: NodeId::new("demo", "demo::beta_route", NodeKind::Function),
            name: "beta_route".into(),
            kind: NodeKind::Function,
            file: prism_ir::FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        },
    ];
    ctx.projections.validation_checks = vec![prism_projections::ValidationCheck {
        label: "test:route_regression".into(),
        score: 0.88,
        last_seen: 12,
    }];
    ctx.projections.co_change = vec![prism_projections::CoChangeRecord {
        lineage: prism_ir::LineageId::new("lineage:route"),
        count: 3,
    }];

    let run = synthesize_curator_run(&job, &ctx);

    assert!(run.proposals.iter().any(|proposal| matches!(
        proposal,
        CuratorProposal::ConceptCandidate(candidate)
            if candidate.canonical_name.contains("route")
                && candidate.core_members.len() >= 2
    )));
}

#[test]
fn synthesize_promotes_strong_episodic_memory_with_source_provenance() {
    let job = sample_job();
    let mut ctx = sample_context();
    let mut memory = prism_memory::MemoryEntry::new(
        prism_memory::MemoryKind::Episodic,
        "workspace dependencies heading follows up well when same-file implementation owners are present",
    );
    memory.id = prism_memory::MemoryId("memory:episodic-source".to_string());
    memory.anchors = job.focus.clone();
    memory.trust = 0.84;
    memory.metadata = serde_json::json!({
        "provenance": {
            "origin": "manual_store",
            "kind": "manual_memory",
        }
    });
    ctx.memories.push(memory);

    let run = synthesize_curator_run(&job, &ctx);

    assert!(run.proposals.iter().any(|proposal| matches!(
        proposal,
        CuratorProposal::StructuralMemory(candidate)
            if candidate.category.as_deref() == Some("episodic_promotion")
                && candidate
                    .evidence
                    .memory_ids
                    .iter()
                    .any(|id| id.0 == "memory:episodic-source")
    )));
}
