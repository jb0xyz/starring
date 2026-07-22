use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
    PanelCertificateId, PanelReportDigestV1, ProcessInstanceId, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde::{Deserialize, Serialize};

use super::{
    validate_intent, validate_request, RuntimeCertificationCanonicalErrorV2,
    RuntimeCertificationCanonicalFieldV2, RuntimeCertificationCanonicalRootV2,
    RuntimeCertificationRequestCorrelationV2, RuntimeLiveAttestationRecordV2,
    CERTIFICATION_INTENT_MAX_OCTETS, CERTIFICATION_REQUEST_MAX_OCTETS, LIVE_ATTESTATION_MAX_OCTETS,
};
use crate::v2_canonical_value::{RuntimeDiscordSnowflakeV2, RuntimePersistenceU64V2};
use crate::v2_digest::certification_request_digest_v2;
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBindingPinV1,
    RuntimeBuildRevisionV1, RuntimeCanonicalValueErrorV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationIntentV2, RuntimeCertificationOperationIdV2,
    RuntimeCertificationRequestDigestV2, RuntimeCertificationRequestV2, RuntimeDeploymentScopeV1,
    RuntimeExecutionGuardV1, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2, RuntimePanelEvidenceV2,
    RuntimeRouteAdmissionAttestationV2, RuntimeServingLeaseMillisecondsV2,
    RuntimeServingRouteAttestationV2, RuntimeSessionActionIdV1, RuntimeUnixMicrosecondsV2,
};

const FORMAT_VERSION: u8 = 2;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CertificationIntentWireV2 {
    format_version: u8,
    action_id: u64,
    operation_id: String,
    guard: ExecutionGuardWireV2,
    target: DeploymentTargetWireV2,
    binding_pin: BindingPinWireV2,
    process_identity: ProcessIdentityWireV2,
    gateway_owner_lease_id: GatewayOwnerLeaseIdWireV2,
    observed_owner_revision: u64,
    runtime_build_revision: String,
    panel: PanelEvidenceWireV2,
    serving_lease_milliseconds: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionGuardWireV2 {
    scope: DeploymentScopeWireV2,
    expected_revision: u64,
    controller_id: String,
    fencing_token: u64,
    runtime_generation: u64,
    convergence_attempt: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentScopeWireV2 {
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentTargetWireV2 {
    guild_id: String,
    ruleset_key: String,
    version: u32,
    content_hash: String,
    binding_revision: u64,
    binding_fingerprint: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingPinWireV2 {
    tenant_id: String,
    installation_id: String,
    installation_authority_revision: u64,
    binding_revision: u64,
    binding_fingerprint: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessIdentityWireV2 {
    target: DeploymentTargetWireV2,
    runtime_generation: u64,
    process_instance_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GatewayOwnerLeaseIdWireV2 {
    gateway_shard_id: String,
    process_instance_id: String,
    lease_epoch: u64,
    expected_build_revision: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelEvidenceWireV2 {
    certificate_id: String,
    report_digest: String,
    process_identity: ProcessIdentityWireV2,
    controller_fencing_token: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CertificationRequestWireV2 {
    format_version: u8,
    intent: CertificationIntentWireV2,
    intent_fingerprint: String,
    must_commit_before_unix_microseconds: i64,
    route_admission: RouteAdmissionWireV2,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteAdmissionWireV2 {
    barrier_id: String,
    pause: BarrierPauseWireV2,
    gateway: GatewayReadyWireV2,
    gateway_owner_lease_id: GatewayOwnerLeaseIdWireV2,
    attested_owner_revision: u64,
    route: ServingRouteWireV2,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BarrierPauseWireV2 {
    coordinator_generation: u64,
    connection_epoch: u64,
    paused_admission_revision: u64,
    pause_sequence: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GatewayReadyWireV2 {
    process_instance_id: String,
    connection_epoch: u64,
    kind: String,
    admission_revision: u64,
    connected_event_sequence: u64,
    resume_sequence: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServingRouteWireV2 {
    identity: ProcessIdentityWireV2,
    controller_fencing_token: u64,
    route_incarnation: u64,
    activation_sequence: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveAttestationRecordWireV2 {
    format_version: u8,
    request_digest: String,
    request: CertificationRequestWireV2,
}

#[derive(Clone, Copy)]
struct TargetFields {
    guild_id: RuntimeCertificationCanonicalFieldV2,
    ruleset_key: RuntimeCertificationCanonicalFieldV2,
    version: RuntimeCertificationCanonicalFieldV2,
    content_hash: RuntimeCertificationCanonicalFieldV2,
    binding_revision: RuntimeCertificationCanonicalFieldV2,
    binding_fingerprint: RuntimeCertificationCanonicalFieldV2,
}

const TARGET_FIELDS: TargetFields = TargetFields {
    guild_id: RuntimeCertificationCanonicalFieldV2::TargetGuildId,
    ruleset_key: RuntimeCertificationCanonicalFieldV2::TargetRuleSetKey,
    version: RuntimeCertificationCanonicalFieldV2::TargetVersion,
    content_hash: RuntimeCertificationCanonicalFieldV2::TargetContentHash,
    binding_revision: RuntimeCertificationCanonicalFieldV2::TargetBindingRevision,
    binding_fingerprint: RuntimeCertificationCanonicalFieldV2::TargetBindingFingerprint,
};

const PROCESS_TARGET_FIELDS: TargetFields = TargetFields {
    guild_id: RuntimeCertificationCanonicalFieldV2::ProcessTargetGuildId,
    ruleset_key: RuntimeCertificationCanonicalFieldV2::ProcessTargetRuleSetKey,
    version: RuntimeCertificationCanonicalFieldV2::ProcessTargetVersion,
    content_hash: RuntimeCertificationCanonicalFieldV2::ProcessTargetContentHash,
    binding_revision: RuntimeCertificationCanonicalFieldV2::ProcessTargetBindingRevision,
    binding_fingerprint: RuntimeCertificationCanonicalFieldV2::ProcessTargetBindingFingerprint,
};

const PANEL_PROCESS_TARGET_FIELDS: TargetFields = TargetFields {
    guild_id: RuntimeCertificationCanonicalFieldV2::PanelProcessTargetGuildId,
    ruleset_key: RuntimeCertificationCanonicalFieldV2::PanelProcessTargetRuleSetKey,
    version: RuntimeCertificationCanonicalFieldV2::PanelProcessTargetVersion,
    content_hash: RuntimeCertificationCanonicalFieldV2::PanelProcessTargetContentHash,
    binding_revision: RuntimeCertificationCanonicalFieldV2::PanelProcessTargetBindingRevision,
    binding_fingerprint: RuntimeCertificationCanonicalFieldV2::PanelProcessTargetBindingFingerprint,
};

const ROUTE_TARGET_FIELDS: TargetFields = TargetFields {
    guild_id: RuntimeCertificationCanonicalFieldV2::RouteTargetGuildId,
    ruleset_key: RuntimeCertificationCanonicalFieldV2::RouteTargetRuleSetKey,
    version: RuntimeCertificationCanonicalFieldV2::RouteTargetVersion,
    content_hash: RuntimeCertificationCanonicalFieldV2::RouteTargetContentHash,
    binding_revision: RuntimeCertificationCanonicalFieldV2::RouteTargetBindingRevision,
    binding_fingerprint: RuntimeCertificationCanonicalFieldV2::RouteTargetBindingFingerprint,
};

pub(super) fn encode_certification_intent(
    intent: &RuntimeCertificationIntentV2,
) -> Result<Vec<u8>, RuntimeCertificationCanonicalErrorV2> {
    validate_intent(intent)?;
    let root = RuntimeCertificationCanonicalRootV2::Intent;
    let lease = RuntimeServingLeaseMillisecondsV2::from_duration(intent.serving_lease_for)
        .map_err(|reason| {
            canonical(
                RuntimeCertificationCanonicalFieldV2::ServingLeaseMilliseconds,
                reason,
            )
        })?;
    let wire = CertificationIntentWireV2 {
        format_version: FORMAT_VERSION,
        action_id: persistence_u64(
            intent.action_id.get(),
            RuntimeCertificationCanonicalFieldV2::ActionId,
        )?,
        operation_id: intent.operation_id.as_str().to_owned(),
        guard: encode_guard(&intent.guard)?,
        target: encode_target(&intent.target, TARGET_FIELDS)?,
        binding_pin: encode_binding_pin(&intent.binding_pin)?,
        process_identity: encode_process_identity(
            &intent.process_identity,
            PROCESS_TARGET_FIELDS,
            RuntimeCertificationCanonicalFieldV2::ProcessRuntimeGeneration,
        )?,
        gateway_owner_lease_id: encode_owner_lease(&intent.gateway_owner_lease_id)?,
        observed_owner_revision: persistence_u64(
            intent.observed_owner_revision.get(),
            RuntimeCertificationCanonicalFieldV2::ObservedOwnerRevision,
        )?,
        runtime_build_revision: intent.runtime_build_revision.as_str().to_owned(),
        panel: encode_panel(&intent.panel)?,
        serving_lease_milliseconds: lease.get(),
    };
    let encoded = serde_json::to_vec(&wire)
        .map_err(|_| RuntimeCertificationCanonicalErrorV2::Encoding { root })?;
    ensure_size(&encoded)?;
    Ok(encoded)
}

pub(super) fn decode_certification_intent(
    encoded: &[u8],
) -> Result<RuntimeCertificationIntentV2, RuntimeCertificationCanonicalErrorV2> {
    ensure_size(encoded)?;
    let root = RuntimeCertificationCanonicalRootV2::Intent;
    let wire = serde_json::from_slice::<CertificationIntentWireV2>(encoded)
        .map_err(|_| RuntimeCertificationCanonicalErrorV2::Decoding { root })?;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeCertificationCanonicalErrorV2::UnsupportedFormatVersion { root });
    }
    let action_id = decode_non_zero_u64(
        wire.action_id,
        RuntimeCertificationCanonicalFieldV2::ActionId,
    )?;
    let serving_lease =
        RuntimeServingLeaseMillisecondsV2::from_milliseconds(wire.serving_lease_milliseconds)
            .map_err(|reason| {
                canonical(
                    RuntimeCertificationCanonicalFieldV2::ServingLeaseMilliseconds,
                    reason,
                )
            })?;
    let intent = RuntimeCertificationIntentV2 {
        action_id: RuntimeSessionActionIdV1::new(action_id),
        operation_id: RuntimeCertificationOperationIdV2::parse(wire.operation_id)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::OperationId))?,
        guard: decode_guard(wire.guard)?,
        target: decode_target(wire.target, TARGET_FIELDS)?,
        binding_pin: decode_binding_pin(wire.binding_pin)?,
        process_identity: decode_process_identity(
            wire.process_identity,
            PROCESS_TARGET_FIELDS,
            RuntimeCertificationCanonicalFieldV2::ProcessRuntimeGeneration,
            RuntimeCertificationCanonicalFieldV2::ProcessInstanceId,
        )?,
        gateway_owner_lease_id: decode_owner_lease(wire.gateway_owner_lease_id)?,
        observed_owner_revision: decode_non_zero_u64(
            wire.observed_owner_revision,
            RuntimeCertificationCanonicalFieldV2::ObservedOwnerRevision,
        )?,
        runtime_build_revision: RuntimeBuildRevisionV1::parse(wire.runtime_build_revision)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::RuntimeBuildRevision))?,
        panel: decode_panel(wire.panel)?,
        serving_lease_for: Duration::from_millis(serving_lease.get()),
    };
    validate_intent(&intent)?;
    let canonical_bytes = encode_certification_intent(&intent)?;
    if canonical_bytes != encoded {
        return Err(RuntimeCertificationCanonicalErrorV2::NonCanonicalEncoding { root });
    }
    Ok(intent)
}

pub(super) fn encode_certification_request(
    request: &RuntimeCertificationRequestV2,
) -> Result<Vec<u8>, RuntimeCertificationCanonicalErrorV2> {
    validate_request(request)?;
    let root = RuntimeCertificationCanonicalRootV2::Request;
    let wire = encode_request_wire(request)?;
    let encoded = serde_json::to_vec(&wire)
        .map_err(|_| RuntimeCertificationCanonicalErrorV2::Encoding { root })?;
    ensure_root_size(&encoded, root, CERTIFICATION_REQUEST_MAX_OCTETS)?;
    Ok(encoded)
}

pub(super) fn decode_certification_request(
    encoded: &[u8],
) -> Result<RuntimeCertificationRequestV2, RuntimeCertificationCanonicalErrorV2> {
    let root = RuntimeCertificationCanonicalRootV2::Request;
    ensure_root_size(encoded, root, CERTIFICATION_REQUEST_MAX_OCTETS)?;
    let wire = serde_json::from_slice::<CertificationRequestWireV2>(encoded)
        .map_err(|_| RuntimeCertificationCanonicalErrorV2::Decoding { root })?;
    let request = decode_request_wire(wire)?;
    validate_request(&request)?;
    if encode_certification_request(&request)? != encoded {
        return Err(RuntimeCertificationCanonicalErrorV2::NonCanonicalEncoding { root });
    }
    Ok(request)
}

pub(super) fn encode_live_attestation_record(
    record: &RuntimeLiveAttestationRecordV2,
    request_bytes: &[u8],
) -> Result<Vec<u8>, RuntimeCertificationCanonicalErrorV2> {
    let root = RuntimeCertificationCanonicalRootV2::LiveAttestation;
    if encode_certification_request(record.request())? != request_bytes {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::LiveRequestRoot,
            },
        );
    }
    if certification_request_digest_v2(request_bytes) != *record.request_digest() {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::LiveRequestDigest,
            },
        );
    }
    let mut encoded = Vec::with_capacity(115 + request_bytes.len());
    encoded.extend_from_slice(b"{\"format_version\":2,\"request_digest\":\"");
    encoded.extend_from_slice(record.request_digest().as_str().as_bytes());
    encoded.extend_from_slice(b"\",\"request\":");
    encoded.extend_from_slice(request_bytes);
    encoded.push(b'}');
    ensure_root_size(&encoded, root, LIVE_ATTESTATION_MAX_OCTETS)?;
    Ok(encoded)
}

pub(super) fn decode_live_attestation_record(
    encoded: &[u8],
) -> Result<RuntimeLiveAttestationRecordV2, RuntimeCertificationCanonicalErrorV2> {
    let root = RuntimeCertificationCanonicalRootV2::LiveAttestation;
    ensure_root_size(encoded, root, LIVE_ATTESTATION_MAX_OCTETS)?;
    let wire = serde_json::from_slice::<LiveAttestationRecordWireV2>(encoded)
        .map_err(|_| RuntimeCertificationCanonicalErrorV2::Decoding { root })?;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeCertificationCanonicalErrorV2::UnsupportedFormatVersion { root });
    }
    let request_digest = RuntimeCertificationRequestDigestV2::parse(wire.request_digest)
        .map_err(|_| live_invalid(RuntimeCertificationCanonicalFieldV2::RequestDigest))?;
    let request = decode_request_wire(wire.request)?;
    validate_request(&request)?;
    let request_bytes = encode_certification_request(&request)?;
    if certification_request_digest_v2(&request_bytes) != request_digest {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::LiveRequestDigest,
            },
        );
    }
    let record = RuntimeLiveAttestationRecordV2 {
        request_digest,
        request,
    };
    if encode_live_attestation_record(&record, &request_bytes)? != encoded {
        return Err(RuntimeCertificationCanonicalErrorV2::NonCanonicalEncoding { root });
    }
    Ok(record)
}

fn encode_request_wire(
    request: &RuntimeCertificationRequestV2,
) -> Result<CertificationRequestWireV2, RuntimeCertificationCanonicalErrorV2> {
    let root = RuntimeCertificationCanonicalRootV2::Request;
    let intent_bytes = encode_certification_intent(&request.intent)?;
    let intent = serde_json::from_slice::<CertificationIntentWireV2>(&intent_bytes)
        .map_err(|_| RuntimeCertificationCanonicalErrorV2::Encoding { root })?;
    let must_commit_before = RuntimeUnixMicrosecondsV2::from_datetime(request.must_commit_before)
        .map_err(|reason| {
        request_canonical(
            RuntimeCertificationCanonicalFieldV2::MustCommitBeforeUnixMicroseconds,
            reason,
        )
    })?;
    Ok(CertificationRequestWireV2 {
        format_version: FORMAT_VERSION,
        intent,
        intent_fingerprint: request.intent_fingerprint.as_str().to_owned(),
        must_commit_before_unix_microseconds: must_commit_before.get(),
        route_admission: encode_route_admission(&request.route_admission)?,
    })
}

fn decode_request_wire(
    wire: CertificationRequestWireV2,
) -> Result<RuntimeCertificationRequestV2, RuntimeCertificationCanonicalErrorV2> {
    let root = RuntimeCertificationCanonicalRootV2::Request;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeCertificationCanonicalErrorV2::UnsupportedFormatVersion { root });
    }
    let intent_bytes = serde_json::to_vec(&wire.intent)
        .map_err(|_| RuntimeCertificationCanonicalErrorV2::Decoding { root })?;
    let intent = decode_certification_intent(&intent_bytes)?;
    let intent_fingerprint =
        RuntimeCertificationIntentFingerprintV2::parse(wire.intent_fingerprint).map_err(|_| {
            request_invalid(RuntimeCertificationCanonicalFieldV2::IntentFingerprint)
        })?;
    let must_commit_before =
        RuntimeUnixMicrosecondsV2::from_i64(wire.must_commit_before_unix_microseconds)
            .map_err(|reason| {
                request_canonical(
                    RuntimeCertificationCanonicalFieldV2::MustCommitBeforeUnixMicroseconds,
                    reason,
                )
            })?
            .to_datetime();
    Ok(RuntimeCertificationRequestV2 {
        intent,
        intent_fingerprint,
        must_commit_before,
        route_admission: decode_route_admission(wire.route_admission)?,
    })
}

fn encode_guard(
    guard: &RuntimeExecutionGuardV1,
) -> Result<ExecutionGuardWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(ExecutionGuardWireV2 {
        scope: DeploymentScopeWireV2 {
            tenant_id: guard.scope.tenant_id.as_str().to_owned(),
            installation_id: guard.scope.installation_id.as_str().to_owned(),
            deployment_id: guard.scope.deployment_id.as_str().to_owned(),
        },
        expected_revision: persistence_u64(
            guard.expected_revision.get(),
            RuntimeCertificationCanonicalFieldV2::GuardExpectedRevision,
        )?,
        controller_id: guard.controller_id.as_str().to_owned(),
        fencing_token: persistence_u64(
            guard.fencing_token.get(),
            RuntimeCertificationCanonicalFieldV2::GuardFencingToken,
        )?,
        runtime_generation: persistence_u64(
            guard.runtime_generation.get(),
            RuntimeCertificationCanonicalFieldV2::GuardRuntimeGeneration,
        )?,
        convergence_attempt: guard.convergence_attempt.get(),
    })
}

fn decode_guard(
    wire: ExecutionGuardWireV2,
) -> Result<RuntimeExecutionGuardV1, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimeExecutionGuardV1 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse(wire.scope.tenant_id)
                .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GuardTenantId))?,
            installation_id: InstallationId::parse(wire.scope.installation_id)
                .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GuardInstallationId))?,
            deployment_id: DeploymentId::parse(wire.scope.deployment_id)
                .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GuardDeploymentId))?,
        },
        expected_revision: DeploymentRevision::new(persistence_u64(
            wire.expected_revision,
            RuntimeCertificationCanonicalFieldV2::GuardExpectedRevision,
        )?)
        .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GuardExpectedRevision))?,
        controller_id: ControllerId::parse(wire.controller_id)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GuardControllerId))?,
        fencing_token: FencingToken::new(persistence_u64(
            wire.fencing_token,
            RuntimeCertificationCanonicalFieldV2::GuardFencingToken,
        )?)
        .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GuardFencingToken))?,
        runtime_generation: RuntimeGeneration::new(persistence_u64(
            wire.runtime_generation,
            RuntimeCertificationCanonicalFieldV2::GuardRuntimeGeneration,
        )?)
        .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GuardRuntimeGeneration))?,
        convergence_attempt: NonZeroU32::new(wire.convergence_attempt).ok_or_else(|| {
            invalid(RuntimeCertificationCanonicalFieldV2::GuardConvergenceAttempt)
        })?,
    })
}

fn encode_target(
    target: &RuntimeDeploymentTargetV1,
    fields: TargetFields,
) -> Result<DeploymentTargetWireV2, RuntimeCertificationCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::from_u64(target.guild_id.0)
        .map_err(|reason| canonical(fields.guild_id, reason))?;
    Ok(DeploymentTargetWireV2 {
        guild_id: guild_id.canonical_text(),
        ruleset_key: target.ruleset_key.as_str().to_owned(),
        version: target.version.get(),
        content_hash: target.content_hash.to_hex(),
        binding_revision: persistence_u64(target.binding_revision.get(), fields.binding_revision)?,
        binding_fingerprint: target.binding_fingerprint.as_str().to_owned(),
    })
}

fn decode_target(
    wire: DeploymentTargetWireV2,
    fields: TargetFields,
) -> Result<RuntimeDeploymentTargetV1, RuntimeCertificationCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::parse_text(&wire.guild_id)
        .map_err(|reason| canonical(fields.guild_id, reason))?;
    Ok(RuntimeDeploymentTargetV1 {
        guild_id: GuildId(guild_id.get_u64()),
        ruleset_key: RuleSetKey::parse(&wire.ruleset_key)
            .map_err(|_| invalid(fields.ruleset_key))?,
        version: RuleSetVersionId::new(wire.version).map_err(|_| invalid(fields.version))?,
        content_hash: RuleSetContentHash::parse_hex(&wire.content_hash)
            .ok_or_else(|| invalid(fields.content_hash))?,
        binding_revision: BindingRevision::new(persistence_u64(
            wire.binding_revision,
            fields.binding_revision,
        )?)
        .map_err(|_| invalid(fields.binding_revision))?,
        binding_fingerprint: ResourceBindingFingerprint::parse(&wire.binding_fingerprint)
            .map_err(|_| invalid(fields.binding_fingerprint))?,
    })
}

fn encode_binding_pin(
    pin: &RuntimeBindingPinV1,
) -> Result<BindingPinWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(BindingPinWireV2 {
        tenant_id: pin.tenant_id.as_str().to_owned(),
        installation_id: pin.installation_id.as_str().to_owned(),
        installation_authority_revision: persistence_u64(
            pin.installation_authority_revision.get(),
            RuntimeCertificationCanonicalFieldV2::BindingPinInstallationAuthorityRevision,
        )?,
        binding_revision: persistence_u64(
            pin.binding_revision.get(),
            RuntimeCertificationCanonicalFieldV2::BindingPinBindingRevision,
        )?,
        binding_fingerprint: pin.binding_fingerprint.as_str().to_owned(),
    })
}

fn decode_binding_pin(
    wire: BindingPinWireV2,
) -> Result<RuntimeBindingPinV1, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimeBindingPinV1 {
        tenant_id: TenantId::parse(wire.tenant_id)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::BindingPinTenantId))?,
        installation_id: InstallationId::parse(wire.installation_id)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::BindingPinInstallationId))?,
        installation_authority_revision: decode_non_zero_u64(
            wire.installation_authority_revision,
            RuntimeCertificationCanonicalFieldV2::BindingPinInstallationAuthorityRevision,
        )?,
        binding_revision: BindingRevision::new(persistence_u64(
            wire.binding_revision,
            RuntimeCertificationCanonicalFieldV2::BindingPinBindingRevision,
        )?)
        .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::BindingPinBindingRevision))?,
        binding_fingerprint: ResourceBindingFingerprint::parse(&wire.binding_fingerprint).map_err(
            |_| invalid(RuntimeCertificationCanonicalFieldV2::BindingPinBindingFingerprint),
        )?,
    })
}

fn encode_process_identity(
    process: &RuntimeProcessIdentityV1,
    target_fields: TargetFields,
    generation_field: RuntimeCertificationCanonicalFieldV2,
) -> Result<ProcessIdentityWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(ProcessIdentityWireV2 {
        target: encode_target(&process.target, target_fields)?,
        runtime_generation: persistence_u64(process.runtime_generation.get(), generation_field)?,
        process_instance_id: process.process_instance_id.as_str().to_owned(),
    })
}

fn decode_process_identity(
    wire: ProcessIdentityWireV2,
    target_fields: TargetFields,
    generation_field: RuntimeCertificationCanonicalFieldV2,
    process_field: RuntimeCertificationCanonicalFieldV2,
) -> Result<RuntimeProcessIdentityV1, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimeProcessIdentityV1 {
        target: decode_target(wire.target, target_fields)?,
        runtime_generation: RuntimeGeneration::new(persistence_u64(
            wire.runtime_generation,
            generation_field,
        )?)
        .map_err(|_| invalid(generation_field))?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id)
            .map_err(|_| invalid(process_field))?,
    })
}

fn encode_owner_lease(
    lease: &RuntimeGatewayOwnerLeaseIdV1,
) -> Result<GatewayOwnerLeaseIdWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(GatewayOwnerLeaseIdWireV2 {
        gateway_shard_id: lease.gateway_shard_id.as_str().to_owned(),
        process_instance_id: lease.process_instance_id.as_str().to_owned(),
        lease_epoch: persistence_u64(
            lease.lease_epoch.get(),
            RuntimeCertificationCanonicalFieldV2::GatewayLeaseEpoch,
        )?,
        expected_build_revision: lease.expected_build_revision.as_str().to_owned(),
    })
}

fn decode_owner_lease(
    wire: GatewayOwnerLeaseIdWireV2,
) -> Result<RuntimeGatewayOwnerLeaseIdV1, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse(wire.gateway_shard_id)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GatewayShardId))?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::GatewayProcessInstanceId))?,
        lease_epoch: decode_non_zero_u64(
            wire.lease_epoch,
            RuntimeCertificationCanonicalFieldV2::GatewayLeaseEpoch,
        )?,
        expected_build_revision: RuntimeBuildRevisionV1::parse(wire.expected_build_revision)
            .map_err(|_| {
                invalid(RuntimeCertificationCanonicalFieldV2::GatewayExpectedBuildRevision)
            })?,
    })
}

fn encode_route_admission(
    admission: &RuntimeRouteAdmissionAttestationV2,
) -> Result<RouteAdmissionWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(RouteAdmissionWireV2 {
        barrier_id: admission.barrier_id.as_str().to_owned(),
        pause: BarrierPauseWireV2 {
            coordinator_generation: request_persistence_u64(
                admission.pause.coordinator_generation.get(),
                RuntimeCertificationCanonicalFieldV2::PauseCoordinatorGeneration,
            )?,
            connection_epoch: request_persistence_u64(
                admission.pause.connection_epoch.get(),
                RuntimeCertificationCanonicalFieldV2::PauseConnectionEpoch,
            )?,
            paused_admission_revision: request_persistence_u64(
                admission.pause.paused_admission_revision.get(),
                RuntimeCertificationCanonicalFieldV2::PauseAdmissionRevision,
            )?,
            pause_sequence: request_persistence_u64(
                admission.pause.pause_sequence.get(),
                RuntimeCertificationCanonicalFieldV2::PauseSequence,
            )?,
        },
        gateway: GatewayReadyWireV2 {
            process_instance_id: admission.gateway.process_instance_id.as_str().to_owned(),
            connection_epoch: request_persistence_u64(
                admission.gateway.connection_epoch.get(),
                RuntimeCertificationCanonicalFieldV2::GatewayReadyConnectionEpoch,
            )?,
            kind: gateway_kind_tag(admission.gateway.kind).to_owned(),
            admission_revision: request_persistence_u64(
                admission.gateway.admission_revision.get(),
                RuntimeCertificationCanonicalFieldV2::GatewayReadyAdmissionRevision,
            )?,
            connected_event_sequence: request_persistence_u64(
                admission.gateway.connected_event_sequence.get(),
                RuntimeCertificationCanonicalFieldV2::GatewayReadyConnectedEventSequence,
            )?,
            resume_sequence: request_persistence_u64(
                admission.gateway.resume_sequence.get(),
                RuntimeCertificationCanonicalFieldV2::GatewayReadyResumeSequence,
            )?,
        },
        gateway_owner_lease_id: encode_request_owner_lease(&admission.gateway_owner_lease_id)?,
        attested_owner_revision: request_persistence_u64(
            admission.attested_owner_revision.get(),
            RuntimeCertificationCanonicalFieldV2::AttestedOwnerRevision,
        )?,
        route: ServingRouteWireV2 {
            identity: encode_request_process_identity(&admission.route.identity)?,
            controller_fencing_token: request_persistence_u64(
                admission.route.controller_fencing_token.get(),
                RuntimeCertificationCanonicalFieldV2::RouteControllerFencingToken,
            )?,
            route_incarnation: request_persistence_u64(
                admission.route.route_incarnation.get(),
                RuntimeCertificationCanonicalFieldV2::RouteIncarnation,
            )?,
            activation_sequence: request_persistence_u64(
                admission.route.activation_sequence.get(),
                RuntimeCertificationCanonicalFieldV2::RouteActivationSequence,
            )?,
        },
    })
}

fn decode_route_admission(
    wire: RouteAdmissionWireV2,
) -> Result<RuntimeRouteAdmissionAttestationV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimeRouteAdmissionAttestationV2 {
        barrier_id: RuntimeBarrierIdV1::parse(wire.barrier_id)
            .map_err(|_| request_invalid(RuntimeCertificationCanonicalFieldV2::BarrierId))?,
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: request_non_zero_u64(
                wire.pause.coordinator_generation,
                RuntimeCertificationCanonicalFieldV2::PauseCoordinatorGeneration,
            )?,
            connection_epoch: request_non_zero_u64(
                wire.pause.connection_epoch,
                RuntimeCertificationCanonicalFieldV2::PauseConnectionEpoch,
            )?,
            paused_admission_revision: request_non_zero_u64(
                wire.pause.paused_admission_revision,
                RuntimeCertificationCanonicalFieldV2::PauseAdmissionRevision,
            )?,
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(request_non_zero_u64(
                wire.pause.pause_sequence,
                RuntimeCertificationCanonicalFieldV2::PauseSequence,
            )?),
        },
        gateway: RuntimeGatewayReadyAttestationV2 {
            process_instance_id: ProcessInstanceId::parse(wire.gateway.process_instance_id)
                .map_err(|_| {
                    request_invalid(
                        RuntimeCertificationCanonicalFieldV2::GatewayReadyProcessInstanceId,
                    )
                })?,
            connection_epoch: request_non_zero_u64(
                wire.gateway.connection_epoch,
                RuntimeCertificationCanonicalFieldV2::GatewayReadyConnectionEpoch,
            )?,
            kind: decode_gateway_kind(&wire.gateway.kind)?,
            admission_revision: request_non_zero_u64(
                wire.gateway.admission_revision,
                RuntimeCertificationCanonicalFieldV2::GatewayReadyAdmissionRevision,
            )?,
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(request_non_zero_u64(
                wire.gateway.connected_event_sequence,
                RuntimeCertificationCanonicalFieldV2::GatewayReadyConnectedEventSequence,
            )?),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(request_non_zero_u64(
                wire.gateway.resume_sequence,
                RuntimeCertificationCanonicalFieldV2::GatewayReadyResumeSequence,
            )?),
        },
        gateway_owner_lease_id: decode_request_owner_lease(wire.gateway_owner_lease_id)?,
        attested_owner_revision: request_non_zero_u64(
            wire.attested_owner_revision,
            RuntimeCertificationCanonicalFieldV2::AttestedOwnerRevision,
        )?,
        route: RuntimeServingRouteAttestationV2 {
            identity: decode_request_process_identity(wire.route.identity)?,
            controller_fencing_token: FencingToken::new(request_persistence_u64(
                wire.route.controller_fencing_token,
                RuntimeCertificationCanonicalFieldV2::RouteControllerFencingToken,
            )?)
            .map_err(|_| {
                request_invalid(RuntimeCertificationCanonicalFieldV2::RouteControllerFencingToken)
            })?,
            route_incarnation: request_non_zero_u64(
                wire.route.route_incarnation,
                RuntimeCertificationCanonicalFieldV2::RouteIncarnation,
            )?,
            activation_sequence: request_non_zero_u64(
                wire.route.activation_sequence,
                RuntimeCertificationCanonicalFieldV2::RouteActivationSequence,
            )?,
        },
    })
}

fn encode_request_process_identity(
    process: &RuntimeProcessIdentityV1,
) -> Result<ProcessIdentityWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(ProcessIdentityWireV2 {
        target: encode_request_target(&process.target)?,
        runtime_generation: request_persistence_u64(
            process.runtime_generation.get(),
            RuntimeCertificationCanonicalFieldV2::RouteRuntimeGeneration,
        )?,
        process_instance_id: process.process_instance_id.as_str().to_owned(),
    })
}

fn decode_request_process_identity(
    wire: ProcessIdentityWireV2,
) -> Result<RuntimeProcessIdentityV1, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimeProcessIdentityV1 {
        target: decode_request_target(wire.target)?,
        runtime_generation: RuntimeGeneration::new(request_persistence_u64(
            wire.runtime_generation,
            RuntimeCertificationCanonicalFieldV2::RouteRuntimeGeneration,
        )?)
        .map_err(|_| {
            request_invalid(RuntimeCertificationCanonicalFieldV2::RouteRuntimeGeneration)
        })?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id).map_err(|_| {
            request_invalid(RuntimeCertificationCanonicalFieldV2::RouteProcessInstanceId)
        })?,
    })
}

fn encode_request_target(
    target: &RuntimeDeploymentTargetV1,
) -> Result<DeploymentTargetWireV2, RuntimeCertificationCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::from_u64(target.guild_id.0)
        .map_err(|reason| request_canonical(ROUTE_TARGET_FIELDS.guild_id, reason))?;
    Ok(DeploymentTargetWireV2 {
        guild_id: guild_id.canonical_text(),
        ruleset_key: target.ruleset_key.as_str().to_owned(),
        version: target.version.get(),
        content_hash: target.content_hash.to_hex(),
        binding_revision: request_persistence_u64(
            target.binding_revision.get(),
            ROUTE_TARGET_FIELDS.binding_revision,
        )?,
        binding_fingerprint: target.binding_fingerprint.as_str().to_owned(),
    })
}

fn decode_request_target(
    wire: DeploymentTargetWireV2,
) -> Result<RuntimeDeploymentTargetV1, RuntimeCertificationCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::parse_text(&wire.guild_id)
        .map_err(|reason| request_canonical(ROUTE_TARGET_FIELDS.guild_id, reason))?;
    Ok(RuntimeDeploymentTargetV1 {
        guild_id: GuildId(guild_id.get_u64()),
        ruleset_key: RuleSetKey::parse(&wire.ruleset_key)
            .map_err(|_| request_invalid(ROUTE_TARGET_FIELDS.ruleset_key))?,
        version: RuleSetVersionId::new(wire.version)
            .map_err(|_| request_invalid(ROUTE_TARGET_FIELDS.version))?,
        content_hash: RuleSetContentHash::parse_hex(&wire.content_hash)
            .ok_or_else(|| request_invalid(ROUTE_TARGET_FIELDS.content_hash))?,
        binding_revision: BindingRevision::new(request_persistence_u64(
            wire.binding_revision,
            ROUTE_TARGET_FIELDS.binding_revision,
        )?)
        .map_err(|_| request_invalid(ROUTE_TARGET_FIELDS.binding_revision))?,
        binding_fingerprint: ResourceBindingFingerprint::parse(&wire.binding_fingerprint)
            .map_err(|_| request_invalid(ROUTE_TARGET_FIELDS.binding_fingerprint))?,
    })
}

fn encode_request_owner_lease(
    lease: &RuntimeGatewayOwnerLeaseIdV1,
) -> Result<GatewayOwnerLeaseIdWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(GatewayOwnerLeaseIdWireV2 {
        gateway_shard_id: lease.gateway_shard_id.as_str().to_owned(),
        process_instance_id: lease.process_instance_id.as_str().to_owned(),
        lease_epoch: request_persistence_u64(
            lease.lease_epoch.get(),
            RuntimeCertificationCanonicalFieldV2::RouteGatewayLeaseEpoch,
        )?,
        expected_build_revision: lease.expected_build_revision.as_str().to_owned(),
    })
}

fn decode_request_owner_lease(
    wire: GatewayOwnerLeaseIdWireV2,
) -> Result<RuntimeGatewayOwnerLeaseIdV1, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse(wire.gateway_shard_id).map_err(|_| {
            request_invalid(RuntimeCertificationCanonicalFieldV2::RouteGatewayShardId)
        })?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id).map_err(|_| {
            request_invalid(RuntimeCertificationCanonicalFieldV2::RouteGatewayProcessInstanceId)
        })?,
        lease_epoch: request_non_zero_u64(
            wire.lease_epoch,
            RuntimeCertificationCanonicalFieldV2::RouteGatewayLeaseEpoch,
        )?,
        expected_build_revision: RuntimeBuildRevisionV1::parse(wire.expected_build_revision)
            .map_err(|_| {
                request_invalid(
                    RuntimeCertificationCanonicalFieldV2::RouteGatewayExpectedBuildRevision,
                )
            })?,
    })
}

fn gateway_kind_tag(kind: RuntimeGatewayReadyKindV2) -> &'static str {
    match kind {
        RuntimeGatewayReadyKindV2::Ready => "ready",
        RuntimeGatewayReadyKindV2::Resumed => "resumed",
    }
}

fn decode_gateway_kind(
    value: &str,
) -> Result<RuntimeGatewayReadyKindV2, RuntimeCertificationCanonicalErrorV2> {
    match value {
        "ready" => Ok(RuntimeGatewayReadyKindV2::Ready),
        "resumed" => Ok(RuntimeGatewayReadyKindV2::Resumed),
        _ => Err(request_invalid(
            RuntimeCertificationCanonicalFieldV2::GatewayReadyKind,
        )),
    }
}

fn encode_panel(
    panel: &RuntimePanelEvidenceV2,
) -> Result<PanelEvidenceWireV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(PanelEvidenceWireV2 {
        certificate_id: panel.certificate_id.as_str().to_owned(),
        report_digest: panel.report_digest.as_str().to_owned(),
        process_identity: encode_process_identity(
            &panel.process_identity,
            PANEL_PROCESS_TARGET_FIELDS,
            RuntimeCertificationCanonicalFieldV2::PanelProcessRuntimeGeneration,
        )?,
        controller_fencing_token: persistence_u64(
            panel.controller_fencing_token.get(),
            RuntimeCertificationCanonicalFieldV2::PanelControllerFencingToken,
        )?,
    })
}

fn decode_panel(
    wire: PanelEvidenceWireV2,
) -> Result<RuntimePanelEvidenceV2, RuntimeCertificationCanonicalErrorV2> {
    Ok(RuntimePanelEvidenceV2 {
        certificate_id: PanelCertificateId::parse(wire.certificate_id)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::PanelCertificateId))?,
        report_digest: PanelReportDigestV1::parse(wire.report_digest)
            .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::PanelReportDigest))?,
        process_identity: decode_process_identity(
            wire.process_identity,
            PANEL_PROCESS_TARGET_FIELDS,
            RuntimeCertificationCanonicalFieldV2::PanelProcessRuntimeGeneration,
            RuntimeCertificationCanonicalFieldV2::PanelProcessInstanceId,
        )?,
        controller_fencing_token: FencingToken::new(persistence_u64(
            wire.controller_fencing_token,
            RuntimeCertificationCanonicalFieldV2::PanelControllerFencingToken,
        )?)
        .map_err(|_| invalid(RuntimeCertificationCanonicalFieldV2::PanelControllerFencingToken))?,
    })
}

fn persistence_u64(
    value: u64,
    field: RuntimeCertificationCanonicalFieldV2,
) -> Result<u64, RuntimeCertificationCanonicalErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(RuntimePersistenceU64V2::get_u64)
        .map_err(|reason| canonical(field, reason))
}

fn decode_non_zero_u64(
    value: u64,
    field: RuntimeCertificationCanonicalFieldV2,
) -> Result<NonZeroU64, RuntimeCertificationCanonicalErrorV2> {
    NonZeroU64::new(persistence_u64(value, field)?).ok_or_else(|| invalid(field))
}

fn ensure_size(encoded: &[u8]) -> Result<(), RuntimeCertificationCanonicalErrorV2> {
    ensure_root_size(
        encoded,
        RuntimeCertificationCanonicalRootV2::Intent,
        CERTIFICATION_INTENT_MAX_OCTETS,
    )
}

fn ensure_root_size(
    encoded: &[u8],
    root: RuntimeCertificationCanonicalRootV2,
    maximum: usize,
) -> Result<(), RuntimeCertificationCanonicalErrorV2> {
    if encoded.len() > maximum {
        return Err(RuntimeCertificationCanonicalErrorV2::PayloadTooLarge { root });
    }
    Ok(())
}

fn invalid(field: RuntimeCertificationCanonicalFieldV2) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::InvalidField {
        root: RuntimeCertificationCanonicalRootV2::Intent,
        field,
    }
}

fn canonical(
    field: RuntimeCertificationCanonicalFieldV2,
    reason: RuntimeCanonicalValueErrorV2,
) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::CanonicalValue {
        root: RuntimeCertificationCanonicalRootV2::Intent,
        field,
        reason,
    }
}

fn request_persistence_u64(
    value: u64,
    field: RuntimeCertificationCanonicalFieldV2,
) -> Result<u64, RuntimeCertificationCanonicalErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(RuntimePersistenceU64V2::get_u64)
        .map_err(|reason| request_canonical(field, reason))
}

fn request_non_zero_u64(
    value: u64,
    field: RuntimeCertificationCanonicalFieldV2,
) -> Result<NonZeroU64, RuntimeCertificationCanonicalErrorV2> {
    NonZeroU64::new(request_persistence_u64(value, field)?).ok_or_else(|| request_invalid(field))
}

fn request_invalid(
    field: RuntimeCertificationCanonicalFieldV2,
) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::InvalidField {
        root: RuntimeCertificationCanonicalRootV2::Request,
        field,
    }
}

fn request_canonical(
    field: RuntimeCertificationCanonicalFieldV2,
    reason: RuntimeCanonicalValueErrorV2,
) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::CanonicalValue {
        root: RuntimeCertificationCanonicalRootV2::Request,
        field,
        reason,
    }
}

fn live_invalid(
    field: RuntimeCertificationCanonicalFieldV2,
) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::InvalidField {
        root: RuntimeCertificationCanonicalRootV2::LiveAttestation,
        field,
    }
}
