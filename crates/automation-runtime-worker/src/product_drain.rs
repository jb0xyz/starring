use std::future::Future;

use automation_runtime_controller::{
    RuntimeProductDrainScopeLookupV2, RuntimeProductDrainScopeObservationV2,
};

pub trait RuntimeProductDrainObservationPortV2 {
    type Error;

    fn observe_product_drain_scope(
        &self,
        lookup: RuntimeProductDrainScopeLookupV2,
    ) -> impl Future<Output = Result<RuntimeProductDrainScopeObservationV2, Self::Error>> + Send;
}
