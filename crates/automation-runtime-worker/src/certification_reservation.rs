use std::future::Future;

use automation_runtime_controller::{
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationReservationScopeLookupV2,
    RuntimeCertificationReservationScopeObservationV2, RuntimeReservedCertificationIntentV2,
};

pub trait RuntimeCertificationReservationPortV2 {
    type Error;

    fn reserve_certification_intent(
        &self,
        reservation: RuntimeReservedCertificationIntentV2,
    ) -> impl Future<Output = Result<RuntimeCertificationIntentReservationOutcomeV2, Self::Error>> + Send;

    fn observe_certification_reservation_scope(
        &self,
        lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> impl Future<Output = Result<RuntimeCertificationReservationScopeObservationV2, Self::Error>>
           + Send;
}
