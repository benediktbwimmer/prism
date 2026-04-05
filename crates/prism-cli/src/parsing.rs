use anyhow::{bail, Result};
use prism_core::{ValidationFeedbackCategory, ValidationFeedbackVerdict};
use prism_ir::{CredentialCapability, NodeKind, PrincipalKind};
use prism_memory::{
    MemoryEventKind, MemoryKind, MemoryScope, OutcomeEvidence, OutcomeKind, OutcomeResult,
};

pub fn parse_memory_kind(value: &str) -> Result<MemoryKind> {
    match value.to_ascii_lowercase().as_str() {
        "episodic" | "note" | "notes" => Ok(MemoryKind::Episodic),
        "structural" | "rule" | "invariant" => Ok(MemoryKind::Structural),
        "semantic" | "summary" => Ok(MemoryKind::Semantic),
        other => bail!("unknown memory kind `{other}`"),
    }
}

pub fn parse_memory_scope(value: &str) -> Result<MemoryScope> {
    match value.to_ascii_lowercase().as_str() {
        "local" | "private" | "machine" => Ok(MemoryScope::Local),
        "session" | "workspace" => Ok(MemoryScope::Session),
        "repo" | "shared" => Ok(MemoryScope::Repo),
        other => bail!("unknown memory scope `{other}`"),
    }
}

pub fn parse_memory_event_action(value: &str) -> Result<MemoryEventKind> {
    match value.to_ascii_lowercase().as_str() {
        "stored" | "store" => Ok(MemoryEventKind::Stored),
        "promoted" | "promote" => Ok(MemoryEventKind::Promoted),
        "superseded" | "supersede" => Ok(MemoryEventKind::Superseded),
        other => bail!("unknown memory event action `{other}`"),
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
        "tomlkey" | "toml-key" => NodeKind::TomlKey,
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

pub fn parse_principal_kind(value: &str) -> Result<PrincipalKind> {
    match value.to_ascii_lowercase().as_str() {
        "human" => Ok(PrincipalKind::Human),
        "service" => Ok(PrincipalKind::Service),
        "agent" => Ok(PrincipalKind::Agent),
        "system" => Ok(PrincipalKind::System),
        "ci" => Ok(PrincipalKind::Ci),
        "external" => Ok(PrincipalKind::External),
        other => bail!("unknown principal kind `{other}`"),
    }
}

pub fn parse_credential_capability(value: &str) -> Result<CredentialCapability> {
    match value.to_ascii_lowercase().as_str() {
        "mutate_coordination" | "coordination" => Ok(CredentialCapability::MutateCoordination),
        "mutate_repo_memory" | "repo_memory" | "memory" => {
            Ok(CredentialCapability::MutateRepoMemory)
        }
        "read_peer_runtime" | "peer_runtime" | "peer" => Ok(CredentialCapability::ReadPeerRuntime),
        "mint_child_principal" | "mint_child" | "child" => {
            Ok(CredentialCapability::MintChildPrincipal)
        }
        "admin_principals" | "admin" => Ok(CredentialCapability::AdminPrincipals),
        "all" => Ok(CredentialCapability::All),
        other => bail!("unknown credential capability `{other}`"),
    }
}

pub fn parse_validation_feedback_category(value: &str) -> Result<ValidationFeedbackCategory> {
    value.parse().map_err(anyhow::Error::msg)
}

pub fn parse_validation_feedback_verdict(value: &str) -> Result<ValidationFeedbackVerdict> {
    value.parse().map_err(anyhow::Error::msg)
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

#[cfg(test)]
mod tests {
    use super::{parse_credential_capability, parse_principal_kind};
    use prism_ir::{CredentialCapability, PrincipalKind};

    #[test]
    fn parse_legacy_read_peer_runtime_capability() {
        assert_eq!(
            parse_credential_capability("read_peer_runtime").unwrap(),
            CredentialCapability::ReadPeerRuntime
        );
        assert_eq!(
            parse_credential_capability("peer_runtime").unwrap(),
            CredentialCapability::ReadPeerRuntime
        );
    }

    #[test]
    fn parse_principal_kind_supports_service_and_legacy_agent_values() {
        assert_eq!(parse_principal_kind("human").unwrap(), PrincipalKind::Human);
        assert_eq!(
            parse_principal_kind("service").unwrap(),
            PrincipalKind::Service
        );
        assert_eq!(parse_principal_kind("agent").unwrap(), PrincipalKind::Agent);
    }
}
