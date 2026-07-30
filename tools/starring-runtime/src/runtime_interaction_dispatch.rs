use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use automation_runtime::{
    GatewayConnectionObserverV3, GatewayReadyLeaseV3, InteractionExecutionOutcomeV3,
    SharedGatewayAdmissionErrorV3, SharedGatewayInteractionDispatchOutcomeV3,
    SharedGatewayInteractionEnvelopeV3, SharedGatewayInteractionRejectionV3,
    SharedGatewayRejectionAcknowledgementOutcomeV3,
};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use tokio::time::Instant as TokioInstant;

use crate::database::RuntimeInteractionDispatchDatabasePortV1;
use crate::GatewayResourceConfigV1;

pub(crate) type RuntimeInteractionDispatchFutureV1 =
    Pin<Box<dyn Future<Output = SharedGatewayInteractionDispatchOutcomeV3> + Send + 'static>>;
pub(crate) type RuntimeInteractionRejectionFutureV1 =
    Pin<Box<dyn Future<Output = SharedGatewayRejectionAcknowledgementOutcomeV3> + Send + 'static>>;

pub(crate) enum RuntimeInteractionDispatchReservationOutcomeV1<R> {
    Reserved(R),
    Ignored,
    Rejected {
        reason: SharedGatewayInteractionRejectionV3,
        envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    },
}

pub(crate) trait RuntimeInteractionDispatchPortV1: Send + Sync + 'static {
    type Reservation: Send + 'static;

    fn dispatch_capacity_v1(&self) -> NonZeroUsize;

    fn reserve_v1(
        &self,
        envelope: SharedGatewayInteractionEnvelopeV3,
        ready_lease: Option<GatewayReadyLeaseV3>,
        observer: &GatewayConnectionObserverV3,
    ) -> RuntimeInteractionDispatchReservationOutcomeV1<Self::Reservation>;

    fn cancel_v1(&self, reservation: Self::Reservation) -> Box<SharedGatewayInteractionEnvelopeV3>;

    fn dispatch_v1(
        self: Arc<Self>,
        reservation: Self::Reservation,
    ) -> RuntimeInteractionDispatchFutureV1;

    fn acknowledge_rejection_v1(
        self: Arc<Self>,
        envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    ) -> RuntimeInteractionRejectionFutureV1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionRejectionSchedulingV1 {
    Enqueued,
    Dropped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionEnqueueOutcomeV1 {
    Enqueued,
    Ignored,
    Rejected {
        acknowledgement: RuntimeInteractionRejectionSchedulingV1,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionCompletionV1 {
    Dispatch,
    RejectionAcknowledgement,
    Idle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionDrainOutcomeV1 {
    Clean,
    DeadlineExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeInteractionReopenErrorV1 {
    #[error("runtime interaction lane is already accepting")]
    AlreadyAccepting,
    #[error("runtime interaction lane is permanently sealed")]
    Sealed,
    #[error("runtime interaction lane still has retained work")]
    InFlight,
    #[error("runtime interaction lane product readiness is unavailable")]
    ProductNotReady,
    #[error("runtime interaction lane gateway readiness is unavailable")]
    GatewayNotReady,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeInteractionLaneStateV1 {
    Accepting,
    Paused,
    Sealed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionDispatchReportV1 {
    pub(crate) enqueued: u64,
    pub(crate) completed: u64,
    pub(crate) execution_failed: u64,
    pub(crate) ignored: u64,
    pub(crate) not_accepting: u64,
    pub(crate) product_not_ready: u64,
    pub(crate) route_rejected: u64,
    pub(crate) gateway_not_ready: u64,
    pub(crate) overloaded: u64,
    pub(crate) admission_rejected: u64,
    pub(crate) rejection_acknowledged: u64,
    pub(crate) rejection_acknowledgement_failed: u64,
    pub(crate) rejection_acknowledgement_timed_out: u64,
    pub(crate) rejection_acknowledgement_dropped: u64,
    pub(crate) dispatch_cancelled: u64,
    pub(crate) rejection_acknowledgement_cancelled: u64,
}

pub(crate) struct RuntimeDiscordInteractionDispatchLaneV1<P>
where
    P: RuntimeInteractionDispatchPortV1,
{
    port: Arc<P>,
    dispatch_capacity: NonZeroUsize,
    rejection_capacity: NonZeroUsize,
    dispatches: FuturesUnordered<RuntimeInteractionDispatchFutureV1>,
    rejections: FuturesUnordered<RuntimeInteractionRejectionFutureV1>,
    state: RuntimeInteractionLaneStateV1,
    report: RuntimeInteractionDispatchReportV1,
}

pub(crate) type RuntimeProductionDiscordInteractionDispatchLaneV1 =
    RuntimeDiscordInteractionDispatchLaneV1<RuntimeInteractionDispatchDatabasePortV1>;

pub(crate) fn compose_runtime_discord_interaction_dispatch_lane_v1(
    port: RuntimeInteractionDispatchDatabasePortV1,
    gateway: GatewayResourceConfigV1,
) -> RuntimeProductionDiscordInteractionDispatchLaneV1 {
    RuntimeDiscordInteractionDispatchLaneV1::new_v1(
        port,
        gateway.rejection_acknowledgement_capacity(),
    )
}

impl<P> RuntimeDiscordInteractionDispatchLaneV1<P>
where
    P: RuntimeInteractionDispatchPortV1,
{
    pub(crate) fn new_v1(port: P, rejection_capacity: NonZeroUsize) -> Self {
        let dispatch_capacity = port.dispatch_capacity_v1();
        Self {
            port: Arc::new(port),
            dispatch_capacity,
            rejection_capacity,
            dispatches: FuturesUnordered::new(),
            rejections: FuturesUnordered::new(),
            state: RuntimeInteractionLaneStateV1::Paused,
            report: RuntimeInteractionDispatchReportV1::default(),
        }
    }

    pub(crate) fn has_in_flight_v1(&self) -> bool {
        !self.dispatches.is_empty() || !self.rejections.is_empty()
    }

    pub(crate) fn report_v1(&self) -> RuntimeInteractionDispatchReportV1 {
        self.report
    }

    pub(crate) fn try_enqueue_v1<F>(
        &mut self,
        envelope: SharedGatewayInteractionEnvelopeV3,
        observer: &GatewayConnectionObserverV3,
        ready_lease: Option<GatewayReadyLeaseV3>,
        mut product_ready: F,
    ) -> RuntimeInteractionEnqueueOutcomeV1
    where
        F: FnMut() -> bool,
    {
        if self.state != RuntimeInteractionLaneStateV1::Accepting {
            increment_v1(&mut self.report.not_accepting);
            return self.reject_v1(Box::new(envelope));
        }
        if !product_ready() {
            increment_v1(&mut self.report.product_not_ready);
            return self.reject_v1(Box::new(envelope));
        }
        let reservation = self.port.reserve_v1(envelope, ready_lease, observer);
        match reservation {
            RuntimeInteractionDispatchReservationOutcomeV1::Ignored => {
                increment_v1(&mut self.report.ignored);
                RuntimeInteractionEnqueueOutcomeV1::Ignored
            }
            RuntimeInteractionDispatchReservationOutcomeV1::Rejected { reason, envelope } => {
                self.record_reservation_rejection_v1(&reason);
                self.reject_v1(envelope)
            }
            RuntimeInteractionDispatchReservationOutcomeV1::Reserved(reservation) => {
                if self.dispatches.len() >= self.dispatch_capacity.get() {
                    increment_v1(&mut self.report.overloaded);
                    let envelope = self.port.cancel_v1(reservation);
                    return self.reject_v1(envelope);
                }
                if !product_ready() {
                    increment_v1(&mut self.report.product_not_ready);
                    let envelope = self.port.cancel_v1(reservation);
                    return self.reject_v1(envelope);
                }
                self.dispatches
                    .push(Arc::clone(&self.port).dispatch_v1(reservation));
                increment_v1(&mut self.report.enqueued);
                RuntimeInteractionEnqueueOutcomeV1::Enqueued
            }
        }
    }

    pub(crate) async fn poll_next_completion_v1(&mut self) -> RuntimeInteractionCompletionV1 {
        if !self.has_in_flight_v1() {
            return RuntimeInteractionCompletionV1::Idle;
        }
        tokio::select! {
            outcome = self.dispatches.next(), if !self.dispatches.is_empty() => {
                if let Some(outcome) = outcome {
                    self.record_dispatch_v1(outcome);
                }
                RuntimeInteractionCompletionV1::Dispatch
            }
            outcome = self.rejections.next(), if !self.rejections.is_empty() => {
                if let Some(outcome) = outcome {
                    self.record_rejection_acknowledgement_v1(outcome);
                }
                RuntimeInteractionCompletionV1::RejectionAcknowledgement
            }
        }
    }

    pub(crate) async fn pause_and_drain_until_v1(
        &mut self,
        deadline: Instant,
    ) -> RuntimeInteractionDrainOutcomeV1 {
        if self.state != RuntimeInteractionLaneStateV1::Sealed {
            self.state = RuntimeInteractionLaneStateV1::Paused;
        }
        self.drain_retained_until_v1(deadline).await
    }

    pub(crate) async fn seal_and_drain_until_v1(
        &mut self,
        deadline: Instant,
    ) -> RuntimeInteractionDrainOutcomeV1 {
        self.state = RuntimeInteractionLaneStateV1::Sealed;
        self.drain_retained_until_v1(deadline).await
    }

    pub(crate) fn reopen_v1<F>(
        &mut self,
        observer: &GatewayConnectionObserverV3,
        ready_lease: Option<GatewayReadyLeaseV3>,
        mut product_ready: F,
    ) -> Result<(), RuntimeInteractionReopenErrorV1>
    where
        F: FnMut() -> bool,
    {
        match self.state {
            RuntimeInteractionLaneStateV1::Accepting => {
                return Err(RuntimeInteractionReopenErrorV1::AlreadyAccepting)
            }
            RuntimeInteractionLaneStateV1::Sealed => {
                return Err(RuntimeInteractionReopenErrorV1::Sealed)
            }
            RuntimeInteractionLaneStateV1::Paused => {}
        }
        if self.has_in_flight_v1() {
            return Err(RuntimeInteractionReopenErrorV1::InFlight);
        }
        if !product_ready() {
            return Err(RuntimeInteractionReopenErrorV1::ProductNotReady);
        }
        let Some(ready_lease) = ready_lease else {
            return Err(RuntimeInteractionReopenErrorV1::GatewayNotReady);
        };
        if !observer.ready_lease_is_current(&ready_lease) {
            return Err(RuntimeInteractionReopenErrorV1::GatewayNotReady);
        }
        self.state = RuntimeInteractionLaneStateV1::Accepting;
        Ok(())
    }

    async fn drain_retained_until_v1(
        &mut self,
        deadline: Instant,
    ) -> RuntimeInteractionDrainOutcomeV1 {
        loop {
            if !self.has_in_flight_v1() {
                return RuntimeInteractionDrainOutcomeV1::Clean;
            }
            if Instant::now() >= deadline {
                self.abort_v1();
                return RuntimeInteractionDrainOutcomeV1::DeadlineExceeded;
            }
            if tokio::time::timeout_at(
                TokioInstant::from_std(deadline),
                self.poll_next_completion_v1(),
            )
            .await
            .is_err()
            {
                self.abort_v1();
                return RuntimeInteractionDrainOutcomeV1::DeadlineExceeded;
            }
        }
    }

    pub(crate) fn abort_v1(&mut self) {
        self.state = RuntimeInteractionLaneStateV1::Sealed;
        self.report.dispatch_cancelled = self
            .report
            .dispatch_cancelled
            .saturating_add(self.dispatches.len() as u64);
        self.report.rejection_acknowledgement_cancelled = self
            .report
            .rejection_acknowledgement_cancelled
            .saturating_add(self.rejections.len() as u64);
        self.dispatches = FuturesUnordered::new();
        self.rejections = FuturesUnordered::new();
    }

    fn reject_v1(
        &mut self,
        envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    ) -> RuntimeInteractionEnqueueOutcomeV1 {
        let acknowledgement = if self.rejections.len() >= self.rejection_capacity.get() {
            increment_v1(&mut self.report.rejection_acknowledgement_dropped);
            RuntimeInteractionRejectionSchedulingV1::Dropped
        } else {
            self.rejections
                .push(Arc::clone(&self.port).acknowledge_rejection_v1(envelope));
            RuntimeInteractionRejectionSchedulingV1::Enqueued
        };
        RuntimeInteractionEnqueueOutcomeV1::Rejected { acknowledgement }
    }

    fn record_dispatch_v1(&mut self, outcome: SharedGatewayInteractionDispatchOutcomeV3) {
        match outcome {
            SharedGatewayInteractionDispatchOutcomeV3::Executed(outcome) => match outcome {
                InteractionExecutionOutcomeV3::Ignored => increment_v1(&mut self.report.ignored),
                InteractionExecutionOutcomeV3::StaticFailed
                | InteractionExecutionOutcomeV3::InstanceFailed => {
                    increment_v1(&mut self.report.execution_failed)
                }
                InteractionExecutionOutcomeV3::StaticExecuted
                | InteractionExecutionOutcomeV3::StaticNoOp
                | InteractionExecutionOutcomeV3::InstanceExecuted
                | InteractionExecutionOutcomeV3::InstanceNoOp => {
                    increment_v1(&mut self.report.completed)
                }
            },
            SharedGatewayInteractionDispatchOutcomeV3::Ignored => {
                increment_v1(&mut self.report.ignored)
            }
            SharedGatewayInteractionDispatchOutcomeV3::Rejected { error, envelope } => {
                self.record_admission_error_v1(&error);
                let _ = self.reject_v1(envelope);
            }
        }
    }

    fn record_reservation_rejection_v1(&mut self, reason: &SharedGatewayInteractionRejectionV3) {
        match reason {
            SharedGatewayInteractionRejectionV3::Route(_) => {
                increment_v1(&mut self.report.route_rejected)
            }
            SharedGatewayInteractionRejectionV3::Admission(error) => {
                self.record_admission_error_v1(error)
            }
        }
    }

    fn record_admission_error_v1(&mut self, error: &SharedGatewayAdmissionErrorV3) {
        match error {
            SharedGatewayAdmissionErrorV3::NotReady => {
                increment_v1(&mut self.report.gateway_not_ready)
            }
            SharedGatewayAdmissionErrorV3::Overloaded => increment_v1(&mut self.report.overloaded),
            SharedGatewayAdmissionErrorV3::Router(_) => {
                increment_v1(&mut self.report.admission_rejected)
            }
        }
    }

    fn record_rejection_acknowledgement_v1(
        &mut self,
        outcome: SharedGatewayRejectionAcknowledgementOutcomeV3,
    ) {
        match outcome {
            SharedGatewayRejectionAcknowledgementOutcomeV3::Sent => {
                increment_v1(&mut self.report.rejection_acknowledged)
            }
            SharedGatewayRejectionAcknowledgementOutcomeV3::Failed => {
                increment_v1(&mut self.report.rejection_acknowledgement_failed)
            }
            SharedGatewayRejectionAcknowledgementOutcomeV3::TimedOut => {
                increment_v1(&mut self.report.rejection_acknowledgement_timed_out)
            }
        }
    }
}

impl<P> Debug for RuntimeDiscordInteractionDispatchLaneV1<P>
where
    P: RuntimeInteractionDispatchPortV1,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordInteractionDispatchLaneV1(<redacted>)")
    }
}

fn increment_v1(value: &mut u64) {
    *value = value.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use automation_runtime::{
        encode_button, shared_gateway_control_channel_v3, GatewayControlConfigV3,
        GatewayReadyKindV3, SharedGatewayInteractionApplicationIdV3, SharedGatewayInteractionIdV3,
        SharedGatewayInteractionIdentityV3, SharedGatewayInteractionTokenV3,
    };
    use discord_model::{ChannelId, GuildId, UserId};

    use super::*;

    enum TestDispatchBehaviorV1 {
        Complete,
        Pending,
    }

    struct TestReservationV1 {
        envelope: SharedGatewayInteractionEnvelopeV3,
    }

    struct TestPortV1 {
        dispatch_capacity: NonZeroUsize,
        dispatch_behavior: TestDispatchBehaviorV1,
        acknowledgement_pending: bool,
        reserve_calls: Arc<AtomicUsize>,
    }

    impl RuntimeInteractionDispatchPortV1 for TestPortV1 {
        type Reservation = TestReservationV1;

        fn dispatch_capacity_v1(&self) -> NonZeroUsize {
            self.dispatch_capacity
        }

        fn reserve_v1(
            &self,
            envelope: SharedGatewayInteractionEnvelopeV3,
            _ready_lease: Option<GatewayReadyLeaseV3>,
            _observer: &GatewayConnectionObserverV3,
        ) -> RuntimeInteractionDispatchReservationOutcomeV1<Self::Reservation> {
            self.reserve_calls.fetch_add(1, Ordering::SeqCst);
            RuntimeInteractionDispatchReservationOutcomeV1::Reserved(TestReservationV1 { envelope })
        }

        fn cancel_v1(
            &self,
            reservation: Self::Reservation,
        ) -> Box<SharedGatewayInteractionEnvelopeV3> {
            Box::new(reservation.envelope)
        }

        fn dispatch_v1(
            self: Arc<Self>,
            _reservation: Self::Reservation,
        ) -> RuntimeInteractionDispatchFutureV1 {
            match self.dispatch_behavior {
                TestDispatchBehaviorV1::Complete => Box::pin(async {
                    SharedGatewayInteractionDispatchOutcomeV3::Executed(
                        InteractionExecutionOutcomeV3::StaticExecuted,
                    )
                }),
                TestDispatchBehaviorV1::Pending => Box::pin(std::future::pending()),
            }
        }

        fn acknowledge_rejection_v1(
            self: Arc<Self>,
            _envelope: Box<SharedGatewayInteractionEnvelopeV3>,
        ) -> RuntimeInteractionRejectionFutureV1 {
            if self.acknowledgement_pending {
                Box::pin(std::future::pending())
            } else {
                Box::pin(async { SharedGatewayRejectionAcknowledgementOutcomeV3::Sent })
            }
        }
    }

    fn envelope_v1() -> SharedGatewayInteractionEnvelopeV3 {
        let identity = SharedGatewayInteractionIdentityV3::new(
            GuildId(7),
            ChannelId(8),
            UserId(9),
            SharedGatewayInteractionApplicationIdV3::new(10).unwrap(),
            SharedGatewayInteractionIdV3::new(11).unwrap(),
        )
        .unwrap();
        SharedGatewayInteractionEnvelopeV3::message_component_v3(
            identity,
            encode_button(GuildId(7), "study", "create"),
            Some("ko".to_string()),
            SharedGatewayInteractionTokenV3::new("interaction-secret".to_string()).unwrap(),
        )
        .unwrap()
    }

    struct TestReadyV1 {
        _owners: Box<dyn std::any::Any>,
        observer: GatewayConnectionObserverV3,
        lease: GatewayReadyLeaseV3,
    }

    fn ready_v1() -> TestReadyV1 {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let observer = control.connection_observer();
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let lease = observer.issue_ready_lease(epoch).unwrap();
        TestReadyV1 {
            _owners: Box::new((control, runtime)),
            observer,
            lease,
        }
    }

    fn ready_lane_v1(
        port: TestPortV1,
        rejection_capacity: NonZeroUsize,
    ) -> (
        RuntimeDiscordInteractionDispatchLaneV1<TestPortV1>,
        TestReadyV1,
    ) {
        let ready = ready_v1();
        let mut lane = RuntimeDiscordInteractionDispatchLaneV1::new_v1(port, rejection_capacity);
        lane.reopen_v1(&ready.observer, Some(ready.lease), || true)
            .unwrap();
        (lane, ready)
    }

    #[tokio::test]
    async fn production_lane_starts_paused_and_requires_explicit_verified_reopen() {
        let reserve_calls = Arc::new(AtomicUsize::new(0));
        let mut lane = RuntimeDiscordInteractionDispatchLaneV1::new_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Complete,
                acknowledgement_pending: false,
                reserve_calls: Arc::clone(&reserve_calls),
            },
            NonZeroUsize::new(1).unwrap(),
        );
        let ready = ready_v1();

        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Rejected {
                acknowledgement: RuntimeInteractionRejectionSchedulingV1::Enqueued
            }
        );
        assert_eq!(reserve_calls.load(Ordering::SeqCst), 0);
        assert_eq!(lane.report_v1().not_accepting, 1);
        assert_eq!(lane.report_v1().product_not_ready, 0);
        assert_eq!(
            lane.poll_next_completion_v1().await,
            RuntimeInteractionCompletionV1::RejectionAcknowledgement
        );
        lane.reopen_v1(&ready.observer, Some(ready.lease), || true)
            .unwrap();
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Enqueued
        );
        assert_eq!(reserve_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_initial_product_readiness_skips_reservation_entirely() {
        let reserve_calls = Arc::new(AtomicUsize::new(0));
        let (mut lane, ready) = ready_lane_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Complete,
                acknowledgement_pending: false,
                reserve_calls: Arc::clone(&reserve_calls),
            },
            NonZeroUsize::new(1).unwrap(),
        );

        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || false,),
            RuntimeInteractionEnqueueOutcomeV1::Rejected {
                acknowledgement: RuntimeInteractionRejectionSchedulingV1::Enqueued
            }
        );
        assert_eq!(reserve_calls.load(Ordering::SeqCst), 0);
        assert_eq!(lane.report_v1().product_not_ready, 1);
        assert_eq!(lane.report_v1().not_accepting, 0);
    }

    #[tokio::test]
    async fn product_readiness_is_checked_before_reservation_and_immediately_before_enqueue() {
        let (mut lane, ready) = ready_lane_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Complete,
                acknowledgement_pending: false,
                reserve_calls: Arc::new(AtomicUsize::new(0)),
            },
            NonZeroUsize::new(1).unwrap(),
        );
        let checks = AtomicUsize::new(0);
        let outcome =
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || {
                checks.fetch_add(1, Ordering::SeqCst) == 0
            });

        assert_eq!(checks.load(Ordering::SeqCst), 2);
        assert_eq!(
            outcome,
            RuntimeInteractionEnqueueOutcomeV1::Rejected {
                acknowledgement: RuntimeInteractionRejectionSchedulingV1::Enqueued
            }
        );
        assert_eq!(lane.report_v1().product_not_ready, 1);
        assert_eq!(lane.report_v1().enqueued, 0);
        assert_eq!(
            lane.poll_next_completion_v1().await,
            RuntimeInteractionCompletionV1::RejectionAcknowledgement
        );
        assert_eq!(lane.report_v1().rejection_acknowledged, 1);
    }

    #[tokio::test]
    async fn dispatch_and_rejection_work_are_independently_bounded() {
        let (mut lane, ready) = ready_lane_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Pending,
                acknowledgement_pending: true,
                reserve_calls: Arc::new(AtomicUsize::new(0)),
            },
            NonZeroUsize::new(1).unwrap(),
        );

        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Enqueued
        );
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Rejected {
                acknowledgement: RuntimeInteractionRejectionSchedulingV1::Enqueued
            }
        );
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Rejected {
                acknowledgement: RuntimeInteractionRejectionSchedulingV1::Dropped
            }
        );
        assert_eq!(lane.report_v1().enqueued, 1);
        assert_eq!(lane.report_v1().overloaded, 2);
        assert_eq!(lane.report_v1().rejection_acknowledgement_dropped, 1);
    }

    #[tokio::test]
    async fn timed_out_pause_drain_cancels_work_and_permanently_seals_lane() {
        let (mut lane, ready) = ready_lane_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Pending,
                acknowledgement_pending: true,
                reserve_calls: Arc::new(AtomicUsize::new(0)),
            },
            NonZeroUsize::new(1).unwrap(),
        );
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Enqueued
        );

        let outcome = lane
            .pause_and_drain_until_v1(Instant::now() + Duration::from_millis(10))
            .await;

        assert_eq!(outcome, RuntimeInteractionDrainOutcomeV1::DeadlineExceeded);
        assert_eq!(lane.report_v1().dispatch_cancelled, 1);
        assert!(!lane.has_in_flight_v1());
        assert_eq!(
            lane.reopen_v1(&ready.observer, Some(ready.lease), || true),
            Err(RuntimeInteractionReopenErrorV1::Sealed)
        );
        assert_eq!(
            format!("{lane:?}"),
            "RuntimeDiscordInteractionDispatchLaneV1(<redacted>)"
        );
    }

    #[tokio::test]
    async fn cancelled_pause_drain_cannot_reopen_with_retained_work() {
        let (mut lane, ready) = ready_lane_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Pending,
                acknowledgement_pending: true,
                reserve_calls: Arc::new(AtomicUsize::new(0)),
            },
            NonZeroUsize::new(1).unwrap(),
        );
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Enqueued
        );
        {
            let mut drain =
                Box::pin(lane.pause_and_drain_until_v1(Instant::now() + Duration::from_secs(1)));
            assert!(
                tokio::time::timeout(Duration::from_millis(1), drain.as_mut())
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            lane.reopen_v1(&ready.observer, Some(ready.lease), || true),
            Err(RuntimeInteractionReopenErrorV1::InFlight)
        );
        lane.abort_v1();
    }

    #[tokio::test]
    async fn clean_pause_drain_rejects_until_verified_reopen_then_accepts_again() {
        let (mut lane, ready) = ready_lane_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Complete,
                acknowledgement_pending: false,
                reserve_calls: Arc::new(AtomicUsize::new(0)),
            },
            NonZeroUsize::new(1).unwrap(),
        );
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Enqueued
        );

        assert_eq!(
            lane.pause_and_drain_until_v1(Instant::now() + Duration::from_secs(1))
                .await,
            RuntimeInteractionDrainOutcomeV1::Clean
        );
        assert_eq!(lane.report_v1().completed, 1);
        assert_eq!(lane.report_v1().dispatch_cancelled, 0);
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Rejected {
                acknowledgement: RuntimeInteractionRejectionSchedulingV1::Enqueued
            }
        );
        assert_eq!(lane.report_v1().not_accepting, 1);
        assert_eq!(
            lane.poll_next_completion_v1().await,
            RuntimeInteractionCompletionV1::RejectionAcknowledgement
        );
        lane.reopen_v1(&ready.observer, Some(ready.lease), || true)
            .unwrap();
        assert_eq!(
            lane.try_enqueue_v1(envelope_v1(), &ready.observer, Some(ready.lease), || true,),
            RuntimeInteractionEnqueueOutcomeV1::Enqueued
        );
    }

    #[tokio::test]
    async fn reopen_requires_current_gateway_and_product_readiness() {
        let mut lane = RuntimeDiscordInteractionDispatchLaneV1::new_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Complete,
                acknowledgement_pending: false,
                reserve_calls: Arc::new(AtomicUsize::new(0)),
            },
            NonZeroUsize::new(1).unwrap(),
        );
        let ready = ready_v1();

        assert_eq!(
            lane.reopen_v1(&ready.observer, Some(ready.lease), || false),
            Err(RuntimeInteractionReopenErrorV1::ProductNotReady)
        );
        assert_eq!(
            lane.reopen_v1(&ready.observer, None, || true),
            Err(RuntimeInteractionReopenErrorV1::GatewayNotReady)
        );
        lane.reopen_v1(&ready.observer, Some(ready.lease), || true)
            .unwrap();
        assert_eq!(
            lane.reopen_v1(&ready.observer, Some(ready.lease), || true),
            Err(RuntimeInteractionReopenErrorV1::AlreadyAccepting)
        );
    }

    #[tokio::test]
    async fn terminal_seal_cannot_reopen_after_clean_drain() {
        let (mut lane, ready) = ready_lane_v1(
            TestPortV1 {
                dispatch_capacity: NonZeroUsize::new(1).unwrap(),
                dispatch_behavior: TestDispatchBehaviorV1::Complete,
                acknowledgement_pending: false,
                reserve_calls: Arc::new(AtomicUsize::new(0)),
            },
            NonZeroUsize::new(1).unwrap(),
        );

        assert_eq!(
            lane.seal_and_drain_until_v1(Instant::now() + Duration::from_secs(1))
                .await,
            RuntimeInteractionDrainOutcomeV1::Clean
        );
        assert_eq!(
            lane.reopen_v1(&ready.observer, Some(ready.lease), || true),
            Err(RuntimeInteractionReopenErrorV1::Sealed)
        );
    }
}
