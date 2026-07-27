use std::num::{NonZeroU32, NonZeroU64};

use automation_runtime_controller::{
    AwaitingCertificationScopeObservationV2, RuntimeAwaitingGatewayReadyResetClassificationV2,
    RuntimeAwaitingGatewayReadyResetReceiptV2, RuntimeCertificationReservationResetReceiptV2,
    RuntimeExecutionReceiptV1, RuntimeReservedCertificationIntentV2,
    RuntimeResetAwaitingGatewayReadyV2,
};
use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeployment, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    TransitionOutcomeV1,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Map, Value};

use super::digest::{
    certification_terminal_digest_v2, reserved_reset_receipt_bytes_v2,
    RuntimeCertificationTerminalProofV2, RuntimeReservedResetReceiptProofV2,
};
use super::reserved_projection::RuntimeReservedStartupRecoveryProgressedProjectionV2;
use crate::RuntimeExecutionPersistenceErrorV1;

pub(super) struct RuntimeReservedStartupRecoveryExpectationV2<'a> {
    pub recovery_id: &'a str,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub selection_authority_revision: i64,
}

pub(super) fn validate_reserved_progressed_projection_v2(
    projection: &RuntimeReservedStartupRecoveryProgressedProjectionV2,
    expected: &RuntimeReservedStartupRecoveryExpectationV2<'_>,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if projection.terminal_at > recorded_at {
        return Err(invalid());
    }
    let (source_snapshot, successor_snapshot, convergence_attempt) =
        validate_deployment_rows(projection, recorded_at)?;
    validate_reservation(projection, &source_snapshot, convergence_attempt)?;
    validate_slot_rows(projection, &source_snapshot, recorded_at)?;
    reconstruct_domain_transition(
        projection,
        source_snapshot,
        successor_snapshot,
        convergence_attempt,
    )?;
    validate_receipt_and_terminal_digest(projection, expected)?;
    Ok(())
}

fn validate_deployment_rows(
    projection: &RuntimeReservedStartupRecoveryProgressedProjectionV2,
    recorded_at: DateTime<Utc>,
) -> Result<
    (
        RuntimeDeploymentSnapshotV1,
        RuntimeDeploymentSnapshotV1,
        NonZeroU32,
    ),
    RuntimeExecutionPersistenceErrorV1,
> {
    let source = object(&projection.source_deployment)?;
    let successor = object(&projection.successor_deployment)?;
    require_equal_except(
        source,
        successor,
        &[
            "snapshot",
            "revision",
            "phase",
            "controller_id",
            "controller_fencing_token",
            "controller_acquired_at",
            "controller_lease_expires_at",
            "updated_at",
        ],
    )?;
    let source_revision = positive_i64(source, "revision")?;
    let successor_revision = positive_i64(successor, "revision")?;
    if source_revision.checked_add(1) != Some(successor_revision)
        || text(source, "phase")? != "awaiting_gateway_ready"
        || text(successor, "phase")? != "reconciling_panels"
        || positive_i64(source, "convergence_attempt_no")?
            != positive_i64(successor, "convergence_attempt_no")?
        || !is_null(source, "live_attestation_id")?
        || !is_null(source, "live_at")?
        || !is_null(successor, "live_attestation_id")?
        || !is_null(successor, "live_at")?
    {
        return Err(invalid());
    }
    for key in [
        "controller_id",
        "controller_fencing_token",
        "controller_acquired_at",
        "controller_lease_expires_at",
    ] {
        if is_null(source, key)? || !is_null(successor, key)? {
            return Err(invalid());
        }
    }
    if required(source, "last_controller_id")? != required(source, "controller_id")?
        || required(source, "last_fencing_token")? != required(source, "controller_fencing_token")?
    {
        return Err(invalid());
    }
    let source_updated_at = timestamp(source, "updated_at")?;
    let successor_updated_at = timestamp(successor, "updated_at")?;
    let expected_updated_at = projection.terminal_at.max(
        source_updated_at
            .checked_add_signed(Duration::microseconds(1))
            .ok_or_else(invalid)?,
    );
    if successor_updated_at != expected_updated_at || successor_updated_at > recorded_at {
        return Err(invalid());
    }
    let source_snapshot_value = required(source, "snapshot")?.clone();
    let successor_snapshot_value = required(successor, "snapshot")?.clone();
    require_equal_except(
        object(&source_snapshot_value)?,
        object(&successor_snapshot_value)?,
        &[
            "revision",
            "phase",
            "controller_lease",
            "panel_certificate",
            "gateway_ready",
            "live",
        ],
    )?;
    let source_snapshot = typed_snapshot(&source_snapshot_value)?;
    let successor_snapshot = typed_snapshot(&successor_snapshot_value)?;
    validate_outer_snapshot_identity(source, &source_snapshot_value)?;
    validate_outer_snapshot_identity(successor, &successor_snapshot_value)?;
    RuntimeDeployment::restore(source_snapshot.clone()).map_err(|_| invalid())?;
    RuntimeDeployment::restore(successor_snapshot.clone()).map_err(|_| invalid())?;
    if !matches!(
        source_snapshot.phase,
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
    ) || !matches!(
        successor_snapshot.phase,
        RuntimeDeploymentPhaseV1::ReconcilingPanels
    ) || source_snapshot.revision.get()
        != u64::try_from(source_revision).map_err(|_| invalid())?
        || successor_snapshot.revision.get()
            != u64::try_from(successor_revision).map_err(|_| invalid())?
        || source_snapshot.panel_certificate.is_none()
        || source_snapshot.gateway_ready.is_some()
        || source_snapshot.live.is_some()
        || successor_snapshot.controller_lease.is_some()
        || successor_snapshot.panel_certificate.is_some()
        || successor_snapshot.gateway_ready.is_some()
        || successor_snapshot.live.is_some()
    {
        return Err(invalid());
    }
    let lease = source_snapshot
        .controller_lease
        .as_ref()
        .ok_or_else(invalid)?;
    if required(source, "controller_id")?
        != &serde_json::to_value(&lease.controller_id).map_err(|_| invalid())?
        || required(source, "controller_fencing_token")?
            != &serde_json::to_value(lease.fencing_token).map_err(|_| invalid())?
        || timestamp(source, "controller_acquired_at")? != lease.acquired_at
        || timestamp(source, "controller_lease_expires_at")? != lease.expires_at
    {
        return Err(invalid());
    }
    let convergence_attempt = positive_u32(positive_i64(source, "convergence_attempt_no")?)?;
    Ok((source_snapshot, successor_snapshot, convergence_attempt))
}

fn validate_reservation(
    projection: &RuntimeReservedStartupRecoveryProgressedProjectionV2,
    source_snapshot: &RuntimeDeploymentSnapshotV1,
    convergence_attempt: NonZeroU32,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let source = object(&projection.source_deployment)?;
    let lease = source_snapshot
        .controller_lease
        .as_ref()
        .ok_or_else(invalid)?;
    let execution = RuntimeExecutionReceiptV1 {
        snapshot: source_snapshot.clone(),
        controller_id: lease.controller_id.clone(),
        fencing_token: lease.fencing_token,
        convergence_attempt,
        acquired_at: lease.acquired_at,
        expires_at: lease.expires_at,
    };
    let reconstructed = RuntimeReservedCertificationIntentV2::new(
        &execution,
        projection.reservation.canonical_intent().clone(),
    )
    .map_err(|_| invalid())?;
    if reconstructed != projection.reservation
        || projection.reservation.operation_id() != &projection.operation_id
        || projection
            .reservation
            .operation_scope()
            .deployment_revision()
            != source_snapshot.revision
        || projection
            .reservation
            .operation_scope()
            .convergence_attempt()
            != convergence_attempt
        || positive_u64(source, "installation_authority_revision")?
            != projection
                .reservation
                .canonical_intent()
                .intent()
                .binding_pin
                .installation_authority_revision
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_slot_rows(
    projection: &RuntimeReservedStartupRecoveryProgressedProjectionV2,
    source_snapshot: &RuntimeDeploymentSnapshotV1,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let source = object(&projection.source_slot_fence)?;
    let successor = object(&projection.successor_slot_fence)?;
    require_equal_except(source, successor, &["writer_epoch", "updated_at"])?;
    let source_epoch = positive_i64(source, "writer_epoch")?;
    let successor_epoch = positive_i64(successor, "writer_epoch")?;
    if source_epoch.checked_add(1) != Some(successor_epoch)
        || required(source, "slot_guild_id")?
            != &serde_json::to_value(source_snapshot.target.guild_id).map_err(|_| invalid())?
        || required(source, "slot_ruleset_key")?
            != &serde_json::to_value(&source_snapshot.target.ruleset_key).map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    for key in [
        "pending_drain_intent_id",
        "pending_product_operation_id",
        "pending_tenant_id",
        "pending_installation_id",
        "pending_deployment_id",
        "pending_expected_revision",
        "pending_marked_at",
    ] {
        if !is_null(source, key)? || !is_null(successor, key)? {
            return Err(invalid());
        }
    }
    let source_updated_at = timestamp(source, "updated_at")?;
    let successor_updated_at = timestamp(successor, "updated_at")?;
    if successor_updated_at < source_updated_at
        || successor_updated_at < projection.terminal_at
        || successor_updated_at > recorded_at
    {
        return Err(invalid());
    }
    Ok(())
}

fn reconstruct_domain_transition(
    projection: &RuntimeReservedStartupRecoveryProgressedProjectionV2,
    source_snapshot: RuntimeDeploymentSnapshotV1,
    successor_snapshot: RuntimeDeploymentSnapshotV1,
    convergence_attempt: NonZeroU32,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let classification = RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
        AwaitingCertificationScopeObservationV2::NoAttestationForReservedOperation {
            snapshot: source_snapshot,
            reserved_operation_id: projection.operation_id.clone(),
            observed_at: projection.terminal_at,
        },
    );
    let RuntimeAwaitingGatewayReadyResetClassificationV2::Eligible(basis) = classification else {
        return Err(invalid());
    };
    let request = RuntimeResetAwaitingGatewayReadyV2::new(basis);
    let receipt = RuntimeAwaitingGatewayReadyResetReceiptV2::new(
        &request,
        TransitionOutcomeV1::Applied {
            revision: projection.resulting_deployment_revision,
        },
        successor_snapshot,
        RuntimeCertificationReservationResetReceiptV2::Consumed {
            operation_id: projection.operation_id.clone(),
            resulting_revision: projection.resulting_deployment_revision,
            consumed_at: projection.terminal_at,
        },
        projection.terminal_at,
    )
    .map_err(|_| invalid())?;
    if receipt.snapshot().revision != projection.resulting_deployment_revision
        || projection.resulting_convergence_attempt != convergence_attempt
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_receipt_and_terminal_digest(
    projection: &RuntimeReservedStartupRecoveryProgressedProjectionV2,
    expected: &RuntimeReservedStartupRecoveryExpectationV2<'_>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let expected_receipt = reserved_reset_receipt_bytes_v2(&RuntimeReservedResetReceiptProofV2 {
        recovery_id: expected.recovery_id,
        originating_emergency_generation: expected.originating_emergency_generation,
        coordinator_generation: expected.coordinator_generation,
        action_authority_revision: expected.action_authority_revision,
        selection_authority_revision: expected.selection_authority_revision,
        source_deployment_frame: &projection.source_deployment_frame,
        successor_deployment_frame: &projection.successor_deployment_frame,
        source_slot_frame: &projection.source_slot_fence_frame,
        successor_slot_frame: &projection.successor_slot_fence_frame,
        reservation_frame: &projection.reservation_frame,
        terminal_at: projection.terminal_at,
    })?;
    if projection.terminal_receipt_bytes.as_ref() != expected_receipt {
        return Err(invalid());
    }
    let operation_scope = projection.reservation.operation_scope();
    let scope = operation_scope.scope();
    let derived_digest = certification_terminal_digest_v2(&RuntimeCertificationTerminalProofV2 {
        operation_id: projection.operation_id.as_str(),
        intent_fingerprint: projection.reservation.intent_fingerprint().as_str(),
        tenant_id: scope.tenant_id.as_str(),
        installation_id: scope.installation_id.as_str(),
        deployment_id: scope.deployment_id.as_str(),
        deployment_revision: i64_value(operation_scope.deployment_revision())?,
        convergence_attempt: i64::from(operation_scope.convergence_attempt().get()),
        terminal_outcome_name: &projection.terminal_outcome_name,
        resulting_phase: &projection.resulting_phase,
        resulting_deployment_revision: i64_value(projection.resulting_deployment_revision)?,
        resulting_convergence_attempt: i64::from(projection.resulting_convergence_attempt.get()),
        terminal_at: projection.terminal_at,
        terminal_receipt_bytes: &projection.terminal_receipt_bytes,
    })?;
    let persisted_digest = projection.terminal_receipt_digest.as_bytes();
    let expected_digest = lower_hex(derived_digest);
    if persisted_digest != expected_digest.as_bytes() {
        return Err(invalid());
    }
    Ok(())
}

fn typed_snapshot(
    value: &Value,
) -> Result<RuntimeDeploymentSnapshotV1, RuntimeExecutionPersistenceErrorV1> {
    let source = object(value)?;
    let mut typed = Map::new();
    for key in [
        "identity",
        "target",
        "runtime_generation",
        "previous_runtime",
        "requested_at",
        "revision",
        "phase",
        "controller_lease",
        "last_fencing_token",
        "preflight",
        "drain",
        "activation",
        "panel_certificate",
        "gateway_ready",
        "live",
        "last_live_recovery",
        "last_runtime_failure",
    ] {
        typed.insert(key.to_owned(), required(source, key)?.clone());
    }
    serde_json::from_value(Value::Object(typed)).map_err(|_| invalid())
}

fn validate_outer_snapshot_identity(
    row: &Map<String, Value>,
    snapshot: &Value,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    for (row_key, pointer) in [
        ("tenant_id", "/identity/tenant_id"),
        ("installation_id", "/identity/installation_id"),
        ("deployment_id", "/identity/deployment_id"),
        ("promotion_id", "/identity/promotion_id"),
        ("activation_request_id", "/identity/activation_request_id"),
        ("guild_id", "/target/guild_id"),
        ("ruleset_key", "/target/ruleset_key"),
        ("target_version", "/target/version"),
        ("target_content_hash", "/target/content_hash"),
        ("binding_revision", "/target/binding_revision"),
        ("binding_fingerprint", "/target/binding_fingerprint"),
        ("runtime_generation", "/runtime_generation"),
        ("previous_runtime", "/previous_runtime"),
        ("revision", "/revision"),
    ] {
        if required(row, row_key)? != snapshot.pointer(pointer).ok_or_else(invalid)? {
            return Err(invalid());
        }
    }
    if timestamp(row, "requested_at")?
        != value_timestamp(snapshot.pointer("/requested_at").ok_or_else(invalid)?)?
    {
        return Err(invalid());
    }
    Ok(())
}

fn require_equal_except(
    left: &Map<String, Value>,
    right: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let mut left = left.clone();
    let mut right = right.clone();
    for key in allowed {
        left.remove(*key);
        right.remove(*key);
    }
    if left == right {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, RuntimeExecutionPersistenceErrorV1> {
    value.as_object().ok_or_else(invalid)
}

fn required<'a>(
    value: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Value, RuntimeExecutionPersistenceErrorV1> {
    value.get(key).ok_or_else(invalid)
}

fn text<'a>(
    value: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, RuntimeExecutionPersistenceErrorV1> {
    required(value, key)?.as_str().ok_or_else(invalid)
}

fn positive_i64(
    value: &Map<String, Value>,
    key: &str,
) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    let value = required(value, key)?.as_i64().ok_or_else(invalid)?;
    if value > 0 {
        Ok(value)
    } else {
        Err(invalid())
    }
}

fn positive_u64(
    value: &Map<String, Value>,
    key: &str,
) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(positive_i64(value, key)?)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid)
}

fn positive_u32(value: i64) -> Result<NonZeroU32, RuntimeExecutionPersistenceErrorV1> {
    u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(invalid)
}

fn timestamp(
    value: &Map<String, Value>,
    key: &str,
) -> Result<DateTime<Utc>, RuntimeExecutionPersistenceErrorV1> {
    value_timestamp(required(value, key)?)
}

fn value_timestamp(value: &Value) -> Result<DateTime<Utc>, RuntimeExecutionPersistenceErrorV1> {
    value
        .as_str()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())
}

fn is_null(
    value: &Map<String, Value>,
    key: &str,
) -> Result<bool, RuntimeExecutionPersistenceErrorV1> {
    Ok(required(value, key)?.is_null())
}

fn i64_value(value: DeploymentRevision) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value.get()).map_err(|_| invalid())
}

fn lower_hex(value: [u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in value {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use automation_runtime_controller::{
        RuntimeCertificationIntentFingerprintV2, RuntimeCertificationOperationIdV2,
        RuntimeDeploymentScopeV1, RuntimeReservedCertificationIntentV2,
    };
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ControllerId, ControllerLeaseV1,
        DeploymentRevision, DrainAttestationV1, FencingToken, PanelCertificateId,
        PanelCertificateV1, PanelReportDigestV1, PreflightAttestationV1, ProcessInstanceId,
        RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
        RuntimeDeploymentTargetV1, RuntimeGeneration,
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + second, 0).unwrap()
    }

    fn awaiting_snapshot() -> RuntimeDeploymentSnapshotV1 {
        let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(json!({
            "deployment_id": "deployment:1",
            "tenant_id": "tenant:1",
            "installation_id": "installation:1",
            "promotion_id": "9".repeat(64),
            "activation_request_id": "activation:1"
        }))
        .unwrap();
        let target: RuntimeDeploymentTargetV1 = serde_json::from_value(json!({
            "guild_id": "7",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "b".repeat(64),
            "binding_revision": 3,
            "binding_fingerprint": "a".repeat(64)
        }))
        .unwrap();
        let runtime_generation = RuntimeGeneration::new(4).unwrap();
        let fencing_token = FencingToken::new(3).unwrap();
        let snapshot = RuntimeDeploymentSnapshotV1 {
            identity: identity.clone(),
            target: target.clone(),
            runtime_generation,
            previous_runtime: None,
            requested_at: at(1),
            revision: DeploymentRevision::new(8).unwrap(),
            phase: RuntimeDeploymentPhaseV1::AwaitingGatewayReady,
            controller_lease: Some(ControllerLeaseV1 {
                controller_id: ControllerId::parse("controller:1").unwrap(),
                fencing_token,
                acquired_at: at(10),
                expires_at: at(100),
            }),
            last_fencing_token: Some(fencing_token),
            preflight: Some(PreflightAttestationV1 {
                target: target.clone(),
                runtime_generation,
                observed_runtime: None,
                checked_at: at(11),
            }),
            drain: Some(DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: runtime_generation,
                drained_at: at(12),
            }),
            activation: Some(ActivationAttestationV1 {
                activation_request_id: identity.activation_request_id,
                target: target.clone(),
                runtime_generation,
                kind: ActivationOutcomeKindV1::Activated,
                activated_at: at(13),
            }),
            panel_certificate: Some(PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
                report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
                target,
                runtime_generation,
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                declared_count: 0,
                installed_count: 0,
                unchanged_count: 0,
                skipped_transient_count: 0,
                skipped_unresolved_channel_count: 0,
                failed_count: 0,
                ambiguous_outcome_count: 0,
                stale_message_cleanup_pending_count: 0,
                orphan_message_cleanup_pending_count: 0,
                reposted_old_message_cleanup_pending_count: 0,
                reconciled_at: at(14),
            }),
            gateway_ready: None,
            live: None,
            last_live_recovery: None,
            last_runtime_failure: None,
        };
        RuntimeDeployment::restore(snapshot.clone()).unwrap();
        snapshot
    }

    fn successor_snapshot(source: &RuntimeDeploymentSnapshotV1) -> RuntimeDeploymentSnapshotV1 {
        let mut successor = source.clone();
        successor.revision = source.revision.next().unwrap();
        successor.phase = RuntimeDeploymentPhaseV1::ReconcilingPanels;
        successor.controller_lease = None;
        successor.panel_certificate = None;
        RuntimeDeployment::restore(successor.clone()).unwrap();
        successor
    }

    fn canonical_intent_bytes() -> Vec<u8> {
        let target = format!(
            concat!(
                "{{\"guild_id\":\"7\",\"ruleset_key\":\"studyroom\",\"version\":1,",
                "\"content_hash\":\"{}\",\"binding_revision\":3,",
                "\"binding_fingerprint\":\"{}\"}}"
            ),
            "b".repeat(64),
            "a".repeat(64),
        );
        let process = format!(
            "{{\"target\":{target},\"runtime_generation\":4,\"process_instance_id\":\"process:1\"}}"
        );
        format!(
            concat!(
                "{{\"format_version\":2,\"action_id\":1,",
                "\"operation_id\":\"00112233445566778899aabbccddeeff\",",
                "\"guard\":{{\"scope\":{{\"tenant_id\":\"tenant:1\",",
                "\"installation_id\":\"installation:1\",",
                "\"deployment_id\":\"deployment:1\"}},\"expected_revision\":8,",
                "\"controller_id\":\"controller:1\",\"fencing_token\":3,",
                "\"runtime_generation\":4,\"convergence_attempt\":5}},",
                "\"target\":{target},",
                "\"binding_pin\":{{\"tenant_id\":\"tenant:1\",",
                "\"installation_id\":\"installation:1\",",
                "\"installation_authority_revision\":6,\"binding_revision\":3,",
                "\"binding_fingerprint\":\"{}\"}},",
                "\"process_identity\":{process},",
                "\"gateway_owner_lease_id\":{{\"gateway_shard_id\":\"shard:0\",",
                "\"process_instance_id\":\"process:1\",\"lease_epoch\":5,",
                "\"expected_build_revision\":\"build:1\"}},",
                "\"observed_owner_revision\":7,\"runtime_build_revision\":\"build:1\",",
                "\"panel\":{{\"certificate_id\":\"panel:1\",",
                "\"report_digest\":\"{}\",\"process_identity\":{process},",
                "\"controller_fencing_token\":3}},\"serving_lease_milliseconds\":30000}}"
            ),
            "a".repeat(64),
            "c".repeat(64),
            target = target,
            process = process,
        )
        .into_bytes()
    }

    fn reservation() -> RuntimeReservedCertificationIntentV2 {
        let bytes = canonical_intent_bytes();
        let fingerprint = fingerprint(&bytes);
        RuntimeReservedCertificationIntentV2::from_persisted(
            RuntimeDeploymentScopeV1::from_identity(&awaiting_snapshot().identity),
            DeploymentRevision::new(8).unwrap(),
            NonZeroU32::new(5).unwrap(),
            &RuntimeCertificationOperationIdV2::parse("00112233445566778899aabbccddeeff").unwrap(),
            &bytes,
            &fingerprint,
        )
        .unwrap()
    }

    fn fingerprint(bytes: &[u8]) -> RuntimeCertificationIntentFingerprintV2 {
        let domain = b"starring.runtime.certification_intent.v2\0";
        let mut framed = Vec::new();
        framed.extend_from_slice(&(domain.len() as i64).to_be_bytes());
        framed.extend_from_slice(domain);
        framed.extend_from_slice(&(bytes.len() as i64).to_be_bytes());
        framed.extend_from_slice(bytes);
        RuntimeCertificationIntentFingerprintV2::parse(lower_hex(Sha256::digest(framed).into()))
            .unwrap()
    }

    fn deployment_row(snapshot: &RuntimeDeploymentSnapshotV1, updated_at: DateTime<Utc>) -> Value {
        let lease = snapshot.controller_lease.as_ref();
        json!({
            "tenant_id": snapshot.identity.tenant_id,
            "installation_id": snapshot.identity.installation_id,
            "deployment_id": snapshot.identity.deployment_id,
            "promotion_id": snapshot.identity.promotion_id,
            "activation_request_id": snapshot.identity.activation_request_id,
            "guild_id": snapshot.target.guild_id,
            "ruleset_key": snapshot.target.ruleset_key,
            "target_version": snapshot.target.version,
            "target_content_hash": snapshot.target.content_hash,
            "binding_revision": snapshot.target.binding_revision,
            "binding_fingerprint": snapshot.target.binding_fingerprint,
            "installation_authority_revision": 6,
            "runtime_generation": snapshot.runtime_generation,
            "previous_runtime": snapshot.previous_runtime,
            "requested_at": snapshot.requested_at,
            "revision": snapshot.revision,
            "phase": match snapshot.phase {
                RuntimeDeploymentPhaseV1::AwaitingGatewayReady => "awaiting_gateway_ready",
                RuntimeDeploymentPhaseV1::ReconcilingPanels => "reconciling_panels",
                _ => panic!(),
            },
            "controller_id": lease.map(|value| &value.controller_id),
            "controller_fencing_token": lease.map(|value| value.fencing_token),
            "controller_acquired_at": lease.map(|value| value.acquired_at),
            "controller_lease_expires_at": lease.map(|value| value.expires_at),
            "last_controller_id": "controller:1",
            "last_fencing_token": 3,
            "convergence_attempt_no": 5,
            "live_attestation_id": null,
            "live_at": null,
            "updated_at": updated_at,
            "snapshot": snapshot,
            "unchanged_marker": "exact"
        })
    }

    fn jsonb_frame(value: &Value) -> Box<[u8]> {
        [vec![1], serde_json::to_vec(value).unwrap()]
            .concat()
            .into_boxed_slice()
    }

    fn expectation() -> RuntimeReservedStartupRecoveryExpectationV2<'static> {
        RuntimeReservedStartupRecoveryExpectationV2 {
            recovery_id: "0123456789abcdef0123456789abcdef",
            originating_emergency_generation: 2,
            coordinator_generation: 3,
            action_authority_revision: 5,
            selection_authority_revision: 4,
        }
    }

    fn valid_projection() -> RuntimeReservedStartupRecoveryProgressedProjectionV2 {
        let source_snapshot = awaiting_snapshot();
        let successor_snapshot = successor_snapshot(&source_snapshot);
        let source_deployment = deployment_row(&source_snapshot, at(15));
        let successor_deployment = deployment_row(&successor_snapshot, at(20));
        let source_slot_fence = json!({
            "slot_guild_id": "7",
            "slot_ruleset_key": "studyroom",
            "writer_epoch": 10,
            "pending_drain_intent_id": null,
            "pending_product_operation_id": null,
            "pending_tenant_id": null,
            "pending_installation_id": null,
            "pending_deployment_id": null,
            "pending_expected_revision": null,
            "pending_marked_at": null,
            "updated_at": at(16),
            "unchanged_marker": "exact"
        });
        let mut successor_slot_fence = source_slot_fence.clone();
        successor_slot_fence["writer_epoch"] = json!(11);
        successor_slot_fence["updated_at"] = json!(at(20));
        let reservation = reservation();
        let mut projection = RuntimeReservedStartupRecoveryProgressedProjectionV2 {
            operation_id: reservation.operation_id().clone(),
            source_deployment_frame: jsonb_frame(&source_deployment),
            source_deployment,
            successor_deployment_frame: jsonb_frame(&successor_deployment),
            successor_deployment,
            source_slot_fence_frame: jsonb_frame(&source_slot_fence),
            source_slot_fence,
            successor_slot_fence_frame: jsonb_frame(&successor_slot_fence),
            successor_slot_fence,
            reservation,
            reservation_frame: b"reservation-frame".to_vec().into_boxed_slice(),
            terminal_outcome_name: "awaiting_reset".to_owned(),
            resulting_phase: "reconciling_panels".to_owned(),
            resulting_deployment_revision: DeploymentRevision::new(9).unwrap(),
            resulting_convergence_attempt: NonZeroU32::new(5).unwrap(),
            terminal_at: at(20),
            terminal_receipt_bytes: b"pending".to_vec().into_boxed_slice(),
            terminal_receipt_digest: "0".repeat(64),
        };
        projection.terminal_receipt_bytes =
            reserved_reset_receipt_bytes_v2(&RuntimeReservedResetReceiptProofV2 {
                recovery_id: expectation().recovery_id,
                originating_emergency_generation: 2,
                coordinator_generation: 3,
                action_authority_revision: 5,
                selection_authority_revision: 4,
                source_deployment_frame: &projection.source_deployment_frame,
                successor_deployment_frame: &projection.successor_deployment_frame,
                source_slot_frame: &projection.source_slot_fence_frame,
                successor_slot_frame: &projection.successor_slot_fence_frame,
                reservation_frame: &projection.reservation_frame,
                terminal_at: projection.terminal_at,
            })
            .unwrap()
            .into_boxed_slice();
        let scope = projection.reservation.operation_scope();
        projection.terminal_receipt_digest = lower_hex(
            certification_terminal_digest_v2(&RuntimeCertificationTerminalProofV2 {
                operation_id: projection.operation_id.as_str(),
                intent_fingerprint: projection.reservation.intent_fingerprint().as_str(),
                tenant_id: scope.scope().tenant_id.as_str(),
                installation_id: scope.scope().installation_id.as_str(),
                deployment_id: scope.scope().deployment_id.as_str(),
                deployment_revision: 8,
                convergence_attempt: 5,
                terminal_outcome_name: &projection.terminal_outcome_name,
                resulting_phase: &projection.resulting_phase,
                resulting_deployment_revision: 9,
                resulting_convergence_attempt: 5,
                terminal_at: projection.terminal_at,
                terminal_receipt_bytes: &projection.terminal_receipt_bytes,
            })
            .unwrap(),
        );
        projection
    }

    #[test]
    fn reserved_progression_reconstructs_domain_receipt_and_both_digests() {
        validate_reserved_progressed_projection_v2(&valid_projection(), &expectation(), at(21))
            .unwrap();
    }

    #[test]
    fn reserved_progression_rejects_deployment_reservation_and_slot_forgery() {
        let mut deployment = valid_projection();
        deployment.successor_deployment["unchanged_marker"] = json!("forged");
        assert!(
            validate_reserved_progressed_projection_v2(&deployment, &expectation(), at(21))
                .is_err()
        );

        let mut intent = valid_projection();
        intent.source_deployment["installation_authority_revision"] = json!(7);
        assert!(
            validate_reserved_progressed_projection_v2(&intent, &expectation(), at(21)).is_err()
        );

        let mut slot = valid_projection();
        slot.successor_slot_fence["writer_epoch"] = json!(12);
        assert!(validate_reserved_progressed_projection_v2(&slot, &expectation(), at(21)).is_err());
    }

    #[test]
    fn reserved_progression_rejects_receipt_terminal_and_time_forgery() {
        let mut receipt = valid_projection();
        receipt.terminal_receipt_bytes[0] ^= 1;
        assert!(
            validate_reserved_progressed_projection_v2(&receipt, &expectation(), at(21)).is_err()
        );

        let mut digest = valid_projection();
        digest.terminal_receipt_digest.replace_range(0..2, "ff");
        assert!(
            validate_reserved_progressed_projection_v2(&digest, &expectation(), at(21)).is_err()
        );

        let future = valid_projection();
        assert!(
            validate_reserved_progressed_projection_v2(&future, &expectation(), at(19)).is_err()
        );
    }
}
