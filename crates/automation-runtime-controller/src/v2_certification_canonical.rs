mod wire;

#[cfg(test)]
mod tests;

use crate::v2_digest::{
    certification_intent_fingerprint_v2, certification_request_digest_v2,
    live_attestation_digest_v2,
};
use crate::{
    RuntimeCanonicalValueErrorV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationIntentV2, RuntimeCertificationRequestDigestV2,
    RuntimeCertificationRequestV2, RuntimeEvidenceErrorV2, RuntimeLiveAttestationDigestV2,
    RuntimeServingSlotV2,
};

const CERTIFICATION_INTENT_MAX_OCTETS: usize = 32_768;
const CERTIFICATION_REQUEST_MAX_OCTETS: usize = 65_536;
const LIVE_ATTESTATION_MAX_OCTETS: usize = 131_072;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationCanonicalRootV2 {
    Intent,
    Request,
    LiveAttestation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationCanonicalFieldV2 {
    ActionId,
    OperationId,
    GuardTenantId,
    GuardInstallationId,
    GuardDeploymentId,
    GuardExpectedRevision,
    GuardControllerId,
    GuardFencingToken,
    GuardRuntimeGeneration,
    GuardConvergenceAttempt,
    TargetGuildId,
    TargetRuleSetKey,
    TargetVersion,
    TargetContentHash,
    TargetBindingRevision,
    TargetBindingFingerprint,
    BindingPinTenantId,
    BindingPinInstallationId,
    BindingPinInstallationAuthorityRevision,
    BindingPinBindingRevision,
    BindingPinBindingFingerprint,
    ProcessTargetGuildId,
    ProcessTargetRuleSetKey,
    ProcessTargetVersion,
    ProcessTargetContentHash,
    ProcessTargetBindingRevision,
    ProcessTargetBindingFingerprint,
    ProcessRuntimeGeneration,
    ProcessInstanceId,
    GatewayShardId,
    GatewayProcessInstanceId,
    GatewayLeaseEpoch,
    GatewayExpectedBuildRevision,
    ObservedOwnerRevision,
    RuntimeBuildRevision,
    PanelCertificateId,
    PanelReportDigest,
    PanelProcessTargetGuildId,
    PanelProcessTargetRuleSetKey,
    PanelProcessTargetVersion,
    PanelProcessTargetContentHash,
    PanelProcessTargetBindingRevision,
    PanelProcessTargetBindingFingerprint,
    PanelProcessRuntimeGeneration,
    PanelProcessInstanceId,
    PanelControllerFencingToken,
    ServingLeaseMilliseconds,
    IntentFingerprint,
    MustCommitBeforeUnixMicroseconds,
    BarrierId,
    PauseCoordinatorGeneration,
    PauseConnectionEpoch,
    PauseAdmissionRevision,
    PauseSequence,
    GatewayReadyProcessInstanceId,
    GatewayReadyConnectionEpoch,
    GatewayReadyKind,
    GatewayReadyAdmissionRevision,
    GatewayReadyConnectedEventSequence,
    GatewayReadyResumeSequence,
    RouteGatewayShardId,
    RouteGatewayProcessInstanceId,
    RouteGatewayLeaseEpoch,
    RouteGatewayExpectedBuildRevision,
    AttestedOwnerRevision,
    RouteTargetGuildId,
    RouteTargetRuleSetKey,
    RouteTargetVersion,
    RouteTargetContentHash,
    RouteTargetBindingRevision,
    RouteTargetBindingFingerprint,
    RouteRuntimeGeneration,
    RouteProcessInstanceId,
    RouteControllerFencingToken,
    RouteIncarnation,
    RouteActivationSequence,
    RequestDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationIntentCorrelationV2 {
    BindingPinScope,
    BindingPinTarget,
    GuardRuntimeGeneration,
    ProcessTarget,
    GatewayOwnerProcessInstance,
    GatewayOwnerBuildRevision,
    PanelProcessIdentity,
    PanelControllerFencingToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationRequestCorrelationV2 {
    IntentFingerprint,
    ReservedIntentRoot,
    RouteServingSlot,
    RouteProcessIdentity,
    RouteControllerFencingToken,
    GatewayOwnerLease,
    GatewayOwnerRevision,
    LiveRequestDigest,
    LiveRequestRoot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCertificationCanonicalErrorV2 {
    #[error("runtime certification {root:?} canonical payload exceeds its size limit")]
    PayloadTooLarge {
        root: RuntimeCertificationCanonicalRootV2,
    },
    #[error("runtime certification {root:?} canonical payload encoding failed")]
    Encoding {
        root: RuntimeCertificationCanonicalRootV2,
    },
    #[error("runtime certification {root:?} canonical payload decoding failed")]
    Decoding {
        root: RuntimeCertificationCanonicalRootV2,
    },
    #[error("runtime certification {root:?} canonical payload format version is unsupported")]
    UnsupportedFormatVersion {
        root: RuntimeCertificationCanonicalRootV2,
    },
    #[error("runtime certification {root:?} canonical payload has a noncanonical representation")]
    NonCanonicalEncoding {
        root: RuntimeCertificationCanonicalRootV2,
    },
    #[error("runtime certification {root:?} canonical field {field:?} is invalid")]
    InvalidField {
        root: RuntimeCertificationCanonicalRootV2,
        field: RuntimeCertificationCanonicalFieldV2,
    },
    #[error("runtime certification {root:?} canonical field {field:?} is invalid: {reason}")]
    CanonicalValue {
        root: RuntimeCertificationCanonicalRootV2,
        field: RuntimeCertificationCanonicalFieldV2,
        reason: RuntimeCanonicalValueErrorV2,
    },
    #[error("runtime certification intent fields disagree on {field:?}")]
    CorrelationMismatch {
        field: RuntimeCertificationIntentCorrelationV2,
    },
    #[error("runtime certification {root:?} persisted fingerprint does not match its payload")]
    PersistedFingerprintMismatch {
        root: RuntimeCertificationCanonicalRootV2,
    },
    #[error("runtime certification route admission evidence is invalid: {reason}")]
    RouteAdmission { reason: RuntimeEvidenceErrorV2 },
    #[error("runtime certification request fields disagree on {field:?}")]
    RequestCorrelationMismatch {
        field: RuntimeCertificationRequestCorrelationV2,
    },
    #[error("runtime certification {root:?} persisted digest does not match its payload")]
    PersistedDigestMismatch {
        root: RuntimeCertificationCanonicalRootV2,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCanonicalCertificationIntentV2 {
    intent: RuntimeCertificationIntentV2,
    bytes: Box<[u8]>,
    fingerprint: RuntimeCertificationIntentFingerprintV2,
}

impl RuntimeCanonicalCertificationIntentV2 {
    pub fn new(
        intent: RuntimeCertificationIntentV2,
    ) -> Result<Self, RuntimeCertificationCanonicalErrorV2> {
        let bytes = wire::encode_certification_intent(&intent)?;
        let fingerprint = certification_intent_fingerprint_v2(&bytes);
        Ok(Self {
            intent,
            bytes: bytes.into_boxed_slice(),
            fingerprint,
        })
    }

    pub fn from_persisted(
        bytes: &[u8],
        persisted_fingerprint: &RuntimeCertificationIntentFingerprintV2,
    ) -> Result<Self, RuntimeCertificationCanonicalErrorV2> {
        let intent = wire::decode_certification_intent(bytes)?;
        let fingerprint = certification_intent_fingerprint_v2(bytes);
        if fingerprint != *persisted_fingerprint {
            return Err(
                RuntimeCertificationCanonicalErrorV2::PersistedFingerprintMismatch {
                    root: RuntimeCertificationCanonicalRootV2::Intent,
                },
            );
        }
        Ok(Self {
            intent,
            bytes: bytes.to_vec().into_boxed_slice(),
            fingerprint,
        })
    }

    pub fn intent(&self) -> &RuntimeCertificationIntentV2 {
        &self.intent
    }

    pub fn certification_intent_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn intent_fingerprint(&self) -> &RuntimeCertificationIntentFingerprintV2 {
        &self.fingerprint
    }

    pub fn bind_live_record(
        &self,
        record: RuntimeLiveAttestationRecordV2,
    ) -> Result<RuntimeCanonicalLiveAttestationV2, RuntimeCertificationCanonicalErrorV2> {
        let embedded_intent =
            RuntimeCanonicalCertificationIntentV2::new(record.request.intent.clone())?;
        if embedded_intent.certification_intent_bytes() != self.certification_intent_bytes()
            || embedded_intent.intent_fingerprint() != self.intent_fingerprint()
        {
            return Err(
                RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                    field: RuntimeCertificationRequestCorrelationV2::ReservedIntentRoot,
                },
            );
        }
        let request_bytes = wire::encode_certification_request(&record.request)?;
        let request_digest = certification_request_digest_v2(&request_bytes);
        if request_digest != record.request_digest {
            return Err(
                RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                    field: RuntimeCertificationRequestCorrelationV2::LiveRequestDigest,
                },
            );
        }
        let live_record_bytes = wire::encode_live_attestation_record(&record, &request_bytes)?;
        let live_digest = live_attestation_digest_v2(&live_record_bytes);
        Ok(RuntimeCanonicalLiveAttestationV2 {
            reserved_intent: self.clone(),
            record,
            request_bytes: request_bytes.into_boxed_slice(),
            live_record_bytes: live_record_bytes.into_boxed_slice(),
            live_digest,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLiveAttestationRecordV2 {
    request_digest: RuntimeCertificationRequestDigestV2,
    request: RuntimeCertificationRequestV2,
}

impl RuntimeLiveAttestationRecordV2 {
    pub fn from_request(
        request: RuntimeCertificationRequestV2,
    ) -> Result<Self, RuntimeCertificationCanonicalErrorV2> {
        let request_bytes = wire::encode_certification_request(&request)?;
        let request_digest = certification_request_digest_v2(&request_bytes);
        Ok(Self {
            request_digest,
            request,
        })
    }

    pub fn request_digest(&self) -> &RuntimeCertificationRequestDigestV2 {
        &self.request_digest
    }

    pub fn request(&self) -> &RuntimeCertificationRequestV2 {
        &self.request
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCanonicalLiveAttestationV2 {
    reserved_intent: RuntimeCanonicalCertificationIntentV2,
    record: RuntimeLiveAttestationRecordV2,
    request_bytes: Box<[u8]>,
    live_record_bytes: Box<[u8]>,
    live_digest: RuntimeLiveAttestationDigestV2,
}

impl RuntimeCanonicalLiveAttestationV2 {
    pub fn from_persisted(
        reserved_intent: &RuntimeCanonicalCertificationIntentV2,
        request_bytes: &[u8],
        persisted_request_digest: &RuntimeCertificationRequestDigestV2,
        live_record_bytes: &[u8],
        persisted_live_digest: &RuntimeLiveAttestationDigestV2,
    ) -> Result<Self, RuntimeCertificationCanonicalErrorV2> {
        let request = wire::decode_certification_request(request_bytes)?;
        let request_digest = certification_request_digest_v2(request_bytes);
        if request_digest != *persisted_request_digest {
            return Err(
                RuntimeCertificationCanonicalErrorV2::PersistedDigestMismatch {
                    root: RuntimeCertificationCanonicalRootV2::Request,
                },
            );
        }
        let record = wire::decode_live_attestation_record(live_record_bytes)?;
        if record.request_digest != request_digest {
            return Err(
                RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                    field: RuntimeCertificationRequestCorrelationV2::LiveRequestDigest,
                },
            );
        }
        if record.request != request {
            return Err(
                RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                    field: RuntimeCertificationRequestCorrelationV2::LiveRequestRoot,
                },
            );
        }
        let canonical = reserved_intent.bind_live_record(record)?;
        if canonical.request_bytes.as_ref() != request_bytes
            || canonical.live_record_bytes.as_ref() != live_record_bytes
        {
            return Err(
                RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                    field: RuntimeCertificationRequestCorrelationV2::LiveRequestRoot,
                },
            );
        }
        if canonical.live_digest != *persisted_live_digest {
            return Err(
                RuntimeCertificationCanonicalErrorV2::PersistedDigestMismatch {
                    root: RuntimeCertificationCanonicalRootV2::LiveAttestation,
                },
            );
        }
        Ok(canonical)
    }

    pub fn reserved_intent(&self) -> &RuntimeCanonicalCertificationIntentV2 {
        &self.reserved_intent
    }

    pub fn record(&self) -> &RuntimeLiveAttestationRecordV2 {
        &self.record
    }

    pub fn request(&self) -> &RuntimeCertificationRequestV2 {
        self.record.request()
    }

    pub fn certification_intent_bytes(&self) -> &[u8] {
        self.reserved_intent.certification_intent_bytes()
    }

    pub fn intent_fingerprint(&self) -> &RuntimeCertificationIntentFingerprintV2 {
        self.reserved_intent.intent_fingerprint()
    }

    pub fn certification_request_bytes(&self) -> &[u8] {
        &self.request_bytes
    }

    pub fn request_digest(&self) -> &RuntimeCertificationRequestDigestV2 {
        self.record.request_digest()
    }

    pub fn live_attestation_record_bytes(&self) -> &[u8] {
        &self.live_record_bytes
    }

    pub fn live_attestation_digest(&self) -> &RuntimeLiveAttestationDigestV2 {
        &self.live_digest
    }
}

fn validate_intent(
    intent: &RuntimeCertificationIntentV2,
) -> Result<(), RuntimeCertificationCanonicalErrorV2> {
    if !intent.binding_pin.matches_scope(&intent.guard.scope) {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::BindingPinScope,
        });
    }
    if !intent.binding_pin.matches_target(&intent.target) {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::BindingPinTarget,
        });
    }
    if intent.guard.runtime_generation != intent.process_identity.runtime_generation {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::GuardRuntimeGeneration,
        });
    }
    if intent.target != intent.process_identity.target {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::ProcessTarget,
        });
    }
    if intent.gateway_owner_lease_id.process_instance_id
        != intent.process_identity.process_instance_id
    {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::GatewayOwnerProcessInstance,
        });
    }
    if intent.gateway_owner_lease_id.expected_build_revision != intent.runtime_build_revision {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::GatewayOwnerBuildRevision,
        });
    }
    if intent.panel.process_identity != intent.process_identity {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::PanelProcessIdentity,
        });
    }
    if intent.panel.controller_fencing_token != intent.guard.fencing_token {
        return Err(RuntimeCertificationCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeCertificationIntentCorrelationV2::PanelControllerFencingToken,
        });
    }
    Ok(())
}

fn validate_request(
    request: &RuntimeCertificationRequestV2,
) -> Result<(), RuntimeCertificationCanonicalErrorV2> {
    let intent_bytes = wire::encode_certification_intent(&request.intent)?;
    let intent_fingerprint = certification_intent_fingerprint_v2(&intent_bytes);
    if intent_fingerprint != request.intent_fingerprint {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::IntentFingerprint,
            },
        );
    }
    request
        .route_admission
        .validate()
        .map_err(|reason| RuntimeCertificationCanonicalErrorV2::RouteAdmission { reason })?;
    let route = &request.route_admission.route;
    let intent = &request.intent;
    let expected_slot = RuntimeServingSlotV2::from_target(&intent.target);
    if route.slot() != expected_slot {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::RouteServingSlot,
            },
        );
    }
    if route.identity != intent.process_identity {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::RouteProcessIdentity,
            },
        );
    }
    if route.controller_fencing_token != intent.guard.fencing_token {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::RouteControllerFencingToken,
            },
        );
    }
    if request.route_admission.gateway_owner_lease_id != intent.gateway_owner_lease_id {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::GatewayOwnerLease,
            },
        );
    }
    if request.route_admission.attested_owner_revision != intent.observed_owner_revision {
        return Err(
            RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch {
                field: RuntimeCertificationRequestCorrelationV2::GatewayOwnerRevision,
            },
        );
    }
    Ok(())
}
