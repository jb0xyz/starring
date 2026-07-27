use automation_runtime_convergence::{
    LiveLossKindV1, RecoverLiveRequestV1, RuntimeDeployment, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, TransitionOutcomeV1,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Map, Value};

use super::projection::RuntimeStartupRecoveryProgressedProjectionV2;
use crate::RuntimeExecutionPersistenceErrorV1;

pub(super) fn validate_progressed_projection_v2(
    projection: &RuntimeStartupRecoveryProgressedProjectionV2,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if projection.recovered_at > recorded_at {
        return Err(invalid());
    }
    validate_deployment_rows(projection, recorded_at)?;
    validate_slot_rows(projection, recorded_at)?;
    validate_serving_row(projection)?;
    Ok(())
}

fn validate_deployment_rows(
    projection: &RuntimeStartupRecoveryProgressedProjectionV2,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let previous = object(&projection.previous_deployment)?;
    let terminal = object(&projection.terminal_deployment)?;
    require_equal_except(
        previous,
        terminal,
        &[
            "snapshot",
            "revision",
            "phase",
            "live_attestation_id",
            "live_at",
            "updated_at",
        ],
    )?;
    let previous_revision = positive_i64(previous, "revision")?;
    let terminal_revision = positive_i64(terminal, "revision")?;
    if previous_revision.checked_add(1) != Some(terminal_revision)
        || text(previous, "phase")? != "live"
        || text(terminal, "phase")? != "runtime_pending"
        || nullable_text(previous, "live_attestation_id")?.is_none()
        || !is_null(terminal, "live_attestation_id")?
        || nullable_timestamp(previous, "live_at")?.is_none()
        || !is_null(terminal, "live_at")?
    {
        return Err(invalid());
    }
    let previous_updated_at = timestamp(previous, "updated_at")?;
    let terminal_updated_at = timestamp(terminal, "updated_at")?;
    let next_previous_updated_at = previous_updated_at
        .checked_add_signed(Duration::microseconds(1))
        .ok_or_else(invalid)?;
    let expected_updated_at = projection.recovered_at.max(next_previous_updated_at);
    if terminal_updated_at != expected_updated_at || terminal_updated_at > recorded_at {
        return Err(invalid());
    }

    let previous_snapshot_value = required(previous, "snapshot")?.clone();
    let terminal_snapshot_value = required(terminal, "snapshot")?.clone();
    require_equal_except(
        object(&previous_snapshot_value)?,
        object(&terminal_snapshot_value)?,
        &[
            "revision",
            "phase",
            "panel_certificate",
            "gateway_ready",
            "live",
            "last_live_recovery",
        ],
    )?;
    let previous_snapshot = typed_snapshot(&previous_snapshot_value)?;
    let terminal_snapshot = typed_snapshot(&terminal_snapshot_value)?;
    validate_outer_snapshot_identity(previous, &previous_snapshot_value)?;
    validate_outer_snapshot_identity(terminal, &terminal_snapshot_value)?;
    if !matches!(previous_snapshot.phase, RuntimeDeploymentPhaseV1::Live)
        || previous_snapshot.controller_lease.is_some()
        || previous_snapshot.live.is_none()
    {
        return Err(invalid());
    }
    let live = previous_snapshot.live.as_ref().ok_or_else(invalid)?;
    let loss_kind = loss_kind(projection.recovery_kind)?;
    let expected_revision = previous_snapshot.revision;
    let expected_outcome = TransitionOutcomeV1::Applied {
        revision: expected_revision.next().map_err(|_| invalid())?,
    };
    let mut reconstructed =
        RuntimeDeployment::restore(previous_snapshot.clone()).map_err(|_| invalid())?;
    let outcome = reconstructed
        .recover_live(RecoverLiveRequestV1 {
            expected_revision,
            expected_runtime_generation: previous_snapshot.runtime_generation,
            expected_process_instance_id: live.process_instance_id.clone(),
            kind: loss_kind,
            evidence_at: projection.evidence_at,
            recovered_at: projection.recovered_at,
        })
        .map_err(|_| invalid())?;
    if outcome != expected_outcome || reconstructed.snapshot() != terminal_snapshot {
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

fn validate_slot_rows(
    projection: &RuntimeStartupRecoveryProgressedProjectionV2,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let previous = object(&projection.previous_slot_fence)?;
    let terminal = object(&projection.terminal_slot_fence)?;
    require_equal_except(previous, terminal, &["writer_epoch", "updated_at"])?;
    let previous_epoch = positive_i64(previous, "writer_epoch")?;
    let terminal_epoch = positive_i64(terminal, "writer_epoch")?;
    if previous_epoch.checked_add(1) != Some(terminal_epoch) {
        return Err(invalid());
    }
    let deployment = object(&projection.previous_deployment)?;
    if required(previous, "slot_guild_id")? != required(deployment, "guild_id")?
        || required(previous, "slot_ruleset_key")? != required(deployment, "ruleset_key")?
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
        if !is_null(previous, key)? || !is_null(terminal, key)? {
            return Err(invalid());
        }
    }
    let previous_updated_at = timestamp(previous, "updated_at")?;
    let terminal_updated_at = timestamp(terminal, "updated_at")?;
    if terminal_updated_at < previous_updated_at || terminal_updated_at > recorded_at {
        return Err(invalid());
    }
    Ok(())
}

fn validate_serving_row(
    projection: &RuntimeStartupRecoveryProgressedProjectionV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let serving = object(&projection.serving_lease)?;
    let deployment = object(&projection.previous_deployment)?;
    for (serving_key, deployment_key) in [
        ("guild_id", "guild_id"),
        ("ruleset_key", "ruleset_key"),
        ("tenant_id", "tenant_id"),
        ("installation_id", "installation_id"),
        ("deployment_id", "deployment_id"),
        ("attestation_id", "live_attestation_id"),
        ("runtime_generation", "runtime_generation"),
        ("target_version", "target_version"),
        ("target_content_hash", "target_content_hash"),
        ("binding_revision", "binding_revision"),
        ("binding_fingerprint", "binding_fingerprint"),
    ] {
        if required(serving, serving_key)? != required(deployment, deployment_key)? {
            return Err(invalid());
        }
    }
    let snapshot = required(deployment, "snapshot")?;
    if required(serving, "process_instance_id")?
        != snapshot
            .pointer("/live/process_instance_id")
            .ok_or_else(invalid)?
    {
        return Err(invalid());
    }
    if positive_i64(serving, "lease_epoch").is_err() || positive_i64(serving, "revision").is_err() {
        return Err(invalid());
    }
    let acquired_at = timestamp(serving, "acquired_at")?;
    let last_heartbeat_at = timestamp(serving, "last_heartbeat_at")?;
    let expires_at = timestamp(serving, "expires_at")?;
    if acquired_at > last_heartbeat_at
        || last_heartbeat_at > expires_at
        || expires_at > projection.recovered_at
    {
        return Err(invalid());
    }
    let connected = boolean(serving, "connected")?;
    let serving_state = boolean(serving, "serving")?;
    if connected != serving_state {
        return Err(invalid());
    }
    match projection.recovery_kind {
        1 if !connected
            && last_heartbeat_at == expires_at
            && projection.evidence_at == last_heartbeat_at => {}
        2 if connected
            && last_heartbeat_at < expires_at
            && projection.evidence_at == expires_at => {}
        _ => return Err(invalid()),
    }
    let certified_at: DateTime<Utc> = snapshot
        .pointer("/live/certified_at")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    if projection.evidence_at < certified_at || projection.evidence_at > projection.recovered_at {
        return Err(invalid());
    }
    Ok(())
}

fn loss_kind(value: i16) -> Result<LiveLossKindV1, RuntimeExecutionPersistenceErrorV1> {
    match value {
        1 => Ok(LiveLossKindV1::ServingDisconnected),
        2 => Ok(LiveLossKindV1::ServingLeaseExpired),
        _ => Err(invalid()),
    }
}

fn require_equal_except(
    left: &Map<String, Value>,
    right: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let mut left = (*left).clone();
    let mut right = (*right).clone();
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

fn nullable_text<'a>(
    value: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, RuntimeExecutionPersistenceErrorV1> {
    match required(value, key)? {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        _ => Err(invalid()),
    }
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

fn boolean(
    value: &Map<String, Value>,
    key: &str,
) -> Result<bool, RuntimeExecutionPersistenceErrorV1> {
    required(value, key)?.as_bool().ok_or_else(invalid)
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

fn nullable_timestamp(
    value: &Map<String, Value>,
    key: &str,
) -> Result<Option<DateTime<Utc>>, RuntimeExecutionPersistenceErrorV1> {
    match required(value, key)? {
        Value::Null => Ok(None),
        Value::String(value) => value.parse().map(Some).map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

fn is_null(
    value: &Map<String, Value>,
    key: &str,
) -> Result<bool, RuntimeExecutionPersistenceErrorV1> {
    Ok(required(value, key)?.is_null())
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, CommandGuardV1, ControllerId,
        DrainAttestationV1, FencingToken, GatewayReadyAttestationV1, GatewayReadyKindV1,
        LeaseRequestV1, PanelCertificateId, PanelCertificateV1, PanelReportDigestV1,
        PreflightAttestationV1, ProcessInstanceId, RuntimeDeploymentIdentityV1,
        RuntimeDeploymentTargetV1, RuntimeGeneration,
    };
    use serde_json::json;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + second, 0).unwrap()
    }

    fn live_deployment() -> RuntimeDeployment {
        let target: RuntimeDeploymentTargetV1 = serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "2".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "3".repeat(64)
        }))
        .unwrap();
        let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(json!({
            "deployment_id": "deployment",
            "tenant_id": "tenant",
            "installation_id": "installation",
            "promotion_id": "1".repeat(64),
            "activation_request_id": "activation"
        }))
        .unwrap();
        let runtime_generation = RuntimeGeneration::FIRST;
        let controller = ControllerId::parse("controller").unwrap();
        let process = ProcessInstanceId::parse("process").unwrap();
        let mut deployment =
            RuntimeDeployment::request(identity, target.clone(), runtime_generation, None, at(0))
                .unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller.clone(),
                fencing_token: FencingToken::FIRST,
                now: at(1),
                expires_at: at(100),
            })
            .unwrap();
        let guard = |deployment: &RuntimeDeployment, now| CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: controller.clone(),
            fencing_token: FencingToken::FIRST,
            runtime_generation,
            now,
        };
        deployment
            .accept_preflight(
                &guard(&deployment, at(2)),
                PreflightAttestationV1 {
                    target: target.clone(),
                    runtime_generation,
                    observed_runtime: None,
                    checked_at: at(2),
                },
            )
            .unwrap();
        deployment
            .request_drain(&guard(&deployment, at(3)))
            .unwrap();
        deployment
            .accept_drain(
                &guard(&deployment, at(4)),
                DrainAttestationV1 {
                    previous_runtime: None,
                    target_runtime_generation: runtime_generation,
                    drained_at: at(4),
                },
            )
            .unwrap();
        deployment
            .begin_activation(&guard(&deployment, at(5)))
            .unwrap();
        deployment
            .accept_activation(
                &guard(&deployment, at(6)),
                ActivationAttestationV1 {
                    activation_request_id:
                        automation_runtime_convergence::ActivationRequestId::parse("activation")
                            .unwrap(),
                    target: target.clone(),
                    runtime_generation,
                    kind: ActivationOutcomeKindV1::Activated,
                    activated_at: at(6),
                },
            )
            .unwrap();
        deployment
            .begin_panel_reconciliation(&guard(&deployment, at(7)))
            .unwrap();
        deployment
            .accept_panel_certificate(
                &guard(&deployment, at(8)),
                PanelCertificateV1 {
                    certificate_id: PanelCertificateId::parse("panel").unwrap(),
                    report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
                    target: target.clone(),
                    runtime_generation,
                    process_instance_id: process.clone(),
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
                },
            )
            .unwrap();
        deployment
            .certify_live(
                &guard(&deployment, at(10)),
                GatewayReadyAttestationV1 {
                    target,
                    runtime_generation,
                    process_instance_id: process,
                    kind: GatewayReadyKindV1::DiscordReady,
                    ready_at: at(9),
                },
                at(10),
            )
            .unwrap();
        deployment
    }

    fn deployment_row(snapshot: &RuntimeDeploymentSnapshotV1) -> Value {
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
            "runtime_generation": snapshot.runtime_generation,
            "previous_runtime": snapshot.previous_runtime,
            "requested_at": snapshot.requested_at,
            "revision": snapshot.revision,
            "phase": "live",
            "live_attestation_id": "5".repeat(64),
            "live_at": at(10),
            "updated_at": at(10),
            "snapshot": snapshot,
            "convergence_attempt_no": 1,
            "unchanged_marker": "exact"
        })
    }

    fn valid_projection() -> RuntimeStartupRecoveryProgressedProjectionV2 {
        let previous = live_deployment();
        let previous_snapshot = previous.snapshot();
        let live = previous_snapshot.live.as_ref().unwrap();
        let mut terminal = previous.clone();
        terminal
            .recover_live(RecoverLiveRequestV1 {
                expected_revision: previous_snapshot.revision,
                expected_runtime_generation: previous_snapshot.runtime_generation,
                expected_process_instance_id: live.process_instance_id.clone(),
                kind: LiveLossKindV1::ServingLeaseExpired,
                evidence_at: at(11),
                recovered_at: at(12),
            })
            .unwrap();
        let terminal_snapshot = terminal.snapshot();
        let previous_deployment = deployment_row(&previous_snapshot);
        let mut terminal_deployment = previous_deployment.clone();
        terminal_deployment["snapshot"] = serde_json::to_value(&terminal_snapshot).unwrap();
        terminal_deployment["revision"] = serde_json::to_value(terminal_snapshot.revision).unwrap();
        terminal_deployment["phase"] = json!("runtime_pending");
        terminal_deployment["live_attestation_id"] = Value::Null;
        terminal_deployment["live_at"] = Value::Null;
        terminal_deployment["updated_at"] = json!(at(12));
        let previous_slot_fence = json!({
            "slot_guild_id": previous_snapshot.target.guild_id,
            "slot_ruleset_key": previous_snapshot.target.ruleset_key,
            "writer_epoch": 7,
            "pending_drain_intent_id": null,
            "pending_product_operation_id": null,
            "pending_tenant_id": null,
            "pending_installation_id": null,
            "pending_deployment_id": null,
            "pending_expected_revision": null,
            "pending_marked_at": null,
            "updated_at": at(8),
            "unchanged_marker": "exact"
        });
        let mut terminal_slot_fence = previous_slot_fence.clone();
        terminal_slot_fence["writer_epoch"] = json!(8);
        terminal_slot_fence["updated_at"] = json!(at(11));
        let serving_lease = json!({
            "guild_id": previous_snapshot.target.guild_id,
            "ruleset_key": previous_snapshot.target.ruleset_key,
            "tenant_id": previous_snapshot.identity.tenant_id,
            "installation_id": previous_snapshot.identity.installation_id,
            "deployment_id": previous_snapshot.identity.deployment_id,
            "attestation_id": "5".repeat(64),
            "process_instance_id": live.process_instance_id,
            "runtime_generation": previous_snapshot.runtime_generation,
            "target_version": previous_snapshot.target.version,
            "target_content_hash": previous_snapshot.target.content_hash,
            "binding_revision": previous_snapshot.target.binding_revision,
            "binding_fingerprint": previous_snapshot.target.binding_fingerprint,
            "lease_epoch": 9,
            "revision": 10,
            "connected": true,
            "serving": true,
            "acquired_at": at(10),
            "last_heartbeat_at": at(10),
            "expires_at": at(11)
        });
        RuntimeStartupRecoveryProgressedProjectionV2 {
            previous_deployment,
            terminal_deployment,
            previous_slot_fence,
            terminal_slot_fence,
            serving_lease,
            recovery_kind: 2,
            evidence_at: at(11),
            recovered_at: at(12),
        }
    }

    #[test]
    fn progressed_projection_reconstructs_the_exact_domain_and_database_transition() {
        validate_progressed_projection_v2(&valid_projection(), at(13)).unwrap();
    }

    #[test]
    fn progressed_projection_rejects_every_mutated_row_forgery() {
        let mut deployment_identity = valid_projection();
        deployment_identity.terminal_deployment["unchanged_marker"] = json!("forged");
        assert!(validate_progressed_projection_v2(&deployment_identity, at(13)).is_err());

        let mut snapshot = valid_projection();
        snapshot.terminal_deployment["snapshot"]["last_live_recovery"]["recovered_at"] =
            json!(at(13));
        assert!(validate_progressed_projection_v2(&snapshot, at(13)).is_err());

        let mut slot = valid_projection();
        slot.terminal_slot_fence["writer_epoch"] = json!(9);
        assert!(validate_progressed_projection_v2(&slot, at(13)).is_err());

        let mut serving = valid_projection();
        serving.serving_lease["target_content_hash"] = json!("6".repeat(64));
        assert!(validate_progressed_projection_v2(&serving, at(13)).is_err());

        let mut evidence = valid_projection();
        evidence.evidence_at = at(10);
        assert!(validate_progressed_projection_v2(&evidence, at(13)).is_err());
    }

    #[test]
    fn slot_terminal_timestamp_cannot_cross_the_journal_record() {
        let mut projection = valid_projection();
        projection.terminal_slot_fence["updated_at"] = json!(at(14));
        assert!(validate_progressed_projection_v2(&projection, at(13)).is_err());
    }

    #[test]
    fn opaque_snapshot_extensions_must_be_preserved_exactly() {
        let mut preserved = valid_projection();
        preserved.previous_deployment["snapshot"]["opaque_extension"] =
            json!({"padding": "x".repeat(256)});
        preserved.terminal_deployment["snapshot"]["opaque_extension"] =
            preserved.previous_deployment["snapshot"]["opaque_extension"].clone();
        validate_progressed_projection_v2(&preserved, at(13)).unwrap();

        preserved.terminal_deployment["snapshot"]["opaque_extension"]["padding"] = json!("forged");
        assert!(validate_progressed_projection_v2(&preserved, at(13)).is_err());
    }

    #[test]
    fn opaque_snapshot_numeric_extensions_preserve_beyond_f64_precision() {
        let previous_extension: Value =
            serde_json::from_str(r#"{"counter":9007199254740992.0}"#).unwrap();
        let forged_extension: Value =
            serde_json::from_str(r#"{"counter":9007199254740993.0}"#).unwrap();
        assert_ne!(previous_extension, forged_extension);

        let mut projection = valid_projection();
        projection.previous_deployment["snapshot"]["opaque_extension"] = previous_extension;
        projection.terminal_deployment["snapshot"]["opaque_extension"] = forged_extension;
        assert!(validate_progressed_projection_v2(&projection, at(13)).is_err());
    }

    #[test]
    fn maximum_previous_update_timestamp_fails_closed_without_overflow() {
        let mut projection = valid_projection();
        projection.previous_deployment["updated_at"] = json!(DateTime::<Utc>::MAX_UTC);
        projection.terminal_deployment["updated_at"] = json!(DateTime::<Utc>::MAX_UTC);
        assert!(validate_progressed_projection_v2(&projection, at(13)).is_err());
    }

    #[test]
    fn mutation_clock_cannot_cross_the_journal_record() {
        assert!(validate_progressed_projection_v2(&valid_projection(), at(11)).is_err());
    }
}
