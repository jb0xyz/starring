use std::fmt::{Debug, Formatter};
use std::time::Duration;

use automation_runtime::{
    AcquiredInteractionLifecyclePermitV1, AuthoritativeInteractionClaimV1,
    InteractionEffectPermitV1, InteractionInitialResponseIntentDispositionV1,
    InteractionInitialResponseIntentV1, InteractionInitialResponseResultV1,
    InteractionTerminalFinishV1, SharedGatewayDurableReceiptClaimInputV1,
    SharedGatewayInteractionIdentityV3, SharedGatewayInteractionKindV3,
    ACQUIRED_INTERACTION_CLAIM_LEASE_V1,
};
use automation_runtime_interaction::{
    InteractionActionPlanDigestV1, InteractionReceiptClaimRootV1,
};
use tokio::sync::Mutex;
use tokio::time::{timeout_at, Instant};

pub(crate) const RUNTIME_INTERACTION_RECEIPT_CLAIM_DEADLINE_V1: Duration =
    Duration::from_millis(600);
pub(crate) const RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE_V1: Duration =
    ACQUIRED_INTERACTION_CLAIM_LEASE_V1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionReceiptClosedReasonV1 {
    InvalidInput,
    InvalidAuthority,
    Conflict,
    PersistenceCorrupt,
    Timeout,
    Unavailable,
    Indeterminate,
    TokenEnvelope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionReceiptDuplicateClassV1 {
    Completed,
    InFlight,
    Terminal,
    RecoveryRequired,
}

pub(crate) enum RuntimeInteractionReceiptClaimDispositionV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    Acquired(Box<RuntimeAcquiredInteractionPermitV1<P>>),
    Duplicate(RuntimeInteractionReceiptDuplicateClassV1),
    Closed(RuntimeInteractionReceiptClosedReasonV1),
}

impl<P> Debug for RuntimeInteractionReceiptClaimDispositionV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acquired(_) => formatter.write_str("Acquired(<redacted>)"),
            Self::Duplicate(class) => formatter.debug_tuple("Duplicate").field(class).finish(),
            Self::Closed(reason) => formatter.debug_tuple("Closed").field(reason).finish(),
        }
    }
}

pub(crate) enum RuntimeInteractionReceiptPersistenceClaimOutcomeV1<C> {
    Acquired(C),
    CompletedDuplicate,
    InFlightDuplicate,
    TerminalDuplicate,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionReceiptPersistenceMutationDispositionV1 {
    Applied,
    ExactReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeInteractionReceiptPermitErrorV1 {
    #[error("runtime interaction receipt persistence rejected the operation")]
    Persistence(RuntimeInteractionReceiptClosedReasonV1),
    #[error("runtime interaction receipt contract rejected the operation")]
    Contract,
    #[error("runtime interaction receipt execution replay is not authorized")]
    ExecutionReplayNotAuthorized,
}

pub(crate) trait RuntimeInteractionReceiptPersistencePortV1:
    Clone + Send + Sync + 'static
{
    type Claim: Send;

    fn claim_root_v1(claim: &Self::Claim) -> &InteractionReceiptClaimRootV1;

    async fn claim_receipt_v1(
        &self,
        input: SharedGatewayDurableReceiptClaimInputV1,
        identity: SharedGatewayInteractionIdentityV3,
        kind: SharedGatewayInteractionKindV3,
    ) -> Result<
        RuntimeInteractionReceiptPersistenceClaimOutcomeV1<Self::Claim>,
        RuntimeInteractionReceiptClosedReasonV1,
    >;

    async fn commit_initial_response_intent_v1(
        &self,
        claim: &mut Self::Claim,
        intent: &InteractionInitialResponseIntentV1,
    ) -> Result<InteractionInitialResponseIntentDispositionV1, RuntimeInteractionReceiptPermitErrorV1>;

    async fn commit_initial_response_result_v1(
        &self,
        claim: &mut Self::Claim,
        result: &InteractionInitialResponseResultV1,
    ) -> Result<(), RuntimeInteractionReceiptPermitErrorV1>;

    async fn commit_action_plan_v1(
        &self,
        claim: &mut Self::Claim,
        digest: &InteractionActionPlanDigestV1,
    ) -> Result<(), RuntimeInteractionReceiptPermitErrorV1>;

    async fn commit_execution_intent_v1(
        &self,
        claim: &mut Self::Claim,
    ) -> Result<
        RuntimeInteractionReceiptPersistenceMutationDispositionV1,
        RuntimeInteractionReceiptPermitErrorV1,
    >;

    async fn commit_terminal_v1(
        &self,
        claim: &mut Self::Claim,
        finish: &InteractionTerminalFinishV1,
    ) -> Result<(), RuntimeInteractionReceiptPermitErrorV1>;
}

struct RuntimeInteractionReceiptCheckpointV1<C> {
    claim: C,
    execution_intent_applied_in_this_permit: bool,
}

impl<C> RuntimeInteractionReceiptCheckpointV1<C> {
    fn authorize_execution_v1(
        &mut self,
        disposition: RuntimeInteractionReceiptPersistenceMutationDispositionV1,
    ) -> Result<(), RuntimeInteractionReceiptPermitErrorV1> {
        authorize_execution_disposition_v1(
            &mut self.execution_intent_applied_in_this_permit,
            disposition,
        )
    }
}

fn authorize_execution_disposition_v1(
    applied_in_this_permit: &mut bool,
    disposition: RuntimeInteractionReceiptPersistenceMutationDispositionV1,
) -> Result<(), RuntimeInteractionReceiptPermitErrorV1> {
    match disposition {
        RuntimeInteractionReceiptPersistenceMutationDispositionV1::Applied => {
            *applied_in_this_permit = true;
            Ok(())
        }
        RuntimeInteractionReceiptPersistenceMutationDispositionV1::ExactReplay
            if *applied_in_this_permit =>
        {
            Ok(())
        }
        RuntimeInteractionReceiptPersistenceMutationDispositionV1::ExactReplay => {
            Err(RuntimeInteractionReceiptPermitErrorV1::ExecutionReplayNotAuthorized)
        }
    }
}

pub(crate) struct RuntimeAcquiredInteractionPermitV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    persistence: P,
    claim_root: InteractionReceiptClaimRootV1,
    initial_response_deadline: Instant,
    checkpoint: Mutex<RuntimeInteractionReceiptCheckpointV1<P::Claim>>,
}

impl<P> RuntimeAcquiredInteractionPermitV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    fn new(persistence: P, claim: P::Claim, initial_response_deadline: Instant) -> Self {
        let claim_root = P::claim_root_v1(&claim).clone();
        Self {
            persistence,
            claim_root,
            initial_response_deadline,
            checkpoint: Mutex::new(RuntimeInteractionReceiptCheckpointV1 {
                claim,
                execution_intent_applied_in_this_permit: false,
            }),
        }
    }
}

impl<P> Debug for RuntimeAcquiredInteractionPermitV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcquiredInteractionPermitV1(<redacted>)")
    }
}

pub(crate) async fn claim_runtime_interaction_receipt_v1<P>(
    persistence: &P,
    input: SharedGatewayDurableReceiptClaimInputV1,
    identity: SharedGatewayInteractionIdentityV3,
    kind: SharedGatewayInteractionKindV3,
    initial_response_deadline: Instant,
) -> RuntimeInteractionReceiptClaimDispositionV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    let deadline = std::cmp::min(
        initial_response_deadline,
        Instant::now() + RUNTIME_INTERACTION_RECEIPT_CLAIM_DEADLINE_V1,
    );
    let result = timeout_at(
        deadline,
        persistence.claim_receipt_v1(input, identity, kind),
    )
    .await;
    match result {
        Ok(Ok(RuntimeInteractionReceiptPersistenceClaimOutcomeV1::Acquired(claim))) => {
            RuntimeInteractionReceiptClaimDispositionV1::Acquired(Box::new(
                RuntimeAcquiredInteractionPermitV1::new(
                    persistence.clone(),
                    claim,
                    initial_response_deadline,
                ),
            ))
        }
        Ok(Ok(RuntimeInteractionReceiptPersistenceClaimOutcomeV1::CompletedDuplicate)) => {
            RuntimeInteractionReceiptClaimDispositionV1::Duplicate(
                RuntimeInteractionReceiptDuplicateClassV1::Completed,
            )
        }
        Ok(Ok(RuntimeInteractionReceiptPersistenceClaimOutcomeV1::InFlightDuplicate)) => {
            RuntimeInteractionReceiptClaimDispositionV1::Duplicate(
                RuntimeInteractionReceiptDuplicateClassV1::InFlight,
            )
        }
        Ok(Ok(RuntimeInteractionReceiptPersistenceClaimOutcomeV1::TerminalDuplicate)) => {
            RuntimeInteractionReceiptClaimDispositionV1::Duplicate(
                RuntimeInteractionReceiptDuplicateClassV1::Terminal,
            )
        }
        Ok(Ok(RuntimeInteractionReceiptPersistenceClaimOutcomeV1::RecoveryRequired)) => {
            RuntimeInteractionReceiptClaimDispositionV1::Duplicate(
                RuntimeInteractionReceiptDuplicateClassV1::RecoveryRequired,
            )
        }
        Ok(Err(reason)) => RuntimeInteractionReceiptClaimDispositionV1::Closed(reason),
        Err(_) => RuntimeInteractionReceiptClaimDispositionV1::Closed(
            RuntimeInteractionReceiptClosedReasonV1::Timeout,
        ),
    }
}

impl<P> InteractionEffectPermitV1 for RuntimeAcquiredInteractionPermitV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    type Error = RuntimeInteractionReceiptPermitErrorV1;

    fn initial_response_deadline_v1(&self) -> Instant {
        self.initial_response_deadline
    }

    async fn commit_initial_response_intent_v1(
        &self,
        intent: &InteractionInitialResponseIntentV1,
    ) -> Result<InteractionInitialResponseIntentDispositionV1, Self::Error> {
        let mut checkpoint = self.checkpoint.lock().await;
        self.persistence
            .commit_initial_response_intent_v1(&mut checkpoint.claim, intent)
            .await
    }

    async fn commit_initial_response_result_v1(
        &self,
        result: &InteractionInitialResponseResultV1,
    ) -> Result<(), Self::Error> {
        let mut checkpoint = self.checkpoint.lock().await;
        self.persistence
            .commit_initial_response_result_v1(&mut checkpoint.claim, result)
            .await
    }

    async fn commit_idempotent_execution_intent_v1(&self) -> Result<(), Self::Error> {
        let mut checkpoint = self.checkpoint.lock().await;
        let disposition = self
            .persistence
            .commit_execution_intent_v1(&mut checkpoint.claim)
            .await?;
        checkpoint.authorize_execution_v1(disposition)
    }
}

impl<P> AcquiredInteractionLifecyclePermitV1 for RuntimeAcquiredInteractionPermitV1<P>
where
    P: RuntimeInteractionReceiptPersistencePortV1,
{
    fn authoritative_claim_v1(&self) -> AuthoritativeInteractionClaimV1<'_> {
        AuthoritativeInteractionClaimV1::new(&self.claim_root)
    }

    async fn bind_action_plan_digest_v1(
        &self,
        digest: &InteractionActionPlanDigestV1,
    ) -> Result<(), Self::Error> {
        let mut checkpoint = self.checkpoint.lock().await;
        self.persistence
            .commit_action_plan_v1(&mut checkpoint.claim, digest)
            .await
    }

    async fn finish_interaction_v1(
        &self,
        finish: &InteractionTerminalFinishV1,
    ) -> Result<(), Self::Error> {
        let mut checkpoint = self.checkpoint.lock().await;
        self.persistence
            .commit_terminal_v1(&mut checkpoint.claim, finish)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_exact_replay_requires_applied_intent_in_the_same_live_permit() {
        let mut fresh = false;
        assert_eq!(
            authorize_execution_disposition_v1(
                &mut fresh,
                RuntimeInteractionReceiptPersistenceMutationDispositionV1::ExactReplay,
            ),
            Err(RuntimeInteractionReceiptPermitErrorV1::ExecutionReplayNotAuthorized)
        );
        assert!(authorize_execution_disposition_v1(
            &mut fresh,
            RuntimeInteractionReceiptPersistenceMutationDispositionV1::Applied,
        )
        .is_ok());
        assert!(authorize_execution_disposition_v1(
            &mut fresh,
            RuntimeInteractionReceiptPersistenceMutationDispositionV1::ExactReplay,
        )
        .is_ok());
    }

    #[test]
    fn permit_failures_are_finite_and_redacted() {
        let error = RuntimeInteractionReceiptPermitErrorV1::Persistence(
            RuntimeInteractionReceiptClosedReasonV1::PersistenceCorrupt,
        );
        assert_eq!(
            error.to_string(),
            "runtime interaction receipt persistence rejected the operation"
        );
    }

    #[tokio::test]
    async fn absolute_claim_deadline_closes_a_stalled_operation() {
        let started = Instant::now();
        let deadline = started + RUNTIME_INTERACTION_RECEIPT_CLAIM_DEADLINE_V1;
        let result = timeout_at(deadline, std::future::pending::<()>()).await;
        assert!(result.is_err());
        assert!(started.elapsed() >= RUNTIME_INTERACTION_RECEIPT_CLAIM_DEADLINE_V1);
    }
}
