mod wire;

#[cfg(test)]
mod tests;

use crate::v2_digest::certification_intent_fingerprint_v2;
use crate::{
    RuntimeCanonicalValueErrorV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationIntentV2,
};

const CERTIFICATION_INTENT_MAX_OCTETS: usize = 32_768;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationCanonicalRootV2 {
    Intent,
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
