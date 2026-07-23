use automation_runtime_controller::{
    RuntimeProductDrainScopeLookupV2, RuntimeProductDrainScopeObservationV2,
};
use automation_runtime_worker::RuntimeProductDrainObservationPortV2;

fn assert_port<T>()
where
    T: RuntimeProductDrainObservationPortV2<Error = ()>,
{
}

fn assert_signature<T>(port: &T, lookup: RuntimeProductDrainScopeLookupV2)
where
    T: RuntimeProductDrainObservationPortV2<Error = ()>,
{
    let future = port.observe_product_drain_scope(lookup);
    std::mem::drop(future);
}

#[test]
fn product_drain_observation_port_is_scope_only() {
    let _ = assert_port::<NeverPort>;
    let _ = assert_signature::<NeverPort>;
}

struct NeverPort;

impl RuntimeProductDrainObservationPortV2 for NeverPort {
    type Error = ();

    async fn observe_product_drain_scope(
        &self,
        _lookup: RuntimeProductDrainScopeLookupV2,
    ) -> Result<RuntimeProductDrainScopeObservationV2, Self::Error> {
        std::future::pending().await
    }
}
