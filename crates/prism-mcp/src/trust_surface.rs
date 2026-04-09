use std::path::Path;

use axum::{http::StatusCode, Json};
use prism_core::{
    configured_coordination_authority_store_provider, AuthenticatedPrincipal,
    CoordinationAuthorityMutationError, CoordinationAuthorityMutationStatus,
    CoordinationAuthorityStoreProvider, CoordinationReadConsistency, CoordinationReadRequest,
    CoordinationStateView, ProtectedStateStreamReport, SharedCoordinationRefDiagnostics,
    WorktreeMode, WorktreeMutatorSlotError, WorktreeRegistrationRecord,
};
use prism_ir::EventId;
use prism_js::{
    RuntimeDescriptorCapabilityView, RuntimeDiscoveryModeView, RuntimeSharedCoordinationRefView,
    RuntimeSharedCoordinationRuntimeDescriptorView,
};
use prism_query::{
    CoordinationTransactionError, CoordinationTransactionProtocolIndeterminate,
    CoordinationTransactionProtocolRejection, CoordinationTransactionProtocolState,
};
use rmcp::model::ErrorData as McpError;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    CoordinationMutationResult, MutationViolationView, PrismMutationBridgeExecutionArgs,
    ProtectedStateStreamView,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthorityStampView {
    backend_kind: String,
    logical_repo_id: String,
    snapshot_id: String,
    transaction_id: Option<String>,
    committed_at: Option<u64>,
    provenance: AuthorityStampProvenanceView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthorityStampProvenanceView {
    ref_name: Option<String>,
    head_commit: Option<String>,
    manifest_digest: Option<String>,
}

pub(crate) fn mutation_capability_denied_error(
    required_capability: &str,
    authenticated: &AuthenticatedPrincipal,
) -> McpError {
    McpError::invalid_params(
        "prism_mutate credential lacks the required capability",
        Some(json!({
            "code": "mutation_capability_denied",
            "requiredCapability": required_capability,
            "credentialId": authenticated.credential.credential_id.0,
            "principalId": authenticated.principal.principal_id.0,
            "nextAction": "Use a credential with the required capability or mint a new child principal with narrower capabilities for this mutation lane.",
        })),
    )
}

pub(crate) fn mutation_auth_missing_error() -> McpError {
    McpError::invalid_params(
        "prism_mutate requires either `credential` or an attached bridge execution binding",
        Some(json!({
            "code": "mutation_auth_missing",
            "nextAction": "Supply `credential` directly, or call `prism_bridge_adopt` on a stdio bridge attached to a registered agent worktree before retrying the mutation.",
        })),
    )
}

pub(crate) fn mutation_auth_failed_error(credential_id: &str, error: &str) -> McpError {
    McpError::invalid_params(
        "prism_mutate credential rejected",
        Some(json!({
            "code": "mutation_auth_failed",
            "credentialId": credential_id,
            "error": error,
            "nextAction": "Use `prism auth login` or mint a fresh credential, then retry the mutation.",
        })),
    )
}

pub(crate) fn mutation_worktree_unregistered_error(
    authenticated: Option<&AuthenticatedPrincipal>,
    next_action: &str,
) -> McpError {
    let mut data = json!({
        "code": "mutation_worktree_unregistered",
        "nextAction": next_action,
    });
    if let Some(authenticated) = authenticated {
        let Value::Object(object) = &mut data else {
            unreachable!()
        };
        object.insert(
            "principalId".to_string(),
            Value::String(authenticated.principal.principal_id.0.to_string()),
        );
        object.insert(
            "principalKind".to_string(),
            serde_json::to_value(authenticated.principal.kind).unwrap_or(Value::Null),
        );
    }
    McpError::invalid_params(
        "authoritative mutations require a registered worktree",
        Some(data),
    )
}

pub(crate) fn mutation_worktree_mode_mismatch_error(
    authenticated: &AuthenticatedPrincipal,
    registration: &WorktreeRegistrationRecord,
    required_mode: WorktreeMode,
) -> McpError {
    McpError::invalid_params(
        "principal kind does not match the current registered worktree mode",
        Some(json!({
            "code": "mutation_worktree_mode_mismatch",
            "principalId": authenticated.principal.principal_id.0,
            "principalKind": authenticated.principal.kind,
            "worktreeId": registration.worktree_id,
            "agentLabel": registration.agent_label,
            "worktreeMode": worktree_mode_label(registration.mode),
            "requiredWorktreeMode": worktree_mode_label(required_mode),
            "nextAction": "Use a worktree registered in the matching mode for this actor, or retry through the appropriate bridge or human session.",
        })),
    )
}

pub(crate) fn mutation_worktree_mutator_slot_error(error: WorktreeMutatorSlotError) -> McpError {
    match error {
        WorktreeMutatorSlotError::Conflict(conflict) => McpError::invalid_params(
            "prism_mutate conflicts with the active worktree mutator session",
            Some(json!({
                "code": "mutation_worktree_mutator_slot_conflict",
                "worktreeId": conflict.worktree_id,
                "currentOwner": {
                    "sessionId": conflict.current_owner.session_id,
                    "authorityId": conflict.current_owner.authority_id,
                    "principalId": conflict.current_owner.principal_id,
                    "name": conflict.current_owner.principal_name,
                    "credentialId": conflict.current_owner.credential_id,
                    "lastHeartbeatAt": conflict.current_owner.last_heartbeat_at,
                },
                "attemptedSessionId": conflict.attempted_session_id,
                "attemptedPrincipal": {
                    "authorityId": conflict.attempted_principal.authority_id,
                    "principalId": conflict.attempted_principal.principal_id,
                    "name": conflict.attempted_principal.principal_name,
                },
                "staleAt": conflict.stale_at,
                "nextAction": "Retry after the current worktree mutator session goes stale, or have a human operator explicitly take over the worktree before retrying authenticated mutations.",
            })),
        ),
        WorktreeMutatorSlotError::TakeoverRequiresHuman {
            principal_id,
            principal_kind,
        } => McpError::invalid_params(
            "only a human principal can authorize worktree mutator takeover",
            Some(json!({
                "code": "mutation_worktree_mutator_takeover_requires_human",
                "principalId": principal_id,
                "principalKind": principal_kind,
                "nextAction": "Retry with an authenticated human operator session if this worktree really needs an explicit takeover.",
            })),
        ),
        WorktreeMutatorSlotError::Storage(error) => McpError::internal_error(
            "failed to update the worktree mutator slot",
            Some(json!({
                "code": "mutation_worktree_mutator_slot_storage_failed",
                "error": error.to_string(),
            })),
        ),
    }
}

pub(crate) fn mutation_bridge_execution_requires_agent_worktree_error(
    registration: &WorktreeRegistrationRecord,
) -> McpError {
    McpError::invalid_params(
        "bridge execution requires a worktree registered in agent mode",
        Some(json!({
            "code": "mutation_bridge_execution_requires_agent_worktree",
            "worktreeId": registration.worktree_id,
            "agentLabel": registration.agent_label,
            "worktreeMode": worktree_mode_label(registration.mode),
            "nextAction": "Use a bridge attached to a registered agent worktree, or supply an explicit human credential for direct operator mutation.",
        })),
    )
}

pub(crate) fn mutation_bridge_execution_mismatch_error(
    registration: &WorktreeRegistrationRecord,
    bridge_execution: &PrismMutationBridgeExecutionArgs,
) -> McpError {
    McpError::invalid_params(
        "bridge execution binding does not match the current registered worktree",
        Some(json!({
            "code": "mutation_bridge_execution_mismatch",
            "expectedWorktreeId": registration.worktree_id,
            "expectedAgentLabel": registration.agent_label,
            "receivedWorktreeId": bridge_execution.worktree_id,
            "receivedAgentLabel": bridge_execution.agent_label,
            "nextAction": "Call `prism_bridge_adopt` again so the bridge reattaches to the current registered worktree lane.",
        })),
    )
}

pub(crate) fn peer_runtime_auth_failed_response(
    credential_id: &str,
    error: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "code": "peer_runtime_auth_failed",
            "message": error,
            "credentialId": credential_id,
        })),
    )
}

pub(crate) fn peer_runtime_capability_denied_response(
    credential_id: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "code": "peer_runtime_capability_denied",
            "message": "credential lacks read_peer_runtime capability",
            "credentialId": credential_id,
        })),
    )
}

pub(crate) fn protected_state_stream_view(
    report: ProtectedStateStreamReport,
) -> ProtectedStateStreamView {
    ProtectedStateStreamView {
        stream: report.stream,
        stream_id: report.stream_id,
        protected_path: report.protected_path,
        verification_status: report.verification_status,
        last_verified_event_id: report.last_verified_event_id,
        last_verified_entry_hash: report.last_verified_entry_hash,
        trust_bundle_id: report.trust_bundle_id,
        diagnostic_code: report.diagnostic_code,
        diagnostic_summary: report.diagnostic_summary,
        repair_hint: report.repair_hint,
    }
}

pub(crate) fn runtime_coordination_authority_view(
    value: SharedCoordinationRefDiagnostics,
) -> RuntimeSharedCoordinationRefView {
    RuntimeSharedCoordinationRefView {
        ref_name: value.ref_name,
        head_commit: value.head_commit,
        history_depth: value.history_depth,
        max_history_commits: value.max_history_commits,
        snapshot_file_count: value.snapshot_file_count,
        verification_status: value.verification_status,
        authoritative_hydration_allowed: value.authoritative_hydration_allowed,
        degraded: value.degraded,
        verification_error: value.verification_error,
        repair_hint: value.repair_hint,
        current_manifest_digest: value.current_manifest_digest,
        last_verified_manifest_digest: value.last_verified_manifest_digest,
        previous_manifest_digest: value.previous_manifest_digest,
        last_successful_publish_at: value.last_successful_publish_at,
        last_successful_publish_retry_count: value.last_successful_publish_retry_count,
        publish_retry_budget: value.publish_retry_budget,
        compacted_head: value.compacted_head,
        needs_compaction: value.needs_compaction,
        compaction_status: value.compaction_status,
        compaction_mode: value.compaction_mode,
        last_compacted_at: value.last_compacted_at,
        compaction_previous_head_commit: value.compaction_previous_head_commit,
        compaction_previous_history_depth: value.compaction_previous_history_depth,
        archive_boundary_manifest_digest: value.archive_boundary_manifest_digest,
        summary_published_at: value.summary_published_at,
        summary_freshness_status: value.summary_freshness_status,
        authoritative_fallback_required: value.authoritative_fallback_required,
        freshness_reason: value.freshness_reason,
        lagging_task_shard_refs: value.lagging_task_shard_refs,
        lagging_claim_shard_refs: value.lagging_claim_shard_refs,
        lagging_runtime_refs: value.lagging_runtime_refs,
        newest_authoritative_ref_at: value.newest_authoritative_ref_at,
        runtime_descriptor_count: value.runtime_descriptor_count,
        runtime_descriptors: value
            .runtime_descriptors
            .into_iter()
            .map(runtime_shared_coordination_runtime_descriptor_view)
            .collect(),
    }
}

#[allow(dead_code)]
pub(crate) fn runtime_shared_coordination_ref_view(
    value: SharedCoordinationRefDiagnostics,
) -> RuntimeSharedCoordinationRefView {
    runtime_coordination_authority_view(value)
}

pub(crate) fn coordination_authority_protocol_state(
    authority_error: &CoordinationAuthorityMutationError,
) -> CoordinationTransactionProtocolState {
    match authority_error.status {
        CoordinationAuthorityMutationStatus::Conflict => CoordinationTransactionProtocolState {
            outcome: "Rejected".to_string(),
            commit: None,
            authority_version: None,
            rejection: Some(CoordinationTransactionProtocolRejection {
                stage: "commit".to_string(),
                category: "conflict".to_string(),
                reason_code: authority_error.reason_code.to_string(),
                message: authority_error.message.clone(),
            }),
            indeterminate: None,
        },
        CoordinationAuthorityMutationStatus::Rejected => CoordinationTransactionProtocolState {
            outcome: "Rejected".to_string(),
            commit: None,
            authority_version: None,
            rejection: Some(CoordinationTransactionProtocolRejection {
                stage: "commit".to_string(),
                category: "domain_violation".to_string(),
                reason_code: authority_error.reason_code.to_string(),
                message: authority_error.message.clone(),
            }),
            indeterminate: None,
        },
        CoordinationAuthorityMutationStatus::Indeterminate => {
            CoordinationTransactionProtocolState {
                outcome: "Indeterminate".to_string(),
                commit: None,
                authority_version: None,
                rejection: None,
                indeterminate: Some(CoordinationTransactionProtocolIndeterminate {
                    reason_code: authority_error.reason_code.to_string(),
                    message: authority_error.message.clone(),
                }),
            }
        }
    }
}

pub(crate) fn coordination_query_protocol_result(
    event_id: &EventId,
    protocol_error: &CoordinationTransactionError,
) -> Option<CoordinationMutationResult> {
    match protocol_error {
        CoordinationTransactionError::Rejected(rejection) => {
            if matches!(
                rejection.stage,
                prism_query::CoordinationTransactionValidationStage::Domain
                    | prism_query::CoordinationTransactionValidationStage::Commit
            ) {
                return None;
            }
            let state = serde_json::to_value(protocol_error.protocol_state()).ok()?;
            Some(CoordinationMutationResult {
                event_id: event_id.0.to_string(),
                event_ids: Vec::new(),
                rejected: true,
                violations: vec![coordination_protocol_violation(
                    &rejection.reason_code,
                    &rejection.message,
                    rejection.stage.tag(),
                    rejection.category.tag(),
                )],
                state,
            })
        }
        CoordinationTransactionError::Indeterminate { .. } => Some(CoordinationMutationResult {
            event_id: event_id.0.to_string(),
            event_ids: Vec::new(),
            rejected: false,
            violations: Vec::new(),
            state: serde_json::to_value(protocol_error.protocol_state()).ok()?,
        }),
    }
}

pub(crate) fn coordination_authority_protocol_result(
    event_id: &EventId,
    authority_error: &CoordinationAuthorityMutationError,
) -> Option<CoordinationMutationResult> {
    let state = coordination_protocol_state_value(
        coordination_authority_protocol_state(authority_error),
        None,
        None,
    )?;
    let rejected = !matches!(
        authority_error.status,
        CoordinationAuthorityMutationStatus::Indeterminate
    );
    let violations = if rejected {
        let category = match authority_error.status {
            CoordinationAuthorityMutationStatus::Conflict => "conflict",
            CoordinationAuthorityMutationStatus::Rejected => "domain_violation",
            CoordinationAuthorityMutationStatus::Indeterminate => unreachable!(),
        };
        vec![coordination_protocol_violation(
            &authority_error.reason_code,
            &authority_error.message,
            "commit",
            category,
        )]
    } else {
        Vec::new()
    };
    Some(CoordinationMutationResult {
        event_id: event_id.0.to_string(),
        event_ids: Vec::new(),
        rejected,
        violations,
        state,
    })
}

pub(crate) fn coordination_protocol_state_value(
    protocol_state: CoordinationTransactionProtocolState,
    workspace_root: Option<&Path>,
    authority_store_provider: Option<&CoordinationAuthorityStoreProvider>,
) -> Option<Value> {
    let mut state = serde_json::to_value(protocol_state).ok()?;
    attach_coordination_authority_stamp(&mut state, workspace_root, authority_store_provider);
    Some(state)
}

pub(crate) fn attach_coordination_authority_stamp(
    state: &mut Value,
    workspace_root: Option<&Path>,
    authority_store_provider: Option<&CoordinationAuthorityStoreProvider>,
) {
    let Some(authority_stamp) =
        coordination_transaction_authority_stamp_view(workspace_root, authority_store_provider)
    else {
        return;
    };
    let Value::Object(object) = state else {
        return;
    };
    object.insert("authorityStamp".to_string(), authority_stamp);
}

fn coordination_transaction_authority_stamp_view(
    workspace_root: Option<&Path>,
    authority_store_provider: Option<&CoordinationAuthorityStoreProvider>,
) -> Option<Value> {
    let workspace_root = workspace_root?;
    let provider = authority_store_provider
        .cloned()
        .or_else(|| configured_coordination_authority_store_provider(workspace_root).ok())?;
    let store = provider.open(workspace_root).ok()?;
    let authority = store
        .read_current(CoordinationReadRequest {
            consistency: CoordinationReadConsistency::Strong,
            view: CoordinationStateView::Summary,
        })
        .ok()?
        .authority?;
    serde_json::to_value(AuthorityStampView {
        backend_kind: format!("{:?}", authority.backend_kind),
        logical_repo_id: authority.logical_repo_id,
        snapshot_id: authority.snapshot_id,
        transaction_id: authority.transaction_id,
        committed_at: authority.committed_at,
        provenance: AuthorityStampProvenanceView {
            ref_name: authority.provenance.ref_name,
            head_commit: authority.provenance.head_commit,
            manifest_digest: authority.provenance.manifest_digest,
        },
    })
    .ok()
}

fn runtime_shared_coordination_runtime_descriptor_view(
    value: prism_coordination::RuntimeDescriptor,
) -> RuntimeSharedCoordinationRuntimeDescriptorView {
    RuntimeSharedCoordinationRuntimeDescriptorView {
        runtime_id: value.runtime_id,
        repo_id: value.repo_id,
        worktree_id: value.worktree_id,
        principal_id: value.principal_id,
        instance_started_at: value.instance_started_at,
        last_seen_at: value.last_seen_at,
        branch_ref: value.branch_ref,
        checked_out_commit: value.checked_out_commit,
        capabilities: value
            .capabilities
            .into_iter()
            .map(|capability| match capability {
                prism_coordination::RuntimeDescriptorCapability::CoordinationRefPublisher => {
                    RuntimeDescriptorCapabilityView::CoordinationRefPublisher
                }
                prism_coordination::RuntimeDescriptorCapability::BoundedPeerReads => {
                    RuntimeDescriptorCapabilityView::BoundedPeerReads
                }
                prism_coordination::RuntimeDescriptorCapability::BundleExports => {
                    RuntimeDescriptorCapabilityView::BundleExports
                }
            })
            .collect(),
        discovery_mode: match value.discovery_mode {
            prism_coordination::RuntimeDiscoveryMode::None => RuntimeDiscoveryModeView::None,
            prism_coordination::RuntimeDiscoveryMode::LanDirect => {
                RuntimeDiscoveryModeView::LanDirect
            }
            prism_coordination::RuntimeDiscoveryMode::PublicUrl => {
                RuntimeDiscoveryModeView::PublicUrl
            }
            prism_coordination::RuntimeDiscoveryMode::Full => RuntimeDiscoveryModeView::Full,
        },
        peer_endpoint: value.peer_endpoint,
        public_endpoint: value.public_endpoint,
        peer_transport_identity: value.peer_transport_identity,
        blob_snapshot_head: value.blob_snapshot_head,
        export_policy: value.export_policy,
    }
}

fn worktree_mode_label(mode: WorktreeMode) -> &'static str {
    match mode {
        WorktreeMode::Human => "human",
        WorktreeMode::Agent => "agent",
    }
}

fn coordination_protocol_violation(
    code: &str,
    summary: &str,
    stage: &str,
    category: &str,
) -> MutationViolationView {
    MutationViolationView {
        code: code.to_string(),
        summary: summary.to_string(),
        plan_id: None,
        task_id: None,
        claim_id: None,
        artifact_id: None,
        details: json!({
            "stage": stage,
            "category": category,
        }),
    }
}

#[cfg(test)]
mod tests {
    use prism_core::{
        CoordinationAuthorityMutationError, ProtectedStateStreamReport,
        SharedCoordinationRefDiagnostics, WorktreeMode, WorktreeRegistrationRecord,
    };
    use prism_ir::{EventId, PrincipalKind};
    use prism_query::CoordinationTransactionError;
    use serde_json::Value;

    use super::{
        coordination_authority_protocol_result, coordination_authority_protocol_state,
        coordination_query_protocol_result, mutation_auth_failed_error,
        mutation_auth_missing_error, mutation_bridge_execution_mismatch_error,
        mutation_bridge_execution_requires_agent_worktree_error, mutation_capability_denied_error,
        mutation_worktree_mode_mismatch_error, mutation_worktree_unregistered_error,
        peer_runtime_auth_failed_response, peer_runtime_capability_denied_response,
        protected_state_stream_view, runtime_coordination_authority_view,
    };

    #[test]
    fn authority_conflict_maps_to_commit_conflict_protocol_state() {
        let state =
            coordination_authority_protocol_state(&CoordinationAuthorityMutationError::conflict(
                "authority_transaction_conflict",
                "authority stamp no longer matches the current shared-ref head",
                None,
            ));

        assert_eq!(state.outcome, "Rejected");
        let rejection = state.rejection.expect("rejection");
        assert_eq!(rejection.stage, "commit");
        assert_eq!(rejection.category, "conflict");
        assert_eq!(rejection.reason_code, "authority_transaction_conflict");
    }

    #[test]
    fn authority_indeterminate_maps_to_indeterminate_protocol_state() {
        let state =
            coordination_authority_protocol_state(&CoordinationAuthorityMutationError::indeterminate(
                "shared_ref_transport_uncertain",
                "shared coordination ref publish may have succeeded but the outcome could not be verified",
                None,
            ));

        assert_eq!(state.outcome, "Indeterminate");
        assert!(state.rejection.is_none());
        assert_eq!(
            state.indeterminate.expect("indeterminate").reason_code,
            "shared_ref_transport_uncertain"
        );
    }

    #[test]
    fn protocol_query_rejection_builds_structured_mutation_result() {
        let error =
            CoordinationTransactionError::Rejected(prism_query::CoordinationTransactionRejection {
                stage: prism_query::CoordinationTransactionValidationStage::InputShape,
                category: prism_query::CoordinationTransactionRejectionCategory::DomainViolation,
                reason_code: "invalid_mutation_shape",
                message: "mutation payload is invalid".to_string(),
            });
        let result = coordination_query_protocol_result(&EventId::new("event:test"), &error)
            .expect("protocol result");
        assert!(result.rejected);
        assert_eq!(result.violations[0].code, "invalid_mutation_shape");
        assert_eq!(result.state["outcome"], "Rejected");
    }

    #[test]
    fn authority_conflict_builds_structured_mutation_result() {
        let error = CoordinationAuthorityMutationError::conflict(
            "authority_transaction_conflict",
            "authority stamp no longer matches",
            None,
        );
        let result = coordination_authority_protocol_result(&EventId::new("event:test"), &error)
            .expect("authority result");
        assert!(result.rejected);
        assert_eq!(result.violations[0].code, "authority_transaction_conflict");
        assert_eq!(result.state["rejection"]["category"], "conflict");
    }

    #[test]
    fn capability_denial_error_uses_stable_payload_shape() {
        let authenticated = prism_core::AuthenticatedPrincipal {
            principal: prism_ir::PrincipalProfile {
                authority_id: prism_ir::PrincipalAuthorityId::new("authority:test"),
                principal_id: prism_ir::PrincipalId::new("principal:test"),
                kind: prism_ir::PrincipalKind::Human,
                name: "Test".to_string(),
                role: None,
                status: prism_ir::PrincipalStatus::Active,
                created_at: 1,
                updated_at: 1,
                parent_principal_id: None,
                profile: Value::Null,
            },
            credential: prism_ir::CredentialRecord {
                credential_id: prism_ir::CredentialId::new("credential:test"),
                authority_id: prism_ir::PrincipalAuthorityId::new("authority:test"),
                principal_id: prism_ir::PrincipalId::new("principal:test"),
                token_verifier: "verifier".to_string(),
                capabilities: vec![prism_ir::CredentialCapability::MutateRepoMemory],
                status: prism_ir::CredentialStatus::Active,
                created_at: 1,
                last_used_at: None,
                revoked_at: None,
            },
        };
        let error = mutation_capability_denied_error("mutate_coordination", &authenticated);
        let data = error.data.expect("payload");
        assert_eq!(data["code"], "mutation_capability_denied");
        assert_eq!(data["requiredCapability"], "mutate_coordination");
        assert_eq!(data["credentialId"], "credential:test");
        assert_eq!(data["principalId"], "principal:test");
    }

    #[test]
    fn auth_missing_error_uses_stable_payload_shape() {
        let error = mutation_auth_missing_error();
        let data = error.data.expect("payload");
        assert_eq!(data["code"], "mutation_auth_missing");
        assert!(data["nextAction"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
    }

    #[test]
    fn auth_failed_error_uses_stable_payload_shape() {
        let error = mutation_auth_failed_error("credential:test", "invalid signature");
        let data = error.data.expect("payload");
        assert_eq!(data["code"], "mutation_auth_failed");
        assert_eq!(data["credentialId"], "credential:test");
        assert_eq!(data["error"], "invalid signature");
    }

    #[test]
    fn worktree_unregistered_error_includes_principal_context_when_available() {
        let authenticated = prism_core::AuthenticatedPrincipal {
            principal: prism_ir::PrincipalProfile {
                authority_id: prism_ir::PrincipalAuthorityId::new("authority:test"),
                principal_id: prism_ir::PrincipalId::new("principal:test"),
                kind: PrincipalKind::Service,
                name: "Test".to_string(),
                role: None,
                status: prism_ir::PrincipalStatus::Active,
                created_at: 1,
                updated_at: 1,
                parent_principal_id: None,
                profile: Value::Null,
            },
            credential: prism_ir::CredentialRecord {
                credential_id: prism_ir::CredentialId::new("credential:test"),
                authority_id: prism_ir::PrincipalAuthorityId::new("authority:test"),
                principal_id: prism_ir::PrincipalId::new("principal:test"),
                token_verifier: "verifier".to_string(),
                capabilities: vec![],
                status: prism_ir::CredentialStatus::Active,
                created_at: 1,
                last_used_at: None,
                revoked_at: None,
            },
        };
        let error = mutation_worktree_unregistered_error(
            Some(&authenticated),
            "Register this worktree first.",
        );
        let data = error.data.expect("payload");
        assert_eq!(data["code"], "mutation_worktree_unregistered");
        assert_eq!(data["principalId"], "principal:test");
        assert_eq!(data["principalKind"], "service");
    }

    #[test]
    fn bridge_execution_errors_use_registered_worktree_shape() {
        let registration = WorktreeRegistrationRecord {
            worktree_id: "worktree:123".to_string(),
            agent_label: "codex-a".to_string(),
            mode: WorktreeMode::Human,
            registered_at: 1,
            last_registered_at: 1,
        };
        let requires_agent = mutation_bridge_execution_requires_agent_worktree_error(&registration);
        let requires_agent_data = requires_agent.data.expect("payload");
        assert_eq!(
            requires_agent_data["code"],
            "mutation_bridge_execution_requires_agent_worktree"
        );
        assert_eq!(requires_agent_data["worktreeMode"], "human");

        let mismatch = mutation_bridge_execution_mismatch_error(
            &registration,
            &crate::PrismMutationBridgeExecutionArgs {
                worktree_id: "worktree:other".to_string(),
                agent_label: "codex-b".to_string(),
            },
        );
        let mismatch_data = mismatch.data.expect("payload");
        assert_eq!(mismatch_data["code"], "mutation_bridge_execution_mismatch");
        assert_eq!(mismatch_data["expectedWorktreeId"], "worktree:123");
        assert_eq!(mismatch_data["receivedWorktreeId"], "worktree:other");
    }

    #[test]
    fn worktree_mode_mismatch_error_uses_stable_payload_shape() {
        let authenticated = prism_core::AuthenticatedPrincipal {
            principal: prism_ir::PrincipalProfile {
                authority_id: prism_ir::PrincipalAuthorityId::new("authority:test"),
                principal_id: prism_ir::PrincipalId::new("principal:test"),
                kind: PrincipalKind::Human,
                name: "Test".to_string(),
                role: None,
                status: prism_ir::PrincipalStatus::Active,
                created_at: 1,
                updated_at: 1,
                parent_principal_id: None,
                profile: Value::Null,
            },
            credential: prism_ir::CredentialRecord {
                credential_id: prism_ir::CredentialId::new("credential:test"),
                authority_id: prism_ir::PrincipalAuthorityId::new("authority:test"),
                principal_id: prism_ir::PrincipalId::new("principal:test"),
                token_verifier: "verifier".to_string(),
                capabilities: vec![],
                status: prism_ir::CredentialStatus::Active,
                created_at: 1,
                last_used_at: None,
                revoked_at: None,
            },
        };
        let registration = WorktreeRegistrationRecord {
            worktree_id: "worktree:123".to_string(),
            agent_label: "codex-a".to_string(),
            mode: WorktreeMode::Agent,
            registered_at: 1,
            last_registered_at: 1,
        };
        let error = mutation_worktree_mode_mismatch_error(
            &authenticated,
            &registration,
            WorktreeMode::Human,
        );
        let data = error.data.expect("payload");
        assert_eq!(data["code"], "mutation_worktree_mode_mismatch");
        assert_eq!(data["worktreeMode"], "agent");
        assert_eq!(data["requiredWorktreeMode"], "human");
    }

    #[test]
    fn protected_state_stream_view_preserves_trust_fields() {
        let view = protected_state_stream_view(ProtectedStateStreamReport {
            stream: "repo_concept_events".to_string(),
            stream_id: "concepts:events".to_string(),
            protected_path: ".prism/concepts/events.jsonl".to_string(),
            verification_status: "Verified".to_string(),
            last_verified_event_id: Some("event:1".to_string()),
            last_verified_entry_hash: Some("hash:1".to_string()),
            trust_bundle_id: Some("bundle:1".to_string()),
            diagnostic_code: Some("ok".to_string()),
            diagnostic_summary: Some("verified".to_string()),
            repair_hint: None,
        });
        assert_eq!(view.stream_id, "concepts:events");
        assert_eq!(view.verification_status, "Verified");
        assert_eq!(view.trust_bundle_id.as_deref(), Some("bundle:1"));
    }

    #[test]
    fn runtime_coordination_authority_view_preserves_descriptor_trust_fields() {
        let view = runtime_coordination_authority_view(SharedCoordinationRefDiagnostics {
            ref_name: "refs/prism/coordination".to_string(),
            head_commit: Some("abc123".to_string()),
            history_depth: 3,
            max_history_commits: 64,
            snapshot_file_count: 5,
            verification_status: "verified".to_string(),
            authoritative_hydration_allowed: true,
            degraded: false,
            verification_error: None,
            repair_hint: Some("none".to_string()),
            current_manifest_digest: Some("digest:1".to_string()),
            last_verified_manifest_digest: Some("digest:1".to_string()),
            previous_manifest_digest: None,
            last_successful_publish_at: Some(1),
            last_successful_publish_retry_count: 0,
            publish_retry_budget: 3,
            compacted_head: false,
            needs_compaction: false,
            compaction_status: "clean".to_string(),
            compaction_mode: None,
            last_compacted_at: None,
            compaction_previous_head_commit: None,
            compaction_previous_history_depth: None,
            archive_boundary_manifest_digest: None,
            summary_published_at: Some(1),
            summary_freshness_status: "current".to_string(),
            authoritative_fallback_required: false,
            freshness_reason: None,
            lagging_task_shard_refs: 0,
            lagging_claim_shard_refs: 0,
            lagging_runtime_refs: 0,
            newest_authoritative_ref_at: Some(1),
            runtime_descriptor_count: 1,
            runtime_descriptors: vec![prism_coordination::RuntimeDescriptor {
                runtime_id: "runtime:1".to_string(),
                repo_id: "repo:1".to_string(),
                worktree_id: "worktree:1".to_string(),
                principal_id: "principal:1".to_string(),
                instance_started_at: 1,
                last_seen_at: 2,
                branch_ref: Some("main".to_string()),
                checked_out_commit: Some("abc123".to_string()),
                capabilities: vec![
                    prism_coordination::RuntimeDescriptorCapability::CoordinationRefPublisher,
                ],
                discovery_mode: prism_coordination::RuntimeDiscoveryMode::PublicUrl,
                peer_endpoint: Some("http://peer".to_string()),
                public_endpoint: Some("https://public".to_string()),
                peer_transport_identity: Some("transport:1".to_string()),
                blob_snapshot_head: Some("blob:1".to_string()),
                export_policy: Some("manual".to_string()),
            }],
        });
        assert_eq!(view.verification_status, "verified");
        assert_eq!(view.summary_freshness_status, "current");
        assert_eq!(view.runtime_descriptors.len(), 1);
        assert_eq!(view.runtime_descriptors[0].runtime_id, "runtime:1");
        assert_eq!(
            view.runtime_descriptors[0].discovery_mode,
            prism_js::RuntimeDiscoveryModeView::PublicUrl
        );
    }

    #[test]
    fn peer_runtime_auth_error_payloads_use_stable_shape() {
        let (status, body) =
            peer_runtime_auth_failed_response("credential:test", "invalid signature");
        assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(body.0["code"], "peer_runtime_auth_failed");
        assert_eq!(body.0["credentialId"], "credential:test");

        let (status, body) = peer_runtime_capability_denied_response("credential:test");
        assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
        assert_eq!(body.0["code"], "peer_runtime_capability_denied");
        assert_eq!(body.0["credentialId"], "credential:test");
    }
}
