use std::num::NonZeroU64;

use automation_runtime_convergence::DeploymentRevision;
use serde::{Deserialize, Deserializer, Serialize};

use super::{
    RuntimeCanonicalDrainIntentStateV3, RuntimeDrainActionDigestV3,
    RuntimeDrainCanonicalStateDigestV3, RuntimeDrainIntentCanonicalStateValueV3,
    RuntimeDrainTeardownCanonicalErrorV3, RuntimePreviousProcessDrainCertificationResolutionKindV3,
    RuntimePreviousProcessDrainCertificationResolutionV3, RuntimePreviousProcessDrainProgressV3,
    RuntimePreviousProcessRouteAbsenceBasisDecodedV3, RuntimePreviousProcessRouteAbsenceBasisV3,
    RuntimeRouteAbsentAcknowledgementV3, DRAIN_INTENT_STATE_MAX_OCTETS_V3,
};
use crate::v2_drain_intent_canonical_state::wire::{
    decode_claim, decode_compact_root_binding_v2, decode_process_identity, decode_provenance,
    decode_serving_identity, decode_timestamp, encode_claim, encode_compact_root_binding_v2,
    encode_process_identity, encode_provenance, encode_root, encode_serving_identity, ensure_size,
    fencing_token, non_zero, persistence_u64, timestamp, DrainClaimWireV2,
    DrainIntentRootBindingWireV2, ProcessIdentityWireV2, ServingIdentityWireV2,
};
use crate::{
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationOperationIdV2,
    RuntimeDrainIntentCanonicalStateErrorV2, RuntimeDrainIntentCanonicalStateFieldV2,
    RuntimeDrainIntentKeyV2, RuntimePersistedProductDrainRootV2,
};

const FORMAT_VERSION: u8 = 3;

#[derive(Deserialize)]
struct DrainIntentStateVersionWireV3 {
    format_version: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DrainIntentStateRootWireV3 {
    format_version: u8,
    root: DrainIntentRootBindingWireV2,
    intent_revision: u64,
    state: DrainIntentStateWireV3,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum DrainIntentStateWireV3 {
    #[serde(rename = "route_absent_acknowledged")]
    RouteAbsentAcknowledged {
        acknowledgement: Box<RouteAbsentAcknowledgementWireV3>,
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
struct RouteAbsentAcknowledgementWireV3 {
    successor_claim: DrainClaimWireV2,
    absence_basis: PreviousProcessRouteAbsenceBasisWireV3,
    provenance_json: String,
    registry_observation_sequence: u64,
    certification: PreviousProcessDrainCertificationResolutionWireV3,
    acknowledged_at_unix_microseconds: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum PreviousProcessRouteAbsenceBasisWireV3 {
    #[serde(rename = "previous_process_route_teardown")]
    PreviousProcessRouteTeardown {
        predecessor_intent_revision: u64,
        predecessor_state_digest: String,
        predecessor_progress: PreviousProcessDrainProgressWireV3,
        route_identity: ProcessIdentityWireV2,
        route_incarnation: u64,
        source_route_fence: u64,
        possible_route_fence_ceiling: u64,
        predecessor_claim_terminal_digest: String,
        #[serde(deserialize_with = "deserialize_required_option")]
        predecessor_refence_terminal_digest: Option<String>,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum PreviousProcessDrainProgressWireV3 {
    #[serde(rename = "routed_claimed")]
    RoutedClaimed,
    #[serde(rename = "refenced")]
    Refenced,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum PreviousProcessDrainCertificationResolutionWireV3 {
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

pub(super) fn encode_state(
    canonical: &RuntimeCanonicalDrainIntentStateV3,
) -> Result<Vec<u8>, RuntimeDrainTeardownCanonicalErrorV3> {
    let wire = DrainIntentStateRootWireV3 {
        format_version: FORMAT_VERSION,
        root: encode_compact_root_binding_v2(canonical.key(), canonical.drain_intent_digest())?,
        intent_revision: persistence_u64(
            canonical.intent_revision().get(),
            RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
        )?,
        state: encode_state_value(canonical)?,
    };
    encode_root(&wire).map_err(Into::into)
}

pub(super) fn decode_state(
    root: &RuntimePersistedProductDrainRootV2,
    intent_revision: NonZeroU64,
    encoded: &[u8],
) -> Result<RuntimeCanonicalDrainIntentStateV3, RuntimeDrainTeardownCanonicalErrorV3> {
    if encoded.len() > DRAIN_INTENT_STATE_MAX_OCTETS_V3 {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::PayloadTooLarge.into());
    }
    ensure_size(encoded)?;
    let version = serde_json::from_slice::<DrainIntentStateVersionWireV3>(encoded)
        .map_err(|_| RuntimeDrainTeardownCanonicalErrorV3::Decoding)?;
    if version.format_version != FORMAT_VERSION {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::UnsupportedFormatVersion);
    }
    let wire = serde_json::from_slice::<DrainIntentStateRootWireV3>(encoded)
        .map_err(|_| RuntimeDrainTeardownCanonicalErrorV3::Decoding)?;
    let decoded_revision = non_zero(
        wire.intent_revision,
        RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
    )?;
    if decoded_revision != intent_revision {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::ImmutableRootMismatch);
    }
    let (key, drain_intent_digest) = decode_compact_root_binding_v2(wire.root)?;
    if key != root.canonical().drain_preimage().key
        || drain_intent_digest != *root.canonical().drain_intent_digest()
    {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::ImmutableRootMismatch);
    }
    let state = decode_state_value(&key, wire.state)?;
    let canonical = RuntimeCanonicalDrainIntentStateV3::build(
        key,
        drain_intent_digest,
        intent_revision,
        state,
    )?;
    if canonical.state_bytes() != encoded {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::NonCanonicalEncoding);
    }
    Ok(canonical)
}

fn encode_state_value(
    canonical: &RuntimeCanonicalDrainIntentStateV3,
) -> Result<DrainIntentStateWireV3, RuntimeDrainTeardownCanonicalErrorV3> {
    match &canonical.state {
        RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged { acknowledgement } => {
            Ok(DrainIntentStateWireV3::RouteAbsentAcknowledged {
                acknowledgement: Box::new(encode_acknowledgement(acknowledgement)?),
            })
        }
        RuntimeDrainIntentCanonicalStateValueV3::Consumed {
            resulting_revision,
            consumed_at,
        } => Ok(DrainIntentStateWireV3::Consumed {
            resulting_revision: persistence_u64(
                resulting_revision.get(),
                RuntimeDrainIntentCanonicalStateFieldV2::ResultingRevision,
            )?,
            consumed_at_unix_microseconds: timestamp(
                consumed_at.to_owned(),
                RuntimeDrainIntentCanonicalStateFieldV2::ConsumedAt,
            )?,
        }),
        RuntimeDrainIntentCanonicalStateValueV3::Cancelled { cancelled_at } => {
            Ok(DrainIntentStateWireV3::Cancelled {
                cancelled_at_unix_microseconds: timestamp(
                    cancelled_at.to_owned(),
                    RuntimeDrainIntentCanonicalStateFieldV2::CancelledAt,
                )?,
            })
        }
    }
}

fn decode_state_value(
    key: &RuntimeDrainIntentKeyV2,
    wire: DrainIntentStateWireV3,
) -> Result<RuntimeDrainIntentCanonicalStateValueV3, RuntimeDrainTeardownCanonicalErrorV3> {
    match wire {
        DrainIntentStateWireV3::RouteAbsentAcknowledged { acknowledgement } => Ok(
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged {
                acknowledgement: Box::new(decode_acknowledgement(key, *acknowledgement)?),
            },
        ),
        DrainIntentStateWireV3::Consumed {
            resulting_revision,
            consumed_at_unix_microseconds,
        } => Ok(RuntimeDrainIntentCanonicalStateValueV3::Consumed {
            resulting_revision: DeploymentRevision::new(persistence_u64(
                resulting_revision,
                RuntimeDrainIntentCanonicalStateFieldV2::ResultingRevision,
            )?)
            .map_err(|_| RuntimeDrainTeardownCanonicalErrorV3::CanonicalValue)?,
            consumed_at: decode_timestamp(
                consumed_at_unix_microseconds,
                RuntimeDrainIntentCanonicalStateFieldV2::ConsumedAt,
            )?,
        }),
        DrainIntentStateWireV3::Cancelled {
            cancelled_at_unix_microseconds,
        } => Ok(RuntimeDrainIntentCanonicalStateValueV3::Cancelled {
            cancelled_at: decode_timestamp(
                cancelled_at_unix_microseconds,
                RuntimeDrainIntentCanonicalStateFieldV2::CancelledAt,
            )?,
        }),
    }
}

fn encode_acknowledgement(
    acknowledgement: &RuntimeRouteAbsentAcknowledgementV3,
) -> Result<RouteAbsentAcknowledgementWireV3, RuntimeDrainTeardownCanonicalErrorV3> {
    Ok(RouteAbsentAcknowledgementWireV3 {
        successor_claim: encode_claim(acknowledgement.successor_claim())?,
        absence_basis: encode_absence_basis(acknowledgement.absence_basis())?,
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
    wire: RouteAbsentAcknowledgementWireV3,
) -> Result<RuntimeRouteAbsentAcknowledgementV3, RuntimeDrainTeardownCanonicalErrorV3> {
    let successor_claim = decode_claim(key, wire.successor_claim)?;
    let absence_basis = decode_absence_basis(wire.absence_basis)?;
    let certification = decode_certification(key, &absence_basis, wire.certification)?;
    RuntimeRouteAbsentAcknowledgementV3::new(
        key,
        successor_claim,
        absence_basis,
        decode_provenance(wire.provenance_json)?,
        non_zero(
            wire.registry_observation_sequence,
            RuntimeDrainIntentCanonicalStateFieldV2::AcknowledgementObservationSequence,
        )?,
        certification,
        decode_timestamp(
            wire.acknowledged_at_unix_microseconds,
            RuntimeDrainIntentCanonicalStateFieldV2::AcknowledgedAt,
        )?,
    )
}

fn encode_absence_basis(
    basis: &RuntimePreviousProcessRouteAbsenceBasisV3,
) -> Result<PreviousProcessRouteAbsenceBasisWireV3, RuntimeDrainTeardownCanonicalErrorV3> {
    Ok(
        PreviousProcessRouteAbsenceBasisWireV3::PreviousProcessRouteTeardown {
            predecessor_intent_revision: persistence_u64(
                basis.predecessor_intent_revision().get(),
                RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
            )?,
            predecessor_state_digest: basis.predecessor_state_digest().as_str().to_owned(),
            predecessor_progress: match basis.predecessor_progress() {
                RuntimePreviousProcessDrainProgressV3::RoutedClaimed => {
                    PreviousProcessDrainProgressWireV3::RoutedClaimed
                }
                RuntimePreviousProcessDrainProgressV3::Refenced => {
                    PreviousProcessDrainProgressWireV3::Refenced
                }
            },
            route_identity: encode_process_identity(basis.route_identity())?,
            route_incarnation: persistence_u64(
                basis.route_incarnation().get(),
                RuntimeDrainIntentCanonicalStateFieldV2::RouteIncarnation,
            )?,
            source_route_fence: persistence_u64(
                basis.source_route_fence().get(),
                RuntimeDrainIntentCanonicalStateFieldV2::RouteControllerFencingToken,
            )?,
            possible_route_fence_ceiling: persistence_u64(
                basis.possible_route_fence_ceiling().get(),
                RuntimeDrainIntentCanonicalStateFieldV2::ControllerFencingToken,
            )?,
            predecessor_claim_terminal_digest: basis
                .predecessor_claim_terminal_digest()
                .as_str()
                .to_owned(),
            predecessor_refence_terminal_digest: basis
                .predecessor_refence_terminal_digest()
                .map(|digest| digest.as_str().to_owned()),
        },
    )
}

fn decode_absence_basis(
    wire: PreviousProcessRouteAbsenceBasisWireV3,
) -> Result<RuntimePreviousProcessRouteAbsenceBasisV3, RuntimeDrainTeardownCanonicalErrorV3> {
    match wire {
        PreviousProcessRouteAbsenceBasisWireV3::PreviousProcessRouteTeardown {
            predecessor_intent_revision,
            predecessor_state_digest,
            predecessor_progress,
            route_identity,
            route_incarnation,
            source_route_fence,
            possible_route_fence_ceiling,
            predecessor_claim_terminal_digest,
            predecessor_refence_terminal_digest,
        } => RuntimePreviousProcessRouteAbsenceBasisV3::from_decoded(
            RuntimePreviousProcessRouteAbsenceBasisDecodedV3 {
                predecessor_intent_revision: non_zero(
                    predecessor_intent_revision,
                    RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
                )?,
                predecessor_state_digest: RuntimeDrainCanonicalStateDigestV3::parse(
                    predecessor_state_digest,
                )?,
                predecessor_progress: match predecessor_progress {
                    PreviousProcessDrainProgressWireV3::RoutedClaimed => {
                        RuntimePreviousProcessDrainProgressV3::RoutedClaimed
                    }
                    PreviousProcessDrainProgressWireV3::Refenced => {
                        RuntimePreviousProcessDrainProgressV3::Refenced
                    }
                },
                route_identity: decode_process_identity(route_identity)?,
                route_incarnation: non_zero(
                    route_incarnation,
                    RuntimeDrainIntentCanonicalStateFieldV2::RouteIncarnation,
                )?,
                source_route_fence: fencing_token(
                    source_route_fence,
                    RuntimeDrainIntentCanonicalStateFieldV2::RouteControllerFencingToken,
                )?,
                possible_route_fence_ceiling: fencing_token(
                    possible_route_fence_ceiling,
                    RuntimeDrainIntentCanonicalStateFieldV2::ControllerFencingToken,
                )?,
                predecessor_claim_terminal_digest: RuntimeDrainActionDigestV3::parse(
                    predecessor_claim_terminal_digest,
                )?,
                predecessor_refence_terminal_digest: predecessor_refence_terminal_digest
                    .map(RuntimeDrainActionDigestV3::parse)
                    .transpose()?,
            },
        ),
    }
}

fn encode_certification(
    certification: &RuntimePreviousProcessDrainCertificationResolutionV3,
) -> Result<PreviousProcessDrainCertificationResolutionWireV3, RuntimeDrainTeardownCanonicalErrorV3>
{
    match certification.kind() {
        RuntimePreviousProcessDrainCertificationResolutionKindV3::NoOperationReserved => Ok(
            PreviousProcessDrainCertificationResolutionWireV3::NoOperationReserved {},
        ),
        RuntimePreviousProcessDrainCertificationResolutionKindV3::NoAttestationForReservedOperation => {
            Ok(PreviousProcessDrainCertificationResolutionWireV3::NoAttestationForReservedOperation {
                operation_id: certification
                    .operation_id()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?
                    .as_str()
                    .to_owned(),
                intent_fingerprint: certification
                    .intent_fingerprint()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?
                    .as_str()
                    .to_owned(),
            })
        }
        RuntimePreviousProcessDrainCertificationResolutionKindV3::CommittedAndDisconnected => {
            Ok(PreviousProcessDrainCertificationResolutionWireV3::CommittedAndDisconnected {
                operation_id: certification
                    .operation_id()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?
                    .as_str()
                    .to_owned(),
                serving_identity: Box::new(encode_serving_identity(
                    certification
                        .serving_identity()
                        .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?,
                )?),
                disconnected_revision: persistence_u64(
                    certification
                        .disconnected_revision()
                        .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?
                        .get(),
                    RuntimeDrainIntentCanonicalStateFieldV2::DisconnectedRevision,
                )?,
            })
        }
    }
}

fn decode_certification(
    key: &RuntimeDrainIntentKeyV2,
    basis: &RuntimePreviousProcessRouteAbsenceBasisV3,
    wire: PreviousProcessDrainCertificationResolutionWireV3,
) -> Result<
    RuntimePreviousProcessDrainCertificationResolutionV3,
    RuntimeDrainTeardownCanonicalErrorV3,
> {
    let certification = match wire {
        PreviousProcessDrainCertificationResolutionWireV3::NoOperationReserved {} => {
            RuntimePreviousProcessDrainCertificationResolutionV3::from_decoded_no_operation_reserved(
            )
        }
        PreviousProcessDrainCertificationResolutionWireV3::NoAttestationForReservedOperation {
            operation_id,
            intent_fingerprint,
        } => RuntimePreviousProcessDrainCertificationResolutionV3::from_decoded_no_attestation(
            RuntimeCertificationOperationIdV2::parse(operation_id)
                .map_err(|_| RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?,
            RuntimeCertificationIntentFingerprintV2::parse(intent_fingerprint)
                .map_err(|_| RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?,
        ),
        PreviousProcessDrainCertificationResolutionWireV3::CommittedAndDisconnected {
            operation_id,
            serving_identity,
            disconnected_revision,
        } => RuntimePreviousProcessDrainCertificationResolutionV3::from_decoded_committed(
            RuntimeCertificationOperationIdV2::parse(operation_id)
                .map_err(|_| RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?,
            decode_serving_identity(*serving_identity)?,
            non_zero(
                disconnected_revision,
                RuntimeDrainIntentCanonicalStateFieldV2::DisconnectedRevision,
            )?,
        ),
    };
    certification.validate_for_basis(key, basis)?;
    Ok(certification)
}

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
