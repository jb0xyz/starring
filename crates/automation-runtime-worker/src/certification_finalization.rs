#[cfg(test)]
mod tests;

use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Duration;

use automation_runtime_controller::{
    RuntimeBarrierIdV1, RuntimeCanonicalLiveAttestationV2, RuntimeCertificationDivergenceV2,
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationLookupV2,
    RuntimeCertificationObservationV2, RuntimeCertificationReceiptV2,
    RuntimeCertificationRequestV2, RuntimeCertificationReservationAuthorityV2,
    RuntimeCertificationReservationScopeLookupV2,
    RuntimeCertificationReservationScopeObservationKindV2,
    RuntimeCertificationReservationScopeObservationV2, RuntimeGatewayReadyKindV2,
    RuntimeLiveAttestationRecordV2, RuntimeReservedCertificationIntentV2,
    RuntimeRouteAdmissionAttestationV2,
};
use automation_runtime_convergence::{
    RuntimeDeployment, RuntimeDeploymentPhaseV1, TransitionOutcomeV1,
};

use crate::{
    RuntimeCertificationReservationPortV2, RuntimePausedGatewayObservationV2,
    RuntimeRecoveryPendingV2,
};

pub type RuntimeCertificationCommitResultV2<E, R, W> =
    Result<RuntimeCertificationReceiptV2, RuntimeCommitCompletionErrorV2<E, R, W>>;

pub type RuntimeCertificationAbortRecoveryResultV2<E, R, W> = Result<
    RuntimeDefinitelyRolledBackCertificationV2<W>,
    RuntimeRecoveryPendingV2<E, RuntimeCertificationAbortRecoveryV2<E, R>>,
>;

pub type RuntimeCertificationLookupRecoveryResultV2<E, R, W> = Result<
    RuntimeCertificationRecoveryResolutionV2<W>,
    RuntimeRecoveryPendingV2<E, RuntimeCertificationLookupOnlyRecoveryV2<E, R>>,
>;

pub trait RuntimeLiveCertificationPortV2 {
    type Error;
    type Prepared: RuntimePreparedLiveCertificationPortV2<Error = Self::Error> + Send;

    fn prepare_live_v2(
        &self,
        reservation: RuntimeReservedCertificationIntentV2,
    ) -> impl Future<Output = Result<Self::Prepared, Self::Error>> + Send;

    fn observe_live_v2(
        &self,
        lookup: RuntimeCertificationLookupV2,
    ) -> impl Future<Output = Result<RuntimeCertificationObservationV2, Self::Error>> + Send;
}

pub trait RuntimePreparedLiveCertificationPortV2: Sized {
    type Error;
    type TransactionEnded;
    type AbortRecovery: RuntimeAbortRecoveryPortV2<
        Error = Self::Error,
        TransactionEnded = Self::TransactionEnded,
    >;
    type CommitRecovery: RuntimeCommitRecoveryPortV2<
        Error = Self::Error,
        TransactionEnded = Self::TransactionEnded,
    >;

    fn must_commit_before(&self) -> chrono::DateTime<chrono::Utc>;

    fn commit_live_v2(
        self,
        authorized: RuntimeAuthorizedCertificationRequestV2,
    ) -> impl Future<
        Output = RuntimeCertificationCommitResultV2<
            Self::Error,
            Self::CommitRecovery,
            Self::TransactionEnded,
        >,
    > + Send;

    fn abort(
        self,
    ) -> impl Future<
        Output = Result<
            Self::TransactionEnded,
            RuntimeAbortErrorV2<Self::Error, Self::AbortRecovery>,
        >,
    > + Send;
}

pub trait RuntimeAbortRecoveryPortV2: Sized {
    type Error;
    type TransactionEnded;

    fn quiesce(
        self,
        timeout: Duration,
    ) -> impl Future<
        Output = Result<Self::TransactionEnded, RuntimeRecoveryPendingV2<Self::Error, Self>>,
    > + Send;
}

pub trait RuntimeCommitRecoveryPortV2: Sized {
    type Error;
    type TransactionEnded;

    fn lookup(&self) -> &RuntimeCertificationLookupV2;

    fn quiesce_and_observe(
        self,
        timeout: Duration,
    ) -> impl Future<
        Output = Result<
            RuntimeCertificationRecoveryOutcomeV2<Self::TransactionEnded>,
            RuntimeRecoveryPendingV2<Self::Error, Self>,
        >,
    > + Send;
}

#[must_use]
pub struct RuntimeCertificationReservationProposalV2 {
    reservation: RuntimeReservedCertificationIntentV2,
    lookup: RuntimeCertificationReservationScopeLookupV2,
}

impl RuntimeCertificationReservationProposalV2 {
    pub fn from_reserved_intent(
        reservation: RuntimeReservedCertificationIntentV2,
        lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> Result<Self, RuntimeCertificationReservationProposalErrorV2> {
        if reservation.operation_scope() != lookup.operation_scope() {
            return Err(RuntimeCertificationReservationProposalErrorV2::ScopeMismatch);
        }
        Ok(Self {
            reservation,
            lookup,
        })
    }

    pub fn reserved_intent(&self) -> &RuntimeReservedCertificationIntentV2 {
        &self.reservation
    }

    #[allow(clippy::manual_async_fn)]
    pub fn reserve<'a, P>(
        self,
        port: &'a P,
    ) -> impl Future<
        Output = Result<
            RuntimeCheckedCertificationReservationOutcomeV2,
            RuntimeCertificationReservationPortFailureV2<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimeCertificationReservationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        async move {
            let proposed = self.reservation.clone();
            let outcome = match port.reserve_certification_intent(proposed).await {
                Ok(outcome) => outcome,
                Err(source) => {
                    return Err(RuntimeCertificationReservationPortFailureV2 {
                        proposal: self,
                        source,
                    });
                }
            };
            Ok(accept_reservation(self.reservation, outcome))
        }
    }

    #[allow(clippy::manual_async_fn)]
    pub fn observe<'a, P>(
        self,
        port: &'a P,
    ) -> impl Future<
        Output = Result<
            RuntimeCheckedCertificationReservationObservationV2,
            RuntimeCertificationReservationPortFailureV2<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimeCertificationReservationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        async move {
            let observation = match port
                .observe_certification_reservation_scope(self.lookup.clone())
                .await
            {
                Ok(observation) => observation,
                Err(source) => {
                    return Err(RuntimeCertificationReservationPortFailureV2 {
                        proposal: self,
                        source,
                    });
                }
            };
            Ok(accept_reservation_observation(self, observation))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCertificationReservationProposalErrorV2 {
    #[error("runtime certification reservation lookup scope does not match")]
    ScopeMismatch,
}

impl Debug for RuntimeCertificationReservationProposalV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationReservationProposalV2(<redacted>)")
    }
}

pub struct RuntimeCheckedCertificationReservationOutcomeV2 {
    outcome: RuntimeCertificationIntentReservationOutcomeV2,
}

impl RuntimeCheckedCertificationReservationOutcomeV2 {
    pub fn into_session_outcome(self) -> RuntimeCertificationIntentReservationOutcomeV2 {
        self.outcome
    }
}

impl Debug for RuntimeCheckedCertificationReservationOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCheckedCertificationReservationOutcomeV2(<redacted>)")
    }
}

pub struct RuntimeCheckedCertificationReservationObservationV2 {
    observation: RuntimeCertificationReservationScopeObservationV2,
}

impl RuntimeCheckedCertificationReservationObservationV2 {
    pub fn into_observation(self) -> RuntimeCertificationReservationScopeObservationV2 {
        self.observation
    }
}

impl Debug for RuntimeCheckedCertificationReservationObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCheckedCertificationReservationObservationV2(<redacted>)")
    }
}

pub struct RuntimeCertificationReservationPortFailureV2<E> {
    pub proposal: RuntimeCertificationReservationProposalV2,
    pub source: E,
}

impl<E> Debug for RuntimeCertificationReservationPortFailureV2<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationReservationPortFailureV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimeReservedCertificationV2 {
    reservation: RuntimeReservedCertificationIntentV2,
}

impl RuntimeReservedCertificationV2 {
    pub fn from_reservation_authority(
        authority: RuntimeCertificationReservationAuthorityV2,
    ) -> Self {
        Self {
            reservation: authority.into_reserved_intent(),
        }
    }

    pub fn reserved_intent(&self) -> &RuntimeReservedCertificationIntentV2 {
        &self.reservation
    }

    #[allow(clippy::manual_async_fn)]
    pub fn prepare<'a, P>(
        self,
        port: &'a P,
    ) -> impl Future<
        Output = Result<
            RuntimePreparedCertificationV2<P::Prepared>,
            RuntimeCertificationPrepareFailedV2<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimeLiveCertificationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        async move {
            let reservation = self.reservation;
            let prepared = match port.prepare_live_v2(reservation.clone()).await {
                Ok(prepared) => prepared,
                Err(source) => {
                    return Err(RuntimeCertificationPrepareFailedV2 {
                        reservation: RuntimeReservedCertificationV2 { reservation },
                        source,
                    });
                }
            };
            Ok(RuntimePreparedCertificationV2 {
                reservation: RuntimeReservedCertificationV2 { reservation },
                prepared,
            })
        }
    }
}

impl Debug for RuntimeReservedCertificationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeReservedCertificationV2(<redacted>)")
    }
}

pub struct RuntimeCertificationPrepareFailedV2<E> {
    reservation: RuntimeReservedCertificationV2,
    source: E,
}

impl<E> RuntimeCertificationPrepareFailedV2<E> {
    pub fn reserved(&self) -> &RuntimeReservedCertificationV2 {
        &self.reservation
    }

    pub fn source(&self) -> &E {
        &self.source
    }

    pub fn into_parts(self) -> (RuntimeReservedCertificationV2, E) {
        (self.reservation, self.source)
    }
}

impl<E> Debug for RuntimeCertificationPrepareFailedV2<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationPrepareFailedV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimePreparedCertificationV2<P> {
    reservation: RuntimeReservedCertificationV2,
    prepared: P,
}

impl<P> RuntimePreparedCertificationV2<P>
where
    P: RuntimePreparedLiveCertificationPortV2,
{
    pub fn reserved_intent(&self) -> &RuntimeReservedCertificationIntentV2 {
        self.reservation.reserved_intent()
    }

    pub fn must_commit_before(&self) -> chrono::DateTime<chrono::Utc> {
        self.prepared.must_commit_before()
    }

    #[allow(clippy::manual_async_fn)]
    pub fn abort(
        self,
    ) -> impl Future<
        Output = RuntimeCertificationAbortOutcomeV2<
            P::Error,
            P::AbortRecovery,
            P::TransactionEnded,
        >,
    > + Send
    where
        P: Send,
    {
        async move {
            match self.prepared.abort().await {
                Ok(transaction_ended) => RuntimeCertificationAbortOutcomeV2::DefinitelyRolledBack(
                    RuntimeDefinitelyRolledBackCertificationV2 { transaction_ended },
                ),
                Err(error) => RuntimeCertificationAbortOutcomeV2::Indeterminate(
                    RuntimeCertificationAbortRecoveryV2 {
                        source: error.source,
                        recovery: error.recovery,
                    },
                ),
            }
        }
    }

    pub fn complete_barrier_b_v2(
        self,
        barrier_id: RuntimeBarrierIdV1,
        paused_gateway: RuntimePausedGatewayObservationV2,
        route_admission: RuntimeRouteAdmissionAttestationV2,
    ) -> Result<
        RuntimeCompletedCertificationBarrierBV2<P>,
        RuntimeCertificationBarrierBCompletionFailureV2<P>,
    > {
        let canonical = match canonicalize_barrier_b_completion_v2(
            &self,
            &barrier_id,
            &paused_gateway,
            route_admission,
        ) {
            Ok(canonical) => canonical,
            Err(source) => {
                return Err(RuntimeCertificationBarrierBCompletionFailureV2 {
                    prepared: Box::new(self),
                    source,
                });
            }
        };
        Ok(RuntimeCompletedCertificationBarrierBV2 {
            prepared: self.prepared,
            canonical,
        })
    }
}

fn canonicalize_barrier_b_completion_v2<P>(
    prepared: &RuntimePreparedCertificationV2<P>,
    barrier_id: &RuntimeBarrierIdV1,
    paused_gateway: &RuntimePausedGatewayObservationV2,
    route_admission: RuntimeRouteAdmissionAttestationV2,
) -> Result<RuntimeCanonicalLiveAttestationV2, RuntimeCertificationAuthorizationErrorV2>
where
    P: RuntimePreparedLiveCertificationPortV2,
{
    let intent = prepared
        .reservation
        .reserved_intent()
        .canonical_intent()
        .intent();
    if paused_gateway.process_instance_id() != &intent.process_identity.process_instance_id {
        return Err(RuntimeCertificationAuthorizationErrorV2::PausedGatewayMismatch);
    }
    if &route_admission.barrier_id != barrier_id {
        return Err(RuntimeCertificationAuthorizationErrorV2::BarrierIdMismatch);
    }
    if route_admission.pause.coordinator_generation.get()
        != paused_gateway.coordinator_generation().get()
        || route_admission.pause.connection_epoch != paused_gateway.connection_epoch()
        || route_admission.pause.paused_admission_revision != paused_gateway.admission_revision()
        || route_admission.pause.pause_sequence != paused_gateway.transition_sequence()
    {
        return Err(RuntimeCertificationAuthorizationErrorV2::PausedGatewayMismatch);
    }
    if route_admission.gateway.kind != RuntimeGatewayReadyKindV2::Resumed
        || route_admission.gateway.process_instance_id != *paused_gateway.process_instance_id()
        || route_admission.gateway.connection_epoch != paused_gateway.connection_epoch()
        || route_admission.gateway.admission_revision != paused_gateway.admission_revision()
        || route_admission.gateway.connected_event_sequence
            != paused_gateway.connected_event_sequence()
    {
        return Err(RuntimeCertificationAuthorizationErrorV2::ResumedGatewayMismatch);
    }
    let request = RuntimeCertificationRequestV2 {
        intent: intent.clone(),
        intent_fingerprint: prepared
            .reservation
            .reserved_intent()
            .intent_fingerprint()
            .clone(),
        must_commit_before: prepared.prepared.must_commit_before(),
        route_admission,
    };
    let record = RuntimeLiveAttestationRecordV2::from_request(request)?;
    prepared
        .reservation
        .reserved_intent()
        .canonical_intent()
        .bind_live_record(record)
        .map_err(Into::into)
}

impl<P> Debug for RuntimePreparedCertificationV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreparedCertificationV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimeCompletedCertificationBarrierBV2<P> {
    prepared: P,
    canonical: RuntimeCanonicalLiveAttestationV2,
}

impl<P> RuntimeCompletedCertificationBarrierBV2<P> {
    pub fn request(&self) -> &RuntimeCertificationRequestV2 {
        self.canonical.request()
    }

    pub fn authorize_finalization(self) -> RuntimeCertificationFinalizerRegistrationV2<P> {
        let authorized = RuntimeAuthorizedCertificationRequestV2 {
            canonical: self.canonical,
            authority: RuntimeCertificationCommitAuthorityV2 { _private: () },
        };
        RuntimeCertificationFinalizerRegistrationV2 {
            job: RuntimeCertificationFinalizerJobV2 {
                prepared: self.prepared,
                authorized,
            },
        }
    }
}

impl<P> Debug for RuntimeCompletedCertificationBarrierBV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCompletedCertificationBarrierBV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimeCertificationBarrierBCompletionFailureV2<P> {
    prepared: Box<RuntimePreparedCertificationV2<P>>,
    source: RuntimeCertificationAuthorizationErrorV2,
}

impl<P> RuntimeCertificationBarrierBCompletionFailureV2<P> {
    pub fn prepared(&self) -> &RuntimePreparedCertificationV2<P> {
        &self.prepared
    }

    pub fn source(&self) -> &RuntimeCertificationAuthorizationErrorV2 {
        &self.source
    }

    pub fn into_parts(
        self,
    ) -> (
        RuntimePreparedCertificationV2<P>,
        RuntimeCertificationAuthorizationErrorV2,
    ) {
        (*self.prepared, self.source)
    }
}

impl<P> Debug for RuntimeCertificationBarrierBCompletionFailureV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationBarrierBCompletionFailureV2(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeCertificationAuthorizationErrorV2 {
    #[error("runtime certification Barrier B identifier does not match")]
    BarrierIdMismatch,
    #[error("runtime certification Barrier B paused gateway evidence does not match")]
    PausedGatewayMismatch,
    #[error("runtime certification Barrier B resumed gateway evidence does not match")]
    ResumedGatewayMismatch,
    #[error("runtime certification request canonicalization failed")]
    Canonical(#[from] automation_runtime_controller::RuntimeCertificationCanonicalErrorV2),
}

struct RuntimeCertificationCommitAuthorityV2 {
    _private: (),
}

impl Debug for RuntimeCertificationCommitAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationCommitAuthorityV2(<redacted>)")
    }
}

pub struct RuntimeAuthorizedCertificationRequestV2 {
    canonical: RuntimeCanonicalLiveAttestationV2,
    authority: RuntimeCertificationCommitAuthorityV2,
}

impl RuntimeAuthorizedCertificationRequestV2 {
    pub fn request(&self) -> &RuntimeCertificationRequestV2 {
        self.canonical.request()
    }

    pub fn canonical(&self) -> &RuntimeCanonicalLiveAttestationV2 {
        &self.canonical
    }
}

impl Debug for RuntimeAuthorizedCertificationRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let _ = &self.authority;
        formatter.write_str("RuntimeAuthorizedCertificationRequestV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimeCertificationFinalizerRegistrationV2<P> {
    job: RuntimeCertificationFinalizerJobV2<P>,
}

impl<P> RuntimeCertificationFinalizerRegistrationV2<P> {
    pub fn request(&self) -> &RuntimeCertificationRequestV2 {
        self.job.authorized.request()
    }

    pub fn into_owned_job(self) -> RuntimeCertificationFinalizerJobV2<P> {
        self.job
    }

    pub fn accept<F>(
        self,
        finalizer: &F,
    ) -> Result<F::Accepted, RuntimeCertificationFinalizerRejectionV2<P, F::Error>>
    where
        F: RuntimeCertificationFinalizerPortV2<P>,
    {
        finalizer.accept_certification_finalizer(self)
    }
}

impl<P> Debug for RuntimeCertificationFinalizerRegistrationV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationFinalizerRegistrationV2(<redacted>)")
    }
}

pub trait RuntimeCertificationFinalizerPortV2<P> {
    type Error;
    type Accepted;

    fn accept_certification_finalizer(
        &self,
        registration: RuntimeCertificationFinalizerRegistrationV2<P>,
    ) -> Result<Self::Accepted, RuntimeCertificationFinalizerRejectionV2<P, Self::Error>>;
}

pub struct RuntimeCertificationFinalizerRejectionV2<P, E> {
    pub registration: Box<RuntimeCertificationFinalizerRegistrationV2<P>>,
    pub source: E,
}

impl<P, E> Debug for RuntimeCertificationFinalizerRejectionV2<P, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationFinalizerRejectionV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimeCertificationFinalizerJobV2<P> {
    prepared: P,
    authorized: RuntimeAuthorizedCertificationRequestV2,
}

impl<P> RuntimeCertificationFinalizerJobV2<P>
where
    P: RuntimePreparedLiveCertificationPortV2 + Send,
{
    pub fn into_registration(self) -> RuntimeCertificationFinalizerRegistrationV2<P> {
        RuntimeCertificationFinalizerRegistrationV2 { job: self }
    }

    pub fn request(&self) -> &RuntimeCertificationRequestV2 {
        self.authorized.request()
    }

    pub fn lookup(&self) -> RuntimeCertificationLookupV2 {
        lookup_for(self.authorized.canonical())
    }

    #[allow(clippy::manual_async_fn)]
    pub fn run(
        self,
    ) -> impl Future<
        Output = RuntimeCertificationFinalizationOutcomeV2<
            P::Error,
            P::CommitRecovery,
            P::TransactionEnded,
        >,
    > + Send {
        async move {
            let expected = self.authorized.canonical.clone();
            match self.prepared.commit_live_v2(self.authorized).await {
                Ok(receipt) => match accept_committed(&expected, receipt) {
                    Ok(committed) => {
                        RuntimeCertificationFinalizationOutcomeV2::Committed(Box::new(committed))
                    }
                    Err(rejected) => {
                        RuntimeCertificationFinalizationOutcomeV2::RejectedCommitted(rejected)
                    }
                },
                Err(RuntimeCommitCompletionErrorV2::DefinitelyRolledBack {
                    source,
                    transaction_ended,
                }) => RuntimeCertificationFinalizationOutcomeV2::DefinitelyRolledBack {
                    source,
                    rolled_back: RuntimeDefinitelyRolledBackCertificationV2 { transaction_ended },
                },
                Err(RuntimeCommitCompletionErrorV2::CommitUnknown { source, recovery }) => {
                    RuntimeCertificationFinalizationOutcomeV2::Indeterminate(Box::new(
                        RuntimeCertificationLookupOnlyRecoveryV2 {
                            expected,
                            source,
                            recovery,
                        },
                    ))
                }
            }
        }
    }
}

impl<P> Debug for RuntimeCertificationFinalizerJobV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationFinalizerJobV2(<redacted>)")
    }
}

pub enum RuntimeCertificationFinalizationOutcomeV2<E, R, W> {
    Committed(Box<RuntimeCommittedCertificationV2>),
    DefinitelyRolledBack {
        source: E,
        rolled_back: RuntimeDefinitelyRolledBackCertificationV2<W>,
    },
    Indeterminate(Box<RuntimeCertificationLookupOnlyRecoveryV2<E, R>>),
    RejectedCommitted(RuntimeRejectedCommittedCertificationV2),
}

impl<E, R, W> Debug for RuntimeCertificationFinalizationOutcomeV2<E, R, W> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationFinalizationOutcomeV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimeCommittedCertificationV2 {
    canonical: RuntimeCanonicalLiveAttestationV2,
    receipt: RuntimeCertificationReceiptV2,
}

impl RuntimeCommittedCertificationV2 {
    pub fn canonical(&self) -> &RuntimeCanonicalLiveAttestationV2 {
        &self.canonical
    }

    pub fn receipt(&self) -> &RuntimeCertificationReceiptV2 {
        &self.receipt
    }

    pub fn into_parts(
        self,
    ) -> (
        RuntimeCanonicalLiveAttestationV2,
        RuntimeCertificationReceiptV2,
    ) {
        (self.canonical, self.receipt)
    }
}

impl Debug for RuntimeCommittedCertificationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCommittedCertificationV2(<redacted>)")
    }
}

pub struct RuntimeRejectedCommittedCertificationV2 {
    receipt: Box<RuntimeCertificationReceiptV2>,
}

impl RuntimeRejectedCommittedCertificationV2 {
    pub fn receipt(&self) -> &RuntimeCertificationReceiptV2 {
        &self.receipt
    }

    pub fn divergence(&self) -> RuntimeCertificationDivergenceV2 {
        RuntimeCertificationDivergenceV2::CommittedRequestMismatch
    }
}

impl Debug for RuntimeRejectedCommittedCertificationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRejectedCommittedCertificationV2(<redacted>)")
    }
}

pub struct RuntimeDefinitelyRolledBackCertificationV2<W> {
    transaction_ended: W,
}

impl<W> RuntimeDefinitelyRolledBackCertificationV2<W> {
    pub fn transaction_ended(&self) -> &W {
        &self.transaction_ended
    }

    pub fn into_transaction_ended(self) -> W {
        self.transaction_ended
    }
}

impl<W> Debug for RuntimeDefinitelyRolledBackCertificationV2<W> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDefinitelyRolledBackCertificationV2(<redacted>)")
    }
}

pub enum RuntimeCertificationAbortOutcomeV2<E, R, W> {
    DefinitelyRolledBack(RuntimeDefinitelyRolledBackCertificationV2<W>),
    Indeterminate(RuntimeCertificationAbortRecoveryV2<E, R>),
}

impl<E, R, W> Debug for RuntimeCertificationAbortOutcomeV2<E, R, W> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationAbortOutcomeV2(<redacted>)")
    }
}

pub struct RuntimeCertificationAbortRecoveryV2<E, R> {
    source: E,
    recovery: R,
}

impl<E, R> RuntimeCertificationAbortRecoveryV2<E, R>
where
    R: RuntimeAbortRecoveryPortV2<Error = E>,
{
    pub fn source(&self) -> &E {
        &self.source
    }

    #[allow(clippy::manual_async_fn)]
    pub fn quiesce(
        self,
        timeout: Duration,
    ) -> impl Future<Output = RuntimeCertificationAbortRecoveryResultV2<E, R, R::TransactionEnded>> + Send
    where
        E: Send,
        R: Send,
    {
        async move {
            match self.recovery.quiesce(timeout).await {
                Ok(transaction_ended) => {
                    Ok(RuntimeDefinitelyRolledBackCertificationV2 { transaction_ended })
                }
                Err(pending) => Err(RuntimeRecoveryPendingV2 {
                    source: pending.source,
                    recovery: RuntimeCertificationAbortRecoveryV2 {
                        source: self.source,
                        recovery: pending.recovery,
                    },
                }),
            }
        }
    }
}

impl<E, R> Debug for RuntimeCertificationAbortRecoveryV2<E, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationAbortRecoveryV2(<redacted>)")
    }
}

pub struct RuntimeCertificationLookupOnlyRecoveryV2<E, R> {
    expected: RuntimeCanonicalLiveAttestationV2,
    source: E,
    recovery: R,
}

impl<E, R> RuntimeCertificationLookupOnlyRecoveryV2<E, R>
where
    R: RuntimeCommitRecoveryPortV2<Error = E>,
{
    pub fn source(&self) -> &E {
        &self.source
    }

    pub fn lookup(&self) -> &RuntimeCertificationLookupV2 {
        self.recovery.lookup()
    }

    #[allow(clippy::manual_async_fn)]
    pub fn quiesce_and_observe(
        self,
        timeout: Duration,
    ) -> impl Future<Output = RuntimeCertificationLookupRecoveryResultV2<E, R, R::TransactionEnded>> + Send
    where
        E: Send,
        R: Send,
    {
        async move {
            match self.recovery.quiesce_and_observe(timeout).await {
                Ok(outcome) => Ok(accept_recovery(&self.expected, outcome)),
                Err(pending) => Err(RuntimeRecoveryPendingV2 {
                    source: pending.source,
                    recovery: RuntimeCertificationLookupOnlyRecoveryV2 {
                        expected: self.expected,
                        source: self.source,
                        recovery: pending.recovery,
                    },
                }),
            }
        }
    }
}

impl<E, R> Debug for RuntimeCertificationLookupOnlyRecoveryV2<E, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationLookupOnlyRecoveryV2(<redacted>)")
    }
}

#[must_use]
pub struct RuntimeCertificationRecoveryOutcomeV2<W> {
    pub transaction_ended: W,
    pub observation: RuntimeCertificationObservationV2,
}

pub enum RuntimeCertificationRecoveryResolutionV2<W> {
    Committed {
        transaction_ended: W,
        committed: Box<RuntimeCommittedCertificationV2>,
    },
    DefinitelyRolledBack {
        transaction_ended: W,
        observation: Box<RuntimeCertificationObservationV2>,
    },
    Diverged {
        transaction_ended: W,
        divergence: Box<RuntimeCertificationDivergenceV2>,
    },
    Rejected {
        transaction_ended: W,
        observation: Box<RuntimeCertificationObservationV2>,
    },
}

impl<W> Debug for RuntimeCertificationRecoveryResolutionV2<W> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationRecoveryResolutionV2(<redacted>)")
    }
}

pub enum RuntimeCommitCompletionErrorV2<E, R, W> {
    DefinitelyRolledBack { source: E, transaction_ended: W },
    CommitUnknown { source: E, recovery: R },
}

impl<E, R, W> Debug for RuntimeCommitCompletionErrorV2<E, R, W> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCommitCompletionErrorV2(<redacted>)")
    }
}

pub struct RuntimeAbortErrorV2<E, R> {
    pub source: E,
    pub recovery: R,
}

impl<E, R> Debug for RuntimeAbortErrorV2<E, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAbortErrorV2(<redacted>)")
    }
}

fn accept_reservation(
    proposed: RuntimeReservedCertificationIntentV2,
    outcome: RuntimeCertificationIntentReservationOutcomeV2,
) -> RuntimeCheckedCertificationReservationOutcomeV2 {
    let outcome = match outcome {
        RuntimeCertificationIntentReservationOutcomeV2::Reserved(observed) => {
            if observed.require_byte_exact_replay(&proposed).is_ok() {
                RuntimeCertificationIntentReservationOutcomeV2::Reserved(observed)
            } else {
                RuntimeCertificationIntentReservationOutcomeV2::Diverged(
                    RuntimeCertificationDivergenceV2::ReservationMismatch,
                )
            }
        }
        RuntimeCertificationIntentReservationOutcomeV2::Diverged(divergence) => {
            RuntimeCertificationIntentReservationOutcomeV2::Diverged(divergence)
        }
    };
    RuntimeCheckedCertificationReservationOutcomeV2 { outcome }
}

fn accept_reservation_observation(
    proposal: RuntimeCertificationReservationProposalV2,
    observation: RuntimeCertificationReservationScopeObservationV2,
) -> RuntimeCheckedCertificationReservationObservationV2 {
    let exact = match observation.kind() {
        RuntimeCertificationReservationScopeObservationKindV2::Absent { lookup, .. } => {
            lookup == &proposal.lookup
        }
        RuntimeCertificationReservationScopeObservationKindV2::Reserved {
            lookup,
            reservation,
            ..
        } => {
            lookup == &proposal.lookup
                && reservation
                    .require_byte_exact_replay(&proposal.reservation)
                    .is_ok()
        }
        RuntimeCertificationReservationScopeObservationKindV2::Diverged(_) => true,
    };
    if exact {
        RuntimeCheckedCertificationReservationObservationV2 { observation }
    } else {
        RuntimeCheckedCertificationReservationObservationV2 {
            observation: RuntimeCertificationReservationScopeObservationV2::diverged(
                RuntimeCertificationDivergenceV2::ReservationMismatch,
            ),
        }
    }
}

fn lookup_for(canonical: &RuntimeCanonicalLiveAttestationV2) -> RuntimeCertificationLookupV2 {
    let request = canonical.request();
    RuntimeCertificationLookupV2 {
        scope: request.intent.guard.scope.clone(),
        deployment_revision: request.intent.guard.expected_revision,
        convergence_attempt: request.intent.guard.convergence_attempt,
        operation_id: request.intent.operation_id.clone(),
        request_digest: canonical.request_digest().clone(),
    }
}

fn accept_committed(
    expected: &RuntimeCanonicalLiveAttestationV2,
    receipt: RuntimeCertificationReceiptV2,
) -> Result<RuntimeCommittedCertificationV2, RuntimeRejectedCommittedCertificationV2> {
    if committed_matches(expected, &receipt) {
        Ok(RuntimeCommittedCertificationV2 {
            canonical: expected.clone(),
            receipt,
        })
    } else {
        Err(RuntimeRejectedCommittedCertificationV2 {
            receipt: Box::new(receipt),
        })
    }
}

fn committed_matches(
    expected: &RuntimeCanonicalLiveAttestationV2,
    receipt: &RuntimeCertificationReceiptV2,
) -> bool {
    let request = expected.request();
    let intent = &request.intent;
    let Ok(live_revision) = intent.guard.expected_revision.next() else {
        return false;
    };
    if RuntimeDeployment::restore(receipt.snapshot.clone()).is_err()
        || !matches!(receipt.snapshot.phase, RuntimeDeploymentPhaseV1::Live)
        || receipt.snapshot.revision != live_revision
        || !intent.guard.scope.matches(&receipt.snapshot.identity)
        || receipt.snapshot.target != intent.target
        || receipt.snapshot.runtime_generation != intent.guard.runtime_generation
        || receipt.snapshot.controller_lease.is_some()
        || receipt.action_id != intent.action_id
        || receipt.outcome.revision() != live_revision
        || !matches!(
            receipt.outcome,
            TransitionOutcomeV1::Applied { .. } | TransitionOutcomeV1::Replayed { .. }
        )
        || receipt.convergence_attempt != intent.guard.convergence_attempt
        || receipt.operation_id != intent.operation_id
        || receipt.intent_fingerprint != request.intent_fingerprint
        || receipt.request_digest != *expected.request_digest()
        || receipt.attestation_digest != *expected.live_attestation_digest()
        || receipt.route_admission != request.route_admission
        || receipt.certified_at > request.must_commit_before
        || receipt.serving.identity.scope != intent.guard.scope
        || receipt.serving.identity.operation_id != intent.operation_id
        || receipt.serving.identity.attestation_digest != *expected.live_attestation_digest()
        || receipt.serving.identity.process_identity != intent.process_identity
        || receipt.serving.acquired_at != receipt.certified_at
        || receipt.serving.last_heartbeat_at < receipt.serving.acquired_at
        || receipt.serving.last_heartbeat_at >= receipt.serving.expires_at
        || !receipt.serving.connected
        || !receipt.serving.serving
    {
        return false;
    }
    true
}

fn accept_recovery<W>(
    expected: &RuntimeCanonicalLiveAttestationV2,
    outcome: RuntimeCertificationRecoveryOutcomeV2<W>,
) -> RuntimeCertificationRecoveryResolutionV2<W> {
    let RuntimeCertificationRecoveryOutcomeV2 {
        transaction_ended,
        observation,
    } = outcome;
    match observation {
        RuntimeCertificationObservationV2::Committed(receipt) => {
            match accept_committed(expected, receipt) {
                Ok(committed) => RuntimeCertificationRecoveryResolutionV2::Committed {
                    transaction_ended,
                    committed: Box::new(committed),
                },
                Err(rejected) => RuntimeCertificationRecoveryResolutionV2::Rejected {
                    transaction_ended,
                    observation: Box::new(RuntimeCertificationObservationV2::Committed(
                        *rejected.receipt,
                    )),
                },
            }
        }
        RuntimeCertificationObservationV2::NotCommitted { .. } => {
            if not_committed_matches(expected, &observation) {
                RuntimeCertificationRecoveryResolutionV2::DefinitelyRolledBack {
                    transaction_ended,
                    observation: Box::new(observation),
                }
            } else {
                RuntimeCertificationRecoveryResolutionV2::Rejected {
                    transaction_ended,
                    observation: Box::new(observation),
                }
            }
        }
        RuntimeCertificationObservationV2::Diverged(divergence) => {
            RuntimeCertificationRecoveryResolutionV2::Diverged {
                transaction_ended,
                divergence: Box::new(divergence),
            }
        }
    }
}

fn not_committed_matches(
    expected: &RuntimeCanonicalLiveAttestationV2,
    observation: &RuntimeCertificationObservationV2,
) -> bool {
    let RuntimeCertificationObservationV2::NotCommitted {
        snapshot,
        convergence_attempt,
        operation_id,
        request_digest,
        observed_deployment_revision,
        ..
    } = observation
    else {
        return false;
    };
    let request = expected.request();
    let intent = &request.intent;
    RuntimeDeployment::restore(snapshot.clone()).is_ok()
        && matches!(
            snapshot.phase,
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady
        )
        && snapshot.revision == intent.guard.expected_revision
        && intent.guard.scope.matches(&snapshot.identity)
        && snapshot.target == intent.target
        && snapshot.runtime_generation == intent.guard.runtime_generation
        && *convergence_attempt == intent.guard.convergence_attempt
        && *operation_id == intent.operation_id
        && *request_digest == *expected.request_digest()
        && *observed_deployment_revision == intent.guard.expected_revision
}
