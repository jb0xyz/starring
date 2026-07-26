use std::time::Duration;

use automation_runtime_controller::{
    runtime_failure_message_v1, RuntimeClaimNextExecutionV1, RuntimeConvergenceMutationV1,
    RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1, RuntimeMutationReceiptV1,
};
use automation_runtime_convergence::{
    CommandGuardV1, FencingToken, LeaseRequestV1, RuntimeDeploymentPhaseV1,
    RuntimeFailureDispositionV1, RuntimeFailureV1, RuntimePendingConditionV1, TransitionOutcomeV1,
};

use crate::row::{
    DecodedRuntimeClaimRowV1, DecodedRuntimeExecutionRowV1, DecodedRuntimeMutationRowV1,
    RuntimeExecutionOutcomeV1,
};
use crate::RuntimeExecutionPersistenceErrorV1;

pub(crate) fn prove_claim_next_v1(
    row: DecodedRuntimeClaimRowV1,
    request: &RuntimeClaimNextExecutionV1,
) -> Result<RuntimeExecutionReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let previous_convergence_attempt = row.previous_convergence_attempt;
    let row = row.execution;
    if previous_convergence_attempt.checked_add(1) != Some(row.convergence_attempt.get()) {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    if row.controller_id != request.controller_id {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    if row.outcome == RuntimeExecutionOutcomeV1::Applied {
        let expected_fence = match row.previous.snapshot().last_fencing_token {
            Some(value) => value.next().ok(),
            None => Some(FencingToken::FIRST),
        };
        if expected_fence != Some(row.fencing_token) {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
    }
    prove_transition_v1(row, request.lease_for)
}

pub(crate) fn prove_renew_v1(
    row: DecodedRuntimeExecutionRowV1,
    guard: &RuntimeExecutionGuardV1,
    lease_for: Duration,
) -> Result<RuntimeExecutionReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
    let expected_revision = guard.expected_revision.next().map_err(|_| invalid())?;
    let expected_fencing_token = guard.fencing_token.next().map_err(|_| invalid())?;
    let current_snapshot = row.current.snapshot();
    if row.controller_id != guard.controller_id
        || row.fencing_token != expected_fencing_token
        || row.convergence_attempt != guard.convergence_attempt
        || current_snapshot.identity.tenant_id != guard.scope.tenant_id
        || current_snapshot.identity.installation_id != guard.scope.installation_id
        || current_snapshot.identity.deployment_id != guard.scope.deployment_id
        || current_snapshot.runtime_generation != guard.runtime_generation
        || current_snapshot.revision != expected_revision
    {
        return Err(invalid());
    }
    if row.outcome == RuntimeExecutionOutcomeV1::Applied {
        let previous = row.previous.snapshot();
        let Some(previous_lease) = previous.controller_lease.as_ref() else {
            return Err(invalid());
        };
        if previous.revision != guard.expected_revision
            || previous.runtime_generation != guard.runtime_generation
            || previous.identity != current_snapshot.identity
            || previous.last_fencing_token != Some(guard.fencing_token)
            || previous_lease.controller_id != guard.controller_id
            || previous_lease.fencing_token != guard.fencing_token
            || previous_lease.expires_at <= row.acquired_at
        {
            return Err(invalid());
        }
    }
    prove_transition_v1(row, lease_for)
}

pub(crate) fn prove_mutation_v1(
    row: DecodedRuntimeMutationRowV1,
    request: &automation_runtime_controller::RuntimeMutationRequestV1,
) -> Result<RuntimeMutationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
    let expected_revision = request
        .guard
        .expected_revision
        .next()
        .map_err(|_| invalid())?;
    let previous_snapshot = row.previous.snapshot();
    let current_snapshot = row.current.snapshot();
    if row.convergence_attempt != request.guard.convergence_attempt
        || !request.guard.scope.matches(&current_snapshot.identity)
        || current_snapshot.runtime_generation != request.guard.runtime_generation
        || current_snapshot.revision != expected_revision
        || current_snapshot.last_fencing_token != Some(request.guard.fencing_token)
    {
        return Err(invalid());
    }
    match row.outcome {
        RuntimeExecutionOutcomeV1::Applied => {
            if !request.guard.scope.matches(&previous_snapshot.identity)
                || previous_snapshot.runtime_generation != request.guard.runtime_generation
                || previous_snapshot.revision != request.guard.expected_revision
            {
                return Err(invalid());
            }
        }
        RuntimeExecutionOutcomeV1::Replayed => {
            if previous_snapshot != current_snapshot {
                return Err(invalid());
            }
        }
    }
    let effective_clock = mutation_effective_clock_v1(&row, &request.mutation)?;
    if effective_clock > row.mutated_at || effective_clock < current_snapshot.requested_at {
        return Err(invalid());
    }
    validate_clock_bound_v1(&request.mutation, row.mutated_at)?;
    validate_resume_clock_v1(&row, &request.mutation)?;
    if !exact_mutation_poststate_v1(&current_snapshot.phase, &request.mutation) {
        return Err(invalid());
    }
    let command_guard = CommandGuardV1 {
        expected_revision: request.guard.expected_revision,
        controller_id: request.guard.controller_id.clone(),
        fencing_token: request.guard.fencing_token,
        runtime_generation: request.guard.runtime_generation,
        now: row.mutated_at,
    };
    let mut reconstructed = row.previous;
    let transition = apply_mutation_v1(
        &mut reconstructed,
        &command_guard,
        &request.mutation,
        effective_clock,
    )?;
    let expected_outcome = match row.outcome {
        RuntimeExecutionOutcomeV1::Applied => TransitionOutcomeV1::Applied {
            revision: expected_revision,
        },
        RuntimeExecutionOutcomeV1::Replayed => TransitionOutcomeV1::Replayed {
            revision: expected_revision,
        },
    };
    if transition != expected_outcome || reconstructed.snapshot() != current_snapshot {
        return Err(invalid());
    }
    let releases_execution = mutation_releases_execution_v1(&request.mutation);
    if releases_execution {
        if current_snapshot.controller_lease.is_some() {
            return Err(invalid());
        }
        if row.outcome == RuntimeExecutionOutcomeV1::Applied {
            let previous_lease = previous_snapshot
                .controller_lease
                .as_ref()
                .ok_or_else(invalid)?;
            if previous_lease.controller_id != request.guard.controller_id
                || previous_lease.fencing_token != request.guard.fencing_token
            {
                return Err(invalid());
            }
        }
    } else {
        let current_lease = current_snapshot
            .controller_lease
            .as_ref()
            .ok_or_else(invalid)?;
        if current_snapshot.controller_lease != previous_snapshot.controller_lease
            || current_lease.controller_id != request.guard.controller_id
            || current_lease.fencing_token != request.guard.fencing_token
            || current_lease.expires_at <= row.mutated_at
        {
            return Err(invalid());
        }
    }
    Ok(RuntimeMutationReceiptV1 {
        action_id: request.action_id,
        outcome: transition,
        snapshot: current_snapshot,
        convergence_attempt: row.convergence_attempt,
    })
}

fn apply_mutation_v1(
    deployment: &mut automation_runtime_convergence::RuntimeDeployment,
    guard: &CommandGuardV1,
    mutation: &RuntimeConvergenceMutationV1,
    effective_clock: chrono::DateTime<chrono::Utc>,
) -> Result<TransitionOutcomeV1, RuntimeExecutionPersistenceErrorV1> {
    let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
    let transition = match mutation {
        RuntimeConvergenceMutationV1::AcceptPreflight(attestation) => {
            deployment.accept_preflight(guard, attestation.clone())
        }
        RuntimeConvergenceMutationV1::RequestDrain => deployment.request_drain(guard),
        RuntimeConvergenceMutationV1::AcceptDrain(attestation) => {
            deployment.accept_drain(guard, attestation.clone())
        }
        RuntimeConvergenceMutationV1::BeginActivation => deployment.begin_activation(guard),
        RuntimeConvergenceMutationV1::AcceptActivation(attestation) => {
            deployment.accept_activation(guard, attestation.clone())
        }
        RuntimeConvergenceMutationV1::RecordRetryableFailure {
            failure_id,
            kind,
            code,
            attempt,
            retry_after,
        } => {
            let retry_after = chrono::TimeDelta::from_std(*retry_after).map_err(|_| invalid())?;
            let retry_not_before = effective_clock
                .checked_add_signed(retry_after)
                .ok_or_else(invalid)?;
            deployment.record_retryable_failure(
                guard,
                RuntimeFailureV1 {
                    failure_id: failure_id.clone(),
                    kind: *kind,
                    code: code.clone(),
                    message: runtime_failure_message_v1(*kind).to_string(),
                    recorded_at: effective_clock,
                },
                attempt.to_owned(),
                retry_not_before,
            )
        }
        RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id,
            kind,
            code,
        } => deployment.record_blocked_failure(
            guard,
            RuntimeFailureV1 {
                failure_id: failure_id.clone(),
                kind: *kind,
                code: code.clone(),
                message: runtime_failure_message_v1(*kind).to_string(),
                recorded_at: effective_clock,
            },
        ),
        RuntimeConvergenceMutationV1::ResumeRuntimePending => {
            deployment.resume_runtime_pending(guard)
        }
        RuntimeConvergenceMutationV1::BeginPanelReconciliation => {
            deployment.begin_panel_reconciliation(guard)
        }
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate) => {
            deployment.accept_panel_certificate(guard, certificate.clone())
        }
        RuntimeConvergenceMutationV1::Supersede { by, reason } => {
            deployment.supersede(guard, by.clone(), reason.clone(), effective_clock)
        }
        RuntimeConvergenceMutationV1::Cancel { reason } => {
            deployment.cancel(guard, reason.clone(), effective_clock)
        }
    };
    transition.map_err(|_| invalid())
}

fn mutation_effective_clock_v1(
    row: &DecodedRuntimeMutationRowV1,
    mutation: &RuntimeConvergenceMutationV1,
) -> Result<chrono::DateTime<chrono::Utc>, RuntimeExecutionPersistenceErrorV1> {
    if row.outcome == RuntimeExecutionOutcomeV1::Applied {
        return Ok(row.mutated_at);
    }
    let snapshot = row.current.snapshot();
    let clock = match mutation {
        RuntimeConvergenceMutationV1::RecordRetryableFailure { .. } => match &snapshot.phase {
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Retryable { failure, .. },
            } => Some(failure.recorded_at),
            _ => None,
        },
        RuntimeConvergenceMutationV1::RecordBlockedFailure { .. } => match &snapshot.phase {
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { failure },
            } => Some(failure.recorded_at),
            _ => None,
        },
        RuntimeConvergenceMutationV1::Supersede { .. } => match &snapshot.phase {
            RuntimeDeploymentPhaseV1::Superseded { superseded_at, .. } => Some(*superseded_at),
            _ => None,
        },
        RuntimeConvergenceMutationV1::Cancel { .. } => match &snapshot.phase {
            RuntimeDeploymentPhaseV1::Cancelled { cancelled_at, .. } => Some(*cancelled_at),
            _ => None,
        },
        _ => Some(row.mutated_at),
    };
    clock.ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn validate_clock_bound_v1(
    mutation: &RuntimeConvergenceMutationV1,
    mutated_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let evidence_at = match mutation {
        RuntimeConvergenceMutationV1::AcceptPreflight(attestation) => Some(attestation.checked_at),
        RuntimeConvergenceMutationV1::AcceptDrain(attestation) => Some(attestation.drained_at),
        RuntimeConvergenceMutationV1::AcceptActivation(attestation) => {
            Some(attestation.activated_at)
        }
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate) => {
            Some(certificate.reconciled_at)
        }
        _ => None,
    };
    let maximum = mutated_at
        .checked_add_signed(chrono::TimeDelta::seconds(30))
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    if evidence_at.is_some_and(|value| value > maximum) {
        Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
    } else {
        Ok(())
    }
}

fn validate_resume_clock_v1(
    row: &DecodedRuntimeMutationRowV1,
    mutation: &RuntimeConvergenceMutationV1,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if !matches!(mutation, RuntimeConvergenceMutationV1::ResumeRuntimePending) {
        return Ok(());
    }
    let snapshot = match row.outcome {
        RuntimeExecutionOutcomeV1::Applied => row.previous.snapshot(),
        RuntimeExecutionOutcomeV1::Replayed => row.current.snapshot(),
    };
    let (failure_attempt, retry_not_before) = match snapshot.last_runtime_failure {
        Some(RuntimeFailureDispositionV1::Retryable {
            attempt,
            retry_not_before,
            ..
        }) => (attempt, retry_not_before),
        _ => return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt),
    };
    if failure_attempt >= row.convergence_attempt || retry_not_before > row.mutated_at {
        Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
    } else {
        Ok(())
    }
}

fn mutation_releases_execution_v1(mutation: &RuntimeConvergenceMutationV1) -> bool {
    matches!(
        mutation,
        RuntimeConvergenceMutationV1::RecordRetryableFailure { .. }
            | RuntimeConvergenceMutationV1::RecordBlockedFailure { .. }
            | RuntimeConvergenceMutationV1::Supersede { .. }
            | RuntimeConvergenceMutationV1::Cancel { .. }
    )
}

fn exact_mutation_poststate_v1(
    phase: &RuntimeDeploymentPhaseV1,
    mutation: &RuntimeConvergenceMutationV1,
) -> bool {
    match mutation {
        RuntimeConvergenceMutationV1::AcceptPreflight(_) => {
            matches!(phase, RuntimeDeploymentPhaseV1::PreflightReady)
        }
        RuntimeConvergenceMutationV1::RequestDrain => {
            matches!(phase, RuntimeDeploymentPhaseV1::DrainRequested)
        }
        RuntimeConvergenceMutationV1::AcceptDrain(_) => {
            matches!(phase, RuntimeDeploymentPhaseV1::Drained)
        }
        RuntimeConvergenceMutationV1::BeginActivation => {
            matches!(phase, RuntimeDeploymentPhaseV1::ActivationApplying)
        }
        RuntimeConvergenceMutationV1::AcceptActivation(_)
        | RuntimeConvergenceMutationV1::ResumeRuntimePending => matches!(
            phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready
            }
        ),
        RuntimeConvergenceMutationV1::RecordRetryableFailure { .. } => matches!(
            phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Retryable { .. }
            }
        ),
        RuntimeConvergenceMutationV1::RecordBlockedFailure { .. } => matches!(
            phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { .. }
            }
        ),
        RuntimeConvergenceMutationV1::BeginPanelReconciliation => {
            matches!(phase, RuntimeDeploymentPhaseV1::ReconcilingPanels)
        }
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(_) => {
            matches!(phase, RuntimeDeploymentPhaseV1::AwaitingGatewayReady)
        }
        RuntimeConvergenceMutationV1::Supersede { .. } => {
            matches!(phase, RuntimeDeploymentPhaseV1::Superseded { .. })
        }
        RuntimeConvergenceMutationV1::Cancel { .. } => {
            matches!(phase, RuntimeDeploymentPhaseV1::Cancelled { .. })
        }
    }
}

fn prove_transition_v1(
    row: DecodedRuntimeExecutionRowV1,
    lease_for: Duration,
) -> Result<RuntimeExecutionReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
    let expected_duration = chrono::TimeDelta::from_std(lease_for).map_err(|_| invalid())?;
    if row.expires_at.signed_duration_since(row.acquired_at) != expected_duration {
        return Err(invalid());
    }
    let current_snapshot = row.current.snapshot();
    let Some(current_lease) = current_snapshot.controller_lease.as_ref() else {
        return Err(invalid());
    };
    if current_snapshot.requested_at > row.acquired_at
        || current_lease.controller_id != row.controller_id
        || current_lease.fencing_token != row.fencing_token
        || current_lease.acquired_at != row.acquired_at
        || current_lease.expires_at != row.expires_at
        || current_snapshot.last_fencing_token != Some(row.fencing_token)
    {
        return Err(invalid());
    }
    let mut reconstructed = row.previous;
    let transition = reconstructed
        .acquire_lease(LeaseRequestV1 {
            expected_revision: reconstructed.revision(),
            controller_id: row.controller_id.clone(),
            fencing_token: row.fencing_token,
            now: row.acquired_at,
            expires_at: row.expires_at,
        })
        .map_err(|_| invalid())?;
    let expected_outcome = match row.outcome {
        RuntimeExecutionOutcomeV1::Applied => TransitionOutcomeV1::Applied {
            revision: current_snapshot.revision,
        },
        RuntimeExecutionOutcomeV1::Replayed => TransitionOutcomeV1::Replayed {
            revision: current_snapshot.revision,
        },
    };
    if transition != expected_outcome || reconstructed.snapshot() != current_snapshot {
        return Err(invalid());
    }
    Ok(RuntimeExecutionReceiptV1 {
        snapshot: current_snapshot,
        controller_id: row.controller_id,
        fencing_token: row.fencing_token,
        convergence_attempt: row.convergence_attempt,
        acquired_at: row.acquired_at,
        expires_at: row.expires_at,
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_runtime_controller::{
        RuntimeConvergenceSessionV1, RuntimeDeploymentScopeV1, RuntimeExecutionGuardV1,
        RuntimeMutationRequestV1,
    };
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ControllerId, DeploymentId,
        DrainAttestationV1, FencingToken, InstallationId, LeaseRequestV1, PanelCertificateId,
        PanelCertificateV1, PanelReportDigestV1, PreflightAttestationV1, ProcessInstanceId,
        RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentSnapshotV1,
        RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeGeneration,
        SupersedingDeploymentV1, TenantId,
    };
    use chrono::{DateTime, Utc};
    use serde_json::{json, Value};

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_753_142_400 + second, 0).unwrap()
    }

    fn snapshot_value() -> Value {
        json!({
            "identity": {
                "deployment_id": "deployment",
                "tenant_id": "tenant",
                "installation_id": "installation",
                "promotion_id": "1".repeat(64),
                "activation_request_id": "activation"
            },
            "target": {
                "guild_id": "42",
                "ruleset_key": "studyroom",
                "version": 1,
                "content_hash": "2".repeat(64),
                "binding_revision": 1,
                "binding_fingerprint": "3".repeat(64)
            },
            "runtime_generation": 1,
            "previous_runtime": null,
            "requested_at": at(0),
            "revision": 1,
            "phase": { "phase": "requested" },
            "controller_lease": null,
            "last_fencing_token": null,
            "preflight": null,
            "drain": null,
            "activation": null,
            "panel_certificate": null,
            "gateway_ready": null,
            "live": null,
            "last_live_recovery": null,
            "last_runtime_failure": null
        })
    }

    fn deployment(value: Value) -> RuntimeDeployment {
        let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(value).unwrap();
        RuntimeDeployment::restore(snapshot).unwrap()
    }

    fn request() -> RuntimeClaimNextExecutionV1 {
        RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("controller").unwrap(),
            lease_for: Duration::from_secs(60),
        }
    }

    fn applied_claim() -> DecodedRuntimeExecutionRowV1 {
        let previous = deployment(snapshot_value());
        let mut current = previous.clone();
        current
            .acquire_lease(LeaseRequestV1 {
                expected_revision: current.revision(),
                controller_id: ControllerId::parse("controller").unwrap(),
                fencing_token: FencingToken::FIRST,
                now: at(1),
                expires_at: at(61),
            })
            .unwrap();
        DecodedRuntimeExecutionRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Applied,
            previous,
            current,
            controller_id: ControllerId::parse("controller").unwrap(),
            fencing_token: FencingToken::FIRST,
            convergence_attempt: NonZeroU32::MIN,
            acquired_at: at(1),
            expires_at: at(61),
        }
    }

    fn applied_claim_row() -> DecodedRuntimeClaimRowV1 {
        DecodedRuntimeClaimRowV1 {
            execution: applied_claim(),
            previous_convergence_attempt: 0,
        }
    }

    fn replayed_claim() -> DecodedRuntimeClaimRowV1 {
        let applied = applied_claim();
        DecodedRuntimeClaimRowV1 {
            execution: DecodedRuntimeExecutionRowV1 {
                outcome: RuntimeExecutionOutcomeV1::Replayed,
                previous: applied.current.clone(),
                current: applied.current,
                controller_id: applied.controller_id,
                fencing_token: applied.fencing_token,
                convergence_attempt: applied.convergence_attempt,
                acquired_at: applied.acquired_at,
                expires_at: applied.expires_at,
            },
            previous_convergence_attempt: 0,
        }
    }

    fn guard() -> RuntimeExecutionGuardV1 {
        let claimed = applied_claim();
        RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant").unwrap(),
                installation_id: InstallationId::parse("installation").unwrap(),
                deployment_id: DeploymentId::parse("deployment").unwrap(),
            },
            expected_revision: claimed.current.revision(),
            controller_id: claimed.controller_id,
            fencing_token: claimed.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            convergence_attempt: claimed.convergence_attempt,
        }
    }

    fn applied_renewal() -> DecodedRuntimeExecutionRowV1 {
        let claim = applied_claim();
        let previous = claim.current;
        let mut current = previous.clone();
        current
            .acquire_lease(LeaseRequestV1 {
                expected_revision: current.revision(),
                controller_id: ControllerId::parse("controller").unwrap(),
                fencing_token: FencingToken::new(2).unwrap(),
                now: at(10),
                expires_at: at(70),
            })
            .unwrap();
        DecodedRuntimeExecutionRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Applied,
            previous,
            current,
            controller_id: ControllerId::parse("controller").unwrap(),
            fencing_token: FencingToken::new(2).unwrap(),
            convergence_attempt: NonZeroU32::MIN,
            acquired_at: at(10),
            expires_at: at(70),
        }
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        serde_json::from_value(snapshot_value()["target"].clone()).unwrap()
    }

    fn successor() -> SupersedingDeploymentV1 {
        SupersedingDeploymentV1 {
            identity: serde_json::from_value::<RuntimeDeploymentIdentityV1>(json!({
                "deployment_id": "successor",
                "tenant_id": "tenant",
                "installation_id": "installation",
                "promotion_id": "4".repeat(64),
                "activation_request_id": "activation-successor"
            }))
            .unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
        }
    }

    fn execution_receipt(
        deployment: &RuntimeDeployment,
        convergence_attempt: NonZeroU32,
    ) -> RuntimeExecutionReceiptV1 {
        let snapshot = deployment.snapshot();
        let lease = snapshot.controller_lease.as_ref().unwrap();
        let controller_id = lease.controller_id.clone();
        let fencing_token = lease.fencing_token;
        let acquired_at = lease.acquired_at;
        let expires_at = lease.expires_at;
        RuntimeExecutionReceiptV1 {
            snapshot,
            controller_id,
            fencing_token,
            convergence_attempt,
            acquired_at,
            expires_at,
        }
    }

    fn begin_request(
        deployment: &RuntimeDeployment,
        convergence_attempt: NonZeroU32,
        mutation: RuntimeConvergenceMutationV1,
    ) -> RuntimeMutationRequestV1 {
        let mut session = RuntimeConvergenceSessionV1::from_claim(execution_receipt(
            deployment,
            convergence_attempt,
        ))
        .unwrap();
        session.begin_mutation(mutation).unwrap()
    }

    fn prove_applied_mutation(
        previous: RuntimeDeployment,
        convergence_attempt: NonZeroU32,
        mutation: RuntimeConvergenceMutationV1,
        mutated_at: DateTime<Utc>,
    ) -> (
        RuntimeDeployment,
        RuntimeMutationRequestV1,
        RuntimeMutationReceiptV1,
    ) {
        let request = begin_request(&previous, convergence_attempt, mutation);
        let command_guard = CommandGuardV1 {
            expected_revision: request.guard.expected_revision,
            controller_id: request.guard.controller_id.clone(),
            fencing_token: request.guard.fencing_token,
            runtime_generation: request.guard.runtime_generation,
            now: mutated_at,
        };
        let mut current = previous.clone();
        let outcome =
            apply_mutation_v1(&mut current, &command_guard, &request.mutation, mutated_at).unwrap();
        assert!(matches!(outcome, TransitionOutcomeV1::Applied { .. }));
        let row = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Applied,
            previous,
            current: current.clone(),
            convergence_attempt,
            mutated_at,
        };
        let receipt = prove_mutation_v1(row, &request).unwrap();
        assert_eq!(receipt.action_id, request.action_id);
        assert_eq!(receipt.snapshot, current.snapshot());
        (current, request, receipt)
    }

    #[test]
    fn claim_proof_reconstructs_applied_and_replayed_outcomes() {
        let applied = prove_claim_next_v1(applied_claim_row(), &request()).unwrap();
        assert_eq!(applied.snapshot.revision.get(), 2);
        let replayed = prove_claim_next_v1(replayed_claim(), &request()).unwrap();
        assert_eq!(replayed, applied);
    }

    #[test]
    fn claim_proof_rejects_outcome_fence_duration_and_full_snapshot_forgery() {
        let mut wrong_outcome = applied_claim_row();
        wrong_outcome.execution.outcome = RuntimeExecutionOutcomeV1::Replayed;
        assert_eq!(
            prove_claim_next_v1(wrong_outcome, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_fence = applied_claim_row();
        wrong_fence.execution.fencing_token = FencingToken::new(2).unwrap();
        assert_eq!(
            prove_claim_next_v1(wrong_fence, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_duration = applied_claim_row();
        wrong_duration.execution.expires_at = at(60);
        assert_eq!(
            prove_claim_next_v1(wrong_duration, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut forged = applied_claim_row();
        let mut value = serde_json::to_value(forged.execution.current.snapshot()).unwrap();
        value["target"]["ruleset_key"] = json!("other");
        forged.execution.current = deployment(value);
        assert_eq!(
            prove_claim_next_v1(forged, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn claim_proof_requires_exact_attempt_successor_for_applied_and_replayed_rows() {
        let mut same_attempt = applied_claim_row();
        same_attempt.previous_convergence_attempt = 1;
        assert_eq!(
            prove_claim_next_v1(same_attempt, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut skipped_attempt = applied_claim_row();
        skipped_attempt.execution.convergence_attempt = NonZeroU32::new(2).unwrap();
        assert_eq!(
            prove_claim_next_v1(skipped_attempt, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut overflow = replayed_claim();
        overflow.previous_convergence_attempt = u32::MAX;
        overflow.execution.convergence_attempt = NonZeroU32::new(u32::MAX).unwrap();
        assert_eq!(
            prove_claim_next_v1(overflow, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut replay_gap = replayed_claim();
        replay_gap.previous_convergence_attempt = 2;
        assert_eq!(
            prove_claim_next_v1(replay_gap, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn renewal_proof_requires_the_exact_guard_and_one_step_transition() {
        let renewal = applied_renewal();
        let receipt = prove_renew_v1(renewal, &guard(), Duration::from_secs(60)).unwrap();
        assert_eq!(receipt.snapshot.revision.get(), 3);
        assert_eq!(receipt.fencing_token.get(), 2);

        let mut wrong_attempt = guard();
        wrong_attempt.convergence_attempt = NonZeroU32::new(2).unwrap();
        assert_eq!(
            prove_renew_v1(applied_renewal(), &wrong_attempt, Duration::from_secs(60)).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_scope = guard();
        wrong_scope.scope.deployment_id = DeploymentId::parse("other").unwrap();
        assert_eq!(
            prove_renew_v1(applied_renewal(), &wrong_scope, Duration::from_secs(60)).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_outcome = applied_renewal();
        wrong_outcome.outcome = RuntimeExecutionOutcomeV1::Replayed;
        assert_eq!(
            prove_renew_v1(wrong_outcome, &guard(), Duration::from_secs(60)).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn renewal_replay_proves_the_exact_post_state() {
        let applied = applied_renewal();
        let replay = DecodedRuntimeExecutionRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: applied.current.clone(),
            current: applied.current,
            controller_id: applied.controller_id,
            fencing_token: applied.fencing_token,
            convergence_attempt: applied.convergence_attempt,
            acquired_at: applied.acquired_at,
            expires_at: applied.expires_at,
        };
        let receipt = prove_renew_v1(replay, &guard(), Duration::from_secs(60)).unwrap();
        assert_eq!(receipt.snapshot.revision.get(), 3);
    }

    #[test]
    fn mutation_proof_reexecutes_all_twelve_closed_transitions() {
        let claimed = applied_claim().current;
        let attempt = NonZeroU32::MIN;
        let preflight = RuntimeConvergenceMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            observed_runtime: None,
            checked_at: at(2),
        });
        let (preflight_ready, _, _) =
            prove_applied_mutation(claimed.clone(), attempt, preflight, at(2));
        let (drain_requested, _, _) = prove_applied_mutation(
            preflight_ready,
            attempt,
            RuntimeConvergenceMutationV1::RequestDrain,
            at(3),
        );
        let (drained, _, _) = prove_applied_mutation(
            drain_requested,
            attempt,
            RuntimeConvergenceMutationV1::AcceptDrain(DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: RuntimeGeneration::FIRST,
                drained_at: at(4),
            }),
            at(4),
        );
        let (activation_applying, _, _) = prove_applied_mutation(
            drained,
            attempt,
            RuntimeConvergenceMutationV1::BeginActivation,
            at(5),
        );
        let (runtime_pending, _, _) = prove_applied_mutation(
            activation_applying,
            attempt,
            RuntimeConvergenceMutationV1::AcceptActivation(ActivationAttestationV1 {
                activation_request_id: serde_json::from_value::<RuntimeDeploymentIdentityV1>(
                    snapshot_value()["identity"].clone(),
                )
                .unwrap()
                .activation_request_id,
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                kind: ActivationOutcomeKindV1::Activated,
                activated_at: at(6),
            }),
            at(6),
        );
        let (reconciling, _, _) = prove_applied_mutation(
            runtime_pending.clone(),
            attempt,
            RuntimeConvergenceMutationV1::BeginPanelReconciliation,
            at(7),
        );
        let (awaiting_gateway, _, _) = prove_applied_mutation(
            reconciling,
            attempt,
            RuntimeConvergenceMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("certificate").unwrap(),
                report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id: ProcessInstanceId::parse("process").unwrap(),
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
                reconciled_at: at(8),
            }),
            at(8),
        );
        let (retryable, retry_request, retry_receipt) = prove_applied_mutation(
            runtime_pending.clone(),
            attempt,
            RuntimeConvergenceMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse("retry-failure").unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: "gateway_start_failed".to_string(),
                attempt,
                retry_after: Duration::from_secs(5),
            },
            at(9),
        );
        let retry_replay = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: retryable.clone(),
            current: retryable.clone(),
            convergence_attempt: attempt,
            mutated_at: at(10),
        };
        let replayed_retry = prove_mutation_v1(retry_replay, &retry_request).unwrap();
        assert_eq!(replayed_retry.action_id, retry_request.action_id);
        assert_eq!(replayed_retry.snapshot, retry_receipt.snapshot);
        assert!(matches!(
            replayed_retry.outcome,
            TransitionOutcomeV1::Replayed { .. }
        ));
        let mut forged_failure_value = serde_json::to_value(retryable.snapshot()).unwrap();
        forged_failure_value["phase"]["condition"]["failure"]["message"] =
            json!("forged failure message");
        forged_failure_value["last_runtime_failure"]["failure"]["message"] =
            json!("forged failure message");
        let forged_failure = deployment(forged_failure_value);
        let forged_retry_replay = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: forged_failure.clone(),
            current: forged_failure,
            convergence_attempt: attempt,
            mutated_at: at(10),
        };
        assert_eq!(
            prove_mutation_v1(forged_retry_replay, &retry_request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
        prove_applied_mutation(
            runtime_pending,
            attempt,
            RuntimeConvergenceMutationV1::RecordBlockedFailure {
                failure_id: RuntimeFailureId::parse("blocked-failure").unwrap(),
                kind: RuntimeFailureKindV1::InvariantViolation,
                code: "invalid_runtime_state".to_string(),
            },
            at(9),
        );
        prove_applied_mutation(
            awaiting_gateway,
            attempt,
            RuntimeConvergenceMutationV1::Supersede {
                by: successor(),
                reason: "new deployment".to_string(),
            },
            at(9),
        );
        prove_applied_mutation(
            claimed.clone(),
            attempt,
            RuntimeConvergenceMutationV1::Cancel {
                reason: "operator request".to_string(),
            },
            at(9),
        );
        let mut retry_claimed = retryable;
        retry_claimed
            .acquire_lease(LeaseRequestV1 {
                expected_revision: retry_claimed.revision(),
                controller_id: ControllerId::parse("controller").unwrap(),
                fencing_token: FencingToken::new(2).unwrap(),
                now: at(20),
                expires_at: at(80),
            })
            .unwrap();
        let resume_request = begin_request(
            &retry_claimed,
            NonZeroU32::new(2).unwrap(),
            RuntimeConvergenceMutationV1::ResumeRuntimePending,
        );
        let resume_guard = CommandGuardV1 {
            expected_revision: resume_request.guard.expected_revision,
            controller_id: resume_request.guard.controller_id.clone(),
            fencing_token: resume_request.guard.fencing_token,
            runtime_generation: resume_request.guard.runtime_generation,
            now: at(21),
        };
        let mut resumed = retry_claimed.clone();
        apply_mutation_v1(
            &mut resumed,
            &resume_guard,
            &resume_request.mutation,
            at(21),
        )
        .unwrap();
        let mut same_attempt_request = resume_request;
        same_attempt_request.guard.convergence_attempt = NonZeroU32::MIN;
        let invalid_applied_resume = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Applied,
            previous: retry_claimed.clone(),
            current: resumed.clone(),
            convergence_attempt: NonZeroU32::MIN,
            mutated_at: at(21),
        };
        assert_eq!(
            prove_mutation_v1(invalid_applied_resume, &same_attempt_request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
        let invalid_replayed_resume = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: resumed.clone(),
            current: resumed,
            convergence_attempt: NonZeroU32::MIN,
            mutated_at: at(22),
        };
        assert_eq!(
            prove_mutation_v1(invalid_replayed_resume, &same_attempt_request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
        prove_applied_mutation(
            retry_claimed,
            NonZeroU32::new(2).unwrap(),
            RuntimeConvergenceMutationV1::ResumeRuntimePending,
            at(21),
        );
    }

    #[test]
    fn terminal_replay_uses_persisted_evidence_time_and_echoes_action_identity() {
        let claimed = applied_claim().current;
        let mutation = RuntimeConvergenceMutationV1::Cancel {
            reason: "operator request".to_string(),
        };
        let (cancelled, request, applied) =
            prove_applied_mutation(claimed, NonZeroU32::MIN, mutation, at(9));
        let replay = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: cancelled.clone(),
            current: cancelled.clone(),
            convergence_attempt: NonZeroU32::MIN,
            mutated_at: at(10),
        };
        let receipt = prove_mutation_v1(replay, &request).unwrap();
        assert_eq!(receipt.action_id, request.action_id);
        assert_eq!(receipt.snapshot, applied.snapshot);
        assert!(matches!(
            receipt.outcome,
            TransitionOutcomeV1::Replayed { .. }
        ));

        let stale_clock = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: cancelled.clone(),
            current: cancelled.clone(),
            convergence_attempt: NonZeroU32::MIN,
            mutated_at: at(8),
        };
        assert_eq!(
            prove_mutation_v1(stale_clock, &request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_payload = request;
        wrong_payload.mutation = RuntimeConvergenceMutationV1::Cancel {
            reason: "different request".to_string(),
        };
        let replay = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: cancelled.clone(),
            current: cancelled,
            convergence_attempt: NonZeroU32::MIN,
            mutated_at: at(10),
        };
        assert_eq!(
            prove_mutation_v1(replay, &wrong_payload).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn mutation_proof_rejects_forged_outcome_attempt_snapshot_and_clock() {
        let claimed = applied_claim().current;
        let request = begin_request(
            &claimed,
            NonZeroU32::MIN,
            RuntimeConvergenceMutationV1::AcceptPreflight(PreflightAttestationV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                observed_runtime: None,
                checked_at: at(2),
            }),
        );
        let command_guard = CommandGuardV1 {
            expected_revision: request.guard.expected_revision,
            controller_id: request.guard.controller_id.clone(),
            fencing_token: request.guard.fencing_token,
            runtime_generation: request.guard.runtime_generation,
            now: at(2),
        };
        let mut current = claimed.clone();
        apply_mutation_v1(&mut current, &command_guard, &request.mutation, at(2)).unwrap();
        let row = || DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Applied,
            previous: claimed.clone(),
            current: current.clone(),
            convergence_attempt: NonZeroU32::MIN,
            mutated_at: at(2),
        };

        let mut wrong_outcome = row();
        wrong_outcome.outcome = RuntimeExecutionOutcomeV1::Replayed;
        assert_eq!(
            prove_mutation_v1(wrong_outcome, &request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_attempt = row();
        wrong_attempt.convergence_attempt = NonZeroU32::new(2).unwrap();
        assert_eq!(
            prove_mutation_v1(wrong_attempt, &request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut forged_value = serde_json::to_value(current.snapshot()).unwrap();
        forged_value["identity"]["deployment_id"] = json!("other");
        let forged_current = deployment(forged_value);
        let mut forged_snapshot = row();
        forged_snapshot.current = forged_current;
        assert_eq!(
            prove_mutation_v1(forged_snapshot, &request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut future_attestation = request.clone();
        if let RuntimeConvergenceMutationV1::AcceptPreflight(attestation) =
            &mut future_attestation.mutation
        {
            attestation.checked_at = at(33);
        }
        assert_eq!(
            prove_mutation_v1(row(), &future_attestation).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let expired_replay = DecodedRuntimeMutationRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: current.clone(),
            current,
            convergence_attempt: NonZeroU32::MIN,
            mutated_at: at(61),
        };
        assert_eq!(
            prove_mutation_v1(expired_replay, &request).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }
}
