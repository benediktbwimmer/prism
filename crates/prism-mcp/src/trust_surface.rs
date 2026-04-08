use std::path::Path;

use prism_core::{
    AuthenticatedPrincipal, CoordinationAuthorityMutationError,
    CoordinationAuthorityMutationStatus, CoordinationAuthorityStore, CoordinationReadConsistency,
    CoordinationReadRequest, CoordinationStateView, GitSharedRefsCoordinationAuthorityStore,
};
use prism_ir::EventId;
use prism_query::{
    CoordinationTransactionError, CoordinationTransactionProtocolIndeterminate,
    CoordinationTransactionProtocolRejection, CoordinationTransactionProtocolState,
};
use rmcp::model::ErrorData as McpError;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{CoordinationMutationResult, MutationViolationView};

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
) -> Option<Value> {
    let mut state = serde_json::to_value(protocol_state).ok()?;
    attach_coordination_authority_stamp(&mut state, workspace_root);
    Some(state)
}

pub(crate) fn attach_coordination_authority_stamp(
    state: &mut Value,
    workspace_root: Option<&Path>,
) {
    let Some(authority_stamp) = coordination_transaction_authority_stamp_view(workspace_root)
    else {
        return;
    };
    let Value::Object(object) = state else {
        return;
    };
    object.insert("authorityStamp".to_string(), authority_stamp);
}

fn coordination_transaction_authority_stamp_view(workspace_root: Option<&Path>) -> Option<Value> {
    let workspace_root = workspace_root?;
    let store = GitSharedRefsCoordinationAuthorityStore::new(workspace_root);
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
    use prism_core::CoordinationAuthorityMutationError;
    use prism_ir::EventId;
    use prism_query::CoordinationTransactionError;
    use serde_json::Value;

    use super::{
        coordination_authority_protocol_result, coordination_authority_protocol_state,
        coordination_query_protocol_result, mutation_capability_denied_error,
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
}
