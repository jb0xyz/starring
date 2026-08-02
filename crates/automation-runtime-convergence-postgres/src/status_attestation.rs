use std::num::NonZeroU32;

use automation_runtime_controller::{
    RuntimeCanonicalCertificationIntentV2, RuntimeCanonicalLiveAttestationV2,
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationRequestDigestV2,
    RuntimeGatewayReadyKindV2, RuntimeLiveAttestationDigestV2,
};
use automation_runtime_convergence::{
    ControllerId, DeploymentRevision, GatewayReadyKindV1, LiveAttestationV1, RuntimeDeployment,
    RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::evidence::RuntimeDeploymentAttestationEvidenceV2;
use crate::model::{
    AttestationIdV1, GatewayShardIdV1, LiveMetadataV1, PanelReportDigestV1, RuntimeBuildRevisionV1,
};
use crate::row::{metadata, PersistedAttestation, PersistedDeployment};
use crate::RuntimeConvergenceStoreError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusAttestationFormat {
    None,
    LegacyV1,
    CanonicalV2,
}

pub(crate) struct StatusAttestationEvidence {
    pub id: AttestationIdV1,
    pub live: LiveAttestationV1,
    pub deployment_revision: DeploymentRevision,
    pub metadata: LiveMetadataV1,
    pub deployment_id: String,
    pub tenant_id: String,
    pub installation_id: String,
    pub promotion_id: String,
    pub activation_request_id: String,
    pub convergence_attempt: Option<NonZeroU32>,
}

impl StatusAttestationEvidence {
    pub(crate) fn from_legacy(
        persisted: PersistedAttestation,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        let metadata = metadata(&persisted.record)?;
        Ok(Self {
            id: persisted.id,
            live: persisted.record.live,
            deployment_revision: persisted.record.deployment_revision,
            metadata,
            deployment_id: persisted.deployment_id,
            tenant_id: persisted.tenant_id,
            installation_id: persisted.installation_id,
            promotion_id: persisted.promotion_id,
            activation_request_id: persisted.activation_request_id,
            convergence_attempt: persisted.convergence_attempt,
        })
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusAttestationRowV2 {
    attestation_id: String,
    attestation_digest: String,
    deployment_id: String,
    deployment_revision: i64,
    #[serde(default)]
    convergence_attempt_no: Option<i64>,
    tenant_id: String,
    installation_id: String,
    promotion_id: String,
    activation_request_id: String,
    guild_id: String,
    ruleset_key: String,
    target_version: i64,
    target_content_hash: String,
    binding_revision: i64,
    binding_fingerprint: String,
    runtime_generation: i64,
    controller_fencing_token: i64,
    process_instance_id: String,
    runtime_build_revision: String,
    panel_certificate_id: String,
    panel_report_digest: String,
    gateway_shard_id: String,
    gateway_ready_kind: String,
    gateway_ready_at: DateTime<Utc>,
    certified_at: DateTime<Utc>,
    record_format_version: i16,
    record: Value,
    created_at: DateTime<Utc>,
}

pub(crate) fn classify_status_attestation(
    evidence: &RuntimeDeploymentAttestationEvidenceV2,
    projection_present: bool,
) -> Result<StatusAttestationFormat, RuntimeConvergenceStoreError> {
    let v2_roots_absent = evidence.v2_operation_id.is_none()
        && evidence.v2_intent_fingerprint.is_none()
        && evidence.v2_certification_intent_bytes.is_none()
        && evidence.v2_request_digest.is_none()
        && evidence.v2_request_bytes.is_none()
        && evidence.v2_live_attestation_bytes.is_none()
        && evidence.v2_must_commit_before.is_none()
        && evidence.v2_route_admission.is_none()
        && evidence.v2_certified_snapshot.is_none();
    match (
        projection_present,
        evidence.attestation_record_format_version,
        evidence.v2_evidence_state.as_deref(),
    ) {
        (false, None, Some("no_attestation"))
            if evidence.attestation_serving_lease_duration_nanos.is_none()
                && evidence.attestation_convergence_attempt_no.is_none()
                && v2_roots_absent =>
        {
            Ok(StatusAttestationFormat::None)
        }
        (true, Some(1), Some("v1"))
            if positive_i64(evidence.attestation_serving_lease_duration_nanos)
                && positive_i64(evidence.attestation_convergence_attempt_no)
                && v2_roots_absent =>
        {
            Ok(StatusAttestationFormat::LegacyV1)
        }
        (true, Some(2), Some("exact"))
            if positive_i64(evidence.attestation_serving_lease_duration_nanos)
                && positive_i64(evidence.attestation_convergence_attempt_no)
                && evidence.deployment_last_controller_id.is_some()
                && evidence.v2_operation_id.is_some()
                && evidence.v2_intent_fingerprint.is_some()
                && evidence.v2_certification_intent_bytes.is_some()
                && evidence.v2_request_digest.is_some()
                && evidence.v2_request_bytes.is_some()
                && evidence.v2_live_attestation_bytes.is_some()
                && evidence.v2_must_commit_before.is_some()
                && evidence.v2_route_admission.is_some()
                && evidence.v2_certified_snapshot.is_some() =>
        {
            Ok(StatusAttestationFormat::CanonicalV2)
        }
        _ => Err(corrupt("runtime attestation product evidence state")),
    }
}

pub(crate) fn decode_legacy_status_attestation(
    persisted: PersistedAttestation,
    evidence: &RuntimeDeploymentAttestationEvidenceV2,
    deployment: &PersistedDeployment,
) -> Result<StatusAttestationEvidence, RuntimeConvergenceStoreError> {
    validate_projection_context(
        persisted.convergence_attempt,
        persisted.serving_lease_for,
        evidence,
        deployment,
    )?;
    StatusAttestationEvidence::from_legacy(persisted)
}

pub(crate) fn decode_canonical_status_attestation_v2(
    row: StatusAttestationRowV2,
    evidence: RuntimeDeploymentAttestationEvidenceV2,
    deployment: &PersistedDeployment,
) -> Result<StatusAttestationEvidence, RuntimeConvergenceStoreError> {
    let RuntimeDeploymentAttestationEvidenceV2 {
        attestation_record_format_version,
        attestation_serving_lease_duration_nanos,
        attestation_convergence_attempt_no,
        deployment_last_controller_id,
        v2_evidence_state,
        v2_operation_id,
        v2_intent_fingerprint,
        v2_certification_intent_bytes,
        v2_request_digest,
        v2_request_bytes,
        v2_live_attestation_bytes,
        v2_must_commit_before,
        v2_route_admission,
        v2_certified_snapshot,
    } = evidence;
    if attestation_record_format_version != Some(2)
        || v2_evidence_state.as_deref() != Some("exact")
        || row.record_format_version != 2
    {
        return Err(corrupt("runtime attestation V2 format evidence"));
    }
    let convergence_attempt = positive_attempt(required(
        attestation_convergence_attempt_no,
        "runtime attestation V2 convergence attempt",
    )?)?;
    let serving_lease_duration_nanos = required(
        attestation_serving_lease_duration_nanos,
        "runtime attestation V2 serving lease duration",
    )?;
    let deployment_last_controller_id = ControllerId::parse(required(
        deployment_last_controller_id,
        "runtime attestation V2 controller identity",
    )?)
    .map_err(|_| corrupt("runtime attestation V2 controller identity"))?;
    let operation_id = required(v2_operation_id, "runtime attestation V2 operation identity")?;
    let fingerprint = RuntimeCertificationIntentFingerprintV2::parse(required(
        v2_intent_fingerprint,
        "runtime attestation V2 intent fingerprint",
    )?)
    .map_err(|_| corrupt("runtime attestation V2 intent fingerprint"))?;
    let intent_bytes = required(
        v2_certification_intent_bytes,
        "runtime attestation V2 intent bytes",
    )?;
    let canonical_intent =
        RuntimeCanonicalCertificationIntentV2::from_persisted(&intent_bytes, &fingerprint)
            .map_err(|_| corrupt("runtime attestation V2 canonical intent"))?;
    let request_digest = RuntimeCertificationRequestDigestV2::parse(required(
        v2_request_digest,
        "runtime attestation V2 request digest",
    )?)
    .map_err(|_| corrupt("runtime attestation V2 request digest"))?;
    let request_bytes = required(v2_request_bytes, "runtime attestation V2 request bytes")?;
    let live_digest = RuntimeLiveAttestationDigestV2::parse(row.attestation_id.clone())
        .map_err(|_| corrupt("runtime attestation V2 digest"))?;
    let live_bytes = required(
        v2_live_attestation_bytes,
        "runtime attestation V2 live bytes",
    )?;
    let canonical = RuntimeCanonicalLiveAttestationV2::from_persisted(
        &canonical_intent,
        &request_bytes,
        &request_digest,
        &live_bytes,
        &live_digest,
    )
    .map_err(|_| corrupt("runtime attestation V2 canonical record"))?;
    let must_commit_before = required(
        v2_must_commit_before,
        "runtime attestation V2 commit deadline",
    )?;
    let route_admission = required(v2_route_admission, "runtime attestation V2 route admission")?;
    let certified_snapshot_value = required(
        v2_certified_snapshot,
        "runtime attestation V2 certified snapshot",
    )?;
    let certified_snapshot =
        serde_json::from_value::<RuntimeDeploymentSnapshotV1>(certified_snapshot_value)
            .map_err(|_| corrupt("runtime attestation V2 certified snapshot"))?;
    RuntimeDeployment::restore(certified_snapshot.clone())
        .map_err(|_| corrupt("runtime attestation V2 certified snapshot"))?;
    let request_value = serde_json::from_slice::<Value>(&request_bytes)
        .map_err(|_| corrupt("runtime attestation V2 request JSON"))?;
    let canonical_route_admission = request_value
        .as_object()
        .and_then(|request| request.get("route_admission"))
        .ok_or_else(|| corrupt("runtime attestation V2 route admission"))?;
    let live_value = serde_json::from_slice::<Value>(&live_bytes)
        .map_err(|_| corrupt("runtime attestation V2 record JSON"))?;
    let request = canonical.request();
    let intent = &request.intent;
    let snapshot = deployment.deployment.snapshot();
    let live = certified_snapshot
        .live
        .as_ref()
        .ok_or_else(|| corrupt("runtime attestation V2 live snapshot"))?;
    let projected_attempt = row
        .convergence_attempt_no
        .map(positive_attempt)
        .transpose()?;
    let serving_nanos = i64::try_from(intent.serving_lease_for.as_nanos()).ok();
    let valid = row.attestation_id == row.attestation_digest
        && row.attestation_id == canonical.live_attestation_digest().as_str()
        && operation_id == intent.operation_id.as_str()
        && &fingerprint == canonical.intent_fingerprint()
        && &request_digest == canonical.request_digest()
        && request.must_commit_before == must_commit_before
        && row.certified_at <= must_commit_before
        && &route_admission == canonical_route_admission
        && row.record == live_value
        && certified_snapshot == snapshot
        && matches!(&certified_snapshot.phase, RuntimeDeploymentPhaseV1::Live)
        && intent.guard.convergence_attempt == convergence_attempt
        && projected_attempt.is_none_or(|attempt| attempt == convergence_attempt)
        && deployment
            .convergence_attempt
            .is_none_or(|attempt| attempt.started() == Some(convergence_attempt))
        && deployment.last_controller_id.as_ref() == Some(&deployment_last_controller_id)
        && intent.guard.controller_id == deployment_last_controller_id
        && certified_snapshot.last_fencing_token == Some(intent.guard.fencing_token)
        && intent.guard.expected_revision.get().checked_add(1)
            == Some(certified_snapshot.revision.get())
        && intent.guard.scope.tenant_id == certified_snapshot.identity.tenant_id
        && intent.guard.scope.installation_id == certified_snapshot.identity.installation_id
        && intent.guard.scope.deployment_id == certified_snapshot.identity.deployment_id
        && intent.binding_pin.installation_authority_revision.get()
            == deployment.installation_authority_revision
        && serving_nanos == Some(serving_lease_duration_nanos)
        && row.deployment_id == certified_snapshot.identity.deployment_id.as_str()
        && row.tenant_id == certified_snapshot.identity.tenant_id.as_str()
        && row.installation_id == certified_snapshot.identity.installation_id.as_str()
        && row.promotion_id == certified_snapshot.identity.promotion_id.as_str()
        && row.activation_request_id == certified_snapshot.identity.activation_request_id.as_str()
        && row.deployment_revision == persisted_i64(certified_snapshot.revision.get())
        && row.guild_id == intent.target.guild_id.to_string()
        && row.ruleset_key == intent.target.ruleset_key.as_str()
        && row.target_version == i64::from(intent.target.version.get())
        && row.target_content_hash == intent.target.content_hash.to_hex()
        && row.binding_revision == persisted_i64(intent.target.binding_revision.get())
        && row.binding_fingerprint == intent.target.binding_fingerprint.as_str()
        && row.runtime_generation == persisted_i64(intent.guard.runtime_generation.get())
        && row.controller_fencing_token == persisted_i64(intent.guard.fencing_token.get())
        && row.process_instance_id == intent.process_identity.process_instance_id.as_str()
        && row.runtime_build_revision == intent.runtime_build_revision.as_str()
        && row.panel_certificate_id == intent.panel.certificate_id.as_str()
        && row.panel_report_digest == intent.panel.report_digest.as_str()
        && row.gateway_shard_id == intent.gateway_owner_lease_id.gateway_shard_id.as_str()
        && row.gateway_ready_kind == "discord_resumed"
        && matches!(
            request.route_admission.gateway.kind,
            RuntimeGatewayReadyKindV2::Resumed
        )
        && row.gateway_ready_at == row.certified_at
        && row.created_at == row.certified_at
        && live.target == intent.target
        && live.runtime_generation == intent.guard.runtime_generation
        && live.process_instance_id == intent.process_identity.process_instance_id
        && live.activation.activation_request_id
            == certified_snapshot.identity.activation_request_id
        && live.activation.target == intent.target
        && live.activation.runtime_generation == intent.guard.runtime_generation
        && live.panel_certificate.certificate_id == intent.panel.certificate_id
        && live.panel_certificate.report_digest == intent.panel.report_digest
        && live.panel_certificate.target == intent.target
        && live.panel_certificate.runtime_generation == intent.guard.runtime_generation
        && live.panel_certificate.process_instance_id
            == intent.process_identity.process_instance_id
        && matches!(live.gateway_ready.kind, GatewayReadyKindV1::DiscordResumed)
        && live.gateway_ready.target == intent.target
        && live.gateway_ready.runtime_generation == intent.guard.runtime_generation
        && live.gateway_ready.process_instance_id == intent.process_identity.process_instance_id
        && live.gateway_ready.ready_at <= row.gateway_ready_at
        && live.certified_at == row.certified_at;
    if !valid {
        return Err(corrupt("runtime attestation V2 projections"));
    }
    let id = AttestationIdV1::parse(row.attestation_id)
        .map_err(|_| corrupt("runtime attestation V2 identity"))?;
    let metadata = LiveMetadataV1 {
        runtime_build_revision: RuntimeBuildRevisionV1::parse(
            intent.runtime_build_revision.as_str().to_string(),
        )
        .map_err(|_| corrupt("runtime attestation V2 build revision"))?,
        panel_report_digest: PanelReportDigestV1::parse(
            intent.panel.report_digest.as_str().to_string(),
        )
        .map_err(|_| corrupt("runtime attestation V2 panel report digest"))?,
        gateway_shard_id: GatewayShardIdV1::parse(
            intent
                .gateway_owner_lease_id
                .gateway_shard_id
                .as_str()
                .to_string(),
        )
        .map_err(|_| corrupt("runtime attestation V2 gateway shard"))?,
    };
    Ok(StatusAttestationEvidence {
        id,
        live: live.clone(),
        deployment_revision: certified_snapshot.revision,
        metadata,
        deployment_id: row.deployment_id,
        tenant_id: row.tenant_id,
        installation_id: row.installation_id,
        promotion_id: row.promotion_id,
        activation_request_id: row.activation_request_id,
        convergence_attempt: Some(convergence_attempt),
    })
}

fn validate_projection_context(
    projected_attempt: Option<NonZeroU32>,
    projected_lease: Option<std::time::Duration>,
    evidence: &RuntimeDeploymentAttestationEvidenceV2,
    deployment: &PersistedDeployment,
) -> Result<(), RuntimeConvergenceStoreError> {
    let raw_attempt = positive_attempt(required(
        evidence.attestation_convergence_attempt_no,
        "runtime attestation convergence attempt evidence",
    )?)?;
    let lease_nanos = required(
        evidence.attestation_serving_lease_duration_nanos,
        "runtime attestation serving lease duration evidence",
    )?;
    let controller = evidence
        .deployment_last_controller_id
        .clone()
        .map(ControllerId::parse)
        .transpose()
        .map_err(|_| corrupt("runtime deployment controller evidence"))?;
    if projected_attempt.is_some_and(|attempt| attempt != raw_attempt)
        || deployment
            .convergence_attempt
            .and_then(|attempt| attempt.started())
            .is_some_and(|attempt| attempt != raw_attempt)
        || projected_lease
            .is_some_and(|lease| i64::try_from(lease.as_nanos()).ok() != Some(lease_nanos))
        || controller.as_ref() != deployment.last_controller_id.as_ref()
    {
        return Err(corrupt("runtime attestation projection context"));
    }
    Ok(())
}

fn positive_i64(value: Option<i64>) -> bool {
    value.is_some_and(|value| value > 0)
}

fn positive_attempt(value: i64) -> Result<NonZeroU32, RuntimeConvergenceStoreError> {
    u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| corrupt("runtime attestation convergence attempt"))
}

fn persisted_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MIN)
}

fn required<T>(value: Option<T>, error: &'static str) -> Result<T, RuntimeConvergenceStoreError> {
    value.ok_or_else(|| corrupt(error))
}

fn corrupt(error: &'static str) -> RuntimeConvergenceStoreError {
    RuntimeConvergenceStoreError::InvalidPersistedState(error)
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};
    use std::time::Duration;

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_controller::{
        GatewayShardIdV1 as ControllerGatewayShardIdV1, RuntimeBarrierIdV1,
        RuntimeBarrierPauseWitnessV2, RuntimeBindingPinV1,
        RuntimeBuildRevisionV1 as ControllerBuildRevisionV1, RuntimeCertificationOperationIdV2,
        RuntimeCertificationRequestV2, RuntimeCertificationReservationInputV2,
        RuntimeConvergenceSessionV1, RuntimeExecutionReceiptV1, RuntimeGatewayAdmissionSequenceV2,
        RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayReadyAttestationV2,
        RuntimeLiveAttestationRecordV2, RuntimePanelEvidenceV2, RuntimeRouteAdmissionAttestationV2,
        RuntimeServingRouteAttestationV2,
    };
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
        CommandGuardV1, DeploymentId, DrainAttestationV1, FencingToken, GatewayReadyAttestationV1,
        InstallationId, LeaseRequestV1, PanelCertificateId, PanelCertificateV1,
        PanelReportDigestV1 as ConvergencePanelReportDigestV1, PreflightAttestationV1,
        ProcessInstanceId, PromotionId, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::*;
    use crate::model::{RuntimeConvergenceAttemptV1, RuntimeDigestV1};

    struct Fixture {
        row: StatusAttestationRowV2,
        sidecar: RuntimeDeploymentAttestationEvidenceV2,
        deployment: PersistedDeployment,
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + second, 0).unwrap()
    }

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(42),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        }
    }

    fn identity() -> RuntimeDeploymentIdentityV1 {
        RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse("deployment:1").unwrap(),
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            promotion_id: PromotionId::parse("c".repeat(64)).unwrap(),
            activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
        }
    }

    fn guard(
        deployment: &RuntimeDeployment,
        controller_id: &ControllerId,
        fencing_token: FencingToken,
        second: i64,
    ) -> CommandGuardV1 {
        CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
            now: at(second),
        }
    }

    fn fixture() -> Fixture {
        let target = target();
        let identity = identity();
        let process_instance_id = ProcessInstanceId::parse("process:1").unwrap();
        let runtime_generation = RuntimeGeneration::new(2).unwrap();
        let controller_id = ControllerId::parse("controller:1").unwrap();
        let fencing_token = FencingToken::new(3).unwrap();
        let previous = RuntimeProcessIdentityV1 {
            target: RuntimeDeploymentTargetV1 {
                guild_id: GuildId(42),
                ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
                version: RuleSetVersionId::FIRST,
                content_hash: RuleSetContentHash::parse_hex(&"e".repeat(64)).unwrap(),
                binding_revision: BindingRevision::new(2).unwrap(),
                binding_fingerprint: ResourceBindingFingerprint::parse(&"f".repeat(64)).unwrap(),
            },
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: ProcessInstanceId::parse("process:old").unwrap(),
        };
        let mut deployment = RuntimeDeployment::request(
            identity.clone(),
            target.clone(),
            runtime_generation,
            Some(previous.clone()),
            at(0),
        )
        .unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller_id.clone(),
                fencing_token,
                now: at(1),
                expires_at: at(3_600),
            })
            .unwrap();
        deployment
            .accept_preflight(
                &guard(&deployment, &controller_id, fencing_token, 10),
                PreflightAttestationV1 {
                    target: target.clone(),
                    runtime_generation,
                    observed_runtime: Some(previous.clone()),
                    checked_at: at(10),
                },
            )
            .unwrap();
        deployment
            .request_drain(&guard(&deployment, &controller_id, fencing_token, 11))
            .unwrap();
        deployment
            .accept_drain(
                &guard(&deployment, &controller_id, fencing_token, 20),
                DrainAttestationV1 {
                    previous_runtime: Some(previous),
                    target_runtime_generation: runtime_generation,
                    drained_at: at(20),
                },
            )
            .unwrap();
        deployment
            .begin_activation(&guard(&deployment, &controller_id, fencing_token, 21))
            .unwrap();
        deployment
            .accept_activation(
                &guard(&deployment, &controller_id, fencing_token, 30),
                ActivationAttestationV1 {
                    activation_request_id: identity.activation_request_id.clone(),
                    target: target.clone(),
                    runtime_generation,
                    kind: ActivationOutcomeKindV1::Activated,
                    activated_at: at(30),
                },
            )
            .unwrap();
        deployment
            .begin_panel_reconciliation(&guard(&deployment, &controller_id, fencing_token, 31))
            .unwrap();
        let panel_report_digest = ConvergencePanelReportDigestV1::parse("4".repeat(64)).unwrap();
        let panel_certificate_id = PanelCertificateId::parse("panel:1").unwrap();
        deployment
            .accept_panel_certificate(
                &guard(&deployment, &controller_id, fencing_token, 40),
                PanelCertificateV1 {
                    certificate_id: panel_certificate_id.clone(),
                    report_digest: panel_report_digest.clone(),
                    target: target.clone(),
                    runtime_generation,
                    process_instance_id: process_instance_id.clone(),
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
                    reconciled_at: at(40),
                },
            )
            .unwrap();
        let process_identity = RuntimeProcessIdentityV1 {
            target: target.clone(),
            runtime_generation,
            process_instance_id: process_instance_id.clone(),
        };
        let build_revision = ControllerBuildRevisionV1::parse("build:1").unwrap();
        let owner_lease_id = RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: ControllerGatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: process_instance_id.clone(),
            lease_epoch: non_zero(5),
            expected_build_revision: build_revision.clone(),
        };
        let mut session = RuntimeConvergenceSessionV1::from_claim(RuntimeExecutionReceiptV1 {
            snapshot: deployment.snapshot(),
            controller_id: controller_id.clone(),
            fencing_token,
            convergence_attempt: NonZeroU32::new(5).unwrap(),
            acquired_at: at(1),
            expires_at: at(3_600),
        })
        .unwrap();
        let reservation = session
            .begin_certification_reservation_v2(RuntimeCertificationReservationInputV2 {
                operation_id: RuntimeCertificationOperationIdV2::parse(
                    "00112233445566778899aabbccddeeff",
                )
                .unwrap(),
                binding_pin: RuntimeBindingPinV1 {
                    tenant_id: identity.tenant_id.clone(),
                    installation_id: identity.installation_id.clone(),
                    installation_authority_revision: non_zero(6),
                    binding_revision: target.binding_revision,
                    binding_fingerprint: target.binding_fingerprint.clone(),
                },
                gateway_owner_lease_id: owner_lease_id.clone(),
                observed_owner_revision: non_zero(7),
                runtime_build_revision: build_revision,
                panel: RuntimePanelEvidenceV2 {
                    certificate_id: panel_certificate_id,
                    report_digest: panel_report_digest,
                    process_identity: process_identity.clone(),
                    controller_fencing_token: fencing_token,
                },
                serving_lease_for: Duration::from_secs(30),
            })
            .unwrap();
        let canonical_intent = reservation.canonical_intent().clone();
        let request = RuntimeCertificationRequestV2 {
            intent: canonical_intent.intent().clone(),
            intent_fingerprint: canonical_intent.intent_fingerprint().clone(),
            must_commit_before: at(60),
            route_admission: RuntimeRouteAdmissionAttestationV2 {
                barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
                pause: RuntimeBarrierPauseWitnessV2 {
                    coordinator_generation: non_zero(8),
                    connection_epoch: non_zero(9),
                    paused_admission_revision: non_zero(10),
                    pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
                },
                gateway: RuntimeGatewayReadyAttestationV2 {
                    process_instance_id: process_instance_id.clone(),
                    connection_epoch: non_zero(9),
                    kind: RuntimeGatewayReadyKindV2::Resumed,
                    admission_revision: non_zero(10),
                    connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
                    resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
                },
                gateway_owner_lease_id: owner_lease_id,
                attested_owner_revision: non_zero(7),
                route: RuntimeServingRouteAttestationV2 {
                    identity: process_identity,
                    controller_fencing_token: fencing_token,
                    route_incarnation: non_zero(14),
                    activation_sequence: non_zero(15),
                },
            },
        };
        let live_record = RuntimeLiveAttestationRecordV2::from_request(request).unwrap();
        let canonical = canonical_intent.bind_live_record(live_record).unwrap();
        deployment
            .certify_live(
                &guard(&deployment, &controller_id, fencing_token, 50),
                GatewayReadyAttestationV1 {
                    target: target.clone(),
                    runtime_generation,
                    process_instance_id: process_instance_id.clone(),
                    kind: GatewayReadyKindV1::DiscordResumed,
                    ready_at: at(50),
                },
                at(51),
            )
            .unwrap();
        let snapshot = deployment.snapshot();
        let attestation_id = canonical.live_attestation_digest().as_str().to_string();
        let request_value =
            serde_json::from_slice::<Value>(canonical.certification_request_bytes()).unwrap();
        let record =
            serde_json::from_slice::<Value>(canonical.live_attestation_record_bytes()).unwrap();
        let row = StatusAttestationRowV2 {
            attestation_id: attestation_id.clone(),
            attestation_digest: attestation_id.clone(),
            deployment_id: identity.deployment_id.as_str().to_string(),
            deployment_revision: i64::try_from(snapshot.revision.get()).unwrap(),
            convergence_attempt_no: None,
            tenant_id: identity.tenant_id.as_str().to_string(),
            installation_id: identity.installation_id.as_str().to_string(),
            promotion_id: identity.promotion_id.as_str().to_string(),
            activation_request_id: identity.activation_request_id.as_str().to_string(),
            guild_id: target.guild_id.to_string(),
            ruleset_key: target.ruleset_key.as_str().to_string(),
            target_version: i64::from(target.version.get()),
            target_content_hash: target.content_hash.to_hex(),
            binding_revision: i64::try_from(target.binding_revision.get()).unwrap(),
            binding_fingerprint: target.binding_fingerprint.as_str().to_string(),
            runtime_generation: i64::try_from(runtime_generation.get()).unwrap(),
            controller_fencing_token: i64::try_from(fencing_token.get()).unwrap(),
            process_instance_id: process_instance_id.as_str().to_string(),
            runtime_build_revision: "build:1".to_string(),
            panel_certificate_id: "panel:1".to_string(),
            panel_report_digest: "4".repeat(64),
            gateway_shard_id: "shard:0".to_string(),
            gateway_ready_kind: "discord_resumed".to_string(),
            gateway_ready_at: at(51),
            certified_at: at(51),
            record_format_version: 2,
            record,
            created_at: at(51),
        };
        let sidecar = RuntimeDeploymentAttestationEvidenceV2 {
            attestation_record_format_version: Some(2),
            attestation_serving_lease_duration_nanos: Some(30_000_000_000),
            attestation_convergence_attempt_no: Some(5),
            deployment_last_controller_id: Some(controller_id.as_str().to_string()),
            v2_evidence_state: Some("exact".to_string()),
            v2_operation_id: Some(canonical.request().intent.operation_id.as_str().to_string()),
            v2_intent_fingerprint: Some(canonical.intent_fingerprint().as_str().to_string()),
            v2_certification_intent_bytes: Some(canonical.certification_intent_bytes().to_vec()),
            v2_request_digest: Some(canonical.request_digest().as_str().to_string()),
            v2_request_bytes: Some(canonical.certification_request_bytes().to_vec()),
            v2_live_attestation_bytes: Some(canonical.live_attestation_record_bytes().to_vec()),
            v2_must_commit_before: Some(canonical.request().must_commit_before),
            v2_route_admission: Some(request_value["route_admission"].clone()),
            v2_certified_snapshot: Some(serde_json::to_value(&snapshot).unwrap()),
        };
        let persisted = PersistedDeployment {
            deployment,
            installation_authority_revision: 6,
            desired_target_digest: RuntimeDigestV1::parse("d".repeat(64)).unwrap(),
            live_attestation_id: Some(AttestationIdV1::parse(attestation_id).unwrap()),
            last_controller_id: Some(controller_id),
            convergence_attempt: Some(RuntimeConvergenceAttemptV1::new(5)),
            last_failure_attempt: None,
        };
        Fixture {
            row,
            sidecar,
            deployment: persisted,
        }
    }

    #[test]
    fn product_attestation_sidecar_state_matrix_is_closed() {
        let no_attestation = RuntimeDeploymentAttestationEvidenceV2 {
            v2_evidence_state: Some("no_attestation".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            classify_status_attestation(&no_attestation, false),
            Ok(StatusAttestationFormat::None)
        ));

        let legacy = RuntimeDeploymentAttestationEvidenceV2 {
            attestation_record_format_version: Some(1),
            attestation_serving_lease_duration_nanos: Some(1),
            attestation_convergence_attempt_no: Some(1),
            v2_evidence_state: Some("v1".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            classify_status_attestation(&legacy, true),
            Ok(StatusAttestationFormat::LegacyV1)
        ));

        let canonical = fixture().sidecar;
        assert!(matches!(
            classify_status_attestation(&canonical, true),
            Ok(StatusAttestationFormat::CanonicalV2)
        ));

        for invalid in [
            RuntimeDeploymentAttestationEvidenceV2::default(),
            RuntimeDeploymentAttestationEvidenceV2 {
                v2_evidence_state: Some("invalid".to_string()),
                ..Default::default()
            },
            RuntimeDeploymentAttestationEvidenceV2 {
                v2_evidence_state: Some("no_attestation".to_string()),
                v2_request_digest: Some("d".repeat(64)),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                classify_status_attestation(&invalid, false),
                Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime attestation product evidence state"
                ))
            ));
        }
    }

    #[test]
    fn legacy_projection_accepts_absent_controller_history_on_both_sides() {
        let mut fixture = fixture();
        fixture.deployment.last_controller_id = None;
        let evidence = RuntimeDeploymentAttestationEvidenceV2 {
            attestation_record_format_version: Some(1),
            attestation_serving_lease_duration_nanos: Some(30_000_000_000),
            attestation_convergence_attempt_no: Some(5),
            deployment_last_controller_id: None,
            v2_evidence_state: Some("v1".to_string()),
            ..Default::default()
        };

        validate_projection_context(
            NonZeroU32::new(5),
            Some(std::time::Duration::from_secs(30)),
            &evidence,
            &fixture.deployment,
        )
        .unwrap();
    }

    #[test]
    fn legacy_projection_rejects_controller_history_option_mismatch() {
        let present_deployment_fixture = fixture();
        let absent = RuntimeDeploymentAttestationEvidenceV2 {
            attestation_record_format_version: Some(1),
            attestation_serving_lease_duration_nanos: Some(30_000_000_000),
            attestation_convergence_attempt_no: Some(5),
            deployment_last_controller_id: None,
            v2_evidence_state: Some("v1".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            validate_projection_context(
                NonZeroU32::new(5),
                Some(std::time::Duration::from_secs(30)),
                &absent,
                &present_deployment_fixture.deployment,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime attestation projection context"
            ))
        ));

        let mut missing_deployment_fixture = fixture();
        missing_deployment_fixture.deployment.last_controller_id = None;
        let present = RuntimeDeploymentAttestationEvidenceV2 {
            deployment_last_controller_id: Some("controller:1".to_string()),
            ..absent
        };
        assert!(matches!(
            validate_projection_context(
                NonZeroU32::new(5),
                Some(std::time::Duration::from_secs(30)),
                &present,
                &missing_deployment_fixture.deployment,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime attestation projection context"
            ))
        ));
    }

    #[test]
    fn canonical_v2_product_evidence_decodes_to_format_neutral_status() {
        let fixture = fixture();
        let attestation = decode_canonical_status_attestation_v2(
            fixture.row,
            fixture.sidecar,
            &fixture.deployment,
        )
        .unwrap();

        assert_eq!(
            attestation.id,
            fixture.deployment.live_attestation_id.unwrap()
        );
        assert_eq!(attestation.convergence_attempt, NonZeroU32::new(5));
        assert_eq!(
            attestation.metadata.runtime_build_revision.as_str(),
            "build:1"
        );
        assert_eq!(attestation.metadata.gateway_shard_id.as_str(), "shard:0");
    }

    #[test]
    fn canonical_v2_product_evidence_rejects_tampered_roots() {
        let mut route_fixture = fixture();
        route_fixture.sidecar.v2_route_admission = Some(serde_json::json!({"tampered": true}));
        assert!(matches!(
            decode_canonical_status_attestation_v2(
                route_fixture.row,
                route_fixture.sidecar,
                &route_fixture.deployment,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime attestation V2 projections"
            ))
        ));

        let mut controller_fixture = fixture();
        controller_fixture.deployment.last_controller_id = None;
        assert!(matches!(
            decode_canonical_status_attestation_v2(
                controller_fixture.row,
                controller_fixture.sidecar,
                &controller_fixture.deployment,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime attestation V2 projections"
            ))
        ));

        let mut attempt_fixture = fixture();
        attempt_fixture.row.convergence_attempt_no = Some(6);
        assert!(matches!(
            decode_canonical_status_attestation_v2(
                attempt_fixture.row,
                attempt_fixture.sidecar,
                &attempt_fixture.deployment,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime attestation V2 projections"
            ))
        ));

        let mut bytes_fixture = fixture();
        bytes_fixture
            .sidecar
            .v2_request_bytes
            .as_mut()
            .unwrap()
            .push(b' ');
        assert!(matches!(
            decode_canonical_status_attestation_v2(
                bytes_fixture.row,
                bytes_fixture.sidecar,
                &bytes_fixture.deployment,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime attestation V2 canonical record"
            ))
        ));
    }
}
