use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
    ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1,
    RuntimeFailureV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde::{Deserialize, Deserializer, Serialize};

use super::{
    validate_request, RuntimeSuspendAttemptCanonicalErrorV2, RuntimeSuspendAttemptCanonicalFieldV2,
    SUSPEND_ATTEMPT_MAX_OCTETS,
};
use crate::v2_canonical_value::{RuntimeDiscordSnowflakeV2, RuntimePersistenceU64V2};
use crate::{
    GatewayShardIdV1, RuntimeAttemptDispositionV2, RuntimeBarrierIdV1,
    RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1, RuntimeCanonicalValueErrorV2,
    RuntimeClosedRecoveryRouteWitnessV2, RuntimeDeploymentScopeV1, RuntimeDrainObligationV2,
    RuntimeExactLocalRouteIdentityV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeLocalRouteEffectV2, RuntimePreviousServingLeaseIdentityV1,
    RuntimeRecoveryIdV2, RuntimeResumeCheckpointV2, RuntimeRouteMutationProvenanceV2,
    RuntimeServingSlotV2, RuntimeSessionActionIdV1, RuntimeShutdownRouteWitnessV2,
    RuntimeSuspendAttemptRequestV2, RuntimeSuspendedRouteLifecycleV2, RuntimeSuspensionIdV2,
    RuntimeSuspensionSourcePhaseV2, RuntimeUnixMicrosecondsV2,
};

const FORMAT_VERSION: u8 = 2;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuspendAttemptWireV2 {
    format_version: u8,
    suspension_id: String,
    action_id: u64,
    guard: ExecutionGuardWireV2,
    source_phase: String,
    failure: FailureWireV2,
    disposition: AttemptDispositionWireV2,
    checkpoint: String,
    local_effect: LocalRouteEffectWireV2,
    drain_obligation: DrainObligationWireV2,
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
struct FailureWireV2 {
    failure_id: String,
    kind: String,
    code: String,
    message: String,
    recorded_at_unix_microseconds: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum AttemptDispositionWireV2 {
    #[serde(rename = "retryable")]
    Retryable {
        retry_not_before_unix_microseconds: i64,
    },
    #[serde(rename = "blocked")]
    Blocked {},
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactLocalRouteWireV2 {
    identity: ProcessIdentityWireV2,
    controller_fencing_token: u64,
    route_incarnation: u64,
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
struct ServingSlotWireV2 {
    guild_id: String,
    ruleset_key: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousServingIdentityWireV2 {
    scope: DeploymentScopeWireV2,
    attestation_id: String,
    process: ProcessIdentityWireV2,
    lease_epoch: u64,
    revision: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum DrainObligationWireV2 {
    #[serde(rename = "none")]
    None {},
    #[serde(rename = "exact_local_route")]
    ExactLocalRoute { route: ExactLocalRouteWireV2 },
    #[serde(rename = "previous_serving")]
    PreviousServing {
        previous: PreviousServingIdentityWireV2,
    },
    #[serde(rename = "local_and_previous")]
    LocalAndPrevious {
        local: ExactLocalRouteWireV2,
        previous: PreviousServingIdentityWireV2,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum LocalRouteEffectWireV2 {
    #[serde(rename = "none")]
    None {},
    #[serde(rename = "exact_route")]
    ExactRoute {
        route: ExactLocalRouteWireV2,
        lifecycle: String,
    },
    #[serde(rename = "route_absent")]
    RouteAbsent {
        slot: ServingSlotWireV2,
        #[serde(deserialize_with = "deserialize_required_option")]
        expected_route: Option<ExactLocalRouteWireV2>,
        provenance: Box<RouteMutationProvenanceWireV2>,
        observed_sequence: u64,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum RouteMutationProvenanceWireV2 {
    #[serde(rename = "ordinary")]
    Ordinary {
        barrier_id: String,
        pause: BarrierPauseWireV2,
    },
    #[serde(rename = "closed_recovery")]
    ClosedRecovery {
        witness: ClosedRecoveryRouteWitnessWireV2,
    },
    #[serde(rename = "shutdown")]
    Shutdown { witness: ShutdownRouteWitnessWireV2 },
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
struct ClosedRecoveryRouteWitnessWireV2 {
    recovery_id: String,
    originating_emergency_generation: u64,
    recovery_generation: u64,
    recovery_authority_revision: u64,
    gateway_owner_lease_id: GatewayOwnerLeaseIdWireV2,
    observed_owner_revision: u64,
    owner_expires_at_unix_microseconds: i64,
    process_instance_id: String,
    connection_epoch: u64,
    paused_admission_revision: u64,
    connected_event_sequence: u64,
    pause_sequence: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShutdownRouteWitnessWireV2 {
    shutdown_generation: u64,
    gateway_owner_lease_id: GatewayOwnerLeaseIdWireV2,
    observed_owner_revision: u64,
    owner_expires_at_unix_microseconds: i64,
    process_instance_id: String,
    connection_epoch: u64,
    paused_admission_revision: u64,
    connected_event_sequence: u64,
    pause_sequence: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GatewayOwnerLeaseIdWireV2 {
    gateway_shard_id: String,
    process_instance_id: String,
    lease_epoch: u64,
    expected_build_revision: String,
}

#[derive(Clone, Copy)]
struct TargetFields {
    guild_id: RuntimeSuspendAttemptCanonicalFieldV2,
    ruleset_key: RuntimeSuspendAttemptCanonicalFieldV2,
    version: RuntimeSuspendAttemptCanonicalFieldV2,
    content_hash: RuntimeSuspendAttemptCanonicalFieldV2,
    binding_revision: RuntimeSuspendAttemptCanonicalFieldV2,
    binding_fingerprint: RuntimeSuspendAttemptCanonicalFieldV2,
}

const LOCAL_TARGET_FIELDS: TargetFields = TargetFields {
    guild_id: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetGuildId,
    ruleset_key: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetRuleSetKey,
    version: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetVersion,
    content_hash: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetContentHash,
    binding_revision: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetBindingRevision,
    binding_fingerprint: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetBindingFingerprint,
};

const PREVIOUS_TARGET_FIELDS: TargetFields = TargetFields {
    guild_id: RuntimeSuspendAttemptCanonicalFieldV2::PreviousTargetGuildId,
    ruleset_key: RuntimeSuspendAttemptCanonicalFieldV2::PreviousTargetRuleSetKey,
    version: RuntimeSuspendAttemptCanonicalFieldV2::PreviousTargetVersion,
    content_hash: RuntimeSuspendAttemptCanonicalFieldV2::PreviousTargetContentHash,
    binding_revision: RuntimeSuspendAttemptCanonicalFieldV2::PreviousTargetBindingRevision,
    binding_fingerprint: RuntimeSuspendAttemptCanonicalFieldV2::PreviousTargetBindingFingerprint,
};

pub(super) fn encode_suspend_attempt(
    request: &RuntimeSuspendAttemptRequestV2,
) -> Result<Vec<u8>, RuntimeSuspendAttemptCanonicalErrorV2> {
    validate_request(request)?;
    let wire = SuspendAttemptWireV2 {
        format_version: FORMAT_VERSION,
        suspension_id: request.suspension_id.as_str().to_owned(),
        action_id: persistence_u64(
            request.action_id.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ActionId,
        )?,
        guard: encode_guard(&request.guard)?,
        source_phase: source_phase_tag(request.source_phase).to_owned(),
        failure: encode_failure(&request.failure)?,
        disposition: encode_disposition(&request.disposition)?,
        checkpoint: checkpoint_tag(request.checkpoint).to_owned(),
        local_effect: encode_local_effect(&request.local_effect)?,
        drain_obligation: encode_drain_obligation(&request.drain_obligation)?,
    };
    encode_root(&wire)
}

pub(super) fn validate_suspend_attempt_mutable_state(
    local_effect: &RuntimeLocalRouteEffectV2,
    drain_obligation: &RuntimeDrainObligationV2,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    encode_local_effect(local_effect)?;
    encode_drain_obligation(drain_obligation)?;
    Ok(())
}

pub(crate) fn encode_local_effect_bytes(
    local_effect: &RuntimeLocalRouteEffectV2,
) -> Result<Vec<u8>, RuntimeSuspendAttemptCanonicalErrorV2> {
    encode_root(&encode_local_effect(local_effect)?)
}

pub(crate) fn decode_local_effect_bytes(
    encoded: &[u8],
) -> Result<RuntimeLocalRouteEffectV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    ensure_size(encoded)?;
    let wire = serde_json::from_slice::<LocalRouteEffectWireV2>(encoded)
        .map_err(|_| RuntimeSuspendAttemptCanonicalErrorV2::Decoding)?;
    let local_effect = decode_local_effect(wire)?;
    if encode_local_effect_bytes(&local_effect)? != encoded {
        return Err(RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding);
    }
    Ok(local_effect)
}

pub(crate) fn encode_drain_obligation_bytes(
    drain_obligation: &RuntimeDrainObligationV2,
) -> Result<Vec<u8>, RuntimeSuspendAttemptCanonicalErrorV2> {
    encode_root(&encode_drain_obligation(drain_obligation)?)
}

pub(crate) fn decode_drain_obligation_bytes(
    encoded: &[u8],
) -> Result<RuntimeDrainObligationV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    ensure_size(encoded)?;
    let wire = serde_json::from_slice::<DrainObligationWireV2>(encoded)
        .map_err(|_| RuntimeSuspendAttemptCanonicalErrorV2::Decoding)?;
    let drain_obligation = decode_drain_obligation(wire)?;
    if encode_drain_obligation_bytes(&drain_obligation)? != encoded {
        return Err(RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding);
    }
    Ok(drain_obligation)
}

pub(super) fn encode_provenance_bytes(
    provenance: &RuntimeRouteMutationProvenanceV2,
) -> Result<Vec<u8>, RuntimeSuspendAttemptCanonicalErrorV2> {
    encode_root(&encode_provenance(provenance)?)
}

pub(super) fn decode_provenance_bytes(
    encoded: &[u8],
) -> Result<RuntimeRouteMutationProvenanceV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    ensure_size(encoded)?;
    let wire = serde_json::from_slice::<RouteMutationProvenanceWireV2>(encoded)
        .map_err(|_| RuntimeSuspendAttemptCanonicalErrorV2::Decoding)?;
    let provenance = decode_provenance(wire)?;
    if encode_provenance_bytes(&provenance)? != encoded {
        return Err(RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding);
    }
    Ok(provenance)
}

pub(super) fn decode_suspend_attempt(
    encoded: &[u8],
) -> Result<RuntimeSuspendAttemptRequestV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    ensure_size(encoded)?;
    let wire = serde_json::from_slice::<SuspendAttemptWireV2>(encoded)
        .map_err(|_| RuntimeSuspendAttemptCanonicalErrorV2::Decoding)?;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeSuspendAttemptCanonicalErrorV2::UnsupportedFormatVersion);
    }
    let request = RuntimeSuspendAttemptRequestV2 {
        suspension_id: RuntimeSuspensionIdV2::parse(wire.suspension_id)
            .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::SuspensionId))?,
        action_id: RuntimeSessionActionIdV1::new(non_zero_u64(
            wire.action_id,
            RuntimeSuspendAttemptCanonicalFieldV2::ActionId,
        )?),
        guard: decode_guard(wire.guard)?,
        source_phase: decode_source_phase(&wire.source_phase)?,
        failure: decode_failure(wire.failure)?,
        disposition: decode_disposition(wire.disposition)?,
        checkpoint: decode_checkpoint(&wire.checkpoint)?,
        local_effect: decode_local_effect(wire.local_effect)?,
        drain_obligation: decode_drain_obligation(wire.drain_obligation)?,
    };
    validate_request(&request)?;
    let canonical = encode_suspend_attempt(&request)?;
    if canonical != encoded {
        return Err(RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding);
    }
    Ok(request)
}

fn encode_guard(
    guard: &crate::RuntimeExecutionGuardV1,
) -> Result<ExecutionGuardWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(ExecutionGuardWireV2 {
        scope: encode_scope(&guard.scope),
        expected_revision: persistence_u64(
            guard.expected_revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::GuardExpectedRevision,
        )?,
        controller_id: guard.controller_id.as_str().to_owned(),
        fencing_token: persistence_u64(
            guard.fencing_token.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::GuardFencingToken,
        )?,
        runtime_generation: persistence_u64(
            guard.runtime_generation.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::GuardRuntimeGeneration,
        )?,
        convergence_attempt: guard.convergence_attempt.get(),
    })
}

fn decode_guard(
    wire: ExecutionGuardWireV2,
) -> Result<crate::RuntimeExecutionGuardV1, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(crate::RuntimeExecutionGuardV1 {
        scope: decode_scope(
            wire.scope,
            RuntimeSuspendAttemptCanonicalFieldV2::GuardTenantId,
            RuntimeSuspendAttemptCanonicalFieldV2::GuardInstallationId,
            RuntimeSuspendAttemptCanonicalFieldV2::GuardDeploymentId,
        )?,
        expected_revision: DeploymentRevision::new(persistence_u64(
            wire.expected_revision,
            RuntimeSuspendAttemptCanonicalFieldV2::GuardExpectedRevision,
        )?)
        .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::GuardExpectedRevision))?,
        controller_id: ControllerId::parse(wire.controller_id)
            .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::GuardControllerId))?,
        fencing_token: FencingToken::new(persistence_u64(
            wire.fencing_token,
            RuntimeSuspendAttemptCanonicalFieldV2::GuardFencingToken,
        )?)
        .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::GuardFencingToken))?,
        runtime_generation: RuntimeGeneration::new(persistence_u64(
            wire.runtime_generation,
            RuntimeSuspendAttemptCanonicalFieldV2::GuardRuntimeGeneration,
        )?)
        .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::GuardRuntimeGeneration))?,
        convergence_attempt: NonZeroU32::new(wire.convergence_attempt).ok_or_else(|| {
            invalid(RuntimeSuspendAttemptCanonicalFieldV2::GuardConvergenceAttempt)
        })?,
    })
}

fn encode_scope(scope: &RuntimeDeploymentScopeV1) -> DeploymentScopeWireV2 {
    DeploymentScopeWireV2 {
        tenant_id: scope.tenant_id.as_str().to_owned(),
        installation_id: scope.installation_id.as_str().to_owned(),
        deployment_id: scope.deployment_id.as_str().to_owned(),
    }
}

fn decode_scope(
    wire: DeploymentScopeWireV2,
    tenant_field: RuntimeSuspendAttemptCanonicalFieldV2,
    installation_field: RuntimeSuspendAttemptCanonicalFieldV2,
    deployment_field: RuntimeSuspendAttemptCanonicalFieldV2,
) -> Result<RuntimeDeploymentScopeV1, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(wire.tenant_id).map_err(|_| invalid(tenant_field))?,
        installation_id: InstallationId::parse(wire.installation_id)
            .map_err(|_| invalid(installation_field))?,
        deployment_id: DeploymentId::parse(wire.deployment_id)
            .map_err(|_| invalid(deployment_field))?,
    })
}

fn encode_failure(
    failure: &RuntimeFailureV1,
) -> Result<FailureWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(FailureWireV2 {
        failure_id: failure.failure_id.as_str().to_owned(),
        kind: failure_kind_tag(failure.kind).to_owned(),
        code: failure.code.clone(),
        message: failure.message.clone(),
        recorded_at_unix_microseconds: unix_microseconds(
            failure.recorded_at,
            RuntimeSuspendAttemptCanonicalFieldV2::FailureRecordedAtUnixMicroseconds,
        )?,
    })
}

fn decode_failure(
    wire: FailureWireV2,
) -> Result<RuntimeFailureV1, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse(wire.failure_id)
            .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::FailureId))?,
        kind: decode_failure_kind(&wire.kind)?,
        code: wire.code,
        message: wire.message,
        recorded_at: decode_unix_microseconds(
            wire.recorded_at_unix_microseconds,
            RuntimeSuspendAttemptCanonicalFieldV2::FailureRecordedAtUnixMicroseconds,
        )?,
    })
}

fn encode_disposition(
    disposition: &RuntimeAttemptDispositionV2,
) -> Result<AttemptDispositionWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match disposition {
        RuntimeAttemptDispositionV2::Retryable { retry_not_before } => {
            let retry_not_before = *retry_not_before;
            Ok(AttemptDispositionWireV2::Retryable {
                retry_not_before_unix_microseconds: unix_microseconds(
                    retry_not_before,
                    RuntimeSuspendAttemptCanonicalFieldV2::RetryNotBeforeUnixMicroseconds,
                )?,
            })
        }
        RuntimeAttemptDispositionV2::Blocked => Ok(AttemptDispositionWireV2::Blocked {}),
    }
}

fn decode_disposition(
    wire: AttemptDispositionWireV2,
) -> Result<RuntimeAttemptDispositionV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match wire {
        AttemptDispositionWireV2::Retryable {
            retry_not_before_unix_microseconds,
        } => Ok(RuntimeAttemptDispositionV2::Retryable {
            retry_not_before: decode_unix_microseconds(
                retry_not_before_unix_microseconds,
                RuntimeSuspendAttemptCanonicalFieldV2::RetryNotBeforeUnixMicroseconds,
            )?,
        }),
        AttemptDispositionWireV2::Blocked {} => Ok(RuntimeAttemptDispositionV2::Blocked),
    }
}

fn encode_local_effect(
    effect: &RuntimeLocalRouteEffectV2,
) -> Result<LocalRouteEffectWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match effect {
        RuntimeLocalRouteEffectV2::None => Ok(LocalRouteEffectWireV2::None {}),
        RuntimeLocalRouteEffectV2::ExactRoute { route, lifecycle } => {
            Ok(LocalRouteEffectWireV2::ExactRoute {
                route: encode_local_route(route)?,
                lifecycle: route_lifecycle_tag(*lifecycle).to_owned(),
            })
        }
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot,
            expected_route,
            provenance,
            observed_sequence,
        } => Ok(LocalRouteEffectWireV2::RouteAbsent {
            slot: encode_slot(slot)?,
            expected_route: expected_route
                .as_ref()
                .map(encode_local_route)
                .transpose()?,
            provenance: Box::new(encode_provenance(provenance)?),
            observed_sequence: persistence_u64(
                observed_sequence.get(),
                RuntimeSuspendAttemptCanonicalFieldV2::RouteAbsentObservedSequence,
            )?,
        }),
    }
}

fn decode_local_effect(
    wire: LocalRouteEffectWireV2,
) -> Result<RuntimeLocalRouteEffectV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match wire {
        LocalRouteEffectWireV2::None {} => Ok(RuntimeLocalRouteEffectV2::None),
        LocalRouteEffectWireV2::ExactRoute { route, lifecycle } => {
            Ok(RuntimeLocalRouteEffectV2::ExactRoute {
                route: decode_local_route(route)?,
                lifecycle: decode_route_lifecycle(&lifecycle)?,
            })
        }
        LocalRouteEffectWireV2::RouteAbsent {
            slot,
            expected_route,
            provenance,
            observed_sequence,
        } => Ok(RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: decode_slot(slot)?,
            expected_route: expected_route.map(decode_local_route).transpose()?,
            provenance: decode_provenance(*provenance)?,
            observed_sequence: non_zero_u64(
                observed_sequence,
                RuntimeSuspendAttemptCanonicalFieldV2::RouteAbsentObservedSequence,
            )?,
        }),
    }
}

fn encode_drain_obligation(
    obligation: &RuntimeDrainObligationV2,
) -> Result<DrainObligationWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match obligation {
        RuntimeDrainObligationV2::None => Ok(DrainObligationWireV2::None {}),
        RuntimeDrainObligationV2::ExactLocalRoute(route) => {
            Ok(DrainObligationWireV2::ExactLocalRoute {
                route: encode_local_route(route)?,
            })
        }
        RuntimeDrainObligationV2::PreviousServing(previous) => {
            Ok(DrainObligationWireV2::PreviousServing {
                previous: encode_previous(previous)?,
            })
        }
        RuntimeDrainObligationV2::LocalAndPrevious { local, previous } => {
            Ok(DrainObligationWireV2::LocalAndPrevious {
                local: encode_local_route(local)?,
                previous: encode_previous(previous)?,
            })
        }
    }
}

fn decode_drain_obligation(
    wire: DrainObligationWireV2,
) -> Result<RuntimeDrainObligationV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match wire {
        DrainObligationWireV2::None {} => Ok(RuntimeDrainObligationV2::None),
        DrainObligationWireV2::ExactLocalRoute { route } => Ok(
            RuntimeDrainObligationV2::ExactLocalRoute(decode_local_route(route)?),
        ),
        DrainObligationWireV2::PreviousServing { previous } => Ok(
            RuntimeDrainObligationV2::PreviousServing(decode_previous(previous)?),
        ),
        DrainObligationWireV2::LocalAndPrevious { local, previous } => {
            Ok(RuntimeDrainObligationV2::LocalAndPrevious {
                local: decode_local_route(local)?,
                previous: decode_previous(previous)?,
            })
        }
    }
}

fn encode_local_route(
    route: &RuntimeExactLocalRouteIdentityV2,
) -> Result<ExactLocalRouteWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(ExactLocalRouteWireV2 {
        identity: encode_process_identity(
            &route.identity,
            LOCAL_TARGET_FIELDS,
            RuntimeSuspendAttemptCanonicalFieldV2::LocalRuntimeGeneration,
        )?,
        controller_fencing_token: persistence_u64(
            route.controller_fencing_token.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::LocalControllerFencingToken,
        )?,
        route_incarnation: persistence_u64(
            route.route_incarnation.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::LocalRouteIncarnation,
        )?,
    })
}

fn decode_local_route(
    wire: ExactLocalRouteWireV2,
) -> Result<RuntimeExactLocalRouteIdentityV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimeExactLocalRouteIdentityV2 {
        identity: decode_process_identity(
            wire.identity,
            LOCAL_TARGET_FIELDS,
            RuntimeSuspendAttemptCanonicalFieldV2::LocalRuntimeGeneration,
            RuntimeSuspendAttemptCanonicalFieldV2::LocalProcessInstanceId,
        )?,
        controller_fencing_token: FencingToken::new(persistence_u64(
            wire.controller_fencing_token,
            RuntimeSuspendAttemptCanonicalFieldV2::LocalControllerFencingToken,
        )?)
        .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::LocalControllerFencingToken))?,
        route_incarnation: non_zero_u64(
            wire.route_incarnation,
            RuntimeSuspendAttemptCanonicalFieldV2::LocalRouteIncarnation,
        )?,
    })
}

fn encode_previous(
    previous: &RuntimePreviousServingLeaseIdentityV1,
) -> Result<PreviousServingIdentityWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(PreviousServingIdentityWireV2 {
        scope: encode_scope(&previous.scope),
        attestation_id: previous.attestation_id.as_str().to_owned(),
        process: encode_process_identity(
            &previous.process,
            PREVIOUS_TARGET_FIELDS,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousRuntimeGeneration,
        )?,
        lease_epoch: persistence_u64(
            previous.lease_epoch.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousLeaseEpoch,
        )?,
        revision: persistence_u64(
            previous.revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousRevision,
        )?,
    })
}

fn decode_previous(
    wire: PreviousServingIdentityWireV2,
) -> Result<RuntimePreviousServingLeaseIdentityV1, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimePreviousServingLeaseIdentityV1 {
        scope: decode_scope(
            wire.scope,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousTenantId,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousInstallationId,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousDeploymentId,
        )?,
        attestation_id: crate::RuntimeAttestationIdV1::parse(wire.attestation_id)
            .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::PreviousAttestationId))?,
        process: decode_process_identity(
            wire.process,
            PREVIOUS_TARGET_FIELDS,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousRuntimeGeneration,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousProcessInstanceId,
        )?,
        lease_epoch: non_zero_u64(
            wire.lease_epoch,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousLeaseEpoch,
        )?,
        revision: non_zero_u64(
            wire.revision,
            RuntimeSuspendAttemptCanonicalFieldV2::PreviousRevision,
        )?,
    })
}

fn encode_process_identity(
    process: &RuntimeProcessIdentityV1,
    target_fields: TargetFields,
    generation_field: RuntimeSuspendAttemptCanonicalFieldV2,
) -> Result<ProcessIdentityWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(ProcessIdentityWireV2 {
        target: encode_target(&process.target, target_fields)?,
        runtime_generation: persistence_u64(process.runtime_generation.get(), generation_field)?,
        process_instance_id: process.process_instance_id.as_str().to_owned(),
    })
}

fn decode_process_identity(
    wire: ProcessIdentityWireV2,
    target_fields: TargetFields,
    generation_field: RuntimeSuspendAttemptCanonicalFieldV2,
    process_field: RuntimeSuspendAttemptCanonicalFieldV2,
) -> Result<RuntimeProcessIdentityV1, RuntimeSuspendAttemptCanonicalErrorV2> {
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

fn encode_target(
    target: &RuntimeDeploymentTargetV1,
    fields: TargetFields,
) -> Result<DeploymentTargetWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
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
) -> Result<RuntimeDeploymentTargetV1, RuntimeSuspendAttemptCanonicalErrorV2> {
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

fn encode_slot(
    slot: &RuntimeServingSlotV2,
) -> Result<ServingSlotWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::from_u64(slot.guild_id.0).map_err(|reason| {
        canonical(
            RuntimeSuspendAttemptCanonicalFieldV2::RouteAbsentSlotGuildId,
            reason,
        )
    })?;
    Ok(ServingSlotWireV2 {
        guild_id: guild_id.canonical_text(),
        ruleset_key: slot.ruleset_key.as_str().to_owned(),
    })
}

fn decode_slot(
    wire: ServingSlotWireV2,
) -> Result<RuntimeServingSlotV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::parse_text(&wire.guild_id).map_err(|reason| {
        canonical(
            RuntimeSuspendAttemptCanonicalFieldV2::RouteAbsentSlotGuildId,
            reason,
        )
    })?;
    let ruleset_key = RuleSetKey::parse(&wire.ruleset_key)
        .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::RouteAbsentSlotRuleSetKey))?;
    Ok(RuntimeServingSlotV2::new(
        GuildId(guild_id.get_u64()),
        ruleset_key,
    ))
}

fn encode_provenance(
    provenance: &RuntimeRouteMutationProvenanceV2,
) -> Result<RouteMutationProvenanceWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match provenance {
        RuntimeRouteMutationProvenanceV2::Ordinary { barrier_id, pause } => {
            Ok(RouteMutationProvenanceWireV2::Ordinary {
                barrier_id: barrier_id.as_str().to_owned(),
                pause: encode_barrier_pause(pause)?,
            })
        }
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => {
            Ok(RouteMutationProvenanceWireV2::ClosedRecovery {
                witness: encode_closed_recovery(witness)?,
            })
        }
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => {
            Ok(RouteMutationProvenanceWireV2::Shutdown {
                witness: encode_shutdown(witness)?,
            })
        }
    }
}

fn decode_provenance(
    wire: RouteMutationProvenanceWireV2,
) -> Result<RuntimeRouteMutationProvenanceV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match wire {
        RouteMutationProvenanceWireV2::Ordinary { barrier_id, pause } => {
            Ok(RuntimeRouteMutationProvenanceV2::Ordinary {
                barrier_id: RuntimeBarrierIdV1::parse(barrier_id)
                    .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::BarrierId))?,
                pause: decode_barrier_pause(pause)?,
            })
        }
        RouteMutationProvenanceWireV2::ClosedRecovery { witness } => Ok(
            RuntimeRouteMutationProvenanceV2::ClosedRecovery(decode_closed_recovery(witness)?),
        ),
        RouteMutationProvenanceWireV2::Shutdown { witness } => Ok(
            RuntimeRouteMutationProvenanceV2::Shutdown(decode_shutdown(witness)?),
        ),
    }
}

fn encode_barrier_pause(
    pause: &RuntimeBarrierPauseWitnessV2,
) -> Result<BarrierPauseWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(BarrierPauseWireV2 {
        coordinator_generation: persistence_u64(
            pause.coordinator_generation.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::PauseCoordinatorGeneration,
        )?,
        connection_epoch: persistence_u64(
            pause.connection_epoch.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::PauseConnectionEpoch,
        )?,
        paused_admission_revision: persistence_u64(
            pause.paused_admission_revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::PauseAdmissionRevision,
        )?,
        pause_sequence: persistence_u64(
            pause.pause_sequence.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::PauseSequence,
        )?,
    })
}

fn decode_barrier_pause(
    wire: BarrierPauseWireV2,
) -> Result<RuntimeBarrierPauseWitnessV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimeBarrierPauseWitnessV2 {
        coordinator_generation: non_zero_u64(
            wire.coordinator_generation,
            RuntimeSuspendAttemptCanonicalFieldV2::PauseCoordinatorGeneration,
        )?,
        connection_epoch: non_zero_u64(
            wire.connection_epoch,
            RuntimeSuspendAttemptCanonicalFieldV2::PauseConnectionEpoch,
        )?,
        paused_admission_revision: non_zero_u64(
            wire.paused_admission_revision,
            RuntimeSuspendAttemptCanonicalFieldV2::PauseAdmissionRevision,
        )?,
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero_u64(
            wire.pause_sequence,
            RuntimeSuspendAttemptCanonicalFieldV2::PauseSequence,
        )?),
    })
}

fn encode_closed_recovery(
    witness: &RuntimeClosedRecoveryRouteWitnessV2,
) -> Result<ClosedRecoveryRouteWitnessWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(ClosedRecoveryRouteWitnessWireV2 {
        recovery_id: witness.recovery_id.as_str().to_owned(),
        originating_emergency_generation: persistence_u64(
            witness.originating_emergency_generation.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::OriginatingEmergencyGeneration,
        )?,
        recovery_generation: persistence_u64(
            witness.recovery_generation.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::RecoveryGeneration,
        )?,
        recovery_authority_revision: persistence_u64(
            witness.recovery_authority_revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::RecoveryAuthorityRevision,
        )?,
        gateway_owner_lease_id: encode_owner_lease(&witness.gateway_owner_lease_id)?,
        observed_owner_revision: persistence_u64(
            witness.observed_owner_revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ObservedOwnerRevision,
        )?,
        owner_expires_at_unix_microseconds: unix_microseconds(
            witness.owner_expires_at,
            RuntimeSuspendAttemptCanonicalFieldV2::OwnerExpiresAtUnixMicroseconds,
        )?,
        process_instance_id: witness.process_instance_id.as_str().to_owned(),
        connection_epoch: persistence_u64(
            witness.connection_epoch.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenanceConnectionEpoch,
        )?,
        paused_admission_revision: persistence_u64(
            witness.paused_admission_revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePausedAdmissionRevision,
        )?,
        connected_event_sequence: persistence_u64(
            witness.connected_event_sequence.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ConnectedEventSequence,
        )?,
        pause_sequence: persistence_u64(
            witness.pause_sequence.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePauseSequence,
        )?,
    })
}

fn decode_closed_recovery(
    wire: ClosedRecoveryRouteWitnessWireV2,
) -> Result<RuntimeClosedRecoveryRouteWitnessV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse(wire.recovery_id)
            .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::RecoveryId))?,
        originating_emergency_generation: non_zero_u64(
            wire.originating_emergency_generation,
            RuntimeSuspendAttemptCanonicalFieldV2::OriginatingEmergencyGeneration,
        )?,
        recovery_generation: non_zero_u64(
            wire.recovery_generation,
            RuntimeSuspendAttemptCanonicalFieldV2::RecoveryGeneration,
        )?,
        recovery_authority_revision: non_zero_u64(
            wire.recovery_authority_revision,
            RuntimeSuspendAttemptCanonicalFieldV2::RecoveryAuthorityRevision,
        )?,
        gateway_owner_lease_id: decode_owner_lease(wire.gateway_owner_lease_id)?,
        observed_owner_revision: non_zero_u64(
            wire.observed_owner_revision,
            RuntimeSuspendAttemptCanonicalFieldV2::ObservedOwnerRevision,
        )?,
        owner_expires_at: decode_unix_microseconds(
            wire.owner_expires_at_unix_microseconds,
            RuntimeSuspendAttemptCanonicalFieldV2::OwnerExpiresAtUnixMicroseconds,
        )?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id).map_err(|_| {
            invalid(RuntimeSuspendAttemptCanonicalFieldV2::ProvenanceProcessInstanceId)
        })?,
        connection_epoch: non_zero_u64(
            wire.connection_epoch,
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenanceConnectionEpoch,
        )?,
        paused_admission_revision: non_zero_u64(
            wire.paused_admission_revision,
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePausedAdmissionRevision,
        )?,
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero_u64(
            wire.connected_event_sequence,
            RuntimeSuspendAttemptCanonicalFieldV2::ConnectedEventSequence,
        )?),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero_u64(
            wire.pause_sequence,
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePauseSequence,
        )?),
    })
}

fn encode_shutdown(
    witness: &RuntimeShutdownRouteWitnessV2,
) -> Result<ShutdownRouteWitnessWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(ShutdownRouteWitnessWireV2 {
        shutdown_generation: persistence_u64(
            witness.shutdown_generation.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ShutdownGeneration,
        )?,
        gateway_owner_lease_id: encode_owner_lease(&witness.gateway_owner_lease_id)?,
        observed_owner_revision: persistence_u64(
            witness.observed_owner_revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ObservedOwnerRevision,
        )?,
        owner_expires_at_unix_microseconds: unix_microseconds(
            witness.owner_expires_at,
            RuntimeSuspendAttemptCanonicalFieldV2::OwnerExpiresAtUnixMicroseconds,
        )?,
        process_instance_id: witness.process_instance_id.as_str().to_owned(),
        connection_epoch: persistence_u64(
            witness.connection_epoch.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenanceConnectionEpoch,
        )?,
        paused_admission_revision: persistence_u64(
            witness.paused_admission_revision.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePausedAdmissionRevision,
        )?,
        connected_event_sequence: persistence_u64(
            witness.connected_event_sequence.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ConnectedEventSequence,
        )?,
        pause_sequence: persistence_u64(
            witness.pause_sequence.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePauseSequence,
        )?,
    })
}

fn decode_shutdown(
    wire: ShutdownRouteWitnessWireV2,
) -> Result<RuntimeShutdownRouteWitnessV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimeShutdownRouteWitnessV2 {
        shutdown_generation: non_zero_u64(
            wire.shutdown_generation,
            RuntimeSuspendAttemptCanonicalFieldV2::ShutdownGeneration,
        )?,
        gateway_owner_lease_id: decode_owner_lease(wire.gateway_owner_lease_id)?,
        observed_owner_revision: non_zero_u64(
            wire.observed_owner_revision,
            RuntimeSuspendAttemptCanonicalFieldV2::ObservedOwnerRevision,
        )?,
        owner_expires_at: decode_unix_microseconds(
            wire.owner_expires_at_unix_microseconds,
            RuntimeSuspendAttemptCanonicalFieldV2::OwnerExpiresAtUnixMicroseconds,
        )?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id).map_err(|_| {
            invalid(RuntimeSuspendAttemptCanonicalFieldV2::ProvenanceProcessInstanceId)
        })?,
        connection_epoch: non_zero_u64(
            wire.connection_epoch,
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenanceConnectionEpoch,
        )?,
        paused_admission_revision: non_zero_u64(
            wire.paused_admission_revision,
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePausedAdmissionRevision,
        )?,
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero_u64(
            wire.connected_event_sequence,
            RuntimeSuspendAttemptCanonicalFieldV2::ConnectedEventSequence,
        )?),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero_u64(
            wire.pause_sequence,
            RuntimeSuspendAttemptCanonicalFieldV2::ProvenancePauseSequence,
        )?),
    })
}

fn encode_owner_lease(
    lease: &RuntimeGatewayOwnerLeaseIdV1,
) -> Result<GatewayOwnerLeaseIdWireV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(GatewayOwnerLeaseIdWireV2 {
        gateway_shard_id: lease.gateway_shard_id.as_str().to_owned(),
        process_instance_id: lease.process_instance_id.as_str().to_owned(),
        lease_epoch: persistence_u64(
            lease.lease_epoch.get(),
            RuntimeSuspendAttemptCanonicalFieldV2::GatewayLeaseEpoch,
        )?,
        expected_build_revision: lease.expected_build_revision.as_str().to_owned(),
    })
}

fn decode_owner_lease(
    wire: GatewayOwnerLeaseIdWireV2,
) -> Result<RuntimeGatewayOwnerLeaseIdV1, RuntimeSuspendAttemptCanonicalErrorV2> {
    Ok(RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse(wire.gateway_shard_id)
            .map_err(|_| invalid(RuntimeSuspendAttemptCanonicalFieldV2::GatewayShardId))?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id).map_err(|_| {
            invalid(RuntimeSuspendAttemptCanonicalFieldV2::GatewayProcessInstanceId)
        })?,
        lease_epoch: non_zero_u64(
            wire.lease_epoch,
            RuntimeSuspendAttemptCanonicalFieldV2::GatewayLeaseEpoch,
        )?,
        expected_build_revision: RuntimeBuildRevisionV1::parse(wire.expected_build_revision)
            .map_err(|_| {
                invalid(RuntimeSuspendAttemptCanonicalFieldV2::GatewayExpectedBuildRevision)
            })?,
    })
}

fn source_phase_tag(value: RuntimeSuspensionSourcePhaseV2) -> &'static str {
    match value {
        RuntimeSuspensionSourcePhaseV2::Requested => "requested",
        RuntimeSuspensionSourcePhaseV2::PreflightReady => "preflight_ready",
        RuntimeSuspensionSourcePhaseV2::DrainRequested => "drain_requested",
        RuntimeSuspensionSourcePhaseV2::Drained => "drained",
        RuntimeSuspensionSourcePhaseV2::ActivationApplying => "activation_applying",
        RuntimeSuspensionSourcePhaseV2::RuntimePendingReady => "runtime_pending_ready",
        RuntimeSuspensionSourcePhaseV2::ReconcilingPanels => "reconciling_panels",
    }
}

fn decode_source_phase(
    value: &str,
) -> Result<RuntimeSuspensionSourcePhaseV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match value {
        "requested" => Ok(RuntimeSuspensionSourcePhaseV2::Requested),
        "preflight_ready" => Ok(RuntimeSuspensionSourcePhaseV2::PreflightReady),
        "drain_requested" => Ok(RuntimeSuspensionSourcePhaseV2::DrainRequested),
        "drained" => Ok(RuntimeSuspensionSourcePhaseV2::Drained),
        "activation_applying" => Ok(RuntimeSuspensionSourcePhaseV2::ActivationApplying),
        "runtime_pending_ready" => Ok(RuntimeSuspensionSourcePhaseV2::RuntimePendingReady),
        "reconciling_panels" => Ok(RuntimeSuspensionSourcePhaseV2::ReconcilingPanels),
        _ => Err(invalid(RuntimeSuspendAttemptCanonicalFieldV2::SourcePhase)),
    }
}

fn checkpoint_tag(value: RuntimeResumeCheckpointV2) -> &'static str {
    match value {
        RuntimeResumeCheckpointV2::VerifyPreflight => "verify_preflight",
        RuntimeResumeCheckpointV2::RequestDrain => "request_drain",
        RuntimeResumeCheckpointV2::CompleteDrain => "complete_drain",
        RuntimeResumeCheckpointV2::BeginActivation => "begin_activation",
        RuntimeResumeCheckpointV2::ObserveActivation => "observe_activation",
        RuntimeResumeCheckpointV2::BeginPanels => "begin_panels",
        RuntimeResumeCheckpointV2::ReconcilePanels => "reconcile_panels",
    }
}

fn decode_checkpoint(
    value: &str,
) -> Result<RuntimeResumeCheckpointV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match value {
        "verify_preflight" => Ok(RuntimeResumeCheckpointV2::VerifyPreflight),
        "request_drain" => Ok(RuntimeResumeCheckpointV2::RequestDrain),
        "complete_drain" => Ok(RuntimeResumeCheckpointV2::CompleteDrain),
        "begin_activation" => Ok(RuntimeResumeCheckpointV2::BeginActivation),
        "observe_activation" => Ok(RuntimeResumeCheckpointV2::ObserveActivation),
        "begin_panels" => Ok(RuntimeResumeCheckpointV2::BeginPanels),
        "reconcile_panels" => Ok(RuntimeResumeCheckpointV2::ReconcilePanels),
        _ => Err(invalid(RuntimeSuspendAttemptCanonicalFieldV2::Checkpoint)),
    }
}

fn failure_kind_tag(value: RuntimeFailureKindV1) -> &'static str {
    match value {
        RuntimeFailureKindV1::EnvironmentUnavailable => "environment_unavailable",
        RuntimeFailureKindV1::ActivationNotObservable => "activation_not_observable",
        RuntimeFailureKindV1::PanelReconciliation => "panel_reconciliation",
        RuntimeFailureKindV1::GatewayStart => "gateway_start",
        RuntimeFailureKindV1::GatewayReadyTimeout => "gateway_ready_timeout",
        RuntimeFailureKindV1::InvariantViolation => "invariant_violation",
    }
}

fn decode_failure_kind(
    value: &str,
) -> Result<RuntimeFailureKindV1, RuntimeSuspendAttemptCanonicalErrorV2> {
    match value {
        "environment_unavailable" => Ok(RuntimeFailureKindV1::EnvironmentUnavailable),
        "activation_not_observable" => Ok(RuntimeFailureKindV1::ActivationNotObservable),
        "panel_reconciliation" => Ok(RuntimeFailureKindV1::PanelReconciliation),
        "gateway_start" => Ok(RuntimeFailureKindV1::GatewayStart),
        "gateway_ready_timeout" => Ok(RuntimeFailureKindV1::GatewayReadyTimeout),
        "invariant_violation" => Ok(RuntimeFailureKindV1::InvariantViolation),
        _ => Err(invalid(RuntimeSuspendAttemptCanonicalFieldV2::FailureKind)),
    }
}

fn route_lifecycle_tag(value: RuntimeSuspendedRouteLifecycleV2) -> &'static str {
    match value {
        RuntimeSuspendedRouteLifecycleV2::Staged => "staged",
        RuntimeSuspendedRouteLifecycleV2::Draining => "draining",
    }
}

fn decode_route_lifecycle(
    value: &str,
) -> Result<RuntimeSuspendedRouteLifecycleV2, RuntimeSuspendAttemptCanonicalErrorV2> {
    match value {
        "staged" => Ok(RuntimeSuspendedRouteLifecycleV2::Staged),
        "draining" => Ok(RuntimeSuspendedRouteLifecycleV2::Draining),
        _ => Err(invalid(
            RuntimeSuspendAttemptCanonicalFieldV2::RouteLifecycle,
        )),
    }
}

fn persistence_u64(
    value: u64,
    field: RuntimeSuspendAttemptCanonicalFieldV2,
) -> Result<u64, RuntimeSuspendAttemptCanonicalErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(RuntimePersistenceU64V2::get_u64)
        .map_err(|reason| canonical(field, reason))
}

fn non_zero_u64(
    value: u64,
    field: RuntimeSuspendAttemptCanonicalFieldV2,
) -> Result<NonZeroU64, RuntimeSuspendAttemptCanonicalErrorV2> {
    NonZeroU64::new(persistence_u64(value, field)?).ok_or_else(|| invalid(field))
}

fn unix_microseconds(
    value: DateTime<Utc>,
    field: RuntimeSuspendAttemptCanonicalFieldV2,
) -> Result<i64, RuntimeSuspendAttemptCanonicalErrorV2> {
    RuntimeUnixMicrosecondsV2::from_datetime(value)
        .map(RuntimeUnixMicrosecondsV2::get)
        .map_err(|reason| canonical(field, reason))
}

fn decode_unix_microseconds(
    value: i64,
    field: RuntimeSuspendAttemptCanonicalFieldV2,
) -> Result<DateTime<Utc>, RuntimeSuspendAttemptCanonicalErrorV2> {
    RuntimeUnixMicrosecondsV2::from_i64(value)
        .map(RuntimeUnixMicrosecondsV2::to_datetime)
        .map_err(|reason| canonical(field, reason))
}

fn encode_root<T: Serialize>(wire: &T) -> Result<Vec<u8>, RuntimeSuspendAttemptCanonicalErrorV2> {
    let encoded =
        serde_json::to_vec(wire).map_err(|_| RuntimeSuspendAttemptCanonicalErrorV2::Encoding)?;
    ensure_size(&encoded)?;
    Ok(encoded)
}

fn ensure_size(encoded: &[u8]) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    if encoded.len() > SUSPEND_ATTEMPT_MAX_OCTETS {
        return Err(RuntimeSuspendAttemptCanonicalErrorV2::PayloadTooLarge);
    }
    Ok(())
}

fn invalid(field: RuntimeSuspendAttemptCanonicalFieldV2) -> RuntimeSuspendAttemptCanonicalErrorV2 {
    RuntimeSuspendAttemptCanonicalErrorV2::InvalidField { field }
}

fn canonical(
    field: RuntimeSuspendAttemptCanonicalFieldV2,
    reason: RuntimeCanonicalValueErrorV2,
) -> RuntimeSuspendAttemptCanonicalErrorV2 {
    RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue { field, reason }
}

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
