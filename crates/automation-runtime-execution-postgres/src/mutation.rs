use std::time::Duration;

use automation_runtime_controller::{RuntimeConvergenceMutationV1, RuntimeExecutionGuardV1};
use serde_json::{json, Map, Value};

use crate::error::validate_millisecond_duration;
use crate::RuntimeExecutionPersistenceErrorV1;

const MAX_MUTATION_PAYLOAD_BYTES: usize = 262_144;
const MAX_FAILURE_RETRY_AFTER: Duration = Duration::from_secs(86_400);
const MAX_FAILURE_CODE_BYTES: usize = 64;
const MAX_REASON_BYTES: usize = 1_024;

pub(crate) struct EncodedRuntimeMutationV1 {
    pub(crate) kind: &'static str,
    pub(crate) payload: Value,
}

pub(crate) fn encode_runtime_mutation_v1(
    mutation: &RuntimeConvergenceMutationV1,
    guard: &RuntimeExecutionGuardV1,
) -> Result<EncodedRuntimeMutationV1, RuntimeExecutionPersistenceErrorV1> {
    let encoded = match mutation {
        RuntimeConvergenceMutationV1::AcceptPreflight(attestation) => EncodedRuntimeMutationV1 {
            kind: "accept_preflight",
            payload: json!(attestation),
        },
        RuntimeConvergenceMutationV1::RequestDrain => empty_mutation("request_drain"),
        RuntimeConvergenceMutationV1::AcceptDrain(attestation) => EncodedRuntimeMutationV1 {
            kind: "accept_drain",
            payload: json!(attestation),
        },
        RuntimeConvergenceMutationV1::BeginActivation => empty_mutation("begin_activation"),
        RuntimeConvergenceMutationV1::AcceptActivation(attestation) => EncodedRuntimeMutationV1 {
            kind: "accept_activation",
            payload: json!(attestation),
        },
        RuntimeConvergenceMutationV1::RecordRetryableFailure {
            failure_id,
            kind,
            code,
            attempt,
            retry_after,
        } => {
            validate_failure_code(code)?;
            if *attempt != guard.convergence_attempt {
                return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
            }
            let retry_after_milliseconds =
                validate_millisecond_duration(*retry_after, MAX_FAILURE_RETRY_AFTER)?;
            EncodedRuntimeMutationV1 {
                kind: "record_retryable_failure",
                payload: json!({
                    "failure_id": failure_id,
                    "kind": kind,
                    "code": code,
                    "attempt": attempt.get(),
                    "retry_after_milliseconds": retry_after_milliseconds
                }),
            }
        }
        RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id,
            kind,
            code,
        } => {
            validate_failure_code(code)?;
            EncodedRuntimeMutationV1 {
                kind: "record_blocked_failure",
                payload: json!({
                    "failure_id": failure_id,
                    "kind": kind,
                    "code": code
                }),
            }
        }
        RuntimeConvergenceMutationV1::ResumeRuntimePending => {
            empty_mutation("resume_runtime_pending")
        }
        RuntimeConvergenceMutationV1::BeginPanelReconciliation => {
            empty_mutation("begin_panel_reconciliation")
        }
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate) => {
            EncodedRuntimeMutationV1 {
                kind: "accept_panel_certificate",
                payload: json!(certificate),
            }
        }
        RuntimeConvergenceMutationV1::Supersede { by, reason } => {
            validate_reason(reason)?;
            if by.identity.deployment_id == guard.scope.deployment_id
                || by.identity.tenant_id != guard.scope.tenant_id
                || by.identity.installation_id != guard.scope.installation_id
                || by.runtime_generation <= guard.runtime_generation
                || by.runtime_generation.get() > i64::MAX as u64
                || by.target.binding_revision.get() > i64::MAX as u64
            {
                return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
            }
            EncodedRuntimeMutationV1 {
                kind: "supersede",
                payload: json!({"by": by, "reason": reason}),
            }
        }
        RuntimeConvergenceMutationV1::Cancel { reason } => {
            validate_reason(reason)?;
            EncodedRuntimeMutationV1 {
                kind: "cancel",
                payload: json!({"reason": reason}),
            }
        }
    };
    if serde_json::to_vec(&encoded.payload)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?
        .len()
        > MAX_MUTATION_PAYLOAD_BYTES
    {
        return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
    }
    Ok(encoded)
}

fn empty_mutation(kind: &'static str) -> EncodedRuntimeMutationV1 {
    EncodedRuntimeMutationV1 {
        kind,
        payload: Value::Object(Map::new()),
    }
}

fn validate_failure_code(code: &str) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if code.is_empty()
        || code.len() > MAX_FAILURE_CODE_BYTES
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(())
    }
}

fn validate_reason(reason: &str) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if reason.trim().is_empty() || reason.len() > MAX_REASON_BYTES || reason.contains('\0') {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_runtime_controller::RuntimeDeploymentScopeV1;
    use automation_runtime_convergence::{
        ActivationAttestationV1, ControllerId, DeploymentId, DeploymentRevision,
        DrainAttestationV1, FencingToken, InstallationId, PanelCertificateV1,
        PreflightAttestationV1, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
        RuntimeFailureId, RuntimeFailureKindV1, RuntimeGeneration, SupersedingDeploymentV1,
        TenantId,
    };
    use chrono::{DateTime, Utc};

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_753_142_400 + second, 0).unwrap()
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "2".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "3".repeat(64)
        }))
        .unwrap()
    }

    fn identity(deployment_id: &str) -> RuntimeDeploymentIdentityV1 {
        serde_json::from_value(json!({
            "deployment_id": deployment_id,
            "tenant_id": "tenant",
            "installation_id": "installation",
            "promotion_id": "1".repeat(64),
            "activation_request_id": "activation"
        }))
        .unwrap()
    }

    fn guard() -> RuntimeExecutionGuardV1 {
        RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant").unwrap(),
                installation_id: InstallationId::parse("installation").unwrap(),
                deployment_id: DeploymentId::parse("deployment").unwrap(),
            },
            expected_revision: DeploymentRevision::new(2).unwrap(),
            controller_id: ControllerId::parse("controller").unwrap(),
            fencing_token: FencingToken::FIRST,
            runtime_generation: RuntimeGeneration::FIRST,
            convergence_attempt: NonZeroU32::MIN,
        }
    }

    fn mutations() -> Vec<RuntimeConvergenceMutationV1> {
        let target = target();
        vec![
            RuntimeConvergenceMutationV1::AcceptPreflight(PreflightAttestationV1 {
                target: target.clone(),
                runtime_generation: RuntimeGeneration::FIRST,
                observed_runtime: None,
                checked_at: at(2),
            }),
            RuntimeConvergenceMutationV1::RequestDrain,
            RuntimeConvergenceMutationV1::AcceptDrain(DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: RuntimeGeneration::FIRST,
                drained_at: at(3),
            }),
            RuntimeConvergenceMutationV1::BeginActivation,
            RuntimeConvergenceMutationV1::AcceptActivation(ActivationAttestationV1 {
                activation_request_id: identity("deployment").activation_request_id,
                target: target.clone(),
                runtime_generation: RuntimeGeneration::FIRST,
                kind: automation_runtime_convergence::ActivationOutcomeKindV1::Activated,
                activated_at: at(4),
            }),
            RuntimeConvergenceMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse("failure").unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: "gateway_start_failed".to_string(),
                attempt: NonZeroU32::MIN,
                retry_after: Duration::from_millis(1_500),
            },
            RuntimeConvergenceMutationV1::RecordBlockedFailure {
                failure_id: RuntimeFailureId::parse("failure").unwrap(),
                kind: RuntimeFailureKindV1::InvariantViolation,
                code: "invalid_runtime_state".to_string(),
            },
            RuntimeConvergenceMutationV1::ResumeRuntimePending,
            RuntimeConvergenceMutationV1::BeginPanelReconciliation,
            RuntimeConvergenceMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
                certificate_id: automation_runtime_convergence::PanelCertificateId::parse(
                    "certificate",
                )
                .unwrap(),
                report_digest: automation_runtime_convergence::PanelReportDigestV1::parse(
                    "4".repeat(64),
                )
                .unwrap(),
                target: target.clone(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id: automation_runtime_convergence::ProcessInstanceId::parse(
                    "process",
                )
                .unwrap(),
                declared_count: 1,
                installed_count: 1,
                unchanged_count: 0,
                skipped_transient_count: 0,
                skipped_unresolved_channel_count: 0,
                failed_count: 0,
                ambiguous_outcome_count: 0,
                stale_message_cleanup_pending_count: 0,
                orphan_message_cleanup_pending_count: 0,
                reposted_old_message_cleanup_pending_count: 0,
                reconciled_at: at(5),
            }),
            RuntimeConvergenceMutationV1::Supersede {
                by: SupersedingDeploymentV1 {
                    identity: identity("successor"),
                    target,
                    runtime_generation: RuntimeGeneration::new(2).unwrap(),
                },
                reason: "new deployment".to_string(),
            },
            RuntimeConvergenceMutationV1::Cancel {
                reason: "operator request".to_string(),
            },
        ]
    }

    #[test]
    fn every_mutation_has_one_closed_stable_encoding() {
        let expected = [
            ("accept_preflight", 4),
            ("request_drain", 0),
            ("accept_drain", 3),
            ("begin_activation", 0),
            ("accept_activation", 5),
            ("record_retryable_failure", 5),
            ("record_blocked_failure", 3),
            ("resume_runtime_pending", 0),
            ("begin_panel_reconciliation", 0),
            ("accept_panel_certificate", 16),
            ("supersede", 2),
            ("cancel", 1),
        ];
        let encoded = mutations()
            .iter()
            .map(|mutation| encode_runtime_mutation_v1(mutation, &guard()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(encoded.len(), expected.len());
        for (encoded, (kind, key_count)) in encoded.iter().zip(expected) {
            assert_eq!(encoded.kind, kind);
            assert_eq!(encoded.payload.as_object().unwrap().len(), key_count);
            assert!(serde_json::to_vec(&encoded.payload).unwrap().len() <= 262_144);
        }
        assert_eq!(
            encoded[5].payload,
            json!({
                "failure_id": "failure",
                "kind": "gateway_start",
                "code": "gateway_start_failed",
                "attempt": 1,
                "retry_after_milliseconds": 1500
            })
        );
    }

    #[test]
    fn invalid_failure_and_reason_bounds_fail_before_database_access() {
        let invalid_failures = [
            RuntimeConvergenceMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse("failure").unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: "UPPER".to_string(),
                attempt: NonZeroU32::MIN,
                retry_after: Duration::from_secs(1),
            },
            RuntimeConvergenceMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse("failure").unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: "valid".to_string(),
                attempt: NonZeroU32::new(2).unwrap(),
                retry_after: Duration::from_secs(1),
            },
            RuntimeConvergenceMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse("failure").unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: "valid".to_string(),
                attempt: NonZeroU32::MIN,
                retry_after: Duration::from_nanos(1_000_001),
            },
            RuntimeConvergenceMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse("failure").unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: "valid".to_string(),
                attempt: NonZeroU32::MIN,
                retry_after: Duration::from_millis(86_400_001),
            },
        ];
        for mutation in invalid_failures {
            assert!(matches!(
                encode_runtime_mutation_v1(&mutation, &guard()),
                Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
            ));
        }
        for reason in ["   ".to_string(), "x".repeat(1_025)] {
            let mutation = RuntimeConvergenceMutationV1::Cancel { reason };
            assert!(matches!(
                encode_runtime_mutation_v1(&mutation, &guard()),
                Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
            ));
        }
        for mutation in [
            RuntimeConvergenceMutationV1::Cancel {
                reason: "invalid\0reason".to_string(),
            },
            RuntimeConvergenceMutationV1::Supersede {
                by: SupersedingDeploymentV1 {
                    identity: identity("successor"),
                    target: target(),
                    runtime_generation: RuntimeGeneration::new(2).unwrap(),
                },
                reason: "invalid\0reason".to_string(),
            },
        ] {
            assert!(matches!(
                encode_runtime_mutation_v1(&mutation, &guard()),
                Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
            ));
        }
    }
}
