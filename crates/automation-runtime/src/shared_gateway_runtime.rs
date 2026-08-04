use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::time::Duration;

use automation_instance::{InstanceIdGenerator, InstanceRegistrarV1, InstanceRouteReaderV1};
use automation_instance_teardown::InstanceTeardownService;
use automation_ruleset_dispatch::{GuildRoleSnapshotProvider, PinnedInstanceResolverV1};
use automation_runtime_registry::ServingSlotRegistryV1;
use futures::stream::FuturesUnordered;
use futures::StreamExt as FuturesStreamExt;
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt};
use twilight_http::Client;
use twilight_model::gateway::CloseFrame;

use crate::runner::InteractionExecutionOutcomeV3;
use crate::shared_gateway_admission::{
    SharedGatewayAdmissionBudgetV3, SharedGatewayAdmissionErrorV3,
};
use crate::shared_gateway_control::{
    GatewayCommandAckV3, GatewayConnectionObserverV3, GatewayDisconnectKindV3, GatewayReadyKindV3,
    GatewayReadyLeaseV3, GatewayRuntimeCommandOutcomeV3, SharedGatewayRuntimeControlV3,
};
use crate::shared_gateway_dispatcher::{
    acknowledge_shared_gateway_interaction_rejection_v3,
    dispatch_reserved_shared_gateway_interaction_v3, reserve_shared_gateway_interaction_v3,
    SharedGatewayInteractionDispatchOutcomeV3, SharedGatewayInteractionEnvelopeV3,
    SharedGatewayInteractionRejectionV3, SharedGatewayInteractionReservationOutcomeV3,
    SharedGatewayRejectionAcknowledgementOutcomeV3,
};

const MAX_SHARED_GATEWAY_DRAIN_TIMEOUT_V3: Duration = Duration::from_secs(60);
const MAX_SHARED_GATEWAY_REJECTION_ACKNOWLEDGEMENTS_V3: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedGatewayRuntimeConfigV3 {
    drain_timeout: Duration,
    rejection_acknowledgement_capacity: NonZeroUsize,
}

impl SharedGatewayRuntimeConfigV3 {
    pub fn new(drain_timeout: Duration) -> Result<Self, SharedGatewayRuntimeConfigurationErrorV3> {
        if drain_timeout.is_zero() || drain_timeout > MAX_SHARED_GATEWAY_DRAIN_TIMEOUT_V3 {
            return Err(SharedGatewayRuntimeConfigurationErrorV3::DrainTimeout);
        }
        Ok(Self {
            drain_timeout,
            rejection_acknowledgement_capacity: NonZeroUsize::new(64)
                .expect("default rejection acknowledgement capacity is non-zero"),
        })
    }

    pub fn drain_timeout(self) -> Duration {
        self.drain_timeout
    }

    pub fn with_rejection_acknowledgement_capacity(
        mut self,
        capacity: NonZeroUsize,
    ) -> Result<Self, SharedGatewayRuntimeConfigurationErrorV3> {
        if capacity.get() > MAX_SHARED_GATEWAY_REJECTION_ACKNOWLEDGEMENTS_V3 {
            return Err(SharedGatewayRuntimeConfigurationErrorV3::RejectionAcknowledgementCapacity);
        }
        self.rejection_acknowledgement_capacity = capacity;
        Ok(self)
    }

    pub fn rejection_acknowledgement_capacity(self) -> NonZeroUsize {
        self.rejection_acknowledgement_capacity
    }
}

impl Default for SharedGatewayRuntimeConfigV3 {
    fn default() -> Self {
        Self {
            drain_timeout: Duration::from_secs(15),
            rejection_acknowledgement_capacity: NonZeroUsize::new(64)
                .expect("default rejection acknowledgement capacity is non-zero"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SharedGatewayRuntimeConfigurationErrorV3 {
    #[error("shared gateway drain timeout is invalid")]
    DrainTimeout,
    #[error("shared gateway rejection acknowledgement capacity is invalid")]
    RejectionAcknowledgementCapacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedGatewayExitReasonV3 {
    Commanded,
    ControlOrphaned,
    StreamEnded,
    RuntimeFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedGatewayDrainOutcomeV3 {
    Clean,
    DeadlineExceeded,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SharedGatewayRuntimeReportV3 {
    pub completed: u64,
    pub execution_failed: u64,
    pub ignored: u64,
    pub route_rejected: u64,
    pub not_ready: u64,
    pub overloaded: u64,
    pub admission_rejected: u64,
    pub rejection_acknowledged: u64,
    pub rejection_acknowledgement_failed: u64,
    pub rejection_acknowledgement_timed_out: u64,
    pub rejection_acknowledgement_dropped: u64,
    pub cancelled_during_drain: u64,
    pub rejection_acknowledgements_cancelled_during_drain: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedGatewayExitV3 {
    pub reason: SharedGatewayExitReasonV3,
    pub drain: SharedGatewayDrainOutcomeV3,
    pub report: SharedGatewayRuntimeReportV3,
}

type SharedGatewayDispatchFutureV3<'a> =
    Pin<Box<dyn Future<Output = SharedGatewayInteractionDispatchOutcomeV3> + 'a>>;
type SharedGatewayRejectionAcknowledgementFutureV3<'a> =
    Pin<Box<dyn Future<Output = SharedGatewayRejectionAcknowledgementOutcomeV3> + 'a>>;

impl SharedGatewayRuntimeReportV3 {
    fn record_dispatch(
        &mut self,
        outcome: SharedGatewayInteractionDispatchOutcomeV3,
    ) -> Option<Box<SharedGatewayInteractionEnvelopeV3>> {
        match outcome {
            SharedGatewayInteractionDispatchOutcomeV3::Executed(outcome) => {
                if matches!(
                    outcome,
                    InteractionExecutionOutcomeV3::StaticFailed
                        | InteractionExecutionOutcomeV3::InstanceFailed
                ) {
                    increment(&mut self.execution_failed);
                } else if matches!(outcome, InteractionExecutionOutcomeV3::Ignored) {
                    increment(&mut self.ignored);
                } else {
                    increment(&mut self.completed);
                }
                None
            }
            SharedGatewayInteractionDispatchOutcomeV3::Ignored => {
                increment(&mut self.ignored);
                None
            }
            SharedGatewayInteractionDispatchOutcomeV3::Rejected { error, envelope } => {
                self.record_admission_error(&error);
                Some(envelope)
            }
        }
    }

    fn record_reservation_rejection(&mut self, reason: &SharedGatewayInteractionRejectionV3) {
        match reason {
            SharedGatewayInteractionRejectionV3::Route(_) => increment(&mut self.route_rejected),
            SharedGatewayInteractionRejectionV3::Admission(error) => {
                self.record_admission_error(error)
            }
        }
    }

    fn record_admission_error(&mut self, error: &SharedGatewayAdmissionErrorV3) {
        match error {
            SharedGatewayAdmissionErrorV3::NotReady => increment(&mut self.not_ready),
            SharedGatewayAdmissionErrorV3::Overloaded => increment(&mut self.overloaded),
            SharedGatewayAdmissionErrorV3::Router(_) => increment(&mut self.admission_rejected),
        }
    }

    fn record_rejection_acknowledgement(
        &mut self,
        outcome: SharedGatewayRejectionAcknowledgementOutcomeV3,
    ) {
        match outcome {
            SharedGatewayRejectionAcknowledgementOutcomeV3::Sent => {
                increment(&mut self.rejection_acknowledged)
            }
            SharedGatewayRejectionAcknowledgementOutcomeV3::Failed => {
                increment(&mut self.rejection_acknowledgement_failed)
            }
            SharedGatewayRejectionAcknowledgementOutcomeV3::TimedOut => {
                increment(&mut self.rejection_acknowledgement_timed_out)
            }
        }
    }
}

fn increment(value: &mut u64) {
    *value = value.saturating_add(1);
}

fn interaction_callback_client(token: String) -> Client {
    Client::builder()
        .token(token)
        .ratelimiter(None)
        .timeout(Duration::from_secs(2))
        .build()
}

#[allow(clippy::too_many_arguments)]
pub async fn run_shared_gateway_v3(
    token: String,
    http: &Client,
    failure_message: &str,
    registry: &ServingSlotRegistryV1,
    admission_budget: &SharedGatewayAdmissionBudgetV3,
    instances: &(impl InstanceRouteReaderV1 + InstanceRegistrarV1),
    instance_ids: &impl InstanceIdGenerator,
    teardown: &impl InstanceTeardownService,
    pinned_resolver: &impl PinnedInstanceResolverV1,
    snapshot_provider: &impl GuildRoleSnapshotProvider,
    mut control: SharedGatewayRuntimeControlV3,
    config: SharedGatewayRuntimeConfigV3,
) -> SharedGatewayExitV3 {
    let observer = control.connection_observer();
    let interaction_http = interaction_callback_client(token.clone());
    let mut shard = Shard::new(ShardId::ONE, token, Intents::empty());
    let event_types = EventTypeFlags::INTERACTION_CREATE
        | EventTypeFlags::READY
        | EventTypeFlags::RESUMED
        | EventTypeFlags::GATEWAY_RECONNECT
        | EventTypeFlags::GATEWAY_INVALIDATE_SESSION;
    let mut ready_lease = None;
    let mut active: FuturesUnordered<SharedGatewayDispatchFutureV3<'_>> = FuturesUnordered::new();
    let mut rejection_acknowledgements: FuturesUnordered<
        SharedGatewayRejectionAcknowledgementFutureV3<'_>,
    > = FuturesUnordered::new();
    let mut report = SharedGatewayRuntimeReportV3::default();

    loop {
        tokio::select! {
            command = control.process_next_command() => {
                match command {
                    GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::Paused { .. }) => {
                        ready_lease = None;
                    }
                    GatewayRuntimeCommandOutcomeV3::Applied(
                        GatewayCommandAckV3::AdmissionResumed { epoch }
                    ) => {
                        ready_lease = observer.issue_ready_lease(epoch).ok();
                    }
                    GatewayRuntimeCommandOutcomeV3::Applied(
                        GatewayCommandAckV3::Draining { .. }
                    ) => {
                        return finish_gateway(
                            &mut shard,
                            &mut control,
                            &mut active,
                            &mut rejection_acknowledgements,
                            report,
                            SharedGatewayExitReasonV3::Commanded,
                            config.drain_timeout(),
                            &interaction_http,
                            failure_message,
                            config.rejection_acknowledgement_capacity(),
                        )
                        .await;
                    }
                    GatewayRuntimeCommandOutcomeV3::Rejected(_) => {
                        ready_lease = current_ready_lease(&observer);
                    }
                    GatewayRuntimeCommandOutcomeV3::ControlOrphaned => {
                        return finish_gateway(
                            &mut shard,
                            &mut control,
                            &mut active,
                            &mut rejection_acknowledgements,
                            report,
                            SharedGatewayExitReasonV3::ControlOrphaned,
                            config.drain_timeout(),
                            &interaction_http,
                            failure_message,
                            config.rejection_acknowledgement_capacity(),
                        )
                        .await;
                    }
                }
            }
            outcome = active.next(), if !active.is_empty() => {
                if let Some(outcome) = outcome {
                    if let Some(interaction) = report.record_dispatch(outcome) {
                        enqueue_rejection_acknowledgement(
                            interaction,
                            &interaction_http,
                            failure_message,
                            &mut rejection_acknowledgements,
                            config.rejection_acknowledgement_capacity(),
                            &mut report,
                        );
                    }
                }
            }
            outcome = rejection_acknowledgements.next(), if !rejection_acknowledgements.is_empty() => {
                if let Some(outcome) = outcome {
                    report.record_rejection_acknowledgement(outcome);
                }
            }
            item = shard.next_event(event_types) => {
                let Some(item) = item else {
                    return finish_gateway(
                        &mut shard,
                        &mut control,
                        &mut active,
                        &mut rejection_acknowledgements,
                        report,
                        SharedGatewayExitReasonV3::StreamEnded,
                        config.drain_timeout(),
                        &interaction_http,
                        failure_message,
                        config.rejection_acknowledgement_capacity(),
                    )
                    .await;
                };
                let event = match item {
                    Ok(event) => event,
                    Err(_) => {
                        ready_lease = None;
                        if control
                            .mark_disconnected(GatewayDisconnectKindV3::ReceiveError)
                            .is_err()
                        {
                            return finish_gateway(
                                &mut shard,
                                &mut control,
                                &mut active,
                                &mut rejection_acknowledgements,
                                report,
                                SharedGatewayExitReasonV3::RuntimeFailure,
                                config.drain_timeout(),
                                &interaction_http,
                                failure_message,
                                config.rejection_acknowledgement_capacity(),
                            )
                            .await;
                        }
                        continue;
                    }
                };
                match event {
                    Event::Ready(_) => {
                        match control.mark_connected(GatewayReadyKindV3::Ready) {
                            Ok(epoch) => ready_lease = observer.issue_ready_lease(epoch).ok(),
                            Err(_) => {
                                return finish_gateway(
                                    &mut shard,
                                    &mut control,
                                    &mut active,
                                    &mut rejection_acknowledgements,
                                    report,
                                    SharedGatewayExitReasonV3::RuntimeFailure,
                                    config.drain_timeout(),
                                    &interaction_http,
                                    failure_message,
                                    config.rejection_acknowledgement_capacity(),
                                )
                                .await;
                            }
                        }
                    }
                    Event::Resumed => {
                        match control.mark_connected(GatewayReadyKindV3::Resumed) {
                            Ok(epoch) => ready_lease = observer.issue_ready_lease(epoch).ok(),
                            Err(_) => {
                                return finish_gateway(
                                    &mut shard,
                                    &mut control,
                                    &mut active,
                                    &mut rejection_acknowledgements,
                                    report,
                                    SharedGatewayExitReasonV3::RuntimeFailure,
                                    config.drain_timeout(),
                                    &interaction_http,
                                    failure_message,
                                    config.rejection_acknowledgement_capacity(),
                                )
                                .await;
                            }
                        }
                    }
                    Event::GatewayClose(_) => {
                        ready_lease = None;
                        if control
                            .mark_disconnected(GatewayDisconnectKindV3::Close)
                            .is_err()
                        {
                            return finish_gateway(
                                &mut shard,
                                &mut control,
                                &mut active,
                                &mut rejection_acknowledgements,
                                report,
                                SharedGatewayExitReasonV3::RuntimeFailure,
                                config.drain_timeout(),
                                &interaction_http,
                                failure_message,
                                config.rejection_acknowledgement_capacity(),
                            )
                            .await;
                        }
                    }
                    Event::GatewayReconnect => {
                        ready_lease = None;
                        if control
                            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
                            .is_err()
                        {
                            return finish_gateway(
                                &mut shard,
                                &mut control,
                                &mut active,
                                &mut rejection_acknowledgements,
                                report,
                                SharedGatewayExitReasonV3::RuntimeFailure,
                                config.drain_timeout(),
                                &interaction_http,
                                failure_message,
                                config.rejection_acknowledgement_capacity(),
                            )
                            .await;
                        }
                    }
                    Event::GatewayInvalidateSession(_) => {
                        ready_lease = None;
                        if control
                            .mark_disconnected(GatewayDisconnectKindV3::SessionInvalidated)
                            .is_err()
                        {
                            return finish_gateway(
                                &mut shard,
                                &mut control,
                                &mut active,
                                &mut rejection_acknowledgements,
                                report,
                                SharedGatewayExitReasonV3::RuntimeFailure,
                                config.drain_timeout(),
                                &interaction_http,
                                failure_message,
                                config.rejection_acknowledgement_capacity(),
                            )
                            .await;
                        }
                    }
                    Event::InteractionCreate(interaction) => {
                        enqueue_interaction(
                            interaction.0,
                            ready_lease,
                            &observer,
                            admission_budget,
                            registry,
                            instances,
                            instance_ids,
                            teardown,
                            pinned_resolver,
                            snapshot_provider,
                            http,
                            &interaction_http,
                            failure_message,
                            &mut active,
                            &mut rejection_acknowledgements,
                            config.rejection_acknowledgement_capacity(),
                            &mut report,
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn current_ready_lease(observer: &GatewayConnectionObserverV3) -> Option<GatewayReadyLeaseV3> {
    observer
        .current_connection()
        .current_epoch()
        .and_then(|epoch| observer.issue_ready_lease(epoch).ok())
}

#[allow(clippy::too_many_arguments)]
fn enqueue_interaction<'a, I, G, T, PR, S>(
    interaction: twilight_model::application::interaction::Interaction,
    ready_lease: Option<GatewayReadyLeaseV3>,
    observer: &GatewayConnectionObserverV3,
    admission_budget: &SharedGatewayAdmissionBudgetV3,
    registry: &'a ServingSlotRegistryV1,
    instances: &'a I,
    instance_ids: &'a G,
    teardown: &'a T,
    pinned_resolver: &'a PR,
    snapshot_provider: &'a S,
    mutation_http: &'a Client,
    interaction_http: &'a Client,
    failure_message: &'a str,
    active: &mut FuturesUnordered<SharedGatewayDispatchFutureV3<'a>>,
    rejection_acknowledgements: &mut FuturesUnordered<
        SharedGatewayRejectionAcknowledgementFutureV3<'a>,
    >,
    rejection_acknowledgement_capacity: NonZeroUsize,
    report: &mut SharedGatewayRuntimeReportV3,
) where
    I: InstanceRouteReaderV1 + InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    PR: PinnedInstanceResolverV1,
    S: GuildRoleSnapshotProvider,
{
    let envelope =
        match SharedGatewayInteractionEnvelopeV3::from_twilight_interaction_v3(interaction) {
            Ok(Some(envelope)) => envelope,
            Ok(None) => {
                increment(&mut report.ignored);
                return;
            }
            Err(_) => {
                increment(&mut report.route_rejected);
                return;
            }
        };
    match reserve_shared_gateway_interaction_v3(envelope, ready_lease, observer, admission_budget) {
        SharedGatewayInteractionReservationOutcomeV3::Reserved(reserved) => {
            active.push(Box::pin(dispatch_reserved_shared_gateway_interaction_v3(
                *reserved,
                registry,
                instances,
                instance_ids,
                teardown,
                pinned_resolver,
                snapshot_provider,
                mutation_http,
                interaction_http,
                failure_message,
            )));
        }
        SharedGatewayInteractionReservationOutcomeV3::Ignored => {
            increment(&mut report.ignored);
        }
        SharedGatewayInteractionReservationOutcomeV3::Rejected { reason, envelope } => {
            report.record_reservation_rejection(&reason);
            enqueue_rejection_acknowledgement(
                envelope,
                interaction_http,
                failure_message,
                rejection_acknowledgements,
                rejection_acknowledgement_capacity,
                report,
            );
        }
    }
}

fn enqueue_rejection_acknowledgement<'a>(
    envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    http: &'a Client,
    failure_message: &'a str,
    active: &mut FuturesUnordered<SharedGatewayRejectionAcknowledgementFutureV3<'a>>,
    capacity: NonZeroUsize,
    report: &mut SharedGatewayRuntimeReportV3,
) {
    enqueue_rejection_acknowledgement_future(
        Box::pin(acknowledge_rejection(http, envelope, failure_message)),
        active,
        capacity,
        report,
    );
}

fn enqueue_rejection_acknowledgement_future<'a>(
    acknowledgement: SharedGatewayRejectionAcknowledgementFutureV3<'a>,
    active: &mut FuturesUnordered<SharedGatewayRejectionAcknowledgementFutureV3<'a>>,
    capacity: NonZeroUsize,
    report: &mut SharedGatewayRuntimeReportV3,
) {
    if active.len() >= capacity.get() {
        increment(&mut report.rejection_acknowledgement_dropped);
        return;
    }
    active.push(acknowledgement);
}

async fn acknowledge_rejection(
    interaction_http: &Client,
    envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    failure_message: &str,
) -> SharedGatewayRejectionAcknowledgementOutcomeV3 {
    acknowledge_shared_gateway_interaction_rejection_v3(interaction_http, envelope, failure_message)
        .await
}

#[allow(clippy::too_many_arguments)]
async fn finish_gateway<'a, F>(
    shard: &mut Shard,
    control: &mut SharedGatewayRuntimeControlV3,
    active: &mut FuturesUnordered<F>,
    rejection_acknowledgements: &mut FuturesUnordered<
        SharedGatewayRejectionAcknowledgementFutureV3<'a>,
    >,
    mut report: SharedGatewayRuntimeReportV3,
    reason: SharedGatewayExitReasonV3,
    drain_timeout: Duration,
    http: &'a Client,
    failure_message: &'a str,
    rejection_acknowledgement_capacity: NonZeroUsize,
) -> SharedGatewayExitV3
where
    F: Future<Output = SharedGatewayInteractionDispatchOutcomeV3>,
{
    control.begin_runtime_failure_drain();
    shard.close(CloseFrame::NORMAL);
    let drain = drain_active(
        active,
        rejection_acknowledgements,
        &mut report,
        drain_timeout,
        |envelope, acknowledgements, report| {
            enqueue_rejection_acknowledgement(
                envelope,
                http,
                failure_message,
                acknowledgements,
                rejection_acknowledgement_capacity,
                report,
            );
        },
    )
    .await;
    let _ = control.mark_stopped();
    SharedGatewayExitV3 {
        reason,
        drain,
        report,
    }
}

async fn drain_active<'a, F, A>(
    active: &mut FuturesUnordered<F>,
    rejection_acknowledgements: &mut FuturesUnordered<
        SharedGatewayRejectionAcknowledgementFutureV3<'a>,
    >,
    report: &mut SharedGatewayRuntimeReportV3,
    drain_timeout: Duration,
    mut acknowledge_rejection: A,
) -> SharedGatewayDrainOutcomeV3
where
    F: Future<Output = SharedGatewayInteractionDispatchOutcomeV3>,
    A: FnMut(
        Box<SharedGatewayInteractionEnvelopeV3>,
        &mut FuturesUnordered<SharedGatewayRejectionAcknowledgementFutureV3<'a>>,
        &mut SharedGatewayRuntimeReportV3,
    ),
{
    let drain = async {
        while !active.is_empty() || !rejection_acknowledgements.is_empty() {
            tokio::select! {
                outcome = active.next(), if !active.is_empty() => {
                    if let Some(outcome) = outcome {
                        if let Some(interaction) = report.record_dispatch(outcome) {
                            acknowledge_rejection(
                                interaction,
                                rejection_acknowledgements,
                                report,
                            );
                        }
                    }
                }
                outcome = rejection_acknowledgements.next(), if !rejection_acknowledgements.is_empty() => {
                    if let Some(outcome) = outcome {
                        report.record_rejection_acknowledgement(outcome);
                    }
                }
            }
        }
    };
    if tokio::time::timeout(drain_timeout, drain).await.is_ok() {
        SharedGatewayDrainOutcomeV3::Clean
    } else {
        report.cancelled_during_drain = report
            .cancelled_during_drain
            .saturating_add(active.len() as u64);
        report.rejection_acknowledgements_cancelled_during_drain = report
            .rejection_acknowledgements_cancelled_during_drain
            .saturating_add(rejection_acknowledgements.len() as u64);
        *active = FuturesUnordered::new();
        *rejection_acknowledgements = FuturesUnordered::new();
        SharedGatewayDrainOutcomeV3::DeadlineExceeded
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    #[test]
    fn configuration_is_nonzero_and_bounded() {
        assert_eq!(
            SharedGatewayRuntimeConfigV3::default().drain_timeout(),
            Duration::from_secs(15)
        );
        assert_eq!(
            SharedGatewayRuntimeConfigV3::new(Duration::ZERO),
            Err(SharedGatewayRuntimeConfigurationErrorV3::DrainTimeout)
        );
        assert_eq!(
            SharedGatewayRuntimeConfigV3::new(Duration::from_secs(61)),
            Err(SharedGatewayRuntimeConfigurationErrorV3::DrainTimeout)
        );
        assert_eq!(
            SharedGatewayRuntimeConfigV3::default()
                .rejection_acknowledgement_capacity()
                .get(),
            64
        );
        assert_eq!(
            SharedGatewayRuntimeConfigV3::default()
                .with_rejection_acknowledgement_capacity(NonZeroUsize::new(1_025).unwrap()),
            Err(SharedGatewayRuntimeConfigurationErrorV3::RejectionAcknowledgementCapacity)
        );
    }

    #[tokio::test]
    async fn interaction_callbacks_do_not_share_the_mutation_rate_limit_queue() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mutation_http = Client::new("test-token".to_string());
        let interaction_http = interaction_callback_client("test-token".to_string());
        assert!(mutation_http.ratelimiter().is_some());
        assert!(interaction_http.ratelimiter().is_none());
    }

    #[tokio::test]
    async fn rejection_acknowledgement_lane_is_strictly_bounded() {
        let mut acknowledgements: FuturesUnordered<
            SharedGatewayRejectionAcknowledgementFutureV3<'_>,
        > = FuturesUnordered::new();
        let mut report = SharedGatewayRuntimeReportV3::default();
        enqueue_rejection_acknowledgement_future(
            Box::pin(std::future::pending()),
            &mut acknowledgements,
            NonZeroUsize::new(1).unwrap(),
            &mut report,
        );
        enqueue_rejection_acknowledgement_future(
            Box::pin(std::future::ready(
                SharedGatewayRejectionAcknowledgementOutcomeV3::Sent,
            )),
            &mut acknowledgements,
            NonZeroUsize::new(1).unwrap(),
            &mut report,
        );
        assert_eq!(acknowledgements.len(), 1);
        assert_eq!(report.rejection_acknowledgement_dropped, 1);
    }

    #[tokio::test]
    async fn drain_collects_completed_work() {
        let mut active = FuturesUnordered::new();
        active.push(std::future::ready(
            SharedGatewayInteractionDispatchOutcomeV3::Executed(
                InteractionExecutionOutcomeV3::StaticExecuted,
            ),
        ));
        active.push(std::future::ready(
            SharedGatewayInteractionDispatchOutcomeV3::Executed(
                InteractionExecutionOutcomeV3::StaticFailed,
            ),
        ));
        let mut acknowledgements: FuturesUnordered<
            SharedGatewayRejectionAcknowledgementFutureV3<'_>,
        > = FuturesUnordered::new();
        acknowledgements.push(Box::pin(std::future::ready(
            SharedGatewayRejectionAcknowledgementOutcomeV3::Sent,
        )));
        let mut report = SharedGatewayRuntimeReportV3::default();
        assert_eq!(
            drain_active(
                &mut active,
                &mut acknowledgements,
                &mut report,
                Duration::from_secs(1),
                |_, _, _| panic!("completed dispatches do not require rejection acknowledgement"),
            )
            .await,
            SharedGatewayDrainOutcomeV3::Clean
        );
        assert_eq!(report.completed, 1);
        assert_eq!(report.execution_failed, 1);
        assert_eq!(report.rejection_acknowledged, 1);
        assert!(active.is_empty());
        assert!(acknowledgements.is_empty());
    }

    #[tokio::test]
    async fn drain_deadline_cancels_every_bounded_future() {
        struct DropSignal(Arc<AtomicUsize>);

        impl Drop for DropSignal {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let drops = Arc::new(AtomicUsize::new(0));
        let mut active = FuturesUnordered::new();
        for _ in 0..NonZeroUsize::new(3).unwrap().get() {
            let signal = DropSignal(Arc::clone(&drops));
            active.push(async move {
                let _signal = signal;
                std::future::pending().await
            });
        }
        let acknowledgement_drops = Arc::new(AtomicUsize::new(0));
        let mut acknowledgements: FuturesUnordered<
            SharedGatewayRejectionAcknowledgementFutureV3<'_>,
        > = FuturesUnordered::new();
        for _ in 0..NonZeroUsize::new(2).unwrap().get() {
            let signal = DropSignal(Arc::clone(&acknowledgement_drops));
            acknowledgements.push(Box::pin(async move {
                let _signal = signal;
                std::future::pending().await
            }));
        }
        let mut report = SharedGatewayRuntimeReportV3::default();
        assert_eq!(
            drain_active(
                &mut active,
                &mut acknowledgements,
                &mut report,
                Duration::from_millis(1),
                |_, _, _| panic!("pending dispatches do not require rejection acknowledgement"),
            )
            .await,
            SharedGatewayDrainOutcomeV3::DeadlineExceeded
        );
        assert_eq!(report.cancelled_during_drain, 3);
        assert_eq!(report.rejection_acknowledgements_cancelled_during_drain, 2);
        assert!(active.is_empty());
        assert!(acknowledgements.is_empty());
        assert_eq!(drops.load(Ordering::SeqCst), 3);
        assert_eq!(acknowledgement_drops.load(Ordering::SeqCst), 2);
    }
}
