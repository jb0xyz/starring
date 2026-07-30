#[cfg(test)]
mod tests;

use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;
use std::time::Duration;

use crate::{
    RuntimeBindingPinV1, RuntimeBuildRevisionV1, RuntimeCertificationCanonicalErrorV2,
    RuntimeCertificationDivergenceV2, RuntimeCertificationOperationBuildErrorV2,
    RuntimeCertificationOperationIdV2, RuntimeCertificationOperationScopeV2,
    RuntimeConvergenceSessionError, RuntimeGatewayOwnerLeaseIdV1, RuntimePanelEvidenceV2,
    RuntimeReservedCertificationIntentV2, RuntimeSessionActionIdV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationReservationInputV2 {
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub binding_pin: RuntimeBindingPinV1,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub runtime_build_revision: RuntimeBuildRevisionV1,
    pub panel: RuntimePanelEvidenceV2,
    pub serving_lease_for: Duration,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCertificationSessionErrorV2 {
    #[error(transparent)]
    Session(#[from] RuntimeConvergenceSessionError),
    #[error(transparent)]
    Canonical(#[from] RuntimeCertificationCanonicalErrorV2),
    #[error(transparent)]
    Operation(#[from] RuntimeCertificationOperationBuildErrorV2),
    #[error("runtime certification reservation diverged")]
    Diverged {
        divergence: Box<RuntimeCertificationDivergenceV2>,
    },
}

pub struct RuntimeCertificationReservationAuthorityV2 {
    reservation: RuntimeReservedCertificationIntentV2,
}

impl RuntimeCertificationReservationAuthorityV2 {
    pub(crate) fn new(reservation: RuntimeReservedCertificationIntentV2) -> Self {
        Self { reservation }
    }

    pub fn action_id(&self) -> RuntimeSessionActionIdV1 {
        self.reservation.canonical_intent().intent().action_id
    }

    pub fn operation_scope(&self) -> &RuntimeCertificationOperationScopeV2 {
        self.reservation.operation_scope()
    }

    pub fn operation_id(&self) -> &RuntimeCertificationOperationIdV2 {
        self.reservation.operation_id()
    }

    pub fn certification_intent_bytes(&self) -> &[u8] {
        self.reservation.certification_intent_bytes()
    }

    pub fn intent_fingerprint(&self) -> &crate::RuntimeCertificationIntentFingerprintV2 {
        self.reservation.intent_fingerprint()
    }

    pub fn reserved_intent(&self) -> &RuntimeReservedCertificationIntentV2 {
        &self.reservation
    }

    pub fn into_reserved_intent(self) -> RuntimeReservedCertificationIntentV2 {
        self.reservation
    }
}

impl Debug for RuntimeCertificationReservationAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationReservationAuthorityV2(<redacted>)")
    }
}
