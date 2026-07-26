use automation_runtime_controller::{
    RuntimeCertificationReceiptV1, RuntimeCertificationRequestV1, RuntimeClaimNextExecutionV1,
    RuntimeConvergenceErrorClassV1, RuntimeExecutionConvergencePort, RuntimeExecutionReceiptV1,
    RuntimeExecutionUpdateReceiptV1, RuntimeMutationReceiptV1, RuntimeMutationRequestV1,
    RuntimeObservePreviousServingV1, RuntimePreviousServingObservationPort,
    RuntimePreviousServingObservationReceiptV1, RuntimeRenewExecutionV1,
    RuntimeStaleLiveRecoveryReceiptV1,
};

use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

impl RuntimeExecutionConvergencePort for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    async fn claim_next_execution(
        &self,
        request: RuntimeClaimNextExecutionV1,
    ) -> Result<Option<RuntimeExecutionReceiptV1>, Self::Error> {
        PostgresRuntimeExecutionV1::claim_next_execution(self, request).await
    }

    async fn renew_execution(
        &self,
        request: RuntimeRenewExecutionV1,
    ) -> Result<RuntimeExecutionUpdateReceiptV1, Self::Error> {
        PostgresRuntimeExecutionV1::renew_execution(self, request).await
    }

    async fn mutate(
        &self,
        request: RuntimeMutationRequestV1,
    ) -> Result<RuntimeMutationReceiptV1, Self::Error> {
        PostgresRuntimeExecutionV1::mutate(self, request).await
    }

    async fn certify_live(
        &self,
        request: RuntimeCertificationRequestV1,
    ) -> Result<RuntimeCertificationReceiptV1, Self::Error> {
        PostgresRuntimeExecutionV1::certify_live(self, request).await
    }

    async fn recover_next_stale_live(
        &self,
    ) -> Result<Option<RuntimeStaleLiveRecoveryReceiptV1>, Self::Error> {
        PostgresRuntimeExecutionV1::recover_next_stale_live(self).await
    }

    fn classify_error(error: &Self::Error) -> RuntimeConvergenceErrorClassV1 {
        error.class()
    }
}

impl RuntimePreviousServingObservationPort for PostgresRuntimeExecutionV1 {
    async fn observe_previous_serving(
        &self,
        request: RuntimeObservePreviousServingV1,
    ) -> Result<
        RuntimePreviousServingObservationReceiptV1,
        <Self as RuntimeExecutionConvergencePort>::Error,
    > {
        PostgresRuntimeExecutionV1::observe_previous_serving(self, request).await
    }
}
