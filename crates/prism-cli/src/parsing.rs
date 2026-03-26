use anyhow::{bail, Result};
use prism_ir::NodeKind;
use prism_memory::{MemoryKind, OutcomeEvidence, OutcomeKind, OutcomeResult};

pub fn parse_memory_kind(value: &str) -> Result<MemoryKind> {
    match value.to_ascii_lowercase().as_str() {
        "episodic" | "note" | "notes" => Ok(MemoryKind::Episodic),
        "structural" | "rule" | "invariant" => Ok(MemoryKind::Structural),
        "semantic" | "summary" => Ok(MemoryKind::Semantic),
        other => bail!("unknown memory kind `{other}`"),
    }
}

pub fn parse_node_kind_filter(value: Option<&str>) -> Result<Option<NodeKind>> {
    let Some(value) = value else {
        return Ok(None);
    };

    let kind = match value.to_ascii_lowercase().as_str() {
        "workspace" => NodeKind::Workspace,
        "package" => NodeKind::Package,
        "document" => NodeKind::Document,
        "module" => NodeKind::Module,
        "function" => NodeKind::Function,
        "struct" => NodeKind::Struct,
        "enum" => NodeKind::Enum,
        "trait" => NodeKind::Trait,
        "impl" => NodeKind::Impl,
        "method" => NodeKind::Method,
        "field" => NodeKind::Field,
        "typealias" | "type-alias" => NodeKind::TypeAlias,
        "markdownheading" | "markdown-heading" => NodeKind::MarkdownHeading,
        "jsonkey" | "json-key" => NodeKind::JsonKey,
        "yamlkey" | "yaml-key" => NodeKind::YamlKey,
        other => {
            bail!("unknown node kind `{other}`");
        }
    };

    Ok(Some(kind))
}

pub fn parse_outcome_kind(value: &str) -> Result<OutcomeKind> {
    match value.to_ascii_lowercase().as_str() {
        "note-added" | "note" => Ok(OutcomeKind::NoteAdded),
        "hypothesis-proposed" | "hypothesis" => Ok(OutcomeKind::HypothesisProposed),
        "plan-created" | "plan" => Ok(OutcomeKind::PlanCreated),
        "patch-applied" | "patch" => Ok(OutcomeKind::PatchApplied),
        "build-ran" | "build" => Ok(OutcomeKind::BuildRan),
        "test-ran" | "test" => Ok(OutcomeKind::TestRan),
        "review-feedback" | "review" => Ok(OutcomeKind::ReviewFeedback),
        "failure-observed" | "failure" => Ok(OutcomeKind::FailureObserved),
        "regression-observed" | "regression" => Ok(OutcomeKind::RegressionObserved),
        "fix-validated" | "validated" => Ok(OutcomeKind::FixValidated),
        "rollback-performed" | "rollback" => Ok(OutcomeKind::RollbackPerformed),
        "migration-required" | "migration" => Ok(OutcomeKind::MigrationRequired),
        "incident-linked" | "incident" => Ok(OutcomeKind::IncidentLinked),
        "perf-signal-observed" | "perf" => Ok(OutcomeKind::PerfSignalObserved),
        other => bail!("unknown outcome kind `{other}`"),
    }
}

pub fn parse_outcome_result(value: &str) -> Result<OutcomeResult> {
    match value.to_ascii_lowercase().as_str() {
        "success" => Ok(OutcomeResult::Success),
        "failure" => Ok(OutcomeResult::Failure),
        "partial" => Ok(OutcomeResult::Partial),
        "unknown" => Ok(OutcomeResult::Unknown),
        other => bail!("unknown outcome result `{other}`"),
    }
}

pub fn build_evidence(
    tests: Vec<String>,
    failing_tests: Vec<String>,
    builds: Vec<String>,
    failing_builds: Vec<String>,
    issues: Vec<String>,
    commits: Vec<String>,
) -> Vec<OutcomeEvidence> {
    let mut evidence = Vec::new();
    for name in tests {
        evidence.push(OutcomeEvidence::Test { name, passed: true });
    }
    for name in failing_tests {
        evidence.push(OutcomeEvidence::Test {
            name,
            passed: false,
        });
    }
    for target in builds {
        evidence.push(OutcomeEvidence::Build {
            target,
            passed: true,
        });
    }
    for target in failing_builds {
        evidence.push(OutcomeEvidence::Build {
            target,
            passed: false,
        });
    }
    for id in issues {
        evidence.push(OutcomeEvidence::Issue { id });
    }
    for sha in commits {
        evidence.push(OutcomeEvidence::Commit { sha });
    }
    evidence
}
