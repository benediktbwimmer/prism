use std::path::{Path, PathBuf};

use prism_ir::PlanId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProtectedVerificationStatus {
    Verified,
    LegacyUnsigned,
    UnknownTrust,
    Tampered,
    Corrupt,
    Truncated,
    Conflict,
    MigrationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProtectedRepoStream {
    stream: &'static str,
    stream_id: String,
    relative_path: PathBuf,
}

impl ProtectedRepoStream {
    pub(crate) fn concept_events() -> Self {
        Self {
            stream: "repo_concept_events",
            stream_id: "concepts:events".to_string(),
            relative_path: PathBuf::from(".prism/concepts/events.jsonl"),
        }
    }

    pub(crate) fn concept_relations() -> Self {
        Self {
            stream: "repo_concept_relations",
            stream_id: "concepts:relations".to_string(),
            relative_path: PathBuf::from(".prism/concepts/relations.jsonl"),
        }
    }

    pub(crate) fn contract_events() -> Self {
        Self {
            stream: "repo_contract_events",
            stream_id: "contracts:events".to_string(),
            relative_path: PathBuf::from(".prism/contracts/events.jsonl"),
        }
    }

    pub(crate) fn memory_stream(file_name: &str) -> Option<Self> {
        (!file_name.trim().is_empty() && file_name.ends_with(".jsonl")).then(|| Self {
            stream: "repo_memory_events",
            stream_id: format!("memory:{}", file_name.trim_end_matches(".jsonl")),
            relative_path: PathBuf::from(".prism").join("memory").join(file_name),
        })
    }

    pub(crate) fn plan_stream(plan_id: &PlanId) -> Self {
        Self {
            stream: "repo_plan_events",
            stream_id: plan_id.0.to_string(),
            relative_path: PathBuf::from(".prism")
                .join("plans")
                .join("streams")
                .join(format!("{}.jsonl", plan_id.0)),
        }
    }

    pub(crate) fn stream(&self) -> &'static str {
        self.stream
    }

    pub(crate) fn stream_id(&self) -> &str {
        &self.stream_id
    }

    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative_path
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProtectedStreamVerification {
    pub(crate) verification_status: ProtectedVerificationStatus,
    pub(crate) stream_id: String,
    pub(crate) protected_path: String,
    pub(crate) last_verified_event_id: Option<String>,
    pub(crate) last_verified_entry_hash: Option<String>,
    pub(crate) trust_bundle_id: Option<String>,
    pub(crate) diagnostic_code: Option<String>,
    pub(crate) diagnostic_summary: Option<String>,
    pub(crate) repair_hint: Option<String>,
}

pub(crate) fn classify_protected_repo_relative_path(path: &Path) -> Option<ProtectedRepoStream> {
    let segments = path
        .iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [prism, concepts, file]
            if prism == ".prism" && concepts == "concepts" && file == "events.jsonl" =>
        {
            Some(ProtectedRepoStream::concept_events())
        }
        [prism, concepts, file]
            if prism == ".prism" && concepts == "concepts" && file == "relations.jsonl" =>
        {
            Some(ProtectedRepoStream::concept_relations())
        }
        [prism, contracts, file]
            if prism == ".prism" && contracts == "contracts" && file == "events.jsonl" =>
        {
            Some(ProtectedRepoStream::contract_events())
        }
        [prism, memory, file] if prism == ".prism" && memory == "memory" => {
            ProtectedRepoStream::memory_stream(file)
        }
        [prism, plans, streams, file]
            if prism == ".prism"
                && plans == "plans"
                && streams == "streams"
                && file.ends_with(".jsonl") =>
        {
            let plan_id = file.trim_end_matches(".jsonl");
            (!plan_id.trim().is_empty())
                .then(|| ProtectedRepoStream::plan_stream(&PlanId::new(plan_id)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::PlanId;

    use super::{
        classify_protected_repo_relative_path, ProtectedRepoStream, ProtectedVerificationStatus,
    };

    #[test]
    fn classifies_fixed_v1_protected_paths() {
        let concept =
            classify_protected_repo_relative_path(Path::new(".prism/concepts/events.jsonl"))
                .expect("concept events should be protected");
        assert_eq!(concept.stream(), "repo_concept_events");
        assert_eq!(concept.stream_id(), "concepts:events");

        let relations =
            classify_protected_repo_relative_path(Path::new(".prism/concepts/relations.jsonl"))
                .expect("concept relations should be protected");
        assert_eq!(relations.stream(), "repo_concept_relations");

        let contracts =
            classify_protected_repo_relative_path(Path::new(".prism/contracts/events.jsonl"))
                .expect("contract events should be protected");
        assert_eq!(contracts.stream(), "repo_contract_events");

        let memory = classify_protected_repo_relative_path(Path::new(".prism/memory/events.jsonl"))
            .expect("memory events should be protected");
        assert_eq!(memory.stream(), "repo_memory_events");
        assert_eq!(memory.stream_id(), "memory:events");
    }

    #[test]
    fn classifies_plan_streams_using_the_streams_topology() {
        let stream = classify_protected_repo_relative_path(Path::new(
            ".prism/plans/streams/plan:demo.jsonl",
        ))
        .expect("per-plan stream should be protected");
        assert_eq!(stream.stream(), "repo_plan_events");
        assert_eq!(stream.stream_id(), "plan:demo");
        assert_eq!(
            stream.relative_path(),
            Path::new(".prism/plans/streams/plan:demo.jsonl")
        );

        let direct = ProtectedRepoStream::plan_stream(&PlanId::new("plan:demo"));
        assert_eq!(direct, stream);
    }

    #[test]
    fn ignores_non_authoritative_repo_prism_paths() {
        assert!(
            classify_protected_repo_relative_path(Path::new(".prism/plans/index.jsonl")).is_none()
        );
        assert!(classify_protected_repo_relative_path(Path::new(
            ".prism/plans/active/plan:demo.jsonl"
        ))
        .is_none());
        assert!(classify_protected_repo_relative_path(Path::new("PRISM.md")).is_none());
    }

    #[test]
    fn verification_status_serializes_with_spec_names() {
        let json = serde_json::to_string(&ProtectedVerificationStatus::MigrationRequired)
            .expect("status should serialize");
        assert_eq!(json, "\"MigrationRequired\"");
    }
}
