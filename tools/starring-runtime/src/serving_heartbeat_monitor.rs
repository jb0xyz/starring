use std::fmt::{Debug, Formatter};
use std::future::{pending, Future};
use std::num::NonZeroU64;
use std::pin::Pin;
use std::time::{Duration, Instant};

use automation_runtime_controller::{RuntimeServingIdentityV2, RuntimeServingReceiptV2};
use automation_runtime_serving_postgres::{
    PostgresRuntimeServingLeaseV1, RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1,
    MAX_RUNTIME_SERVING_LEASE_DURATION, MIN_RUNTIME_SERVING_LEASE_DURATION,
};
use chrono::{DateTime, Utc};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{timeout_at, Instant as TokioInstant};

use crate::gateway::RuntimeGatewayReadyInvalidationObserverV2;
use crate::process_supervisor::RuntimeProcessShutdownTriggerV1;
use crate::registry::{
    RuntimeRegistryBarrierBServingErrorV2, RuntimeRegistryBarrierBServingMonitorAuthorityV2,
};
use crate::shutdown::RuntimeShutdownObserverV1;
use crate::{RuntimeShutdownCauseV1, RuntimeShutdownTriggerV1, RuntimeShutdownTripV1};

const RUNTIME_SERVING_HEARTBEAT_INTERVAL_V2: Duration = Duration::from_secs(15);
const RUNTIME_SERVING_HEARTBEAT_LEASE_V2: Duration = Duration::from_secs(45);
const RUNTIME_SERVING_HEARTBEAT_OPERATION_TIMEOUT_V2: Duration = Duration::from_secs(5);

type RuntimeServingHeartbeatSignalFutureV2 = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type RuntimeServingHeartbeatDatabaseFutureV2<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone)]
enum RuntimeServingHeartbeatShutdownTriggerV2 {
    Standalone(RuntimeShutdownTriggerV1),
    Process(RuntimeProcessShutdownTriggerV1),
}

impl RuntimeServingHeartbeatShutdownTriggerV2 {
    fn trip(&self, cause: RuntimeShutdownCauseV1) -> RuntimeShutdownTripV1 {
        match self {
            Self::Standalone(trigger) => trigger.trip(cause),
            Self::Process(trigger) => trigger.trip(cause),
        }
    }
}

impl From<RuntimeShutdownTriggerV1> for RuntimeServingHeartbeatShutdownTriggerV2 {
    fn from(trigger: RuntimeShutdownTriggerV1) -> Self {
        Self::Standalone(trigger)
    }
}

impl From<RuntimeProcessShutdownTriggerV1> for RuntimeServingHeartbeatShutdownTriggerV2 {
    fn from(trigger: RuntimeProcessShutdownTriggerV1) -> Self {
        Self::Process(trigger)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeServingHeartbeatMonitorConfigErrorV2 {
    #[error("runtime serving heartbeat interval is invalid")]
    InvalidInterval,
    #[error("runtime serving heartbeat lease duration is invalid")]
    InvalidLease,
    #[error("runtime serving heartbeat operation timeout is invalid")]
    InvalidOperationTimeout,
    #[error("runtime serving heartbeat timing has insufficient lease runway")]
    InsufficientRunway,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeServingHeartbeatMonitorConfigV2 {
    interval: Duration,
    lease_for: Duration,
    operation_timeout: Duration,
}

impl RuntimeServingHeartbeatMonitorConfigV2 {
    pub(crate) fn production_v2() -> Self {
        Self {
            interval: RUNTIME_SERVING_HEARTBEAT_INTERVAL_V2,
            lease_for: RUNTIME_SERVING_HEARTBEAT_LEASE_V2,
            operation_timeout: RUNTIME_SERVING_HEARTBEAT_OPERATION_TIMEOUT_V2,
        }
    }

    pub(crate) fn new_v2(
        interval: Duration,
        lease_for: Duration,
        operation_timeout: Duration,
    ) -> Result<Self, RuntimeServingHeartbeatMonitorConfigErrorV2> {
        if interval.is_zero() || !millisecond_aligned_v2(interval) {
            return Err(RuntimeServingHeartbeatMonitorConfigErrorV2::InvalidInterval);
        }
        if !(MIN_RUNTIME_SERVING_LEASE_DURATION..=MAX_RUNTIME_SERVING_LEASE_DURATION)
            .contains(&lease_for)
            || !millisecond_aligned_v2(lease_for)
        {
            return Err(RuntimeServingHeartbeatMonitorConfigErrorV2::InvalidLease);
        }
        if operation_timeout.is_zero() || !millisecond_aligned_v2(operation_timeout) {
            return Err(RuntimeServingHeartbeatMonitorConfigErrorV2::InvalidOperationTimeout);
        }
        let required = interval
            .checked_add(operation_timeout)
            .ok_or(RuntimeServingHeartbeatMonitorConfigErrorV2::InsufficientRunway)?;
        if required >= lease_for {
            return Err(RuntimeServingHeartbeatMonitorConfigErrorV2::InsufficientRunway);
        }
        Ok(Self {
            interval,
            lease_for,
            operation_timeout,
        })
    }

    pub(crate) const fn interval_v2(self) -> Duration {
        self.interval
    }

    pub(crate) const fn lease_for_v2(self) -> Duration {
        self.lease_for
    }

    pub(crate) const fn operation_timeout_v2(self) -> Duration {
        self.operation_timeout
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeServingHeartbeatMonitorPhaseV2 {
    Ready,
    Heartbeating,
    Stopped,
    FailedClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeServingHeartbeatMonitorHealthV2 {
    phase: RuntimeServingHeartbeatMonitorPhaseV2,
    last_confirmed_at: Instant,
    lease_deadline: Instant,
}

impl RuntimeServingHeartbeatMonitorHealthV2 {
    pub(crate) const fn phase_v2(self) -> RuntimeServingHeartbeatMonitorPhaseV2 {
        self.phase
    }

    pub(crate) const fn last_confirmed_at_v2(self) -> Instant {
        self.last_confirmed_at
    }

    pub(crate) const fn lease_deadline_v2(self) -> Instant {
        self.lease_deadline
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeServingHeartbeatTerminalStatusV2 {
    Commanded,
    ProcessShutdown,
    FailedClosed(RuntimeServingHeartbeatFailureV2),
    ActorPanicked,
}

#[derive(Clone)]
pub(crate) struct RuntimeServingHeartbeatTerminalObserverV2 {
    terminal: watch::Receiver<Option<RuntimeServingHeartbeatTerminalStatusV2>>,
}

impl RuntimeServingHeartbeatTerminalObserverV2 {
    pub(crate) fn current_v2(&self) -> Option<RuntimeServingHeartbeatTerminalStatusV2> {
        *self.terminal.borrow()
    }

    pub(crate) async fn wait_v2(&mut self) -> RuntimeServingHeartbeatTerminalStatusV2 {
        loop {
            if let Some(status) = *self.terminal.borrow_and_update() {
                return status;
            }
            if self.terminal.changed().await.is_err() {
                return RuntimeServingHeartbeatTerminalStatusV2::ActorPanicked;
            }
        }
    }
}

impl Debug for RuntimeServingHeartbeatTerminalObserverV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingHeartbeatTerminalObserverV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeServingHeartbeatStartFailureV2 {
    #[error("runtime serving heartbeat started with an invalid serving receipt")]
    InvalidReceipt,
    #[error("runtime serving heartbeat lease already expired")]
    LeaseExpired,
    #[error("runtime serving heartbeat exact registry route was lost")]
    RegistryLost,
    #[error("runtime serving heartbeat database observation timed out")]
    DatabaseObservationTimedOut,
    #[error("runtime serving heartbeat database observation failed")]
    DatabaseObservationFailed,
    #[error("runtime serving heartbeat database route is absent")]
    DatabaseAbsent,
    #[error("runtime serving heartbeat database route diverged")]
    DatabaseDiverged,
    #[error("runtime serving heartbeat database receipt is not exact")]
    DatabaseReceiptMismatch,
    #[error("runtime serving heartbeat process shutdown began")]
    ProcessShutdown,
    #[error("runtime serving heartbeat gateway owner was lost")]
    OwnerLost,
    #[error("runtime serving heartbeat gateway was invalidated")]
    GatewayLost,
}

impl RuntimeServingHeartbeatStartFailureV2 {
    pub(crate) const fn code_v2(self) -> &'static str {
        match self {
            Self::InvalidReceipt => "runtime_serving_heartbeat_start_invalid_receipt",
            Self::LeaseExpired => "runtime_serving_heartbeat_start_lease_expired",
            Self::RegistryLost => "runtime_serving_heartbeat_start_registry_lost",
            Self::DatabaseObservationTimedOut => {
                "runtime_serving_heartbeat_start_database_observation_timed_out"
            }
            Self::DatabaseObservationFailed => {
                "runtime_serving_heartbeat_start_database_observation_failed"
            }
            Self::DatabaseAbsent => "runtime_serving_heartbeat_start_database_absent",
            Self::DatabaseDiverged => "runtime_serving_heartbeat_start_database_diverged",
            Self::DatabaseReceiptMismatch => {
                "runtime_serving_heartbeat_start_database_receipt_mismatch"
            }
            Self::ProcessShutdown => "runtime_serving_heartbeat_start_process_shutdown",
            Self::OwnerLost => "runtime_serving_heartbeat_start_owner_lost",
            Self::GatewayLost => "runtime_serving_heartbeat_start_gateway_lost",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeServingHeartbeatFailureV2 {
    LeaseExpired,
    RegistryLost,
    IngressAcknowledgementLost,
    OwnershipLost,
    ProductAuthorityChanged,
    DatabaseUnavailable,
    DatabaseProtocolViolation,
    HeartbeatOutcomeUnresolved,
    HeartbeatSuccessorMismatch,
    DatabaseServingAbsent,
    DatabaseServingDiverged,
    OwnerLost,
    GatewayLost,
}

impl RuntimeServingHeartbeatFailureV2 {
    pub(crate) const fn code_v2(self) -> &'static str {
        match self {
            Self::LeaseExpired => "runtime_serving_heartbeat_lease_expired",
            Self::RegistryLost => "runtime_serving_heartbeat_registry_lost",
            Self::IngressAcknowledgementLost => {
                "runtime_serving_heartbeat_ingress_acknowledgement_lost"
            }
            Self::OwnershipLost => "runtime_serving_heartbeat_ownership_lost",
            Self::ProductAuthorityChanged => "runtime_serving_heartbeat_product_authority_changed",
            Self::DatabaseUnavailable => "runtime_serving_heartbeat_database_unavailable",
            Self::DatabaseProtocolViolation => {
                "runtime_serving_heartbeat_database_protocol_violation"
            }
            Self::HeartbeatOutcomeUnresolved => "runtime_serving_heartbeat_outcome_unresolved",
            Self::HeartbeatSuccessorMismatch => "runtime_serving_heartbeat_successor_mismatch",
            Self::DatabaseServingAbsent => "runtime_serving_heartbeat_database_serving_absent",
            Self::DatabaseServingDiverged => "runtime_serving_heartbeat_database_serving_diverged",
            Self::OwnerLost => "runtime_serving_heartbeat_owner_lost",
            Self::GatewayLost => "runtime_serving_heartbeat_gateway_lost",
        }
    }

    const fn shutdown_cause_v2(self) -> RuntimeShutdownCauseV1 {
        match self {
            Self::IngressAcknowledgementLost => {
                RuntimeShutdownCauseV1::IngressAcknowledgementTerminal
            }
            Self::OwnerLost | Self::OwnershipLost => RuntimeShutdownCauseV1::GatewayOwnerTerminal,
            Self::GatewayLost | Self::RegistryLost => RuntimeShutdownCauseV1::ReadinessLost,
            Self::ProductAuthorityChanged => RuntimeShutdownCauseV1::ProductAuthorityChanged,
            Self::LeaseExpired
            | Self::DatabaseUnavailable
            | Self::DatabaseProtocolViolation
            | Self::HeartbeatOutcomeUnresolved
            | Self::HeartbeatSuccessorMismatch
            | Self::DatabaseServingAbsent
            | Self::DatabaseServingDiverged => RuntimeShutdownCauseV1::HealthTerminal,
        }
    }
}

pub(crate) struct RuntimeServingHeartbeatRetainedStateV2<R> {
    last_confirmed_receipt: RuntimeServingReceiptV2,
    registry: R,
}

impl<R> RuntimeServingHeartbeatRetainedStateV2<R> {
    pub(crate) fn last_confirmed_receipt_v2(&self) -> &RuntimeServingReceiptV2 {
        &self.last_confirmed_receipt
    }

    pub(crate) fn into_parts_v2(self) -> (RuntimeServingReceiptV2, R) {
        (self.last_confirmed_receipt, self.registry)
    }
}

impl<R> Debug for RuntimeServingHeartbeatRetainedStateV2<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingHeartbeatRetainedStateV2(<redacted>)")
    }
}

pub(crate) enum RuntimeServingHeartbeatMonitorExitV2<R> {
    Commanded(RuntimeServingHeartbeatRetainedStateV2<R>),
    ProcessShutdown(RuntimeServingHeartbeatRetainedStateV2<R>),
    FailedClosed {
        failure: RuntimeServingHeartbeatFailureV2,
        retained: RuntimeServingHeartbeatRetainedStateV2<R>,
    },
    ActorPanicked,
    StopDeadlineElapsed,
}

impl<R> RuntimeServingHeartbeatMonitorExitV2<R> {
    pub(crate) const fn failure_v2(&self) -> Option<RuntimeServingHeartbeatFailureV2> {
        match self {
            Self::FailedClosed { failure, .. } => Some(*failure),
            Self::Commanded(_)
            | Self::ProcessShutdown(_)
            | Self::ActorPanicked
            | Self::StopDeadlineElapsed => None,
        }
    }

    pub(crate) fn into_retained_v2(self) -> Option<RuntimeServingHeartbeatRetainedStateV2<R>> {
        match self {
            Self::Commanded(retained) | Self::ProcessShutdown(retained) => Some(retained),
            Self::FailedClosed { retained, .. } => Some(retained),
            Self::ActorPanicked | Self::StopDeadlineElapsed => None,
        }
    }
}

impl<R> Debug for RuntimeServingHeartbeatMonitorExitV2<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingHeartbeatMonitorExitV2(<redacted>)")
    }
}

pub(crate) struct RuntimeServingHeartbeatExternalObserversV2 {
    owner_loss: RuntimeServingHeartbeatSignalFutureV2,
    gateway_loss: RuntimeServingHeartbeatSignalFutureV2,
}

impl RuntimeServingHeartbeatExternalObserversV2 {
    pub(crate) fn without_gateway_v2<Owner>(owner_loss: Owner) -> Self
    where
        Owner: Future<Output = ()> + Send + 'static,
    {
        Self {
            owner_loss: Box::pin(owner_loss),
            gateway_loss: Box::pin(pending()),
        }
    }

    pub(crate) fn with_exact_gateway_v2<Owner>(
        owner_loss: Owner,
        gateway_loss: RuntimeGatewayReadyInvalidationObserverV2,
    ) -> Result<Self, RuntimeServingHeartbeatStartFailureV2>
    where
        Owner: Future<Output = ()> + Send + 'static,
    {
        if gateway_loss.current_invalidation_v2().is_some() {
            return Err(RuntimeServingHeartbeatStartFailureV2::GatewayLost);
        }
        Ok(Self {
            owner_loss: Box::pin(owner_loss),
            gateway_loss: Box::pin(async move {
                let _invalidation = gateway_loss.wait_v2().await;
            }),
        })
    }
}

impl Debug for RuntimeServingHeartbeatExternalObserversV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingHeartbeatExternalObserversV2(<redacted>)")
    }
}

trait RuntimeServingHeartbeatDatabasePortV2: Send + Sync + 'static {
    fn observe_serving_v2<'a>(
        &'a self,
        identity: &'a RuntimeServingIdentityV2,
    ) -> RuntimeServingHeartbeatDatabaseFutureV2<
        'a,
        Result<RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1>,
    >;

    fn heartbeat_serving_v2<'a>(
        &'a self,
        identity: &'a RuntimeServingIdentityV2,
        lease_for: Duration,
    ) -> RuntimeServingHeartbeatDatabaseFutureV2<
        'a,
        Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1>,
    >;
}

impl RuntimeServingHeartbeatDatabasePortV2 for PostgresRuntimeServingLeaseV1 {
    fn observe_serving_v2<'a>(
        &'a self,
        identity: &'a RuntimeServingIdentityV2,
    ) -> RuntimeServingHeartbeatDatabaseFutureV2<
        'a,
        Result<RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1>,
    > {
        Box::pin(PostgresRuntimeServingLeaseV1::observe_serving_v2(
            self, identity,
        ))
    }

    fn heartbeat_serving_v2<'a>(
        &'a self,
        identity: &'a RuntimeServingIdentityV2,
        lease_for: Duration,
    ) -> RuntimeServingHeartbeatDatabaseFutureV2<
        'a,
        Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1>,
    > {
        Box::pin(PostgresRuntimeServingLeaseV1::heartbeat_serving_v2(
            self, identity, lease_for,
        ))
    }
}

trait RuntimeServingHeartbeatRegistryPortV2: Send + 'static {
    fn observe_exact_serving_v2(&self) -> Result<(), RuntimeRegistryBarrierBServingErrorV2>;
}

impl RuntimeServingHeartbeatRegistryPortV2 for RuntimeRegistryBarrierBServingMonitorAuthorityV2 {
    fn observe_exact_serving_v2(&self) -> Result<(), RuntimeRegistryBarrierBServingErrorV2> {
        RuntimeRegistryBarrierBServingMonitorAuthorityV2::observe_exact_serving_v2(self).map(|_| ())
    }
}

pub(crate) struct RuntimeServingHeartbeatMonitorCoreV2<R> {
    stop: Option<oneshot::Sender<()>>,
    health: watch::Receiver<RuntimeServingHeartbeatMonitorHealthV2>,
    terminal: RuntimeServingHeartbeatTerminalObserverV2,
    actor: Option<JoinHandle<RuntimeServingHeartbeatActorExitV2<R>>>,
    shutdown: RuntimeServingHeartbeatShutdownTriggerV2,
    armed: bool,
}

pub(crate) struct RuntimeServingHeartbeatMonitorReadyCoreV2<R> {
    monitor: RuntimeServingHeartbeatMonitorCoreV2<R>,
    health: RuntimeServingHeartbeatMonitorHealthV2,
}

pub(crate) type RuntimeServingHeartbeatMonitorV2 =
    RuntimeServingHeartbeatMonitorCoreV2<RuntimeRegistryBarrierBServingMonitorAuthorityV2>;
pub(crate) type RuntimeServingHeartbeatMonitorReadyV2 =
    RuntimeServingHeartbeatMonitorReadyCoreV2<RuntimeRegistryBarrierBServingMonitorAuthorityV2>;
pub(crate) type RuntimeServingHeartbeatRetainedV2 =
    RuntimeServingHeartbeatRetainedStateV2<RuntimeRegistryBarrierBServingMonitorAuthorityV2>;
pub(crate) type RuntimeServingHeartbeatMonitorOutcomeV2 =
    RuntimeServingHeartbeatMonitorExitV2<RuntimeRegistryBarrierBServingMonitorAuthorityV2>;

impl<R> RuntimeServingHeartbeatMonitorReadyCoreV2<R> {
    pub(crate) const fn health_v2(&self) -> RuntimeServingHeartbeatMonitorHealthV2 {
        self.health
    }

    pub(crate) fn into_monitor_v2(self) -> RuntimeServingHeartbeatMonitorCoreV2<R> {
        self.monitor
    }
}

impl<R> Debug for RuntimeServingHeartbeatMonitorReadyCoreV2<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingHeartbeatMonitorReadyV2(<redacted>)")
    }
}

impl<R> RuntimeServingHeartbeatMonitorCoreV2<R> {
    pub(crate) fn health_v2(&self) -> RuntimeServingHeartbeatMonitorHealthV2 {
        *self.health.borrow()
    }

    pub(crate) fn terminal_observer_v2(&self) -> RuntimeServingHeartbeatTerminalObserverV2 {
        self.terminal.clone()
    }

    pub(crate) async fn stop_until_v2(
        mut self,
        deadline: Instant,
    ) -> RuntimeServingHeartbeatMonitorExitV2<R> {
        if Instant::now() >= deadline {
            return self.abort_for_deadline_v2();
        }
        if let Some(stop) = self.stop.take() {
            let _delivered = stop.send(()).is_ok();
        }
        let Some(actor) = self.actor.as_mut() else {
            self.armed = false;
            return RuntimeServingHeartbeatMonitorExitV2::ActorPanicked;
        };
        match timeout_at(TokioInstant::from_std(deadline), actor).await {
            Ok(Ok(exit)) => {
                self.armed = false;
                self.actor.take();
                exit.into_public_v2()
            }
            Ok(Err(_)) => {
                self.shutdown
                    .trip(RuntimeShutdownCauseV1::SupervisorFailure);
                self.armed = false;
                self.actor.take();
                RuntimeServingHeartbeatMonitorExitV2::ActorPanicked
            }
            Err(_) => self.abort_for_deadline_v2(),
        }
    }

    pub(crate) async fn wait_v2(mut self) -> RuntimeServingHeartbeatMonitorExitV2<R> {
        let Some(actor) = self.actor.as_mut() else {
            self.armed = false;
            return RuntimeServingHeartbeatMonitorExitV2::ActorPanicked;
        };
        match actor.await {
            Ok(exit) => {
                self.armed = false;
                self.actor.take();
                exit.into_public_v2()
            }
            Err(_) => {
                self.shutdown
                    .trip(RuntimeShutdownCauseV1::SupervisorFailure);
                self.armed = false;
                self.actor.take();
                RuntimeServingHeartbeatMonitorExitV2::ActorPanicked
            }
        }
    }

    fn abort_for_deadline_v2(&mut self) -> RuntimeServingHeartbeatMonitorExitV2<R> {
        self.shutdown
            .trip(RuntimeShutdownCauseV1::SupervisorFailure);
        if let Some(actor) = self.actor.take() {
            actor.abort();
        }
        self.armed = false;
        RuntimeServingHeartbeatMonitorExitV2::StopDeadlineElapsed
    }
}

impl<R> Drop for RuntimeServingHeartbeatMonitorCoreV2<R> {
    fn drop(&mut self) {
        if self.armed {
            self.shutdown
                .trip(RuntimeShutdownCauseV1::SupervisorFailure);
            if let Some(actor) = self.actor.take() {
                actor.abort();
            }
        }
    }
}

impl<R> Debug for RuntimeServingHeartbeatMonitorCoreV2<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingHeartbeatMonitorV2(<redacted>)")
    }
}

pub(crate) async fn start_runtime_serving_heartbeat_monitor_v2(
    receipt: RuntimeServingReceiptV2,
    database: PostgresRuntimeServingLeaseV1,
    registry: RuntimeRegistryBarrierBServingMonitorAuthorityV2,
    shutdown_trigger: RuntimeShutdownTriggerV1,
    shutdown_observer: RuntimeShutdownObserverV1,
    observers: RuntimeServingHeartbeatExternalObserversV2,
    config: RuntimeServingHeartbeatMonitorConfigV2,
) -> Result<RuntimeServingHeartbeatMonitorReadyV2, RuntimeServingHeartbeatStartFailureV2> {
    start_runtime_serving_heartbeat_monitor_with_ports_v2(
        receipt,
        database,
        registry,
        shutdown_trigger,
        shutdown_observer,
        observers,
        config,
    )
    .await
}

pub(crate) async fn start_runtime_process_serving_heartbeat_monitor_v2(
    receipt: RuntimeServingReceiptV2,
    database: PostgresRuntimeServingLeaseV1,
    registry: RuntimeRegistryBarrierBServingMonitorAuthorityV2,
    shutdown_trigger: RuntimeProcessShutdownTriggerV1,
    shutdown_observer: RuntimeShutdownObserverV1,
    observers: RuntimeServingHeartbeatExternalObserversV2,
    config: RuntimeServingHeartbeatMonitorConfigV2,
) -> Result<RuntimeServingHeartbeatMonitorReadyV2, RuntimeServingHeartbeatStartFailureV2> {
    start_runtime_serving_heartbeat_monitor_with_ports_v2(
        receipt,
        database,
        registry,
        shutdown_trigger,
        shutdown_observer,
        observers,
        config,
    )
    .await
}

async fn start_runtime_serving_heartbeat_monitor_with_ports_v2<D, R, S>(
    receipt: RuntimeServingReceiptV2,
    database: D,
    registry: R,
    shutdown_trigger: S,
    mut shutdown_observer: RuntimeShutdownObserverV1,
    mut observers: RuntimeServingHeartbeatExternalObserversV2,
    config: RuntimeServingHeartbeatMonitorConfigV2,
) -> Result<RuntimeServingHeartbeatMonitorReadyCoreV2<R>, RuntimeServingHeartbeatStartFailureV2>
where
    D: RuntimeServingHeartbeatDatabasePortV2,
    R: RuntimeServingHeartbeatRegistryPortV2,
    S: Into<RuntimeServingHeartbeatShutdownTriggerV2>,
{
    let shutdown_trigger = shutdown_trigger.into();
    let mut emergency = RuntimeServingHeartbeatEmergencyGuardV2::new_v2(
        shutdown_trigger.clone(),
        RuntimeShutdownCauseV1::SupervisorFailure,
    );
    validate_current_receipt_v2(&receipt)
        .map_err(|failure| map_initial_receipt_failure_v2(failure, &shutdown_trigger))?;
    let sampled_at = Instant::now();
    let lease_deadline =
        serving_lease_deadline_v2(&receipt, Utc::now(), sampled_at).ok_or_else(|| {
            shutdown_trigger.trip(RuntimeShutdownCauseV1::HealthTerminal);
            RuntimeServingHeartbeatStartFailureV2::LeaseExpired
        })?;
    registry.observe_exact_serving_v2().map_err(|_| {
        shutdown_trigger.trip(RuntimeShutdownCauseV1::ReadinessLost);
        RuntimeServingHeartbeatStartFailureV2::RegistryLost
    })?;
    if shutdown_observer.observed().is_some() {
        return Err(RuntimeServingHeartbeatStartFailureV2::ProcessShutdown);
    }
    let observation_deadline = sampled_at
        .checked_add(config.operation_timeout)
        .map_or(lease_deadline, |deadline| deadline.min(lease_deadline));
    let database_observation = {
        let observation = timeout_at(
            TokioInstant::from_std(observation_deadline),
            database.observe_serving_v2(&receipt.identity),
        );
        tokio::pin!(observation);
        tokio::select! {
            biased;
            _ = shutdown_observer.wait() => {
                return Err(RuntimeServingHeartbeatStartFailureV2::ProcessShutdown);
            }
            _ = observers.owner_loss.as_mut() => {
                shutdown_trigger.trip(RuntimeShutdownCauseV1::GatewayOwnerTerminal);
                return Err(RuntimeServingHeartbeatStartFailureV2::OwnerLost);
            }
            _ = observers.gateway_loss.as_mut() => {
                shutdown_trigger.trip(RuntimeShutdownCauseV1::ReadinessLost);
                return Err(RuntimeServingHeartbeatStartFailureV2::GatewayLost);
            }
            result = &mut observation => result,
        }
    };
    let observation = database_observation
        .map_err(|_| RuntimeServingHeartbeatStartFailureV2::DatabaseObservationTimedOut)?
        .map_err(|_| RuntimeServingHeartbeatStartFailureV2::DatabaseObservationFailed)?;
    match observation {
        RuntimeServingObservationV2::Current {
            serving,
            observed_at,
        } if *serving == receipt && observed_at < receipt.expires_at => {}
        RuntimeServingObservationV2::Current { .. } => {
            shutdown_trigger.trip(RuntimeShutdownCauseV1::HealthTerminal);
            return Err(RuntimeServingHeartbeatStartFailureV2::DatabaseReceiptMismatch);
        }
        RuntimeServingObservationV2::Absent { .. } => {
            shutdown_trigger.trip(RuntimeShutdownCauseV1::HealthTerminal);
            return Err(RuntimeServingHeartbeatStartFailureV2::DatabaseAbsent);
        }
        RuntimeServingObservationV2::Diverged { .. } => {
            shutdown_trigger.trip(RuntimeShutdownCauseV1::HealthTerminal);
            return Err(RuntimeServingHeartbeatStartFailureV2::DatabaseDiverged);
        }
    }
    registry.observe_exact_serving_v2().map_err(|_| {
        shutdown_trigger.trip(RuntimeShutdownCauseV1::ReadinessLost);
        RuntimeServingHeartbeatStartFailureV2::RegistryLost
    })?;
    let health = RuntimeServingHeartbeatMonitorHealthV2 {
        phase: RuntimeServingHeartbeatMonitorPhaseV2::Ready,
        last_confirmed_at: Instant::now(),
        lease_deadline,
    };
    let (health_sender, health_receiver) = watch::channel(health);
    let (terminal_sender, terminal_receiver) = watch::channel(None);
    let (stop_sender, stop_receiver) = oneshot::channel();
    let actor_shutdown = shutdown_trigger.clone();
    let actor = tokio::spawn(async move {
        run_runtime_serving_heartbeat_actor_v2(RuntimeServingHeartbeatActorInputsV2 {
            receipt,
            database,
            registry,
            shutdown_trigger: actor_shutdown,
            shutdown_observer,
            observers,
            stop: stop_receiver,
            health: health_sender,
            terminal: terminal_sender,
            config,
        })
        .await
    });
    emergency.disarm_v2();
    Ok(RuntimeServingHeartbeatMonitorReadyCoreV2 {
        monitor: RuntimeServingHeartbeatMonitorCoreV2 {
            stop: Some(stop_sender),
            health: health_receiver,
            terminal: RuntimeServingHeartbeatTerminalObserverV2 {
                terminal: terminal_receiver,
            },
            actor: Some(actor),
            shutdown: shutdown_trigger,
            armed: true,
        },
        health,
    })
}

enum RuntimeServingHeartbeatActorExitV2<R> {
    Commanded(RuntimeServingHeartbeatRetainedStateV2<R>),
    ProcessShutdown(RuntimeServingHeartbeatRetainedStateV2<R>),
    FailedClosed {
        failure: RuntimeServingHeartbeatFailureV2,
        retained: RuntimeServingHeartbeatRetainedStateV2<R>,
    },
}

impl<R> RuntimeServingHeartbeatActorExitV2<R> {
    fn status_v2(&self) -> RuntimeServingHeartbeatTerminalStatusV2 {
        match self {
            Self::Commanded(_) => RuntimeServingHeartbeatTerminalStatusV2::Commanded,
            Self::ProcessShutdown(_) => RuntimeServingHeartbeatTerminalStatusV2::ProcessShutdown,
            Self::FailedClosed { failure, .. } => {
                RuntimeServingHeartbeatTerminalStatusV2::FailedClosed(*failure)
            }
        }
    }

    fn into_public_v2(self) -> RuntimeServingHeartbeatMonitorExitV2<R> {
        match self {
            Self::Commanded(retained) => RuntimeServingHeartbeatMonitorExitV2::Commanded(retained),
            Self::ProcessShutdown(retained) => {
                RuntimeServingHeartbeatMonitorExitV2::ProcessShutdown(retained)
            }
            Self::FailedClosed { failure, retained } => {
                RuntimeServingHeartbeatMonitorExitV2::FailedClosed { failure, retained }
            }
        }
    }
}

struct RuntimeServingHeartbeatActorInputsV2<D, R> {
    receipt: RuntimeServingReceiptV2,
    database: D,
    registry: R,
    shutdown_trigger: RuntimeServingHeartbeatShutdownTriggerV2,
    shutdown_observer: RuntimeShutdownObserverV1,
    observers: RuntimeServingHeartbeatExternalObserversV2,
    stop: oneshot::Receiver<()>,
    health: watch::Sender<RuntimeServingHeartbeatMonitorHealthV2>,
    terminal: watch::Sender<Option<RuntimeServingHeartbeatTerminalStatusV2>>,
    config: RuntimeServingHeartbeatMonitorConfigV2,
}

async fn run_runtime_serving_heartbeat_actor_v2<D, R>(
    inputs: RuntimeServingHeartbeatActorInputsV2<D, R>,
) -> RuntimeServingHeartbeatActorExitV2<R>
where
    D: RuntimeServingHeartbeatDatabasePortV2,
    R: RuntimeServingHeartbeatRegistryPortV2,
{
    let RuntimeServingHeartbeatActorInputsV2 {
        mut receipt,
        database,
        registry,
        shutdown_trigger,
        mut shutdown_observer,
        mut observers,
        mut stop,
        health,
        terminal,
        config,
    } = inputs;
    let mut emergency = RuntimeServingHeartbeatEmergencyGuardV2::new_v2(
        shutdown_trigger.clone(),
        RuntimeShutdownCauseV1::SupervisorFailure,
    );
    let exit = loop {
        let now = Instant::now();
        let Some(schedule) = serving_heartbeat_schedule_v2(&receipt, config, Utc::now(), now)
        else {
            break failed_closed_actor_exit_v2(
                RuntimeServingHeartbeatFailureV2::LeaseExpired,
                receipt,
                registry,
                &shutdown_trigger,
                &health,
            );
        };
        health.send_replace(RuntimeServingHeartbeatMonitorHealthV2 {
            phase: RuntimeServingHeartbeatMonitorPhaseV2::Ready,
            last_confirmed_at: now,
            lease_deadline: schedule.lease_deadline,
        });
        let event = tokio::select! {
            biased;
            _ = shutdown_observer.wait() => RuntimeServingHeartbeatActorEventV2::ProcessShutdown,
            _ = observers.owner_loss.as_mut() => RuntimeServingHeartbeatActorEventV2::OwnerLost,
            _ = observers.gateway_loss.as_mut() => RuntimeServingHeartbeatActorEventV2::GatewayLost,
            _ = &mut stop => RuntimeServingHeartbeatActorEventV2::Commanded,
            _ = tokio::time::sleep_until(TokioInstant::from_std(schedule.heartbeat_at)) => {
                RuntimeServingHeartbeatActorEventV2::Heartbeat
            }
        };
        match event {
            RuntimeServingHeartbeatActorEventV2::Commanded => {
                health.send_modify(|snapshot| {
                    snapshot.phase = RuntimeServingHeartbeatMonitorPhaseV2::Stopped;
                });
                break RuntimeServingHeartbeatActorExitV2::Commanded(
                    RuntimeServingHeartbeatRetainedStateV2 {
                        last_confirmed_receipt: receipt,
                        registry,
                    },
                );
            }
            RuntimeServingHeartbeatActorEventV2::ProcessShutdown => {
                health.send_modify(|snapshot| {
                    snapshot.phase = RuntimeServingHeartbeatMonitorPhaseV2::Stopped;
                });
                break RuntimeServingHeartbeatActorExitV2::ProcessShutdown(
                    RuntimeServingHeartbeatRetainedStateV2 {
                        last_confirmed_receipt: receipt,
                        registry,
                    },
                );
            }
            RuntimeServingHeartbeatActorEventV2::OwnerLost => {
                break failed_closed_actor_exit_v2(
                    RuntimeServingHeartbeatFailureV2::OwnerLost,
                    receipt,
                    registry,
                    &shutdown_trigger,
                    &health,
                );
            }
            RuntimeServingHeartbeatActorEventV2::GatewayLost => {
                break failed_closed_actor_exit_v2(
                    RuntimeServingHeartbeatFailureV2::GatewayLost,
                    receipt,
                    registry,
                    &shutdown_trigger,
                    &health,
                );
            }
            RuntimeServingHeartbeatActorEventV2::Heartbeat => {}
        }
        if registry.observe_exact_serving_v2().is_err() {
            break failed_closed_actor_exit_v2(
                RuntimeServingHeartbeatFailureV2::RegistryLost,
                receipt,
                registry,
                &shutdown_trigger,
                &health,
            );
        }
        health.send_modify(|snapshot| {
            snapshot.phase = RuntimeServingHeartbeatMonitorPhaseV2::Heartbeating;
        });
        let heartbeat = execute_runtime_serving_heartbeat_v2(
            &database,
            &receipt,
            config,
            schedule.lease_deadline,
        )
        .await;
        let next = match heartbeat {
            Ok(next) => next,
            Err(failure) => {
                break failed_closed_actor_exit_v2(
                    failure,
                    receipt,
                    registry,
                    &shutdown_trigger,
                    &health,
                );
            }
        };
        receipt = next;
        if registry.observe_exact_serving_v2().is_err() {
            break failed_closed_actor_exit_v2(
                RuntimeServingHeartbeatFailureV2::RegistryLost,
                receipt,
                registry,
                &shutdown_trigger,
                &health,
            );
        }
    };
    terminal.send_replace(Some(exit.status_v2()));
    emergency.disarm_v2();
    exit
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeServingHeartbeatActorEventV2 {
    Commanded,
    ProcessShutdown,
    OwnerLost,
    GatewayLost,
    Heartbeat,
}

fn failed_closed_actor_exit_v2<R>(
    failure: RuntimeServingHeartbeatFailureV2,
    receipt: RuntimeServingReceiptV2,
    registry: R,
    shutdown: &RuntimeServingHeartbeatShutdownTriggerV2,
    health: &watch::Sender<RuntimeServingHeartbeatMonitorHealthV2>,
) -> RuntimeServingHeartbeatActorExitV2<R> {
    shutdown.trip(failure.shutdown_cause_v2());
    health.send_modify(|snapshot| {
        snapshot.phase = RuntimeServingHeartbeatMonitorPhaseV2::FailedClosed;
    });
    RuntimeServingHeartbeatActorExitV2::FailedClosed {
        failure,
        retained: RuntimeServingHeartbeatRetainedStateV2 {
            last_confirmed_receipt: receipt,
            registry,
        },
    }
}

async fn execute_runtime_serving_heartbeat_v2<D>(
    database: &D,
    current: &RuntimeServingReceiptV2,
    config: RuntimeServingHeartbeatMonitorConfigV2,
    lease_deadline: Instant,
) -> Result<RuntimeServingReceiptV2, RuntimeServingHeartbeatFailureV2>
where
    D: RuntimeServingHeartbeatDatabasePortV2,
{
    let mutation_deadline = Instant::now()
        .checked_add(config.operation_timeout)
        .map_or(lease_deadline, |deadline| deadline.min(lease_deadline));
    if Instant::now() >= mutation_deadline {
        return Err(RuntimeServingHeartbeatFailureV2::LeaseExpired);
    }
    let mutation = timeout_at(
        TokioInstant::from_std(mutation_deadline),
        database.heartbeat_serving_v2(&current.identity, config.lease_for),
    )
    .await;
    match mutation {
        Ok(Ok(successor)) => {
            validate_heartbeat_successor_v2(current, &successor, config.lease_for)?;
            Ok(successor)
        }
        Ok(Err(RuntimeServingPersistenceErrorV1::Indeterminate)) | Err(_) => {
            resolve_unknown_runtime_serving_heartbeat_v2(database, current, config, lease_deadline)
                .await
        }
        Ok(Err(error)) => Err(map_heartbeat_persistence_failure_v2(error)),
    }
}

async fn resolve_unknown_runtime_serving_heartbeat_v2<D>(
    database: &D,
    current: &RuntimeServingReceiptV2,
    config: RuntimeServingHeartbeatMonitorConfigV2,
    lease_deadline: Instant,
) -> Result<RuntimeServingReceiptV2, RuntimeServingHeartbeatFailureV2>
where
    D: RuntimeServingHeartbeatDatabasePortV2,
{
    let observation_deadline = Instant::now()
        .checked_add(config.operation_timeout)
        .map_or(lease_deadline, |deadline| deadline.min(lease_deadline));
    if Instant::now() >= observation_deadline {
        return Err(RuntimeServingHeartbeatFailureV2::HeartbeatOutcomeUnresolved);
    }
    let old = timeout_at(
        TokioInstant::from_std(observation_deadline),
        database.observe_serving_v2(&current.identity),
    )
    .await
    .map_err(|_| RuntimeServingHeartbeatFailureV2::HeartbeatOutcomeUnresolved)?
    .map_err(map_observation_persistence_failure_v2)?;
    match old {
        RuntimeServingObservationV2::Current {
            serving,
            observed_at,
        } if *serving == *current && observed_at < current.expires_at => Ok(*serving),
        RuntimeServingObservationV2::Current { .. } => {
            Err(RuntimeServingHeartbeatFailureV2::DatabaseProtocolViolation)
        }
        RuntimeServingObservationV2::Absent { .. } => {
            Err(RuntimeServingHeartbeatFailureV2::DatabaseServingAbsent)
        }
        RuntimeServingObservationV2::Diverged { .. } => {
            let successor_identity = one_step_successor_identity_v2(&current.identity)?;
            let successor = timeout_at(
                TokioInstant::from_std(observation_deadline),
                database.observe_serving_v2(&successor_identity),
            )
            .await
            .map_err(|_| RuntimeServingHeartbeatFailureV2::HeartbeatOutcomeUnresolved)?
            .map_err(map_observation_persistence_failure_v2)?;
            match successor {
                RuntimeServingObservationV2::Current { serving, .. } => {
                    validate_heartbeat_successor_v2(current, &serving, config.lease_for)?;
                    Ok(*serving)
                }
                RuntimeServingObservationV2::Absent { .. }
                | RuntimeServingObservationV2::Diverged { .. } => {
                    Err(RuntimeServingHeartbeatFailureV2::DatabaseServingDiverged)
                }
            }
        }
    }
}

fn validate_current_receipt_v2(
    receipt: &RuntimeServingReceiptV2,
) -> Result<(), RuntimeServingHeartbeatFailureV2> {
    if !receipt.connected
        || !receipt.serving
        || receipt.acquired_at > receipt.last_heartbeat_at
        || receipt.last_heartbeat_at >= receipt.expires_at
    {
        Err(RuntimeServingHeartbeatFailureV2::DatabaseProtocolViolation)
    } else {
        Ok(())
    }
}

fn validate_heartbeat_successor_v2(
    current: &RuntimeServingReceiptV2,
    successor: &RuntimeServingReceiptV2,
    lease_for: Duration,
) -> Result<(), RuntimeServingHeartbeatFailureV2> {
    let expected_identity = one_step_successor_identity_v2(&current.identity)?;
    let expected_lease = chrono::Duration::from_std(lease_for)
        .map_err(|_| RuntimeServingHeartbeatFailureV2::HeartbeatSuccessorMismatch)?;
    if successor.identity != expected_identity
        || successor.acquired_at != current.acquired_at
        || successor.last_heartbeat_at < current.last_heartbeat_at
        || successor
            .expires_at
            .signed_duration_since(successor.last_heartbeat_at)
            != expected_lease
        || !successor.connected
        || !successor.serving
    {
        Err(RuntimeServingHeartbeatFailureV2::HeartbeatSuccessorMismatch)
    } else {
        Ok(())
    }
}

fn one_step_successor_identity_v2(
    current: &RuntimeServingIdentityV2,
) -> Result<RuntimeServingIdentityV2, RuntimeServingHeartbeatFailureV2> {
    let revision = current
        .revision
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeServingHeartbeatFailureV2::HeartbeatSuccessorMismatch)?;
    Ok(RuntimeServingIdentityV2 {
        scope: current.scope.clone(),
        operation_id: current.operation_id.clone(),
        attestation_digest: current.attestation_digest.clone(),
        process_identity: current.process_identity.clone(),
        lease_epoch: current.lease_epoch,
        revision,
    })
}

fn map_initial_receipt_failure_v2(
    _failure: RuntimeServingHeartbeatFailureV2,
    shutdown: &RuntimeServingHeartbeatShutdownTriggerV2,
) -> RuntimeServingHeartbeatStartFailureV2 {
    shutdown.trip(RuntimeShutdownCauseV1::HealthTerminal);
    RuntimeServingHeartbeatStartFailureV2::InvalidReceipt
}

fn map_heartbeat_persistence_failure_v2(
    error: RuntimeServingPersistenceErrorV1,
) -> RuntimeServingHeartbeatFailureV2 {
    match error {
        RuntimeServingPersistenceErrorV1::RetryNotReady => {
            RuntimeServingHeartbeatFailureV2::IngressAcknowledgementLost
        }
        RuntimeServingPersistenceErrorV1::OwnershipLost => {
            RuntimeServingHeartbeatFailureV2::OwnershipLost
        }
        RuntimeServingPersistenceErrorV1::AuthorityChanged => {
            RuntimeServingHeartbeatFailureV2::ProductAuthorityChanged
        }
        RuntimeServingPersistenceErrorV1::Timeout
        | RuntimeServingPersistenceErrorV1::Concurrency
        | RuntimeServingPersistenceErrorV1::Unavailable => {
            RuntimeServingHeartbeatFailureV2::DatabaseUnavailable
        }
        RuntimeServingPersistenceErrorV1::InvalidInput
        | RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch
        | RuntimeServingPersistenceErrorV1::PersistenceCorrupt
        | RuntimeServingPersistenceErrorV1::DatabaseFailure
        | RuntimeServingPersistenceErrorV1::Indeterminate => {
            RuntimeServingHeartbeatFailureV2::DatabaseProtocolViolation
        }
        _ => RuntimeServingHeartbeatFailureV2::DatabaseProtocolViolation,
    }
}

fn map_observation_persistence_failure_v2(
    error: RuntimeServingPersistenceErrorV1,
) -> RuntimeServingHeartbeatFailureV2 {
    match error {
        RuntimeServingPersistenceErrorV1::OwnershipLost => {
            RuntimeServingHeartbeatFailureV2::OwnershipLost
        }
        RuntimeServingPersistenceErrorV1::AuthorityChanged => {
            RuntimeServingHeartbeatFailureV2::ProductAuthorityChanged
        }
        RuntimeServingPersistenceErrorV1::Timeout
        | RuntimeServingPersistenceErrorV1::Concurrency
        | RuntimeServingPersistenceErrorV1::Unavailable
        | RuntimeServingPersistenceErrorV1::Indeterminate => {
            RuntimeServingHeartbeatFailureV2::HeartbeatOutcomeUnresolved
        }
        RuntimeServingPersistenceErrorV1::RetryNotReady => {
            RuntimeServingHeartbeatFailureV2::IngressAcknowledgementLost
        }
        RuntimeServingPersistenceErrorV1::InvalidInput
        | RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch
        | RuntimeServingPersistenceErrorV1::PersistenceCorrupt
        | RuntimeServingPersistenceErrorV1::DatabaseFailure => {
            RuntimeServingHeartbeatFailureV2::DatabaseProtocolViolation
        }
        _ => RuntimeServingHeartbeatFailureV2::DatabaseProtocolViolation,
    }
}

struct RuntimeServingHeartbeatScheduleV2 {
    heartbeat_at: Instant,
    lease_deadline: Instant,
}

fn serving_heartbeat_schedule_v2(
    receipt: &RuntimeServingReceiptV2,
    config: RuntimeServingHeartbeatMonitorConfigV2,
    utc_now: DateTime<Utc>,
    monotonic_now: Instant,
) -> Option<RuntimeServingHeartbeatScheduleV2> {
    validate_current_receipt_v2(receipt).ok()?;
    let lease_deadline = serving_lease_deadline_v2(receipt, utc_now, monotonic_now)?;
    let latest_start = lease_deadline.checked_sub(config.operation_timeout)?;
    let cadence = monotonic_now.checked_add(config.interval)?;
    let heartbeat_at = cadence.min(latest_start);
    if heartbeat_at < monotonic_now {
        return None;
    }
    Some(RuntimeServingHeartbeatScheduleV2 {
        heartbeat_at,
        lease_deadline,
    })
}

fn serving_lease_deadline_v2(
    receipt: &RuntimeServingReceiptV2,
    utc_now: DateTime<Utc>,
    monotonic_now: Instant,
) -> Option<Instant> {
    let remaining = receipt
        .expires_at
        .signed_duration_since(utc_now)
        .to_std()
        .ok()?;
    if remaining.is_zero() {
        return None;
    }
    monotonic_now.checked_add(remaining)
}

fn millisecond_aligned_v2(duration: Duration) -> bool {
    duration.subsec_nanos().is_multiple_of(1_000_000)
}

struct RuntimeServingHeartbeatEmergencyGuardV2 {
    shutdown: RuntimeServingHeartbeatShutdownTriggerV2,
    cause: RuntimeShutdownCauseV1,
    armed: bool,
}

impl RuntimeServingHeartbeatEmergencyGuardV2 {
    fn new_v2(
        shutdown: RuntimeServingHeartbeatShutdownTriggerV2,
        cause: RuntimeShutdownCauseV1,
    ) -> Self {
        Self {
            shutdown,
            cause,
            armed: true,
        }
    }

    fn disarm_v2(&mut self) {
        self.armed = false;
    }
}

impl Drop for RuntimeServingHeartbeatEmergencyGuardV2 {
    fn drop(&mut self) {
        if self.armed {
            self.shutdown.trip(self.cause);
        }
    }
}

impl Debug for RuntimeServingHeartbeatEmergencyGuardV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingHeartbeatEmergencyGuardV2(<redacted>)")
    }
}

#[cfg(test)]
#[path = "serving_heartbeat_monitor_tests.rs"]
mod tests;
