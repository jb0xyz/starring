use automation_runtime_controller::{
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationReservationScopeLookupV2,
    RuntimeCertificationReservationScopeObservationV2, RuntimeReservedCertificationIntentV2,
};
use automation_runtime_worker::RuntimeCertificationReservationPortV2;

fn assert_port<T>()
where
    T: RuntimeCertificationReservationPortV2<Error = ()>,
{
}

fn assert_reserve_signature<T>(port: &T, reservation: RuntimeReservedCertificationIntentV2)
where
    T: RuntimeCertificationReservationPortV2<Error = ()>,
{
    let future = port.reserve_certification_intent(reservation);
    std::mem::drop(future);
}

fn assert_observe_signature<T>(port: &T, lookup: RuntimeCertificationReservationScopeLookupV2)
where
    T: RuntimeCertificationReservationPortV2<Error = ()>,
{
    let future = port.observe_certification_reservation_scope(lookup);
    std::mem::drop(future);
}

#[test]
fn certification_reservation_port_is_public_checked_and_scope_only() {
    let _ = assert_port::<NeverPort>;
    let _ = assert_reserve_signature::<NeverPort>;
    let _ = assert_observe_signature::<NeverPort>;
}

struct NeverPort;

impl RuntimeCertificationReservationPortV2 for NeverPort {
    type Error = ();

    async fn reserve_certification_intent(
        &self,
        _reservation: RuntimeReservedCertificationIntentV2,
    ) -> Result<RuntimeCertificationIntentReservationOutcomeV2, Self::Error> {
        std::future::pending().await
    }

    async fn observe_certification_reservation_scope(
        &self,
        _lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> Result<RuntimeCertificationReservationScopeObservationV2, Self::Error> {
        std::future::pending().await
    }
}
