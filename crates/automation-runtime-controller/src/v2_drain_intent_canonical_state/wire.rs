use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentRevision, FencingToken, InstallationId,
    ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1,
    TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde::{Deserialize, Deserializer, Serialize};

use super::{
    canonical_state_kind, RuntimeDrainIntentCanonicalStateCorrelationV2,
    RuntimeDrainIntentCanonicalStateErrorV2, RuntimeDrainIntentCanonicalStateFieldV2,
    DRAIN_INTENT_STATE_MAX_OCTETS,
};
use crate::v2_canonical_value::{
    RuntimeDiscordSnowflakeV2, RuntimePersistenceU64V2, RuntimeUnixMicrosecondsV2,
};
use crate::v2_drain_claim::{
    validate_drain_claim_for_key, validate_route_absent_acknowledgement_for_key,
};
use crate::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCanonicalRouteMutationProvenanceV2,
    RuntimeCanonicalValueErrorV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1,
    RuntimeDrainCertificationResolutionKindV2, RuntimeDrainCertificationResolutionV2,
    RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimProgressV2, RuntimeDrainClaimSealWitnessV2,
    RuntimeDrainClaimV2, RuntimeDrainIntentDigestV2, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentKeyV2, RuntimeDrainIntentV2, RuntimeExactLocalRouteIdentityV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeLiveAttestationDigestV2,
    RuntimePersistedProductDrainRootV2, RuntimeProductMutationDigestV2,
    RuntimeProductMutationKindV2, RuntimeProductOperationIdV2, RuntimeRouteAbsentAcknowledgementV2,
    RuntimeServingIdentityV2, RuntimeServingSlotV2,
};

const FORMAT_VERSION: u8 = 2;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DrainIntentStateRootWireV2 {
    format_version: u8,
    root: DrainIntentRootBindingWireV2,
    intent_revision: u64,
    state: DrainIntentStateWireV2,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DrainIntentRootBindingWireV2 {
    key: DrainIntentKeyWireV2,
    drain_intent_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DrainIntentKeyWireV2 {
    intent_id: String,
    product_operation_id: String,
    product_mutation_digest: String,
    scope: DeploymentScopeWireV2,
    expected_revision: u64,
    slot: ServingSlotWireV2,
    expected_target: DeploymentTargetWireV2,
    mutation_kind: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum DrainIntentStateWireV2 {
    #[serde(rename = "pending_unclaimed")]
    PendingUnclaimed {},
    #[serde(rename = "pending_claimed")]
    PendingClaimed { claim: Box<DrainClaimWireV2> },
    #[serde(rename = "pending_refenced")]
    PendingRefenced { claim: Box<DrainClaimWireV2> },
    #[serde(rename = "route_absent_acknowledged")]
    RouteAbsentAcknowledged {
        acknowledgement: Box<RouteAbsentAcknowledgementWireV2>,
    },
    #[serde(rename = "consumed")]
    Consumed {
        resulting_revision: u64,
        consumed_at_unix_microseconds: i64,
    },
    #[serde(rename = "cancelled")]
    Cancelled { cancelled_at_unix_microseconds: i64 },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DrainClaimWireV2 {
    gateway_owner_lease_id: GatewayOwnerLeaseIdWireV2,
    observed_owner_revision: u64,
    process_instance_id: String,
    controller_id: String,
    controller_fencing_token: u64,
    claim_epoch: u64,
    claim_revision: u64,
    claim_expires_at_unix_microseconds: i64,
    progress: DrainClaimProgressWireV2,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum DrainClaimProgressWireV2 {
    #[serde(rename = "claimed")]
    Claimed { seal: Box<DrainClaimSealWireV2> },
    #[serde(rename = "refenced")]
    Refenced {
        seal: Box<DrainClaimSealWireV2>,
        provenance_json: String,
        old_route: Box<ExactLocalRouteWireV2>,
        removal_target: Box<ExactLocalRouteWireV2>,
        registry_observation_sequence: u64,
        refenced_at_unix_microseconds: i64,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DrainClaimSealWireV2 {
    process_instance_id: String,
    slot: ServingSlotWireV2,
    intent_id: String,
    seal_generation: u64,
    #[serde(deserialize_with = "deserialize_required_option")]
    expected_route: Option<ExactLocalRouteWireV2>,
    registry_observation_sequence: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteAbsentAcknowledgementWireV2 {
    claim: DrainClaimWireV2,
    #[serde(deserialize_with = "deserialize_required_option")]
    expected_route: Option<ExactLocalRouteWireV2>,
    provenance_json: String,
    registry_observation_sequence: u64,
    certification: DrainCertificationResolutionWireV2,
    acknowledged_at_unix_microseconds: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum DrainCertificationResolutionWireV2 {
    #[serde(rename = "no_operation_reserved")]
    NoOperationReserved {},
    #[serde(rename = "no_attestation_for_reserved_operation")]
    NoAttestationForReservedOperation {
        operation_id: String,
        intent_fingerprint: String,
    },
    #[serde(rename = "committed_and_disconnected")]
    CommittedAndDisconnected {
        operation_id: String,
        serving_identity: Box<ServingIdentityWireV2>,
        disconnected_revision: u64,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServingIdentityWireV2 {
    scope: DeploymentScopeWireV2,
    operation_id: String,
    attestation_digest: String,
    process_identity: ProcessIdentityWireV2,
    lease_epoch: u64,
    revision: u64,
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
pub(crate) struct ProcessIdentityWireV2 {
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
struct DeploymentScopeWireV2 {
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServingSlotWireV2 {
    guild_id: String,
    ruleset_key: String,
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

pub(super) fn encode_state(
    intent: &RuntimeDrainIntentV2,
) -> Result<Vec<u8>, RuntimeDrainIntentCanonicalStateErrorV2> {
    let wire = DrainIntentStateRootWireV2 {
        format_version: FORMAT_VERSION,
        root: encode_root_binding(intent)?,
        intent_revision: persistence_u64(
            intent.intent_revision().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
        )?,
        state: encode_intent_state(intent)?,
    };
    encode_root(&wire)
}

pub(super) fn decode_state(
    root: &RuntimePersistedProductDrainRootV2,
    intent_revision: NonZeroU64,
    encoded: &[u8],
) -> Result<RuntimeDrainIntentV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    ensure_size(encoded)?;
    let wire = serde_json::from_slice::<DrainIntentStateRootWireV2>(encoded)
        .map_err(|_| RuntimeDrainIntentCanonicalStateErrorV2::Decoding)?;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::UnsupportedFormatVersion);
    }
    let decoded_revision = non_zero(
        wire.intent_revision,
        RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
    )?;
    if decoded_revision != intent_revision {
        return Err(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::ImmutableRoot,
            },
        );
    }
    validate_root_binding(root, wire.root)?;
    let intent = decode_intent_state(root, intent_revision, wire.state)?;
    if encode_state(&intent)? != encoded {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::NonCanonicalEncoding);
    }
    Ok(intent)
}

pub(super) struct RuntimeCompactSuccessionSuccessorV2 {
    pub key: RuntimeDrainIntentKeyV2,
    pub drain_intent_digest: RuntimeDrainIntentDigestV2,
    pub intent_revision: NonZeroU64,
    pub acknowledgement: RuntimeRouteAbsentAcknowledgementV2,
}

pub(super) fn encode_compact_pending_unclaimed_source_v2(
    key: &RuntimeDrainIntentKeyV2,
    drain_intent_digest: &RuntimeDrainIntentDigestV2,
    intent_revision: NonZeroU64,
) -> Result<Vec<u8>, RuntimeDrainIntentCanonicalStateErrorV2> {
    encode_compact_state_root_v2(
        key,
        drain_intent_digest,
        intent_revision,
        DrainIntentStateWireV2::PendingUnclaimed {},
    )
}

pub(super) fn encode_compact_pending_claimed_source_v2(
    key: &RuntimeDrainIntentKeyV2,
    drain_intent_digest: &RuntimeDrainIntentDigestV2,
    intent_revision: NonZeroU64,
    predecessor_claim: &RuntimeDrainClaimV2,
) -> Result<Vec<u8>, RuntimeDrainIntentCanonicalStateErrorV2> {
    validate_drain_claim_for_key(predecessor_claim, key)?;
    if predecessor_claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed
        || predecessor_claim
            .progress()
            .seal()
            .expected_route()
            .is_some()
    {
        return Err(correlation(
            RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
        ));
    }
    encode_compact_state_root_v2(
        key,
        drain_intent_digest,
        intent_revision,
        DrainIntentStateWireV2::PendingClaimed {
            claim: Box::new(encode_claim(predecessor_claim)?),
        },
    )
}

pub(super) fn decode_compact_succession_successor_v2(
    encoded: &[u8],
) -> Result<RuntimeCompactSuccessionSuccessorV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    ensure_size(encoded)?;
    let wire = serde_json::from_slice::<DrainIntentStateRootWireV2>(encoded)
        .map_err(|_| RuntimeDrainIntentCanonicalStateErrorV2::Decoding)?;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::UnsupportedFormatVersion);
    }
    let intent_revision = non_zero(
        wire.intent_revision,
        RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
    )?;
    let (key, drain_intent_digest) = decode_compact_root_binding_v2(wire.root)?;
    let acknowledgement = match wire.state {
        DrainIntentStateWireV2::RouteAbsentAcknowledged { acknowledgement } => {
            decode_acknowledgement(&key, *acknowledgement)?
        }
        DrainIntentStateWireV2::PendingUnclaimed {}
        | DrainIntentStateWireV2::PendingClaimed { .. }
        | DrainIntentStateWireV2::PendingRefenced { .. }
        | DrainIntentStateWireV2::Consumed { .. }
        | DrainIntentStateWireV2::Cancelled { .. } => {
            return Err(correlation(
                RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
            ));
        }
    };
    let canonical = encode_compact_route_absent_acknowledged_v2(
        &key,
        &drain_intent_digest,
        intent_revision,
        &acknowledgement,
    )?;
    if canonical != encoded {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::NonCanonicalEncoding);
    }
    Ok(RuntimeCompactSuccessionSuccessorV2 {
        key,
        drain_intent_digest,
        intent_revision,
        acknowledgement,
    })
}

fn encode_compact_route_absent_acknowledged_v2(
    key: &RuntimeDrainIntentKeyV2,
    drain_intent_digest: &RuntimeDrainIntentDigestV2,
    intent_revision: NonZeroU64,
    acknowledgement: &RuntimeRouteAbsentAcknowledgementV2,
) -> Result<Vec<u8>, RuntimeDrainIntentCanonicalStateErrorV2> {
    validate_route_absent_acknowledgement_for_key(acknowledgement, key)?;
    encode_compact_state_root_v2(
        key,
        drain_intent_digest,
        intent_revision,
        DrainIntentStateWireV2::RouteAbsentAcknowledged {
            acknowledgement: Box::new(encode_acknowledgement(acknowledgement)?),
        },
    )
}

fn encode_compact_state_root_v2(
    key: &RuntimeDrainIntentKeyV2,
    drain_intent_digest: &RuntimeDrainIntentDigestV2,
    intent_revision: NonZeroU64,
    state: DrainIntentStateWireV2,
) -> Result<Vec<u8>, RuntimeDrainIntentCanonicalStateErrorV2> {
    encode_root(&DrainIntentStateRootWireV2 {
        format_version: FORMAT_VERSION,
        root: encode_compact_root_binding_v2(key, drain_intent_digest)?,
        intent_revision: persistence_u64(
            intent_revision.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
        )?,
        state,
    })
}

pub(crate) fn encode_compact_root_binding_v2(
    key: &RuntimeDrainIntentKeyV2,
    drain_intent_digest: &RuntimeDrainIntentDigestV2,
) -> Result<DrainIntentRootBindingWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(DrainIntentRootBindingWireV2 {
        key: encode_key(key)?,
        drain_intent_digest: drain_intent_digest.as_str().to_owned(),
    })
}

pub(crate) fn decode_compact_root_binding_v2(
    wire: DrainIntentRootBindingWireV2,
) -> Result<
    (RuntimeDrainIntentKeyV2, RuntimeDrainIntentDigestV2),
    RuntimeDrainIntentCanonicalStateErrorV2,
> {
    let key = decode_key(wire.key)?;
    let drain_intent_digest = RuntimeDrainIntentDigestV2::parse(wire.drain_intent_digest)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::DrainIntentDigest))?;
    Ok((key, drain_intent_digest))
}

fn encode_root_binding(
    intent: &RuntimeDrainIntentV2,
) -> Result<DrainIntentRootBindingWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(DrainIntentRootBindingWireV2 {
        key: encode_key(intent.key())?,
        drain_intent_digest: intent.drain_intent_digest().as_str().to_owned(),
    })
}

fn validate_root_binding(
    root: &RuntimePersistedProductDrainRootV2,
    wire: DrainIntentRootBindingWireV2,
) -> Result<(), RuntimeDrainIntentCanonicalStateErrorV2> {
    let key = decode_key(wire.key)?;
    let drain_digest = RuntimeDrainIntentDigestV2::parse(wire.drain_intent_digest)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::DrainIntentDigest))?;
    if key != root.canonical().drain_preimage().key
        || drain_digest != *root.canonical().drain_intent_digest()
    {
        return Err(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::ImmutableRoot,
            },
        );
    }
    Ok(())
}

fn encode_intent_state(
    intent: &RuntimeDrainIntentV2,
) -> Result<DrainIntentStateWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match canonical_state_kind(intent)? {
        super::RuntimeDrainIntentCanonicalStateKindV2::PendingUnclaimed => {
            Ok(DrainIntentStateWireV2::PendingUnclaimed {})
        }
        super::RuntimeDrainIntentCanonicalStateKindV2::PendingClaimed => {
            let claim = intent.state().pending_claim().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(DrainIntentStateWireV2::PendingClaimed {
                claim: Box::new(encode_claim(claim)?),
            })
        }
        super::RuntimeDrainIntentCanonicalStateKindV2::PendingRefenced => {
            let claim = intent.state().pending_claim().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(DrainIntentStateWireV2::PendingRefenced {
                claim: Box::new(encode_claim(claim)?),
            })
        }
        super::RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged => {
            let acknowledgement = intent.state().acknowledgement().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(DrainIntentStateWireV2::RouteAbsentAcknowledged {
                acknowledgement: Box::new(encode_acknowledgement(acknowledgement)?),
            })
        }
        super::RuntimeDrainIntentCanonicalStateKindV2::Consumed => {
            let resulting_revision = intent.state().resulting_revision().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            let consumed_at = intent.state().consumed_at().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(DrainIntentStateWireV2::Consumed {
                resulting_revision: persistence_u64(
                    resulting_revision.get(),
                    RuntimeDrainIntentCanonicalStateFieldV2::ResultingRevision,
                )?,
                consumed_at_unix_microseconds: timestamp(
                    consumed_at,
                    RuntimeDrainIntentCanonicalStateFieldV2::ConsumedAt,
                )?,
            })
        }
        super::RuntimeDrainIntentCanonicalStateKindV2::Cancelled => {
            let cancelled_at = intent.state().cancelled_at().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(DrainIntentStateWireV2::Cancelled {
                cancelled_at_unix_microseconds: timestamp(
                    cancelled_at,
                    RuntimeDrainIntentCanonicalStateFieldV2::CancelledAt,
                )?,
            })
        }
    }
}

fn decode_intent_state(
    root: &RuntimePersistedProductDrainRootV2,
    intent_revision: NonZeroU64,
    state: DrainIntentStateWireV2,
) -> Result<RuntimeDrainIntentV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match state {
        DrainIntentStateWireV2::PendingUnclaimed {} => Ok(
            RuntimeDrainIntentV2::pending_from_persisted(root, intent_revision, None)?,
        ),
        DrainIntentStateWireV2::PendingClaimed { claim } => {
            let claim = decode_claim(&root.canonical().drain_preimage().key, *claim)?;
            if claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed {
                return Err(correlation(
                    RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
                ));
            }
            Ok(RuntimeDrainIntentV2::pending_from_persisted(
                root,
                intent_revision,
                Some(claim),
            )?)
        }
        DrainIntentStateWireV2::PendingRefenced { claim } => {
            let claim = decode_claim(&root.canonical().drain_preimage().key, *claim)?;
            if claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Refenced {
                return Err(correlation(
                    RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
                ));
            }
            Ok(RuntimeDrainIntentV2::pending_from_persisted(
                root,
                intent_revision,
                Some(claim),
            )?)
        }
        DrainIntentStateWireV2::RouteAbsentAcknowledged { acknowledgement } => {
            let acknowledgement =
                decode_acknowledgement(&root.canonical().drain_preimage().key, *acknowledgement)?;
            Ok(
                RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
                    root,
                    intent_revision,
                    acknowledgement,
                )?,
            )
        }
        DrainIntentStateWireV2::Consumed {
            resulting_revision,
            consumed_at_unix_microseconds,
        } => Ok(RuntimeDrainIntentV2::consumed_from_persisted(
            root,
            intent_revision,
            revision(
                resulting_revision,
                RuntimeDrainIntentCanonicalStateFieldV2::ResultingRevision,
            )?,
            decode_timestamp(
                consumed_at_unix_microseconds,
                RuntimeDrainIntentCanonicalStateFieldV2::ConsumedAt,
            )?,
        )?),
        DrainIntentStateWireV2::Cancelled {
            cancelled_at_unix_microseconds,
        } => Ok(RuntimeDrainIntentV2::cancelled_from_persisted(
            root,
            intent_revision,
            decode_timestamp(
                cancelled_at_unix_microseconds,
                RuntimeDrainIntentCanonicalStateFieldV2::CancelledAt,
            )?,
        )?),
    }
}

pub(crate) fn encode_claim(
    claim: &RuntimeDrainClaimV2,
) -> Result<DrainClaimWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(DrainClaimWireV2 {
        gateway_owner_lease_id: encode_owner_lease(claim.gateway_owner_lease_id())?,
        observed_owner_revision: persistence_u64(
            claim.observed_owner_revision().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::OwnerRevision,
        )?,
        process_instance_id: claim.process_instance_id().as_str().to_owned(),
        controller_id: claim.controller_id().as_str().to_owned(),
        controller_fencing_token: persistence_u64(
            claim.controller_fencing_token().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::ControllerFencingToken,
        )?,
        claim_epoch: persistence_u64(
            claim.claim_epoch().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::ClaimEpoch,
        )?,
        claim_revision: persistence_u64(
            claim.claim_revision().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::ClaimRevision,
        )?,
        claim_expires_at_unix_microseconds: timestamp(
            claim.expires_at(),
            RuntimeDrainIntentCanonicalStateFieldV2::ClaimExpiresAt,
        )?,
        progress: encode_claim_progress(claim.progress())?,
    })
}

pub(crate) fn decode_claim(
    key: &RuntimeDrainIntentKeyV2,
    wire: DrainClaimWireV2,
) -> Result<RuntimeDrainClaimV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    RuntimeDrainClaimV2::new(
        key,
        decode_owner_lease(wire.gateway_owner_lease_id)?,
        non_zero(
            wire.observed_owner_revision,
            RuntimeDrainIntentCanonicalStateFieldV2::OwnerRevision,
        )?,
        ProcessInstanceId::parse(wire.process_instance_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::ProcessInstanceId))?,
        ControllerId::parse(wire.controller_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::ControllerId))?,
        fencing_token(
            wire.controller_fencing_token,
            RuntimeDrainIntentCanonicalStateFieldV2::ControllerFencingToken,
        )?,
        non_zero(
            wire.claim_epoch,
            RuntimeDrainIntentCanonicalStateFieldV2::ClaimEpoch,
        )?,
        non_zero(
            wire.claim_revision,
            RuntimeDrainIntentCanonicalStateFieldV2::ClaimRevision,
        )?,
        decode_timestamp(
            wire.claim_expires_at_unix_microseconds,
            RuntimeDrainIntentCanonicalStateFieldV2::ClaimExpiresAt,
        )?,
        decode_claim_progress(key, wire.progress)?,
    )
    .map_err(Into::into)
}

fn encode_claim_progress(
    progress: &RuntimeDrainClaimProgressV2,
) -> Result<DrainClaimProgressWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match progress.kind() {
        RuntimeDrainClaimProgressKindV2::Claimed => Ok(DrainClaimProgressWireV2::Claimed {
            seal: Box::new(encode_seal(progress.seal())?),
        }),
        RuntimeDrainClaimProgressKindV2::Refenced => {
            let provenance = progress.provenance().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            let old_route = progress.old_route().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            let removal_target = progress.removal_target().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            let registry_observation_sequence =
                progress.registry_observation_sequence().ok_or_else(|| {
                    correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
                })?;
            let refenced_at = progress.refenced_at().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(DrainClaimProgressWireV2::Refenced {
                seal: Box::new(encode_seal(progress.seal())?),
                provenance_json: encode_provenance(provenance)?,
                old_route: Box::new(encode_route(old_route)?),
                removal_target: Box::new(encode_route(removal_target)?),
                registry_observation_sequence: persistence_u64(
                    registry_observation_sequence.get(),
                    RuntimeDrainIntentCanonicalStateFieldV2::RefenceObservationSequence,
                )?,
                refenced_at_unix_microseconds: timestamp(
                    refenced_at,
                    RuntimeDrainIntentCanonicalStateFieldV2::RefencedAt,
                )?,
            })
        }
    }
}

fn decode_claim_progress(
    key: &RuntimeDrainIntentKeyV2,
    wire: DrainClaimProgressWireV2,
) -> Result<RuntimeDrainClaimProgressV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match wire {
        DrainClaimProgressWireV2::Claimed { seal } => Ok(RuntimeDrainClaimProgressV2::claimed(
            decode_seal(key, *seal)?,
        )),
        DrainClaimProgressWireV2::Refenced {
            seal,
            provenance_json,
            old_route,
            removal_target,
            registry_observation_sequence,
            refenced_at_unix_microseconds,
        } => RuntimeDrainClaimProgressV2::refenced(
            decode_seal(key, *seal)?,
            decode_provenance(provenance_json)?,
            decode_route(*old_route)?,
            decode_route(*removal_target)?,
            non_zero(
                registry_observation_sequence,
                RuntimeDrainIntentCanonicalStateFieldV2::RefenceObservationSequence,
            )?,
            decode_timestamp(
                refenced_at_unix_microseconds,
                RuntimeDrainIntentCanonicalStateFieldV2::RefencedAt,
            )?,
        )
        .map_err(Into::into),
    }
}

fn encode_seal(
    seal: &RuntimeDrainClaimSealWitnessV2,
) -> Result<DrainClaimSealWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(DrainClaimSealWireV2 {
        process_instance_id: seal.process_instance_id().as_str().to_owned(),
        slot: encode_slot(seal.slot())?,
        intent_id: seal.intent_id().as_str().to_owned(),
        seal_generation: persistence_u64(
            seal.seal_generation().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::SealGeneration,
        )?,
        expected_route: seal.expected_route().map(encode_route).transpose()?,
        registry_observation_sequence: persistence_u64(
            seal.registry_observation_sequence().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::SealObservationSequence,
        )?,
    })
}

fn decode_seal(
    key: &RuntimeDrainIntentKeyV2,
    wire: DrainClaimSealWireV2,
) -> Result<RuntimeDrainClaimSealWitnessV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    let process_instance_id = ProcessInstanceId::parse(wire.process_instance_id)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::ProcessInstanceId))?;
    let slot = decode_slot(wire.slot)?;
    let intent_id = RuntimeDrainIntentIdV2::parse(wire.intent_id)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::DrainIntentId))?;
    if slot != key.slot || intent_id != key.intent_id {
        return Err(correlation(
            RuntimeDrainIntentCanonicalStateCorrelationV2::ImmutableRoot,
        ));
    }
    RuntimeDrainClaimSealWitnessV2::new(
        key,
        process_instance_id,
        non_zero(
            wire.seal_generation,
            RuntimeDrainIntentCanonicalStateFieldV2::SealGeneration,
        )?,
        wire.expected_route.map(decode_route).transpose()?,
        non_zero(
            wire.registry_observation_sequence,
            RuntimeDrainIntentCanonicalStateFieldV2::SealObservationSequence,
        )?,
    )
    .map_err(Into::into)
}

fn encode_acknowledgement(
    acknowledgement: &RuntimeRouteAbsentAcknowledgementV2,
) -> Result<RouteAbsentAcknowledgementWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(RouteAbsentAcknowledgementWireV2 {
        claim: encode_claim(acknowledgement.claim())?,
        expected_route: acknowledgement
            .expected_route()
            .map(encode_route)
            .transpose()?,
        provenance_json: encode_provenance(acknowledgement.provenance())?,
        registry_observation_sequence: persistence_u64(
            acknowledgement.registry_observation_sequence().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::AcknowledgementObservationSequence,
        )?,
        certification: encode_certification(acknowledgement.certification())?,
        acknowledged_at_unix_microseconds: timestamp(
            acknowledgement.acknowledged_at(),
            RuntimeDrainIntentCanonicalStateFieldV2::AcknowledgedAt,
        )?,
    })
}

fn decode_acknowledgement(
    key: &RuntimeDrainIntentKeyV2,
    wire: RouteAbsentAcknowledgementWireV2,
) -> Result<RuntimeRouteAbsentAcknowledgementV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    let claim = decode_claim(key, wire.claim)?;
    RuntimeRouteAbsentAcknowledgementV2::new(
        key,
        claim.clone(),
        wire.expected_route.map(decode_route).transpose()?,
        decode_provenance(wire.provenance_json)?,
        non_zero(
            wire.registry_observation_sequence,
            RuntimeDrainIntentCanonicalStateFieldV2::AcknowledgementObservationSequence,
        )?,
        decode_certification(key, &claim, wire.certification)?,
        decode_timestamp(
            wire.acknowledged_at_unix_microseconds,
            RuntimeDrainIntentCanonicalStateFieldV2::AcknowledgedAt,
        )?,
    )
    .map_err(Into::into)
}

fn encode_certification(
    certification: &RuntimeDrainCertificationResolutionV2,
) -> Result<DrainCertificationResolutionWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match certification.kind() {
        RuntimeDrainCertificationResolutionKindV2::NoOperationReserved => {
            Ok(DrainCertificationResolutionWireV2::NoOperationReserved {})
        }
        RuntimeDrainCertificationResolutionKindV2::NoAttestationForReservedOperation => {
            let operation_id = certification.operation_id().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            let intent_fingerprint = certification.intent_fingerprint().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(
                DrainCertificationResolutionWireV2::NoAttestationForReservedOperation {
                    operation_id: operation_id.as_str().to_owned(),
                    intent_fingerprint: intent_fingerprint.as_str().to_owned(),
                },
            )
        }
        RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected => {
            let operation_id = certification.operation_id().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            let serving_identity = certification.serving_identity().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            let disconnected_revision = certification.disconnected_revision().ok_or_else(|| {
                correlation(RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress)
            })?;
            Ok(
                DrainCertificationResolutionWireV2::CommittedAndDisconnected {
                    operation_id: operation_id.as_str().to_owned(),
                    serving_identity: Box::new(encode_serving_identity(serving_identity)?),
                    disconnected_revision: persistence_u64(
                        disconnected_revision.get(),
                        RuntimeDrainIntentCanonicalStateFieldV2::DisconnectedRevision,
                    )?,
                },
            )
        }
    }
}

fn decode_certification(
    key: &RuntimeDrainIntentKeyV2,
    claim: &RuntimeDrainClaimV2,
    wire: DrainCertificationResolutionWireV2,
) -> Result<RuntimeDrainCertificationResolutionV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match wire {
        DrainCertificationResolutionWireV2::NoOperationReserved {} => {
            Ok(RuntimeDrainCertificationResolutionV2::no_operation_reserved())
        }
        DrainCertificationResolutionWireV2::NoAttestationForReservedOperation {
            operation_id,
            intent_fingerprint,
        } => Ok(
            RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
                RuntimeCertificationOperationIdV2::parse(operation_id).map_err(|_| {
                    invalid(RuntimeDrainIntentCanonicalStateFieldV2::CertificationOperationId)
                })?,
                RuntimeCertificationIntentFingerprintV2::parse(intent_fingerprint).map_err(
                    |_| {
                        invalid(
                            RuntimeDrainIntentCanonicalStateFieldV2::CertificationIntentFingerprint,
                        )
                    },
                )?,
            ),
        ),
        DrainCertificationResolutionWireV2::CommittedAndDisconnected {
            operation_id,
            serving_identity,
            disconnected_revision,
        } => RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
            key,
            claim,
            RuntimeCertificationOperationIdV2::parse(operation_id).map_err(|_| {
                invalid(RuntimeDrainIntentCanonicalStateFieldV2::CertificationOperationId)
            })?,
            decode_serving_identity(*serving_identity)?,
            non_zero(
                disconnected_revision,
                RuntimeDrainIntentCanonicalStateFieldV2::DisconnectedRevision,
            )?,
        )
        .map_err(Into::into),
    }
}

pub(crate) fn encode_serving_identity(
    serving: &RuntimeServingIdentityV2,
) -> Result<ServingIdentityWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(ServingIdentityWireV2 {
        scope: encode_scope(&serving.scope),
        operation_id: serving.operation_id.as_str().to_owned(),
        attestation_digest: serving.attestation_digest.as_str().to_owned(),
        process_identity: encode_process_identity(&serving.process_identity)?,
        lease_epoch: persistence_u64(
            serving.lease_epoch.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::ServingLeaseEpoch,
        )?,
        revision: persistence_u64(
            serving.revision.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::ServingRevision,
        )?,
    })
}

pub(crate) fn decode_serving_identity(
    wire: ServingIdentityWireV2,
) -> Result<RuntimeServingIdentityV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(RuntimeServingIdentityV2 {
        scope: decode_scope(wire.scope)?,
        operation_id: RuntimeCertificationOperationIdV2::parse(wire.operation_id).map_err(
            |_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::CertificationOperationId),
        )?,
        attestation_digest: RuntimeLiveAttestationDigestV2::parse(wire.attestation_digest)
            .map_err(|_| {
                invalid(RuntimeDrainIntentCanonicalStateFieldV2::CertificationAttestationDigest)
            })?,
        process_identity: decode_process_identity(wire.process_identity)?,
        lease_epoch: non_zero(
            wire.lease_epoch,
            RuntimeDrainIntentCanonicalStateFieldV2::ServingLeaseEpoch,
        )?,
        revision: non_zero(
            wire.revision,
            RuntimeDrainIntentCanonicalStateFieldV2::ServingRevision,
        )?,
    })
}

pub(crate) fn encode_provenance(
    provenance: &crate::RuntimeRouteMutationProvenanceV2,
) -> Result<String, RuntimeDrainIntentCanonicalStateErrorV2> {
    let canonical = RuntimeCanonicalRouteMutationProvenanceV2::new(provenance.clone())
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::Provenance))?;
    String::from_utf8(canonical.provenance_bytes().to_vec())
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::Provenance))
}

pub(crate) fn decode_provenance(
    canonical_json: String,
) -> Result<crate::RuntimeRouteMutationProvenanceV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    RuntimeCanonicalRouteMutationProvenanceV2::from_persisted(canonical_json.as_bytes())
        .map(|canonical| canonical.provenance().clone())
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::Provenance))
}

fn encode_owner_lease(
    owner: &RuntimeGatewayOwnerLeaseIdV1,
) -> Result<GatewayOwnerLeaseIdWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(GatewayOwnerLeaseIdWireV2 {
        gateway_shard_id: owner.gateway_shard_id.as_str().to_owned(),
        process_instance_id: owner.process_instance_id.as_str().to_owned(),
        lease_epoch: persistence_u64(
            owner.lease_epoch.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::GatewayLeaseEpoch,
        )?,
        expected_build_revision: owner.expected_build_revision.as_str().to_owned(),
    })
}

fn decode_owner_lease(
    wire: GatewayOwnerLeaseIdWireV2,
) -> Result<RuntimeGatewayOwnerLeaseIdV1, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse(wire.gateway_shard_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::GatewayShardId))?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id).map_err(|_| {
            invalid(RuntimeDrainIntentCanonicalStateFieldV2::GatewayProcessInstanceId)
        })?,
        lease_epoch: non_zero(
            wire.lease_epoch,
            RuntimeDrainIntentCanonicalStateFieldV2::GatewayLeaseEpoch,
        )?,
        expected_build_revision: RuntimeBuildRevisionV1::parse(wire.expected_build_revision)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::GatewayBuildRevision))?,
    })
}

fn encode_route(
    route: &RuntimeExactLocalRouteIdentityV2,
) -> Result<ExactLocalRouteWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(ExactLocalRouteWireV2 {
        identity: encode_process_identity(&route.identity)?,
        controller_fencing_token: persistence_u64(
            route.controller_fencing_token.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::RouteControllerFencingToken,
        )?,
        route_incarnation: persistence_u64(
            route.route_incarnation.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::RouteIncarnation,
        )?,
    })
}

fn decode_route(
    wire: ExactLocalRouteWireV2,
) -> Result<RuntimeExactLocalRouteIdentityV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(RuntimeExactLocalRouteIdentityV2 {
        identity: decode_process_identity(wire.identity)?,
        controller_fencing_token: fencing_token(
            wire.controller_fencing_token,
            RuntimeDrainIntentCanonicalStateFieldV2::RouteControllerFencingToken,
        )?,
        route_incarnation: non_zero(
            wire.route_incarnation,
            RuntimeDrainIntentCanonicalStateFieldV2::RouteIncarnation,
        )?,
    })
}

pub(crate) fn encode_process_identity(
    identity: &RuntimeProcessIdentityV1,
) -> Result<ProcessIdentityWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(ProcessIdentityWireV2 {
        target: encode_target(&identity.target)?,
        runtime_generation: persistence_u64(
            identity.runtime_generation.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::RouteRuntimeGeneration,
        )?,
        process_instance_id: identity.process_instance_id.as_str().to_owned(),
    })
}

pub(crate) fn decode_process_identity(
    wire: ProcessIdentityWireV2,
) -> Result<RuntimeProcessIdentityV1, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(RuntimeProcessIdentityV1 {
        target: decode_target(wire.target)?,
        runtime_generation: runtime_generation(
            wire.runtime_generation,
            RuntimeDrainIntentCanonicalStateFieldV2::RouteRuntimeGeneration,
        )?,
        process_instance_id: ProcessInstanceId::parse(wire.process_instance_id).map_err(|_| {
            invalid(RuntimeDrainIntentCanonicalStateFieldV2::RouteProcessInstanceId)
        })?,
    })
}

fn encode_key(
    key: &RuntimeDrainIntentKeyV2,
) -> Result<DrainIntentKeyWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(DrainIntentKeyWireV2 {
        intent_id: key.intent_id.as_str().to_owned(),
        product_operation_id: key.product_operation_id.as_str().to_owned(),
        product_mutation_digest: key.product_mutation_digest.as_str().to_owned(),
        scope: encode_scope(&key.scope),
        expected_revision: persistence_u64(
            key.expected_revision.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::ExpectedRevision,
        )?,
        slot: encode_slot(&key.slot)?,
        expected_target: encode_target(&key.expected_target)?,
        mutation_kind: mutation_kind_tag(key.mutation_kind).to_owned(),
    })
}

fn decode_key(
    wire: DrainIntentKeyWireV2,
) -> Result<RuntimeDrainIntentKeyV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(RuntimeDrainIntentKeyV2 {
        intent_id: RuntimeDrainIntentIdV2::parse(wire.intent_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::DrainIntentId))?,
        product_operation_id: RuntimeProductOperationIdV2::parse(wire.product_operation_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::ProductOperationId))?,
        product_mutation_digest: RuntimeProductMutationDigestV2::parse(
            wire.product_mutation_digest,
        )
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::ProductMutationDigest))?,
        scope: decode_scope(wire.scope)?,
        expected_revision: revision(
            wire.expected_revision,
            RuntimeDrainIntentCanonicalStateFieldV2::ExpectedRevision,
        )?,
        slot: decode_slot(wire.slot)?,
        expected_target: decode_target(wire.expected_target)?,
        mutation_kind: decode_mutation_kind(&wire.mutation_kind)?,
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
) -> Result<RuntimeDeploymentScopeV1, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(wire.tenant_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::TenantId))?,
        installation_id: InstallationId::parse(wire.installation_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::InstallationId))?,
        deployment_id: automation_runtime_convergence::DeploymentId::parse(wire.deployment_id)
            .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::DeploymentId))?,
    })
}

fn encode_slot(
    slot: &RuntimeServingSlotV2,
) -> Result<ServingSlotWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(ServingSlotWireV2 {
        guild_id: RuntimeDiscordSnowflakeV2::from_u64(slot.guild_id.0)
            .map_err(|reason| {
                canonical(RuntimeDrainIntentCanonicalStateFieldV2::SlotGuildId, reason)
            })?
            .canonical_text(),
        ruleset_key: slot.ruleset_key.as_str().to_owned(),
    })
}

fn decode_slot(
    wire: ServingSlotWireV2,
) -> Result<RuntimeServingSlotV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::parse_text(&wire.guild_id).map_err(|reason| {
        canonical(RuntimeDrainIntentCanonicalStateFieldV2::SlotGuildId, reason)
    })?;
    let ruleset_key = RuleSetKey::parse(&wire.ruleset_key)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::SlotRuleSetKey))?;
    Ok(RuntimeServingSlotV2::new(
        GuildId(guild_id.get_u64()),
        ruleset_key,
    ))
}

fn encode_target(
    target: &RuntimeDeploymentTargetV1,
) -> Result<DeploymentTargetWireV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    Ok(DeploymentTargetWireV2 {
        guild_id: RuntimeDiscordSnowflakeV2::from_u64(target.guild_id.0)
            .map_err(|reason| {
                canonical(
                    RuntimeDrainIntentCanonicalStateFieldV2::TargetGuildId,
                    reason,
                )
            })?
            .canonical_text(),
        ruleset_key: target.ruleset_key.as_str().to_owned(),
        version: target.version.get(),
        content_hash: target.content_hash.to_hex(),
        binding_revision: persistence_u64(
            target.binding_revision.get(),
            RuntimeDrainIntentCanonicalStateFieldV2::TargetBindingRevision,
        )?,
        binding_fingerprint: target.binding_fingerprint.as_str().to_owned(),
    })
}

fn decode_target(
    wire: DeploymentTargetWireV2,
) -> Result<RuntimeDeploymentTargetV1, RuntimeDrainIntentCanonicalStateErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::parse_text(&wire.guild_id).map_err(|reason| {
        canonical(
            RuntimeDrainIntentCanonicalStateFieldV2::TargetGuildId,
            reason,
        )
    })?;
    let ruleset_key = RuleSetKey::parse(&wire.ruleset_key)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::TargetRuleSetKey))?;
    let version = RuleSetVersionId::new(wire.version)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::TargetVersion))?;
    let content_hash = RuleSetContentHash::parse_hex(&wire.content_hash)
        .ok_or_else(|| invalid(RuntimeDrainIntentCanonicalStateFieldV2::TargetContentHash))?;
    let binding_revision = binding_revision(
        wire.binding_revision,
        RuntimeDrainIntentCanonicalStateFieldV2::TargetBindingRevision,
    )?;
    let binding_fingerprint = ResourceBindingFingerprint::parse(&wire.binding_fingerprint)
        .map_err(|_| invalid(RuntimeDrainIntentCanonicalStateFieldV2::TargetBindingFingerprint))?;
    Ok(RuntimeDeploymentTargetV1 {
        guild_id: GuildId(guild_id.get_u64()),
        ruleset_key,
        version,
        content_hash,
        binding_revision,
        binding_fingerprint,
    })
}

fn mutation_kind_tag(kind: RuntimeProductMutationKindV2) -> &'static str {
    match kind {
        RuntimeProductMutationKindV2::Apply => "apply",
        RuntimeProductMutationKindV2::Supersede => "supersede",
        RuntimeProductMutationKindV2::Cancel => "cancel",
        RuntimeProductMutationKindV2::AuthorityChange => "authority_change",
        RuntimeProductMutationKindV2::Teardown => "teardown",
    }
}

fn decode_mutation_kind(
    value: &str,
) -> Result<RuntimeProductMutationKindV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match value {
        "apply" => Ok(RuntimeProductMutationKindV2::Apply),
        "supersede" => Ok(RuntimeProductMutationKindV2::Supersede),
        "cancel" => Ok(RuntimeProductMutationKindV2::Cancel),
        "authority_change" => Ok(RuntimeProductMutationKindV2::AuthorityChange),
        "teardown" => Ok(RuntimeProductMutationKindV2::Teardown),
        _ => Err(invalid(
            RuntimeDrainIntentCanonicalStateFieldV2::MutationKind,
        )),
    }
}

pub(crate) fn encode_root<T: Serialize>(
    wire: &T,
) -> Result<Vec<u8>, RuntimeDrainIntentCanonicalStateErrorV2> {
    let encoded =
        serde_json::to_vec(wire).map_err(|_| RuntimeDrainIntentCanonicalStateErrorV2::Encoding)?;
    ensure_size(&encoded)?;
    Ok(encoded)
}

pub(crate) fn ensure_size(encoded: &[u8]) -> Result<(), RuntimeDrainIntentCanonicalStateErrorV2> {
    if encoded.is_empty() {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::Decoding);
    }
    if encoded.len() > DRAIN_INTENT_STATE_MAX_OCTETS {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::PayloadTooLarge);
    }
    Ok(())
}

pub(crate) fn persistence_u64(
    value: u64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<u64, RuntimeDrainIntentCanonicalStateErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(RuntimePersistenceU64V2::get_u64)
        .map_err(|reason| canonical(field, reason))
}

pub(crate) fn non_zero(
    value: u64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<NonZeroU64, RuntimeDrainIntentCanonicalStateErrorV2> {
    let value = persistence_u64(value, field)?;
    NonZeroU64::new(value).ok_or_else(|| invalid(field))
}

fn revision(
    value: u64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<DeploymentRevision, RuntimeDrainIntentCanonicalStateErrorV2> {
    DeploymentRevision::new(persistence_u64(value, field)?).map_err(|_| invalid(field))
}

fn runtime_generation(
    value: u64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<RuntimeGeneration, RuntimeDrainIntentCanonicalStateErrorV2> {
    RuntimeGeneration::new(persistence_u64(value, field)?).map_err(|_| invalid(field))
}

fn binding_revision(
    value: u64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<BindingRevision, RuntimeDrainIntentCanonicalStateErrorV2> {
    BindingRevision::new(persistence_u64(value, field)?).map_err(|_| invalid(field))
}

pub(crate) fn fencing_token(
    value: u64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<FencingToken, RuntimeDrainIntentCanonicalStateErrorV2> {
    FencingToken::new(persistence_u64(value, field)?).map_err(|_| invalid(field))
}

pub(crate) fn timestamp(
    value: DateTime<Utc>,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<i64, RuntimeDrainIntentCanonicalStateErrorV2> {
    RuntimeUnixMicrosecondsV2::from_datetime(value)
        .map(RuntimeUnixMicrosecondsV2::get)
        .map_err(|reason| canonical(field, reason))
}

pub(crate) fn decode_timestamp(
    value: i64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<DateTime<Utc>, RuntimeDrainIntentCanonicalStateErrorV2> {
    RuntimeUnixMicrosecondsV2::from_i64(value)
        .map(RuntimeUnixMicrosecondsV2::to_datetime)
        .map_err(|reason| canonical(field, reason))
}

fn invalid(
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> RuntimeDrainIntentCanonicalStateErrorV2 {
    RuntimeDrainIntentCanonicalStateErrorV2::InvalidField { field }
}

fn canonical(
    field: RuntimeDrainIntentCanonicalStateFieldV2,
    reason: RuntimeCanonicalValueErrorV2,
) -> RuntimeDrainIntentCanonicalStateErrorV2 {
    RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue { field, reason }
}

fn correlation(
    field: RuntimeDrainIntentCanonicalStateCorrelationV2,
) -> RuntimeDrainIntentCanonicalStateErrorV2 {
    RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch { field }
}

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
