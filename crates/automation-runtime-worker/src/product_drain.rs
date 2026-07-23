use std::future::Future;
use std::time::Duration;

use automation_runtime_controller::{
    RuntimeProductDrainScopeLookupV2, RuntimeProductDrainScopeObservationV2,
};

use crate::recovery::RuntimeRecoveryPendingV2;

pub trait RuntimeProductDrainObservationPortV2 {
    type Error;

    fn observe_product_drain_scope(
        &self,
        lookup: RuntimeProductDrainScopeLookupV2,
    ) -> impl Future<Output = Result<RuntimeProductDrainScopeObservationV2, Self::Error>> + Send;
}

#[must_use]
pub struct RuntimeProductDrainRecoveryOutcomeV2<W> {
    pub transaction_ended: W,
    pub observation: RuntimeProductDrainScopeObservationV2,
}

pub trait RuntimeProductDrainUnknownRecoveryPortV2: Sized {
    type Error;
    type TransactionEnded;

    fn lookup(&self) -> &RuntimeProductDrainScopeLookupV2;

    fn quiesce_and_observe(
        self,
        timeout: Duration,
    ) -> impl Future<
        Output = Result<
            RuntimeProductDrainRecoveryOutcomeV2<Self::TransactionEnded>,
            RuntimeRecoveryPendingV2<Self::Error, Self>,
        >,
    > + Send;
}
