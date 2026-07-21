use crate::{
    RuntimeCertificationReceiptV1, RuntimeCertificationRequestV1, RuntimeClaimNextExecutionV1,
    RuntimeDisconnectServingV1, RuntimeExecutionReceiptV1, RuntimeExecutionUpdateReceiptV1,
    RuntimeHeartbeatServingV1, RuntimeMutationReceiptV1, RuntimeMutationRequestV1,
    RuntimeRenewExecutionV1, RuntimeServingUpdateReceiptV1, RuntimeStaleLiveRecoveryReceiptV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConvergenceErrorClassV1 {
    Retryable,
    RetryNotReady,
    OwnershipLost,
    Superseded,
    AuthorityBlocked,
    InvalidState,
}

#[allow(async_fn_in_trait)]
pub trait RuntimeConvergencePort {
    type Error;

    async fn claim_next_execution(
        &self,
        request: RuntimeClaimNextExecutionV1,
    ) -> Result<Option<RuntimeExecutionReceiptV1>, Self::Error>;

    async fn renew_execution(
        &self,
        request: RuntimeRenewExecutionV1,
    ) -> Result<RuntimeExecutionUpdateReceiptV1, Self::Error>;

    async fn mutate(
        &self,
        request: RuntimeMutationRequestV1,
    ) -> Result<RuntimeMutationReceiptV1, Self::Error>;

    async fn certify_live(
        &self,
        request: RuntimeCertificationRequestV1,
    ) -> Result<RuntimeCertificationReceiptV1, Self::Error>;

    async fn heartbeat_serving(
        &self,
        request: RuntimeHeartbeatServingV1,
    ) -> Result<RuntimeServingUpdateReceiptV1, Self::Error>;

    async fn mark_serving_disconnected(
        &self,
        request: RuntimeDisconnectServingV1,
    ) -> Result<RuntimeServingUpdateReceiptV1, Self::Error>;

    async fn recover_next_stale_live(
        &self,
    ) -> Result<Option<RuntimeStaleLiveRecoveryReceiptV1>, Self::Error>;

    fn classify_error(error: &Self::Error) -> RuntimeConvergenceErrorClassV1;
}
