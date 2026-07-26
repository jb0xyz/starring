use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeConvergenceSessionV1,
    RuntimeExecutionReceiptV1, RuntimeLiveMetadataV1,
};
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

fn certification_request() -> RuntimeCertificationRequestV1 {
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
    let panel_report_digest = PanelReportDigestV1::parse("4".repeat(64)).unwrap();
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
                activation_request_id: automation_runtime_convergence::ActivationRequestId::parse(
                    "activation",
                )
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
                report_digest: panel_report_digest.clone(),
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
    let snapshot = deployment.snapshot();
    let acquired_at = snapshot.controller_lease.as_ref().unwrap().acquired_at;
    let expires_at = snapshot.controller_lease.as_ref().unwrap().expires_at;
    let mut session = RuntimeConvergenceSessionV1::from_claim(RuntimeExecutionReceiptV1 {
        snapshot,
        controller_id: controller,
        fencing_token: FencingToken::FIRST,
        convergence_attempt: NonZeroU32::MIN,
        acquired_at,
        expires_at,
    })
    .unwrap();
    session
        .begin_certification(
            GatewayReadyAttestationV1 {
                target,
                runtime_generation,
                process_instance_id: process,
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: at(9),
            },
            RuntimeLiveMetadataV1 {
                runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
                panel_report_digest,
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            },
            Duration::from_secs(45),
        )
        .unwrap()
}

fn prepare_row(
    request: &RuntimeCertificationRequestV1,
    preparation: RuntimeCertificationPreparationV1,
) -> RuntimeCertificationPrepareRowV1 {
    let observed_snapshot = match preparation {
        RuntimeCertificationPreparationV1::Apply => request_snapshot(request),
        RuntimeCertificationPreparationV1::Replayed => certified_snapshot(request),
    };
    RuntimeCertificationPrepareRowV1 {
        preparation_name: Some(
            match preparation {
                RuntimeCertificationPreparationV1::Apply => "apply",
                RuntimeCertificationPreparationV1::Replayed => "replayed",
            }
            .to_string(),
        ),
        observed_snapshot: Some(Json(serde_json::to_value(observed_snapshot).unwrap())),
        convergence_attempt_no: Some(1),
        mutation_clock: Some(match preparation {
            RuntimeCertificationPreparationV1::Apply => at(10),
            RuntimeCertificationPreparationV1::Replayed => at(11),
        }),
        certified_at: Some(at(10)),
    }
}

fn certified_deployment(request: &RuntimeCertificationRequestV1) -> RuntimeDeployment {
    let mut deployment = RuntimeDeployment::restore(request_snapshot(request)).unwrap();
    let guard = CommandGuardV1 {
        expected_revision: request.guard.expected_revision,
        controller_id: request.guard.controller_id.clone(),
        fencing_token: request.guard.fencing_token,
        runtime_generation: request.guard.runtime_generation,
        now: at(10),
    };
    deployment
        .certify_live(&guard, request.gateway_ready.clone(), at(10))
        .unwrap();
    deployment
}

fn request_snapshot(request: &RuntimeCertificationRequestV1) -> RuntimeDeploymentSnapshotV1 {
    let target = request.gateway_ready.target.clone();
    let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(json!({
        "deployment_id": request.guard.scope.deployment_id.as_str(),
        "tenant_id": request.guard.scope.tenant_id.as_str(),
        "installation_id": request.guard.scope.installation_id.as_str(),
        "promotion_id": "1".repeat(64),
        "activation_request_id": "activation"
    }))
    .unwrap();
    let mut deployment = RuntimeDeployment::request(
        identity,
        target.clone(),
        request.guard.runtime_generation,
        None,
        at(0),
    )
    .unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: request.guard.controller_id.clone(),
            fencing_token: request.guard.fencing_token,
            now: at(1),
            expires_at: at(100),
        })
        .unwrap();
    let command = |deployment: &RuntimeDeployment, now| CommandGuardV1 {
        expected_revision: deployment.revision(),
        controller_id: request.guard.controller_id.clone(),
        fencing_token: request.guard.fencing_token,
        runtime_generation: request.guard.runtime_generation,
        now,
    };
    deployment
        .accept_preflight(
            &command(&deployment, at(2)),
            PreflightAttestationV1 {
                target: target.clone(),
                runtime_generation: request.guard.runtime_generation,
                observed_runtime: None,
                checked_at: at(2),
            },
        )
        .unwrap();
    deployment
        .request_drain(&command(&deployment, at(3)))
        .unwrap();
    deployment
        .accept_drain(
            &command(&deployment, at(4)),
            DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: request.guard.runtime_generation,
                drained_at: at(4),
            },
        )
        .unwrap();
    deployment
        .begin_activation(&command(&deployment, at(5)))
        .unwrap();
    deployment
        .accept_activation(
            &command(&deployment, at(6)),
            ActivationAttestationV1 {
                activation_request_id: automation_runtime_convergence::ActivationRequestId::parse(
                    "activation",
                )
                .unwrap(),
                target: target.clone(),
                runtime_generation: request.guard.runtime_generation,
                kind: ActivationOutcomeKindV1::Activated,
                activated_at: at(6),
            },
        )
        .unwrap();
    deployment
        .begin_panel_reconciliation(&command(&deployment, at(7)))
        .unwrap();
    deployment
        .accept_panel_certificate(
            &command(&deployment, at(8)),
            PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("panel").unwrap(),
                report_digest: request.metadata.panel_report_digest.clone(),
                target,
                runtime_generation: request.guard.runtime_generation,
                process_instance_id: request.gateway_ready.process_instance_id.clone(),
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
    let snapshot = deployment.snapshot();
    assert_eq!(snapshot.revision, request.guard.expected_revision);
    snapshot
}

fn certified_snapshot(request: &RuntimeCertificationRequestV1) -> RuntimeDeploymentSnapshotV1 {
    certified_deployment(request).snapshot()
}

fn plan(
    request: &RuntimeCertificationRequestV1,
    preparation: RuntimeCertificationPreparationV1,
) -> RuntimeCertificationPlanV1 {
    RuntimeCertificationPlanV1::prove(request, prepare_row(request, preparation).decode().unwrap())
        .unwrap()
}

fn operation_row(
    request: &RuntimeCertificationRequestV1,
    plan: &RuntimeCertificationPlanV1,
) -> RuntimeCertificationOperationRowV1 {
    let previous = match plan.preparation {
        RuntimeCertificationPreparationV1::Apply => plan.observed_snapshot.clone(),
        RuntimeCertificationPreparationV1::Replayed => plan.expected_snapshot.clone(),
    };
    RuntimeCertificationOperationRowV1 {
        outcome_name: Some(
            match plan.preparation {
                RuntimeCertificationPreparationV1::Apply => "applied",
                RuntimeCertificationPreparationV1::Replayed => "replayed",
            }
            .to_string(),
        ),
        previous_snapshot: Some(Json(serde_json::to_value(previous).unwrap())),
        snapshot: Some(Json(serde_json::to_value(&plan.expected_snapshot).unwrap())),
        convergence_attempt_no: Some(1),
        tenant_id: Some(request.guard.scope.tenant_id.as_str().to_string()),
        installation_id: Some(request.guard.scope.installation_id.as_str().to_string()),
        deployment_id: Some(request.guard.scope.deployment_id.as_str().to_string()),
        guild_id: Some(request.gateway_ready.target.guild_id.to_string()),
        ruleset_key: Some(
            request
                .gateway_ready
                .target
                .ruleset_key
                .as_str()
                .to_string(),
        ),
        attestation_id: Some(plan.attestation_id.as_str().to_string()),
        process_instance_id: Some(
            request
                .gateway_ready
                .process_instance_id
                .as_str()
                .to_string(),
        ),
        runtime_generation: Some(request.guard.runtime_generation.get() as i64),
        lease_epoch: Some(1),
        serving_revision: Some(1),
        acquired_at: Some(at(10)),
        last_heartbeat_at: Some(at(10)),
        expires_at: Some(at(55)),
        connected: Some(true),
        serving: Some(true),
    }
}

#[test]
fn certification_queries_are_function_only_and_positionally_exact() {
    assert_eq!(CERTIFY_PREPARE_QUERY.matches('$').count(), 13);
    assert_eq!(CERTIFY_COMMIT_QUERY.matches('$').count(), 18);
    for query in [CERTIFY_PREPARE_QUERY, CERTIFY_COMMIT_QUERY] {
        for forbidden in [
            "runtime_deployments",
            "runtime_attestations",
            "runtime_serving_leases",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
        ] {
            assert!(!query.contains(forbidden));
        }
    }
}

#[test]
fn serving_lease_duration_is_millisecond_exact_and_closed() {
    for valid in [
        Duration::from_secs(1),
        Duration::from_millis(1_001),
        Duration::from_secs(300),
    ] {
        assert_eq!(
            validate_serving_lease_duration(valid).unwrap(),
            i64::try_from(valid.as_millis()).unwrap()
        );
    }
    for invalid in [
        Duration::ZERO,
        Duration::from_millis(999),
        Duration::from_nanos(1_000_000_001),
        Duration::from_millis(300_001),
    ] {
        assert_eq!(
            validate_serving_lease_duration(invalid),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
    }
}

#[test]
fn certification_proof_accepts_exact_applied_and_replayed_receipts() {
    let request = certification_request();
    for preparation in [
        RuntimeCertificationPreparationV1::Apply,
        RuntimeCertificationPreparationV1::Replayed,
    ] {
        let plan = plan(&request, preparation);
        let row = operation_row(&request, &plan).decode().unwrap();
        let receipt = plan.prove_commit(&request, row).unwrap();
        assert_eq!(receipt.snapshot, certified_snapshot(&request));
        assert_eq!(receipt.serving.acquired_at, at(10));
        assert_eq!(receipt.serving.last_heartbeat_at, at(10));
        assert_eq!(receipt.serving.expires_at, at(55));
    }
}

#[test]
fn certification_proof_rejects_projection_identity_and_lease_forgery() {
    let request = certification_request();
    let reject = |mutate: fn(&mut RuntimeCertificationOperationRowV1)| {
        let plan = plan(&request, RuntimeCertificationPreparationV1::Apply);
        let mut row = operation_row(&request, &plan);
        mutate(&mut row);
        let result = row
            .decode()
            .and_then(|row| plan.prove_commit(&request, row));
        assert_eq!(
            result.err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    };
    reject(|row| row.outcome_name = Some("replayed".to_string()));
    reject(|row| row.convergence_attempt_no = Some(2));
    reject(|row| row.previous_snapshot.as_mut().unwrap().0["requested_at"] = json!(at(1)));
    reject(|row| row.snapshot.as_mut().unwrap().0["live"]["certified_at"] = json!(at(11)));
    reject(|row| row.tenant_id = Some("other".to_string()));
    reject(|row| row.guild_id = Some("43".to_string()));
    reject(|row| row.attestation_id = Some("5".repeat(64)));
    reject(|row| row.process_instance_id = Some("other".to_string()));
    reject(|row| row.runtime_generation = Some(2));
    reject(|row| row.lease_epoch = Some(0));
    reject(|row| row.serving_revision = Some(0));
    reject(|row| row.acquired_at = Some(at(9)));
    reject(|row| row.last_heartbeat_at = Some(at(11)));
    reject(|row| row.expires_at = Some(at(56)));
    reject(|row| row.connected = Some(false));
    reject(|row| row.serving = Some(false));
}

#[test]
fn certification_prepare_proof_rejects_snapshot_attempt_and_clock_forgery() {
    let request = certification_request();
    let mut wrong_attempt = prepare_row(&request, RuntimeCertificationPreparationV1::Apply);
    wrong_attempt.convergence_attempt_no = Some(2);
    assert_eq!(
        RuntimeCertificationPlanV1::prove(&request, wrong_attempt.decode().unwrap()).err(),
        Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
    );
    let mut wrong_clock = prepare_row(&request, RuntimeCertificationPreparationV1::Apply);
    wrong_clock.certified_at = Some(at(11));
    assert_eq!(
        wrong_clock.decode().err(),
        Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
    );
    let mut wrong_snapshot = prepare_row(&request, RuntimeCertificationPreparationV1::Apply);
    wrong_snapshot.observed_snapshot.as_mut().unwrap().0["runtime_generation"] = json!(2);
    assert_eq!(
        wrong_snapshot
            .decode()
            .and_then(|prepared| RuntimeCertificationPlanV1::prove(&request, prepared))
            .err(),
        Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
    );
    for mutate in [
        |value: &mut Value| value["identity"]["tenant_id"] = json!("other"),
        |value: &mut Value| value["target"]["version"] = json!(2),
        |value: &mut Value| value["last_fencing_token"] = json!(2),
    ] {
        let mut forged = prepare_row(&request, RuntimeCertificationPreparationV1::Apply);
        mutate(&mut forged.observed_snapshot.as_mut().unwrap().0);
        assert_eq!(
            forged
                .decode()
                .and_then(|prepared| RuntimeCertificationPlanV1::prove(&request, prepared))
                .err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }
}
