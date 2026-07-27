use std::num::{NonZeroU32, NonZeroU64};

use automation_runtime_controller::{
    RuntimeCanonicalRouteMutationProvenanceV2, RuntimeCanonicalSuspendAttemptDrainProgressV2,
    RuntimeCanonicalSuspendedAttemptV2, RuntimeDeploymentScopeV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeLocalRouteEffectV2, RuntimePersistedSuspendAttemptRootV2, RuntimeSuspendAttemptDigestV2,
    RuntimeSuspendedAttemptObservationKindV2, RuntimeSuspendedAttemptObservationV2,
    RuntimeSuspensionIdV2,
};
use automation_runtime_convergence::{
    DeploymentId, DeploymentRevision, InstallationId, RuntimeDeployment,
    RuntimeDeploymentSnapshotV1, TenantId,
};
use chrono::{DateTime, Utc};

use super::closed_evidence::{
    closed_recovery_provenance_v2, validate_closed_recovery_evidence_v2,
    RuntimeClosedRecoveryExpectedEvidenceV2, RuntimeClosedRecoveryProvenanceExpectationV2,
};
use super::suspended_projection::{
    RuntimeSuspendedStartupRecoveryProgressedProjectionV2, RuntimeSuspendedStartupRecoveryRootV2,
    RuntimeSuspendedStartupRecoverySidecarV2,
};
use crate::RuntimeExecutionPersistenceErrorV1;

pub(super) struct RuntimeSuspendedStartupRecoveryExpectationV2<'a> {
    pub recovery_id: &'a str,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub selection_authority_revision: i64,
    pub gateway_owner_lease_id: &'a RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: i64,
    pub owner_expires_at: DateTime<Utc>,
    pub evidence: &'a RuntimeClosedRecoveryExpectedEvidenceV2,
}

pub(super) fn validate_suspended_progressed_projection_v2(
    projection: &RuntimeSuspendedStartupRecoveryProgressedProjectionV2,
    expected: &RuntimeSuspendedStartupRecoveryExpectationV2<'_>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if expected
        .selection_authority_revision
        .checked_add(1)
        .is_none_or(|revision| revision != expected.action_authority_revision)
    {
        return Err(invalid());
    }
    validate_closed_recovery_evidence_v2(&projection.evidence, expected.evidence)?;
    let root = decode_root(&projection.root)?;
    let source = decode_source(&projection.source, &projection.root, &root)?;
    validate_deployment_evidence(&projection.root, &root, &source)?;
    let expected_provenance = expected_provenance(expected)?;
    let projected_provenance =
        RuntimeCanonicalRouteMutationProvenanceV2::from_persisted(&projection.provenance_bytes)
            .map_err(|_| invalid())?;
    if projected_provenance != expected_provenance {
        return Err(invalid());
    }
    let progress = RuntimeCanonicalSuspendAttemptDrainProgressV2::record_local_absent(
        source,
        expected_provenance,
        positive_non_zero(expected.evidence.registry_observation_sequence)?,
    )
    .map_err(|_| invalid())?;
    validate_successor(&projection.successor, &projection.root, &root, &progress)
}

fn validate_deployment_evidence(
    projection: &RuntimeSuspendedStartupRecoveryRootV2,
    root: &RuntimePersistedSuspendAttemptRootV2,
    source: &RuntimeCanonicalSuspendedAttemptV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let snapshot = serde_json::from_slice::<RuntimeDeploymentSnapshotV1>(
        &projection.deployment_snapshot_bytes,
    )
    .map_err(|_| invalid())?;
    RuntimeDeployment::restore(snapshot.clone()).map_err(|_| invalid())?;
    if positive_u32(projection.deployment_convergence_attempt)?
        != root.operation_scope().convergence_attempt()
        || projection.deployment_last_controller_id.as_deref()
            != Some(
                source
                    .suspended_attempt()
                    .source_guard()
                    .controller_id
                    .as_str(),
            )
        || positive_u64(
            projection
                .deployment_last_fencing_token
                .ok_or_else(invalid)?,
        )? != source
            .suspended_attempt()
            .source_guard()
            .fencing_token
            .get()
    {
        return Err(invalid());
    }
    let observation =
        RuntimeSuspendedAttemptObservationV2::new(snapshot, source.suspended_attempt().clone())
            .map_err(|_| invalid())?;
    if observation.kind() != RuntimeSuspendedAttemptObservationKindV2::LocalRoutePresent {
        return Err(invalid());
    }
    Ok(())
}

fn decode_root(
    projection: &RuntimeSuspendedStartupRecoveryRootV2,
) -> Result<RuntimePersistedSuspendAttemptRootV2, RuntimeExecutionPersistenceErrorV1> {
    let scope = RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(&projection.tenant_id).map_err(|_| invalid())?,
        installation_id: InstallationId::parse(&projection.installation_id)
            .map_err(|_| invalid())?,
        deployment_id: DeploymentId::parse(&projection.deployment_id).map_err(|_| invalid())?,
    };
    let deployment_revision =
        DeploymentRevision::new(positive_u64(projection.deployment_revision)?)
            .map_err(|_| invalid())?;
    let convergence_attempt = positive_u32(projection.convergence_attempt)?;
    let suspension_id =
        RuntimeSuspensionIdV2::parse(&projection.suspension_id).map_err(|_| invalid())?;
    let request_digest = RuntimeSuspendAttemptDigestV2::parse(lower_hex(projection.request_digest))
        .map_err(|_| invalid())?;
    RuntimePersistedSuspendAttemptRootV2::from_persisted(
        scope,
        deployment_revision,
        convergence_attempt,
        &suspension_id,
        &projection.request_bytes,
        &request_digest,
    )
    .map_err(|_| invalid())
}

fn decode_source(
    source_projection: &RuntimeSuspendedStartupRecoverySidecarV2,
    root_projection: &RuntimeSuspendedStartupRecoveryRootV2,
    root: &RuntimePersistedSuspendAttemptRootV2,
) -> Result<RuntimeCanonicalSuspendedAttemptV2, RuntimeExecutionPersistenceErrorV1> {
    require_same_root(source_projection, root_projection)?;
    let source = RuntimeCanonicalSuspendedAttemptV2::from_persisted(
        root,
        positive_non_zero(source_projection.sidecar_revision)?,
        &source_projection.local_effect_kind,
        &source_projection.local_effect_bytes,
        &source_projection.drain_obligation_kind,
        &source_projection.drain_obligation_bytes,
        source_projection.suspended_at,
    )
    .map_err(|_| invalid())?;
    let route = match source.suspended_attempt().local_effect() {
        RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route,
        RuntimeLocalRouteEffectV2::None | RuntimeLocalRouteEffectV2::RouteAbsent { .. } => {
            return Err(invalid());
        }
    };
    let slot = route.slot();
    if source_projection.slot_guild_id != slot.guild_id.0.to_string()
        || source_projection.slot_ruleset_key != slot.ruleset_key.as_str()
    {
        return Err(invalid());
    }
    Ok(source)
}

fn validate_successor(
    successor: &RuntimeSuspendedStartupRecoverySidecarV2,
    root_projection: &RuntimeSuspendedStartupRecoveryRootV2,
    root: &RuntimePersistedSuspendAttemptRootV2,
    progress: &RuntimeCanonicalSuspendAttemptDrainProgressV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    require_same_root(successor, root_projection)?;
    let source = &progress.source().suspended_attempt();
    let expected_revision = source
        .sidecar_revision()
        .get()
        .checked_add(1)
        .ok_or_else(invalid)?;
    let (expected_guild_id, expected_ruleset_key) = exact_route_slot(source)?;
    if positive_u64(successor.sidecar_revision)? != expected_revision
        || successor.slot_guild_id != expected_guild_id
        || successor.slot_ruleset_key != expected_ruleset_key
        || successor.suspended_at != source.suspended_at()
        || successor.local_effect_kind != progress.replacement_local_effect_kind()
        || successor.local_effect_bytes.as_ref() != progress.replacement_local_effect_bytes()
        || successor.drain_obligation_kind != progress.replacement_drain_obligation_kind()
        || successor.drain_obligation_bytes.as_ref()
            != progress.replacement_drain_obligation_bytes()
    {
        return Err(invalid());
    }
    let decoded = RuntimeCanonicalSuspendedAttemptV2::from_persisted(
        root,
        NonZeroU64::new(expected_revision).ok_or_else(invalid)?,
        &successor.local_effect_kind,
        &successor.local_effect_bytes,
        &successor.drain_obligation_kind,
        &successor.drain_obligation_bytes,
        successor.suspended_at,
    )
    .map_err(|_| invalid())?;
    if decoded.suspended_attempt().local_effect() != progress.progress().replacement_local_effect()
        || decoded.suspended_attempt().drain_obligation()
            != progress.progress().replacement_drain_obligation()
    {
        return Err(invalid());
    }
    Ok(())
}

fn exact_route_slot(
    suspended: &automation_runtime_controller::RuntimeSuspendedAttemptV2,
) -> Result<(String, String), RuntimeExecutionPersistenceErrorV1> {
    let route = match suspended.local_effect() {
        RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route,
        RuntimeLocalRouteEffectV2::None | RuntimeLocalRouteEffectV2::RouteAbsent { .. } => {
            return Err(invalid());
        }
    };
    let slot = route.slot();
    Ok((
        slot.guild_id.0.to_string(),
        slot.ruleset_key.as_str().to_owned(),
    ))
}

fn require_same_root(
    sidecar: &RuntimeSuspendedStartupRecoverySidecarV2,
    root: &RuntimeSuspendedStartupRecoveryRootV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if sidecar.suspension_id != root.suspension_id || sidecar.request_digest != root.request_digest
    {
        Err(invalid())
    } else {
        Ok(())
    }
}

fn expected_provenance(
    expected: &RuntimeSuspendedStartupRecoveryExpectationV2<'_>,
) -> Result<RuntimeCanonicalRouteMutationProvenanceV2, RuntimeExecutionPersistenceErrorV1> {
    closed_recovery_provenance_v2(&RuntimeClosedRecoveryProvenanceExpectationV2 {
        recovery_id: expected.recovery_id,
        originating_emergency_generation: expected.originating_emergency_generation,
        coordinator_generation: expected.coordinator_generation,
        action_authority_revision: expected.action_authority_revision,
        gateway_owner_lease_id: expected.gateway_owner_lease_id,
        owner_revision: expected.owner_revision,
        owner_expires_at: expected.owner_expires_at,
        evidence: expected.evidence,
    })
}

fn positive_non_zero(value: i64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    NonZeroU64::new(positive_u64(value)?).ok_or_else(invalid)
}

fn positive_u64(value: i64) -> Result<u64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(invalid)
}

fn positive_u32(value: i64) -> Result<NonZeroU32, RuntimeExecutionPersistenceErrorV1> {
    u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(invalid)
}

fn lower_hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use crate::startup_recovery_execution::closed_evidence::RuntimeClosedRecoveryEvidenceV2;
    use automation_runtime_controller::{
        GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCanonicalSuspendedAttemptV2,
    };
    use automation_runtime_convergence::{ProcessInstanceId, RuntimeDeploymentSnapshotV1};
    use serde_json::json;
    use sha2::{Digest, Sha256};

    use super::*;

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn exact_route_json() -> &'static str {
        concat!(
            "{\"identity\":{\"target\":{\"guild_id\":\"9223372036854775808\",",
            "\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",",
            "\"binding_revision\":3,",
            "\"binding_fingerprint\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},",
            "\"runtime_generation\":9,\"process_instance_id\":\"process:current\"},",
            "\"controller_fencing_token\":8,\"route_incarnation\":10}"
        )
    }

    fn request_bytes() -> Box<[u8]> {
        [
            "{\"format_version\":2,",
            "\"suspension_id\":\"00112233445566778899aabbccddeeff\",",
            "\"action_id\":10,",
            "\"guard\":{\"scope\":{\"tenant_id\":\"tenant:1\",",
            "\"installation_id\":\"installation:1\",\"deployment_id\":\"deployment:1\"},",
            "\"expected_revision\":7,\"controller_id\":\"controller:1\",",
            "\"fencing_token\":8,\"runtime_generation\":9,\"convergence_attempt\":2},",
            "\"source_phase\":\"requested\",",
            "\"failure\":{\"failure_id\":\"failure:1\",\"kind\":\"environment_unavailable\",",
            "\"code\":\"dependency_unavailable\",\"message\":\"dependency unavailable\",",
            "\"recorded_at_unix_microseconds\":20000000},",
            "\"disposition\":{\"kind\":\"retryable\",",
            "\"retry_not_before_unix_microseconds\":40000000},",
            "\"checkpoint\":\"verify_preflight\",",
            "\"local_effect\":{\"kind\":\"exact_route\",\"route\":",
            exact_route_json(),
            ",\"lifecycle\":\"draining\"},",
            "\"drain_obligation\":{\"kind\":\"exact_local_route\",\"route\":",
            exact_route_json(),
            "}}",
        ]
        .concat()
        .into_bytes()
        .into_boxed_slice()
    }

    fn source_local_effect_bytes() -> Box<[u8]> {
        [
            "{\"kind\":\"exact_route\",\"route\":",
            exact_route_json(),
            ",\"lifecycle\":\"draining\"}",
        ]
        .concat()
        .into_bytes()
        .into_boxed_slice()
    }

    fn source_drain_obligation_bytes() -> Box<[u8]> {
        [
            "{\"kind\":\"exact_local_route\",\"route\":",
            exact_route_json(),
            "}",
        ]
        .concat()
        .into_bytes()
        .into_boxed_slice()
    }

    fn request_digest(request: &[u8]) -> [u8; 32] {
        let domain = b"starring.runtime.suspend_attempt.v2\0";
        let mut digest = Sha256::new();
        digest.update((domain.len() as u64).to_be_bytes());
        digest.update(domain);
        digest.update((request.len() as u64).to_be_bytes());
        digest.update(request);
        digest.finalize().into()
    }

    fn deployment_snapshot_bytes() -> Box<[u8]> {
        let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(json!({
            "identity": {
                "deployment_id": "deployment:1",
                "tenant_id": "tenant:1",
                "installation_id": "installation:1",
                "promotion_id": "9".repeat(64),
                "activation_request_id": "activation:1"
            },
            "target": {
                "guild_id": "9223372036854775808",
                "ruleset_key": "studyroom",
                "version": 1,
                "content_hash": "b".repeat(64),
                "binding_revision": 3,
                "binding_fingerprint": "a".repeat(64)
            },
            "runtime_generation": 9,
            "previous_runtime": null,
            "requested_at": at(1),
            "revision": 7,
            "phase": {
                "phase": "requested"
            },
            "controller_lease": {
                "controller_id": "controller:1",
                "fencing_token": 8,
                "acquired_at": at(2),
                "expires_at": at(100)
            },
            "last_fencing_token": 8,
            "preflight": null,
            "drain": null,
            "activation": null,
            "panel_certificate": null,
            "gateway_ready": null,
            "live": null,
            "last_live_recovery": null,
            "last_runtime_failure": null
        }))
        .unwrap();
        RuntimeDeployment::restore(snapshot.clone()).unwrap();
        serde_json::to_vec(&snapshot).unwrap().into_boxed_slice()
    }

    fn owner() -> RuntimeGatewayOwnerLeaseIdV1 {
        RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:gateway").unwrap(),
            lease_epoch: non_zero(12),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        }
    }

    fn evidence() -> RuntimeClosedRecoveryExpectedEvidenceV2 {
        RuntimeClosedRecoveryExpectedEvidenceV2 {
            paused_process_instance_id: "process:gateway".to_owned(),
            paused_coordinator_generation: 2,
            paused_connection_epoch: 13,
            paused_ready_kind: "ready",
            paused_admission_revision: 14,
            paused_transition_sequence: 17,
            paused_connected_event_sequence: 15,
            paused_last_resume_sequence: Some(16),
            registry_process_instance_id: "process:gateway".to_owned(),
            registry_observation_sequence: 18,
            registry_retained_slot_count: 2,
            registry_retained_empty_tombstone_count: 2,
        }
    }

    fn projected_evidence() -> RuntimeClosedRecoveryEvidenceV2 {
        let evidence = evidence();
        RuntimeClosedRecoveryEvidenceV2 {
            paused_process_instance_id: evidence.paused_process_instance_id,
            paused_coordinator_generation: evidence.paused_coordinator_generation,
            paused_connection_epoch: evidence.paused_connection_epoch,
            paused_ready_kind: evidence.paused_ready_kind,
            paused_admission_revision: evidence.paused_admission_revision,
            paused_transition_sequence: evidence.paused_transition_sequence,
            paused_connected_event_sequence: evidence.paused_connected_event_sequence,
            paused_last_resume_sequence: evidence.paused_last_resume_sequence,
            registry_process_instance_id: evidence.registry_process_instance_id,
            registry_observation_sequence: evidence.registry_observation_sequence,
            registry_retained_slot_count: evidence.registry_retained_slot_count,
            registry_retained_empty_tombstone_count: evidence
                .registry_retained_empty_tombstone_count,
        }
    }

    fn expectation<'a>(
        owner: &'a RuntimeGatewayOwnerLeaseIdV1,
        evidence: &'a RuntimeClosedRecoveryExpectedEvidenceV2,
    ) -> RuntimeSuspendedStartupRecoveryExpectationV2<'a> {
        RuntimeSuspendedStartupRecoveryExpectationV2 {
            recovery_id: "ffeeddccbbaa99887766554433221100",
            originating_emergency_generation: 2,
            coordinator_generation: 3,
            action_authority_revision: 5,
            selection_authority_revision: 4,
            gateway_owner_lease_id: owner,
            owner_revision: 19,
            owner_expires_at: at(200),
            evidence,
        }
    }

    fn projection() -> RuntimeSuspendedStartupRecoveryProgressedProjectionV2 {
        let request = request_bytes();
        let digest = request_digest(&request);
        let root_projection = RuntimeSuspendedStartupRecoveryRootV2 {
            suspension_id: "00112233445566778899aabbccddeeff".to_owned(),
            tenant_id: "tenant:1".to_owned(),
            installation_id: "installation:1".to_owned(),
            deployment_id: "deployment:1".to_owned(),
            deployment_revision: 7,
            convergence_attempt: 2,
            request_digest: digest,
            request_bytes: request,
            deployment_convergence_attempt: 2,
            deployment_last_controller_id: Some("controller:1".to_owned()),
            deployment_last_fencing_token: Some(8),
            deployment_snapshot_format_version: 1,
            deployment_snapshot_bytes: deployment_snapshot_bytes(),
        };
        let root = decode_root(&root_projection).unwrap();
        let source = RuntimeCanonicalSuspendedAttemptV2::from_persisted(
            &root,
            non_zero(1),
            "exact_route",
            &source_local_effect_bytes(),
            "exact_local_route",
            &source_drain_obligation_bytes(),
            at(4),
        )
        .unwrap();
        let owner = owner();
        let evidence = evidence();
        let expected = expectation(&owner, &evidence);
        let provenance = expected_provenance(&expected).unwrap();
        let progress = RuntimeCanonicalSuspendAttemptDrainProgressV2::record_local_absent(
            source.clone(),
            provenance,
            non_zero(18),
        )
        .unwrap();
        RuntimeSuspendedStartupRecoveryProgressedProjectionV2 {
            root: root_projection,
            source: RuntimeSuspendedStartupRecoverySidecarV2 {
                suspension_id: "00112233445566778899aabbccddeeff".to_owned(),
                request_digest: digest,
                sidecar_revision: 1,
                slot_guild_id: "9223372036854775808".to_owned(),
                slot_ruleset_key: "studyroom".to_owned(),
                local_effect_kind: source.local_effect_kind().to_owned(),
                local_effect_bytes: source.local_effect_bytes().to_vec().into_boxed_slice(),
                drain_obligation_kind: source.drain_obligation_kind().to_owned(),
                drain_obligation_bytes: source.drain_obligation_bytes().to_vec().into_boxed_slice(),
                suspended_at: at(4),
            },
            successor: RuntimeSuspendedStartupRecoverySidecarV2 {
                suspension_id: "00112233445566778899aabbccddeeff".to_owned(),
                request_digest: digest,
                sidecar_revision: 2,
                slot_guild_id: "9223372036854775808".to_owned(),
                slot_ruleset_key: "studyroom".to_owned(),
                local_effect_kind: progress.replacement_local_effect_kind().to_owned(),
                local_effect_bytes: progress
                    .replacement_local_effect_bytes()
                    .to_vec()
                    .into_boxed_slice(),
                drain_obligation_kind: progress.replacement_drain_obligation_kind().to_owned(),
                drain_obligation_bytes: progress
                    .replacement_drain_obligation_bytes()
                    .to_vec()
                    .into_boxed_slice(),
                suspended_at: at(4),
            },
            provenance_bytes: progress
                .provenance()
                .provenance_bytes()
                .to_vec()
                .into_boxed_slice(),
            evidence: projected_evidence(),
        }
    }

    fn validate(
        projection: &RuntimeSuspendedStartupRecoveryProgressedProjectionV2,
    ) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        let owner = owner();
        let evidence = evidence();
        validate_suspended_progressed_projection_v2(projection, &expectation(&owner, &evidence))
    }

    #[test]
    fn exact_closed_recovery_projection_reconstructs_the_domain_transition() {
        validate(&projection()).unwrap();
    }

    #[test]
    fn source_and_successor_slots_are_checked_against_the_root_route() {
        for mutate in [
            |projection: &mut RuntimeSuspendedStartupRecoveryProgressedProjectionV2| {
                projection.source.slot_guild_id = "8".to_owned();
            },
            |projection: &mut RuntimeSuspendedStartupRecoveryProgressedProjectionV2| {
                projection.source.slot_ruleset_key = "other".to_owned();
            },
            |projection: &mut RuntimeSuspendedStartupRecoveryProgressedProjectionV2| {
                projection.successor.slot_guild_id = "8".to_owned();
            },
            |projection: &mut RuntimeSuspendedStartupRecoveryProgressedProjectionV2| {
                projection.successor.slot_ruleset_key = "other".to_owned();
            },
        ] {
            let mut corrupted = projection();
            mutate(&mut corrupted);
            assert!(validate(&corrupted).is_err());
        }
    }

    #[test]
    fn source_successor_provenance_and_evidence_drift_fail_closed() {
        let mut source = projection();
        source.source.sidecar_revision = 2;
        assert!(validate(&source).is_err());

        let mut successor = projection();
        successor.successor.local_effect_bytes[10] ^= 1;
        assert!(validate(&successor).is_err());

        let mut provenance = projection();
        provenance.provenance_bytes[10] ^= 1;
        assert!(validate(&provenance).is_err());

        let mut evidence = projection();
        evidence.evidence.registry_observation_sequence += 1;
        assert!(validate(&evidence).is_err());
    }

    #[test]
    fn deployment_attempt_controller_fence_and_snapshot_drift_fail_closed() {
        let mut attempt = projection();
        attempt.root.deployment_convergence_attempt += 1;
        assert!(validate(&attempt).is_err());

        let mut controller = projection();
        controller.root.deployment_last_controller_id = Some("controller:other".to_owned());
        assert!(validate(&controller).is_err());

        let mut fence = projection();
        fence.root.deployment_last_fencing_token = Some(9);
        assert!(validate(&fence).is_err());

        let mut snapshot = projection();
        let mut value: serde_json::Value =
            serde_json::from_slice(&snapshot.root.deployment_snapshot_bytes).unwrap();
        value["runtime_generation"] = json!(10);
        snapshot.root.deployment_snapshot_bytes =
            serde_json::to_vec(&value).unwrap().into_boxed_slice();
        assert!(validate(&snapshot).is_err());
    }
}
