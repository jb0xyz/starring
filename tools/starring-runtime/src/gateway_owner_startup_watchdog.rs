use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeReleaseGatewayOwnerLeaseV1,
};
use automation_runtime_worker::{
    accept_gateway_owner_release_v1, classify_unknown_gateway_owner_release_v1,
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeAcceptedGatewayOwnerReleaseV1,
    RuntimeClosedDrainRecoveryPermitV2, RuntimeGatewayOwnerLeasePortV1,
    RuntimeGatewayOwnerMutationErrorV1, RuntimeGatewayOwnerObservationCompletionV1,
    RuntimeGatewayOwnerObservationErrorClassV1, RuntimeGatewayOwnerReleaseRecoveryV1,
    RuntimeGatewayOwnerRenewalCompletionV1, RuntimeGatewayOwnerRenewalPolicyV1,
    RuntimeGatewayOwnerRenewalScheduleErrorV1, RuntimeGatewayOwnerWatchdogActionV1,
    RuntimeGatewayOwnerWatchdogErrorV1, RuntimeGatewayOwnerWatchdogV1,
};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep_until, timeout_at, Instant as TokioInstant};

use crate::GatewayOwnerTimingConfigV1;

const SHUTDOWN_COMMAND_CAPACITY: usize = 1;
const SUPERVISOR_COMMAND_CAPACITY: usize = 1;
const CLOSED_RECOVERY_COMMAND_CAPACITY: usize = 1;
const RELEASE_ATTEMPTS: usize = 2;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(250);
const DEFAULT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
const MAXIMUM_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_ACTOR_TERMINATION_RESERVE: Duration = Duration::from_millis(100);
const STARTUP_TASK_ABORT_RESERVE: Duration = Duration::from_millis(25);

pub(crate) trait RuntimeGatewayOwnerEmergencyInvalidatorV1: Send + Sync + 'static {
    fn invalidate_gateway_ownership(&self);

    fn gateway_shutdown_watch(&self) -> Option<watch::Receiver<bool>> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerStartupWatchdogConfigV1 {
    lease_for: RuntimeGatewayOwnerLeaseDurationV1,
    policy: RuntimeGatewayOwnerRenewalPolicyV1,
    retry_delay: Duration,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuntimeGatewayOwnerCleanupBoundV1 {
    timeout: Duration,
    deadline_cap: Option<Instant>,
}

#[derive(Clone)]
struct RuntimeGatewayOwnerStartupCleanupCapV1 {
    deadline: Arc<Mutex<Option<Instant>>>,
}

pub(crate) struct RuntimeGatewayOwnerStartupWatchdogStartContextV1 {
    request_started_at: Instant,
    response_observed_at: Instant,
    initial_startup_cleanup_deadline: Option<Instant>,
}

impl RuntimeGatewayOwnerStartupWatchdogStartContextV1 {
    pub(crate) fn new(
        request_started_at: Instant,
        response_observed_at: Instant,
        initial_startup_cleanup_deadline: Option<Instant>,
    ) -> Self {
        Self {
            request_started_at,
            response_observed_at,
            initial_startup_cleanup_deadline,
        }
    }
}

impl RuntimeGatewayOwnerStartupCleanupCapV1 {
    fn new(deadline: Option<Instant>) -> Self {
        Self {
            deadline: Arc::new(Mutex::new(deadline)),
        }
    }

    fn clear(&self) {
        *self
            .deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn limit(&self, candidate: TokioInstant) -> TokioInstant {
        self.deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .map(TokioInstant::from_std)
            .map_or(candidate, |deadline| candidate.min(deadline))
    }
}

impl RuntimeGatewayOwnerCleanupBoundV1 {
    fn capped_at(mut self, deadline_cap: Instant) -> Self {
        self.deadline_cap = Some(deadline_cap);
        self
    }

    fn deadline(self) -> TokioInstant {
        let relative = TokioInstant::now() + self.timeout;
        self.deadline_cap
            .map(TokioInstant::from_std)
            .map_or(relative, |cap| relative.min(cap))
    }
}

impl RuntimeGatewayOwnerStartupWatchdogConfigV1 {
    pub fn new(
        lease_for: Duration,
        renew_before: Duration,
        safety_margin: Duration,
        retry_delay: Duration,
        cleanup: Duration,
    ) -> Result<Self, RuntimeGatewayOwnerStartupWatchdogConfigErrorV1> {
        let lease_for = RuntimeGatewayOwnerLeaseDurationV1::new(lease_for)
            .ok_or(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidLease)?;
        let policy = RuntimeGatewayOwnerRenewalPolicyV1::new(renew_before, safety_margin)
            .map_err(|_| RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidPolicy)?;
        let retry_window = renew_before
            .checked_sub(safety_margin)
            .ok_or(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidPolicy)?;
        if retry_delay.is_zero() || retry_delay >= retry_window {
            return Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidRetryDelay);
        }
        if renew_before >= lease_for.get() {
            return Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidLease);
        }
        if cleanup.is_zero() || cleanup > MAXIMUM_CLEANUP_TIMEOUT {
            return Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidCleanupTimeout);
        }
        Ok(Self {
            lease_for,
            policy,
            retry_delay,
            cleanup: RuntimeGatewayOwnerCleanupBoundV1 {
                timeout: cleanup,
                deadline_cap: None,
            },
        })
    }

    pub fn from_runtime_config(
        timing: GatewayOwnerTimingConfigV1,
    ) -> Result<Self, RuntimeGatewayOwnerStartupWatchdogConfigErrorV1> {
        Self::new(
            timing.lease_for(),
            timing.renew_before(),
            timing.safety_margin(),
            DEFAULT_RETRY_DELAY,
            DEFAULT_CLEANUP_TIMEOUT,
        )
    }

    pub(crate) fn lease_for(self) -> RuntimeGatewayOwnerLeaseDurationV1 {
        self.lease_for
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerStartupWatchdogConfigErrorV1 {
    #[error("runtime gateway owner startup watchdog lease is invalid")]
    InvalidLease,
    #[error("runtime gateway owner startup watchdog renewal policy is invalid")]
    InvalidPolicy,
    #[error("runtime gateway owner startup watchdog retry delay is invalid")]
    InvalidRetryDelay,
    #[error("runtime gateway owner startup watchdog cleanup timeout is invalid")]
    InvalidCleanupTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerStartupWatchdogStartErrorV1 {
    #[error("runtime gateway owner startup watchdog receipt schedule is invalid")]
    InvalidReceipt,
    #[error("runtime gateway owner startup watchdog requires an active runtime")]
    RuntimeUnavailable,
    #[error("runtime gateway owner startup watchdog is already started")]
    AlreadyStarted,
    #[error("runtime gateway owner startup watchdog safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner startup watchdog process identity mismatched")]
    ProcessMismatch,
    #[error("runtime gateway owner startup watchdog shard identity mismatched")]
    ShardMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerStartupWatchdogExitV1 {
    Shutdown,
    SafetyElapsed,
    OwnershipLost,
    RenewalUnknown,
    ReleaseUnconfirmed,
    ProtocolViolation,
    TaskStopped,
}

impl RuntimeGatewayOwnerStartupWatchdogExitV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Shutdown => "runtime_gateway_owner_shutdown",
            Self::SafetyElapsed => "runtime_gateway_owner_safety_elapsed",
            Self::OwnershipLost => "runtime_gateway_owner_lost",
            Self::RenewalUnknown => "runtime_gateway_owner_renewal_unknown",
            Self::ReleaseUnconfirmed => "runtime_gateway_owner_release_unconfirmed",
            Self::ProtocolViolation => "runtime_gateway_owner_protocol_violation",
            Self::TaskStopped => "runtime_gateway_owner_task_stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerReleaseStatusV1 {
    Confirmed,
    Unconfirmed,
    ProtocolViolation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerCurrentObservationV1 {
    receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    safety_deadline: Instant,
}

impl RuntimeGatewayOwnerCurrentObservationV1 {
    fn from_watchdog(watchdog: &RuntimeGatewayOwnerWatchdogV1) -> Self {
        Self {
            receipt: watchdog.schedule().receipt().clone(),
            safety_deadline: watchdog.schedule().safety_deadline(),
        }
    }

    pub fn receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.receipt
    }

    pub fn safety_deadline(&self) -> Instant {
        self.safety_deadline
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerCurrentObservationErrorV1 {
    #[error("runtime gateway owner observation can be retried")]
    Retryable,
    #[error("runtime gateway owner observation safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner observation found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner observation violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner observation supervisor is unavailable")]
    SupervisorUnavailable,
}

pub struct RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P> {
    reason: RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
    port: P,
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
}

impl<P> std::fmt::Debug for RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeGatewayOwnerStartupWatchdogStartFailureV1")
            .field("reason", &self.reason)
            .field("lease_id", &self.lease_id)
            .finish_non_exhaustive()
    }
}

impl<P> RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P> {
    pub(crate) fn new(
        reason: RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
        port: P,
        lease_id: RuntimeGatewayOwnerLeaseIdV1,
    ) -> Self {
        Self {
            reason,
            port,
            lease_id,
        }
    }

    pub fn reason(&self) -> RuntimeGatewayOwnerStartupWatchdogStartErrorV1 {
        self.reason
    }

    pub fn lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.lease_id
    }

    pub async fn cleanup(self, timeout: Duration) -> RuntimeGatewayOwnerReleaseStatusV1
    where
        P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
    {
        if timeout.is_zero() || timeout > MAXIMUM_CLEANUP_TIMEOUT {
            return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
        }
        let deadline = TokioInstant::now() + timeout;
        release_gateway_owner_v1(&self.port, self.lease_id, deadline).await
    }

    pub(crate) async fn cleanup_until(
        self,
        cleanup_deadline: Instant,
    ) -> RuntimeGatewayOwnerReleaseStatusV1
    where
        P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
    {
        release_runtime_gateway_owner_until_v1(&self.port, self.lease_id, cleanup_deadline).await
    }
}

struct RuntimeGatewayOwnerSupervisorHandleV1 {
    shutdown_commands: mpsc::Sender<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    supervisor_commands: mpsc::Sender<RuntimeGatewayOwnerSupervisorCommandV1>,
    closed_recovery_commands: mpsc::Sender<RuntimeGatewayOwnerClosedRecoveryCommandV2>,
    terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
    invalidation: Arc<RuntimeGatewayOwnerInvalidationLatchV1>,
    gateway_lifetime: Arc<AtomicBool>,
    task: Option<tokio::task::JoinHandle<()>>,
    startup_cleanup_cap: RuntimeGatewayOwnerStartupCleanupCapV1,
}

impl Drop for RuntimeGatewayOwnerSupervisorHandleV1 {
    fn drop(&mut self) {
        self.invalidation.invalidate();
    }
}

impl RuntimeGatewayOwnerSupervisorHandleV1 {
    fn clear_startup_cleanup_deadline(&self) {
        self.startup_cleanup_cap.clear();
    }

    fn is_bound_to_gateway_lifetime_v2(&self, expected: &Arc<AtomicBool>) -> bool {
        Arc::ptr_eq(&self.gateway_lifetime, expected)
    }

    fn terminal_status(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        *self.terminal.borrow()
    }

    async fn wait_terminal(&mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        loop {
            if let Some(exit) = *self.terminal.borrow() {
                return exit;
            }
            if self.terminal.changed().await.is_err() {
                return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
            }
        }
    }

    async fn observe_current_gateway_owner_v1(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCurrentObservationErrorV1>
    {
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        if self
            .supervisor_commands
            .send(RuntimeGatewayOwnerSupervisorCommandV1::Observe { response })
            .await
            .is_err()
        {
            return Err(wait_for_observation_terminal_v1(terminal).await);
        }
        match acknowledgement.await {
            Ok(result) => result,
            Err(_) => Err(wait_for_observation_terminal_v1(terminal).await),
        }
    }

    async fn promote_to_production_v1(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerProductionHandoffErrorV1>
    {
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        if self
            .supervisor_commands
            .send(RuntimeGatewayOwnerSupervisorCommandV1::Promote { response })
            .await
            .is_err()
        {
            return Err(wait_for_handoff_terminal_v1(terminal).await);
        }
        match acknowledgement.await {
            Ok(observation) => {
                match accept_production_handoff_observation_v1(observation, Instant::now()) {
                    Ok(observation) => Ok(observation),
                    Err(error) => {
                        self.invalidation.invalidate();
                        Err(error)
                    }
                }
            }
            Err(_) => Err(wait_for_handoff_terminal_v1(terminal).await),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    async fn prepare_closed_recovery_v2(
        &self,
    ) -> Result<
        RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
    > {
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        if self
            .closed_recovery_commands
            .send(RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare { response })
            .await
            .is_err()
        {
            return Err(wait_for_closed_recovery_prepare_terminal_v2(terminal).await);
        }
        match acknowledgement.await {
            Ok(Ok(observation)) => {
                match accept_closed_recovery_prepare_observation_v2(observation, Instant::now()) {
                    Ok(observation) => Ok(observation),
                    Err(error) => {
                        self.invalidation.invalidate();
                        Err(error)
                    }
                }
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err(wait_for_closed_recovery_prepare_terminal_v2(terminal).await),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    async fn commit_closed_recovery_v2(
        &self,
        expected_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    ) -> Result<
        RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerClosedRecoveryCommitErrorV2,
    > {
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        if self
            .closed_recovery_commands
            .send(RuntimeGatewayOwnerClosedRecoveryCommandV2::Commit {
                expected_receipt,
                response,
            })
            .await
            .is_err()
        {
            return Err(wait_for_closed_recovery_commit_terminal_v2(terminal).await);
        }
        match acknowledgement.await {
            Ok(result) => result,
            Err(_) => Err(wait_for_closed_recovery_commit_terminal_v2(terminal).await),
        }
    }

    async fn request_shutdown(
        &mut self,
        cleanup_deadline: Option<Instant>,
    ) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        let (response, acknowledgement) = oneshot::channel();
        if self
            .shutdown_commands
            .send(RuntimeGatewayOwnerStartupShutdownCommandV1 {
                response,
                cleanup_deadline,
            })
            .await
            .is_ok()
        {
            if let Ok(exit) = acknowledgement.await {
                return exit;
            }
        }
        self.wait_terminal().await
    }

    async fn shutdown(mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        let expected = self.request_shutdown(None).await;
        let task_completed = self.join_task().await;
        self.reconcile_shutdown(expected, task_completed)
    }

    async fn shutdown_until(
        mut self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        if Instant::now() >= cleanup_deadline {
            self.abort_and_join_task().await;
            return Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed);
        }
        let shutdown_cutoff = cleanup_deadline
            .checked_sub(STARTUP_TASK_ABORT_RESERVE)
            .unwrap_or(cleanup_deadline);
        let actor_cleanup_deadline = cleanup_deadline
            .checked_sub(STARTUP_ACTOR_TERMINATION_RESERVE)
            .unwrap_or(cleanup_deadline);
        let expected = {
            let shutdown = self.request_shutdown(Some(actor_cleanup_deadline));
            tokio::pin!(shutdown);
            tokio::select! {
                biased;
                _ = sleep_until(TokioInstant::from_std(shutdown_cutoff)) => None,
                exit = &mut shutdown => Some(exit),
            }
        };
        let Some(expected) = expected else {
            self.abort_and_join_task().await;
            return Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed);
        };
        let Some(task_completed) = self.join_task_until(cleanup_deadline).await else {
            self.abort_and_join_task().await;
            return Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed);
        };
        Ok(self.reconcile_shutdown(expected, task_completed))
    }

    async fn join_task(&mut self) -> bool {
        match self.task.take() {
            Some(task) => task.await.is_ok(),
            None => false,
        }
    }

    async fn join_task_until(&mut self, cleanup_deadline: Instant) -> Option<bool> {
        if Instant::now() >= cleanup_deadline {
            return None;
        }
        let mut task = self.task.take()?;
        match timeout_at(TokioInstant::from_std(cleanup_deadline), &mut task).await {
            Ok(result) => Some(result.is_ok()),
            Err(_) => {
                self.task = Some(task);
                None
            }
        }
    }

    async fn abort_and_join_task(&mut self) {
        let Some(task) = self.task.take() else {
            return;
        };
        task.abort();
        let _result = task.await;
    }

    fn reconcile_shutdown(
        &self,
        expected: RuntimeGatewayOwnerStartupWatchdogExitV1,
        task_completed: bool,
    ) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        if !task_completed {
            return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
        }
        match self.terminal_status() {
            Some(published) if published == expected => published,
            Some(_) => RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
            None => RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped,
        }
    }
}

fn accept_production_handoff_observation_v1(
    observation: RuntimeGatewayOwnerCurrentObservationV1,
    observed_at: Instant,
) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerProductionHandoffErrorV1> {
    if observation.safety_deadline() > observed_at {
        Ok(observation)
    } else {
        Err(RuntimeGatewayOwnerProductionHandoffErrorV1::SafetyElapsed)
    }
}

fn accept_closed_recovery_prepare_observation_v2(
    observation: RuntimeGatewayOwnerCurrentObservationV1,
    observed_at: Instant,
) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2>
{
    if observation.safety_deadline() > observed_at {
        Ok(observation)
    } else {
        Err(RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SafetyElapsed)
    }
}

fn accept_closed_recovery_commit_observation_v2(
    acknowledged: RuntimeGatewayOwnerCurrentObservationV1,
    prepared: &RuntimeGatewayOwnerCurrentObservationV1,
    expected_receipt: &RuntimeGatewayOwnerLeaseReceiptV1,
    observed_at: Instant,
) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerClosedRecoveryCommitErrorV2>
{
    if acknowledged.safety_deadline() <= observed_at {
        return Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SafetyElapsed);
    }
    if acknowledged != *prepared || acknowledged.receipt() != expected_receipt {
        return Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation);
    }
    Ok(acknowledged)
}

pub struct RuntimeGatewayOwnerStartupWatchdogHandleV1 {
    inner: Option<RuntimeGatewayOwnerSupervisorHandleV1>,
    prepared_closed_recovery_observation: Option<RuntimeGatewayOwnerCurrentObservationV1>,
}

impl RuntimeGatewayOwnerStartupWatchdogHandleV1 {
    pub fn terminal_status(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    pub async fn wait_terminal(&mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.inner_mut().wait_terminal().await
    }

    pub async fn observe_current_gateway_owner_v1(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCurrentObservationErrorV1>
    {
        self.inner().observe_current_gateway_owner_v1().await
    }

    pub async fn shutdown(mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.take_inner().shutdown().await
    }

    pub(crate) async fn shutdown_until(
        mut self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        self.take_inner().shutdown_until(cleanup_deadline).await
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production composition follows closed recovery handoff"
        )
    )]
    pub(crate) async fn into_production_v1(
        mut self,
        _proof: RuntimeGatewayOwnerProductionHandoffProofV1,
    ) -> Result<
        RuntimeGatewayOwnerProductionSupervisorV1,
        RuntimeGatewayOwnerProductionHandoffErrorV1,
    > {
        let handoff_observation = self.inner().promote_to_production_v1().await?;
        self.inner().clear_startup_cleanup_deadline();
        let inner = self.take_inner();
        Ok(RuntimeGatewayOwnerProductionSupervisorV1 {
            inner: Some(inner),
            handoff_observation,
        })
    }

    #[cfg(test)]
    pub(crate) async fn prepare_closed_recovery_v2(
        mut self,
    ) -> Result<
        RuntimeGatewayOwnerPreparedClosedRecoveryV2,
        RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
    > {
        self.prepare_closed_recovery_in_place_v2().await?;
        self.try_into_prepared_closed_recovery_v2()
            .map_err(|handle| {
                handle.inner().invalidation.invalidate();
                RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation
            })
    }

    pub(crate) async fn prepare_closed_recovery_in_place_v2(
        &mut self,
    ) -> Result<(), RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2> {
        if self.prepared_closed_recovery_observation.is_some() {
            self.prepared_closed_recovery_observation = None;
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation);
        }
        let observation = self.inner().prepare_closed_recovery_v2().await?;
        self.prepared_closed_recovery_observation = Some(observation);
        Ok(())
    }

    pub(crate) fn try_into_prepared_closed_recovery_v2(
        mut self,
    ) -> Result<RuntimeGatewayOwnerPreparedClosedRecoveryV2, Box<Self>> {
        let Some(observation) = self.prepared_closed_recovery_observation.take() else {
            return Err(Box::new(self));
        };
        let inner = self.take_inner();
        Ok(RuntimeGatewayOwnerPreparedClosedRecoveryV2 {
            inner: Some(inner),
            observation,
        })
    }

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_ref()
            .expect("gateway owner supervisor handle")
    }

    fn inner_mut(&mut self) -> &mut RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_mut()
            .expect("gateway owner supervisor handle")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner.take().expect("gateway owner supervisor handle")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1 {
    DeadlineElapsed,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeGatewayOwnerPreparedClosedRecoveryV2 {
    inner: Option<RuntimeGatewayOwnerSupervisorHandleV1>,
    observation: RuntimeGatewayOwnerCurrentObservationV1,
}

impl std::fmt::Debug for RuntimeGatewayOwnerPreparedClosedRecoveryV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerPreparedClosedRecoveryV2(<redacted>)")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeGatewayOwnerPreparedClosedRecoveryV2 {
    pub(crate) fn observation(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.observation
    }

    pub(crate) fn terminal_status(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    pub(crate) async fn wait_terminal(&mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.inner_mut().wait_terminal().await
    }

    pub(crate) fn is_bound_to_gateway_lifetime_v2(&self, expected: &Arc<AtomicBool>) -> bool {
        self.inner().is_bound_to_gateway_lifetime_v2(expected)
    }

    #[cfg(test)]
    pub(crate) async fn abort_and_shutdown_v2(
        mut self,
    ) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        let inner = self.take_inner();
        inner.invalidation.invalidate();
        inner.shutdown().await
    }

    pub(crate) async fn abort_and_shutdown_until_v2(
        mut self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        let inner = self.take_inner();
        inner.invalidation.invalidate();
        inner.shutdown_until(cleanup_deadline).await
    }

    pub(crate) async fn commit_closed_recovery_v2(
        mut self,
        permit: &RuntimeClosedDrainRecoveryPermitV2,
    ) -> Result<
        RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        RuntimeGatewayOwnerClosedRecoveryCommitErrorV2,
    > {
        if Instant::now() >= self.observation.safety_deadline() {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SafetyElapsed);
        }
        if permit.owner_receipt() != self.observation.receipt() {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::OwnerReceiptMismatch);
        }
        let acknowledged = self
            .inner()
            .commit_closed_recovery_v2(permit.owner_receipt().clone())
            .await?;
        let acknowledged = match accept_closed_recovery_commit_observation_v2(
            acknowledged,
            &self.observation,
            permit.owner_receipt(),
            Instant::now(),
        ) {
            Ok(acknowledged) => acknowledged,
            Err(error) => {
                self.inner().invalidation.invalidate();
                return Err(error);
            }
        };
        let inner = self.take_inner();
        Ok(RuntimeGatewayOwnerClosedRecoverySupervisorV2 {
            inner: Some(inner),
            observation: acknowledged,
        })
    }

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_ref()
            .expect("prepared gateway owner recovery handle")
    }

    fn inner_mut(&mut self) -> &mut RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_mut()
            .expect("prepared gateway owner recovery handle")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .take()
            .expect("prepared gateway owner recovery handle")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeGatewayOwnerClosedRecoverySupervisorV2 {
    inner: Option<RuntimeGatewayOwnerSupervisorHandleV1>,
    observation: RuntimeGatewayOwnerCurrentObservationV1,
}

impl std::fmt::Debug for RuntimeGatewayOwnerClosedRecoverySupervisorV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerClosedRecoverySupervisorV2(<redacted>)")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeGatewayOwnerClosedRecoverySupervisorV2 {
    pub(crate) fn observation(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.observation
    }

    pub(crate) fn is_bound_to_gateway_lifetime_v2(&self, expected: &Arc<AtomicBool>) -> bool {
        self.inner().is_bound_to_gateway_lifetime_v2(expected)
    }

    pub(crate) async fn shutdown(mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.take_inner().shutdown().await
    }

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_ref()
            .expect("closed gateway owner recovery handle")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .take()
            .expect("closed gateway owner recovery handle")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2 {
    #[error("runtime gateway owner closed recovery prepare safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner closed recovery prepare found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner closed recovery prepare observation is unavailable")]
    ObservationUnavailable,
    #[error("runtime gateway owner closed recovery prepare violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner closed recovery prepare supervisor is unavailable")]
    SupervisorUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeGatewayOwnerClosedRecoveryCommitErrorV2 {
    #[error("runtime gateway owner closed recovery commit safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner closed recovery commit owner receipt mismatched")]
    OwnerReceiptMismatch,
    #[error("runtime gateway owner closed recovery commit violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner closed recovery commit supervisor is unavailable")]
    SupervisorUnavailable,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "production composition follows closed recovery handoff"
    )
)]
pub(crate) struct RuntimeGatewayOwnerProductionHandoffProofV1 {
    _private: (),
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "production composition follows closed recovery handoff"
    )
)]
pub(crate) struct RuntimeGatewayOwnerProductionSupervisorV1 {
    inner: Option<RuntimeGatewayOwnerSupervisorHandleV1>,
    handoff_observation: RuntimeGatewayOwnerCurrentObservationV1,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "production composition follows closed recovery handoff"
    )
)]
impl RuntimeGatewayOwnerProductionSupervisorV1 {
    pub(crate) fn handoff_observation(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.handoff_observation
    }

    pub(crate) fn terminal_status(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    pub(crate) async fn wait_terminal(&mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.inner_mut().wait_terminal().await
    }

    pub(crate) async fn observe_current_gateway_owner_v1(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCurrentObservationErrorV1>
    {
        self.inner().observe_current_gateway_owner_v1().await
    }

    pub(crate) async fn shutdown(mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.take_inner().shutdown().await
    }

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_ref()
            .expect("gateway owner supervisor handle")
    }

    fn inner_mut(&mut self) -> &mut RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_mut()
            .expect("gateway owner supervisor handle")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner.take().expect("gateway owner supervisor handle")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeGatewayOwnerSupervisorRoleV1 {
    Startup,
    PreparedClosedRecovery,
    ClosedRecovery,
    Production,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayOwnerProductionHandoffErrorV1 {
    #[error("runtime gateway owner production handoff safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner production handoff found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner production handoff violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner production handoff supervisor is unavailable")]
    SupervisorUnavailable,
}

struct RuntimeGatewayOwnerStartupShutdownCommandV1 {
    response: oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>,
    cleanup_deadline: Option<Instant>,
}

enum RuntimeGatewayOwnerSupervisorCommandV1 {
    Observe {
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerCurrentObservationErrorV1,
            >,
        >,
    },
    Promote {
        response: oneshot::Sender<RuntimeGatewayOwnerCurrentObservationV1>,
    },
}

enum RuntimeGatewayOwnerClosedRecoveryCommandV2 {
    Prepare {
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
            >,
        >,
    },
    Commit {
        expected_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2,
            >,
        >,
    },
}

struct RuntimeGatewayOwnerStartupObservationCommandV1 {
    response: oneshot::Sender<
        Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerCurrentObservationErrorV1,
        >,
    >,
}

async fn wait_for_observation_terminal_v1(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerCurrentObservationErrorV1 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_observation_error_v1(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerCurrentObservationErrorV1::SupervisorUnavailable;
        }
    }
}

async fn wait_for_handoff_terminal_v1(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerProductionHandoffErrorV1 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_handoff_error_v1(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerProductionHandoffErrorV1::SupervisorUnavailable;
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
async fn wait_for_closed_recovery_prepare_terminal_v2(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_closed_recovery_prepare_error_v2(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SupervisorUnavailable;
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
async fn wait_for_closed_recovery_commit_terminal_v2(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerClosedRecoveryCommitErrorV2 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_closed_recovery_commit_error_v2(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SupervisorUnavailable;
        }
    }
}

async fn receive_supervisor_command_v1(
    pending: &mut Option<RuntimeGatewayOwnerSupervisorCommandV1>,
    commands: &mut mpsc::Receiver<RuntimeGatewayOwnerSupervisorCommandV1>,
) -> Option<RuntimeGatewayOwnerSupervisorCommandV1> {
    match pending.take() {
        Some(command) => Some(command),
        None => commands.recv().await,
    }
}

async fn receive_closed_recovery_command_v2(
    pending: &mut Option<RuntimeGatewayOwnerClosedRecoveryCommandV2>,
    commands: &mut mpsc::Receiver<RuntimeGatewayOwnerClosedRecoveryCommandV2>,
) -> Option<RuntimeGatewayOwnerClosedRecoveryCommandV2> {
    match pending.take() {
        Some(command) => Some(command),
        None => commands.recv().await,
    }
}

fn map_terminal_observation_error_v1(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerCurrentObservationErrorV1 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::SafetyElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::SupervisorUnavailable
        }
    }
}

fn map_terminal_handoff_error_v1(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerProductionHandoffErrorV1 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerProductionHandoffErrorV1::SafetyElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerProductionHandoffErrorV1::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerProductionHandoffErrorV1::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerProductionHandoffErrorV1::SupervisorUnavailable
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn map_terminal_closed_recovery_prepare_error_v2(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SafetyElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SupervisorUnavailable
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn map_terminal_closed_recovery_commit_error_v2(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerClosedRecoveryCommitErrorV2 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SafetyElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
        | RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SupervisorUnavailable
        }
    }
}

pub(crate) fn start_runtime_gateway_owner_startup_watchdog_v1<P, I>(
    port: P,
    invalidator: I,
    gateway_lifetime: Arc<AtomicBool>,
    accepted_receipt: RuntimeAcceptedGatewayOwnerReceiptV1,
    config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
    start_context: RuntimeGatewayOwnerStartupWatchdogStartContextV1,
) -> Result<
    RuntimeGatewayOwnerStartupWatchdogHandleV1,
    RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P>,
>
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
    P::Error: Send + 'static,
    I: RuntimeGatewayOwnerEmergencyInvalidatorV1,
{
    let RuntimeGatewayOwnerStartupWatchdogStartContextV1 {
        request_started_at,
        response_observed_at,
        initial_startup_cleanup_deadline,
    } = start_context;
    let lease_id = accepted_receipt.receipt().lease_id.clone();
    let invalidation = Arc::new(RuntimeGatewayOwnerInvalidationLatchV1::new(invalidator));
    let watchdog = match RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt,
        config.policy,
        request_started_at,
        response_observed_at,
    ) {
        Ok(watchdog) => watchdog,
        Err(error) => {
            invalidation.invalidate();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                map_schedule_start_error(error),
                port,
                lease_id,
            ));
        }
    };
    if matches!(
        watchdog.action_at(Instant::now()),
        RuntimeGatewayOwnerWatchdogActionV1::InvalidateNow
    ) {
        invalidation.invalidate();
        return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
            RuntimeGatewayOwnerStartupWatchdogStartErrorV1::SafetyElapsed,
            port,
            lease_id,
        ));
    }
    let runtime = match tokio::runtime::Handle::try_current() {
        Ok(runtime) => runtime,
        Err(_) => {
            invalidation.invalidate();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::RuntimeUnavailable,
                port,
                lease_id,
            ));
        }
    };
    let (shutdown_commands, shutdown_receiver) = mpsc::channel(SHUTDOWN_COMMAND_CAPACITY);
    let (supervisor_commands, supervisor_receiver) = mpsc::channel(SUPERVISOR_COMMAND_CAPACITY);
    let (closed_recovery_commands, closed_recovery_receiver) =
        mpsc::channel(CLOSED_RECOVERY_COMMAND_CAPACITY);
    let receivers = RuntimeGatewayOwnerStartupWatchdogReceiversV1 {
        shutdown_commands: shutdown_receiver,
        supervisor_commands: supervisor_receiver,
        closed_recovery_commands: closed_recovery_receiver,
    };
    let (terminal_sender, terminal) = watch::channel(None);
    let guard = RuntimeGatewayOwnerStartupWatchdogGuardV1::new(invalidation.clone());
    let startup_cleanup_cap =
        RuntimeGatewayOwnerStartupCleanupCapV1::new(initial_startup_cleanup_deadline);
    let actor_startup_cleanup_cap = startup_cleanup_cap.clone();
    let task = runtime.spawn(async move {
        let exit = run_gateway_owner_startup_watchdog_v1(
            port,
            watchdog,
            config,
            receivers,
            guard,
            actor_startup_cleanup_cap,
        )
        .await;
        let _result = terminal_sender.send(Some(exit));
    });
    Ok(RuntimeGatewayOwnerStartupWatchdogHandleV1 {
        inner: Some(RuntimeGatewayOwnerSupervisorHandleV1 {
            shutdown_commands,
            supervisor_commands,
            closed_recovery_commands,
            terminal,
            invalidation,
            gateway_lifetime,
            task: Some(task),
            startup_cleanup_cap,
        }),
        prepared_closed_recovery_observation: None,
    })
}

fn map_schedule_start_error(
    error: RuntimeGatewayOwnerRenewalScheduleErrorV1,
) -> RuntimeGatewayOwnerStartupWatchdogStartErrorV1 {
    match error {
        RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed => {
            RuntimeGatewayOwnerStartupWatchdogStartErrorV1::SafetyElapsed
        }
        RuntimeGatewayOwnerRenewalScheduleErrorV1::NonFreshReceipt
        | RuntimeGatewayOwnerRenewalScheduleErrorV1::LeaseTooShort
        | RuntimeGatewayOwnerRenewalScheduleErrorV1::ClockReversed
        | RuntimeGatewayOwnerRenewalScheduleErrorV1::InstantOverflow => {
            RuntimeGatewayOwnerStartupWatchdogStartErrorV1::InvalidReceipt
        }
    }
}

struct RuntimeGatewayOwnerStartupWatchdogReceiversV1 {
    shutdown_commands: mpsc::Receiver<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    supervisor_commands: mpsc::Receiver<RuntimeGatewayOwnerSupervisorCommandV1>,
    closed_recovery_commands: mpsc::Receiver<RuntimeGatewayOwnerClosedRecoveryCommandV2>,
}

async fn run_gateway_owner_startup_watchdog_v1<P>(
    port: P,
    watchdog: RuntimeGatewayOwnerWatchdogV1,
    config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
    receivers: RuntimeGatewayOwnerStartupWatchdogReceiversV1,
    mut guard: RuntimeGatewayOwnerStartupWatchdogGuardV1,
    startup_cleanup_cap: RuntimeGatewayOwnerStartupCleanupCapV1,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
    P::Error: Send + 'static,
{
    let RuntimeGatewayOwnerStartupWatchdogReceiversV1 {
        mut shutdown_commands,
        mut supervisor_commands,
        mut closed_recovery_commands,
    } = receivers;
    let lease_id = watchdog.schedule().receipt().lease_id.clone();
    let mut current = Some(watchdog);
    let mut role = RuntimeGatewayOwnerSupervisorRoleV1::Startup;
    let mut pending_supervisor_command = None;
    let mut pending_closed_recovery_command = None;
    let mut shutdown_acknowledgement = None;
    let stop = 'supervisor: loop {
        match shutdown_commands.try_recv() {
            Ok(command) => {
                break 'supervisor receive_shutdown(
                    Some(command),
                    &mut shutdown_acknowledgement,
                    config.cleanup,
                );
            }
            Err(TryRecvError::Disconnected) => {
                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                    config.cleanup,
                );
            }
            Err(TryRecvError::Empty) => {}
        }
        if pending_closed_recovery_command.is_none() {
            match closed_recovery_commands.try_recv() {
                Ok(command) => pending_closed_recovery_command = Some(command),
                Err(TryRecvError::Disconnected) => {
                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                        config.cleanup,
                    );
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        loop {
            if pending_supervisor_command.is_none() {
                match supervisor_commands.try_recv() {
                    Ok(command) => pending_supervisor_command = Some(command),
                    Err(TryRecvError::Disconnected) => {
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                            config.cleanup,
                        );
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }
            let canceled_observation = matches!(
                &pending_supervisor_command,
                Some(RuntimeGatewayOwnerSupervisorCommandV1::Observe { response })
                    if response.is_closed()
            );
            if !canceled_observation {
                break;
            }
            pending_supervisor_command = None;
        }
        if matches!(
            role,
            RuntimeGatewayOwnerSupervisorRoleV1::PreparedClosedRecovery
                | RuntimeGatewayOwnerSupervisorRoleV1::ClosedRecovery
        ) {
            let watchdog = current.take().expect("gateway owner watchdog state");
            let safety_deadline = watchdog.schedule().safety_deadline();
            tokio::select! {
                biased;
                command = shutdown_commands.recv() => {
                    break 'supervisor receive_shutdown(
                        command,
                        &mut shutdown_acknowledgement,
                        config.cleanup,
                    );
                }
                _ = sleep_until(TokioInstant::from_std(safety_deadline)) => {
                    guard.invalidate_now();
                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                        config.cleanup,
                    );
                }
                command = receive_closed_recovery_command_v2(
                    &mut pending_closed_recovery_command,
                    &mut closed_recovery_commands,
                ) => {
                    let Some(command) = command else {
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                            config.cleanup,
                        );
                    };
                    match (role, command) {
                        (
                            RuntimeGatewayOwnerSupervisorRoleV1::PreparedClosedRecovery,
                            RuntimeGatewayOwnerClosedRecoveryCommandV2::Commit {
                                expected_receipt,
                                response,
                            },
                        ) => {
                            if response.is_closed() {
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                    config.cleanup,
                                );
                            }
                            if Instant::now() >= safety_deadline {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SafetyElapsed,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                                    config.cleanup,
                                );
                            }
                            if watchdog.schedule().receipt() != &expected_receipt {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::OwnerReceiptMismatch,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            let observation =
                                RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&watchdog);
                            role = RuntimeGatewayOwnerSupervisorRoleV1::ClosedRecovery;
                            current = Some(watchdog);
                            if response.send(Ok(observation)).is_err() {
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                    config.cleanup,
                                );
                            }
                        }
                        (_, command) => {
                            reject_frozen_closed_recovery_command_v2(command);
                            guard.invalidate_now();
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                config.cleanup,
                            );
                        }
                    }
                    continue 'supervisor;
                }
                command = receive_supervisor_command_v1(
                    &mut pending_supervisor_command,
                    &mut supervisor_commands,
                ) => {
                    let Some(command) = command else {
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                            config.cleanup,
                        );
                    };
                    reject_frozen_supervisor_command_v2(command);
                    guard.invalidate_now();
                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                        config.cleanup,
                    );
                }
            }
        }
        if matches!(
            &pending_closed_recovery_command,
            Some(RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare { .. })
        ) {
            let Some(RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare { response }) =
                pending_closed_recovery_command.take()
            else {
                unreachable!("matched closed recovery prepare command")
            };
            if role != RuntimeGatewayOwnerSupervisorRoleV1::Startup {
                let _result = response.send(Err(
                    RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation,
                ));
                guard.invalidate_now();
                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                    config.cleanup,
                );
            }
            let watchdog = current.take().expect("gateway owner watchdog state");
            match prepare_closed_recovery_owner_v2(
                &port,
                watchdog,
                response,
                &mut shutdown_commands,
                &mut shutdown_acknowledgement,
                &mut guard,
                config.cleanup,
            )
            .await
            {
                RuntimeGatewayOwnerClosedRecoveryPrepareStepV2::Prepared {
                    successor,
                    observation,
                    response,
                } => {
                    role = RuntimeGatewayOwnerSupervisorRoleV1::PreparedClosedRecovery;
                    current = Some(*successor);
                    if response.send(Ok(observation)).is_err() {
                        guard.invalidate_now();
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                            config.cleanup,
                        );
                    }
                }
                RuntimeGatewayOwnerClosedRecoveryPrepareStepV2::Stop(stop) => {
                    break 'supervisor stop;
                }
            }
            continue 'supervisor;
        }
        if let Some(RuntimeGatewayOwnerClosedRecoveryCommandV2::Commit { response, .. }) =
            pending_closed_recovery_command.take()
        {
            let _result = response.send(Err(
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation,
            ));
            guard.invalidate_now();
            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                config.cleanup,
            );
        }
        let watchdog = current.take().expect("gateway owner watchdog state");
        match watchdog.action_at(Instant::now()) {
            RuntimeGatewayOwnerWatchdogActionV1::WaitUntil(renew_at) => {
                tokio::select! {
                    biased;
                    command = shutdown_commands.recv() => {
                        break 'supervisor receive_shutdown(
                            command,
                            &mut shutdown_acknowledgement,
                            config.cleanup,
                        );
                    }
                    command = receive_closed_recovery_command_v2(
                        &mut pending_closed_recovery_command,
                        &mut closed_recovery_commands,
                    ) => {
                        let Some(command) = command else {
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                config.cleanup,
                            );
                        };
                        pending_closed_recovery_command = Some(command);
                        current = Some(watchdog);
                        continue 'supervisor;
                    }
                    _ = sleep_until(TokioInstant::from_std(renew_at)) => {
                        current = Some(watchdog);
                    }
                    command = receive_supervisor_command_v1(
                        &mut pending_supervisor_command,
                        &mut supervisor_commands,
                    ) => {
                        let Some(command) = command else {
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                config.cleanup,
                            );
                        };
                        match command {
                            RuntimeGatewayOwnerSupervisorCommandV1::Observe { response } => {
                                if response.is_closed() {
                                    current = Some(watchdog);
                                    continue 'supervisor;
                                }
                                let command = RuntimeGatewayOwnerStartupObservationCommandV1 {
                                    response,
                                };
                                match observe_current_gateway_owner_v1(
                                    &port,
                                    watchdog,
                                    command,
                                    &mut shutdown_commands,
                                    &mut shutdown_acknowledgement,
                                    &mut guard,
                                    config.cleanup,
                                ).await {
                                    RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                                        successor,
                                        response,
                                        result,
                                    } => {
                                        current = Some(*successor);
                                        let _result = response.send(result);
                                    }
                                    RuntimeGatewayOwnerStartupObservationStepV1::Stop(stop) => {
                                        break 'supervisor stop;
                                    }
                                }
                            }
                            RuntimeGatewayOwnerSupervisorCommandV1::Promote { response } => {
                                match shutdown_commands.try_recv() {
                                    Ok(command) => {
                                        break 'supervisor receive_shutdown(
                                            Some(command),
                                            &mut shutdown_acknowledgement,
                                            config.cleanup,
                                        );
                                    }
                                    Err(TryRecvError::Disconnected) => {
                                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                            RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                            config.cleanup,
                                        );
                                    }
                                    Err(TryRecvError::Empty) => {}
                                }
                                if response.is_closed() {
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                        config.cleanup,
                                    );
                                }
                                if role != RuntimeGatewayOwnerSupervisorRoleV1::Startup {
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                        config.cleanup,
                                    );
                                }
                                match watchdog.action_at(Instant::now()) {
                                    RuntimeGatewayOwnerWatchdogActionV1::WaitUntil(_) => {}
                                    RuntimeGatewayOwnerWatchdogActionV1::RenewNow => {
                                        current = Some(watchdog);
                                        pending_supervisor_command = Some(
                                            RuntimeGatewayOwnerSupervisorCommandV1::Promote {
                                                response,
                                            },
                                        );
                                        continue 'supervisor;
                                    }
                                    RuntimeGatewayOwnerWatchdogActionV1::InvalidateNow => {
                                        guard.invalidate_now();
                                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                            RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                                            config.cleanup,
                                        );
                                    }
                                }
                                let projection =
                                    RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&watchdog);
                                if response.send(projection).is_err() {
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                        config.cleanup,
                                    );
                                }
                                role = RuntimeGatewayOwnerSupervisorRoleV1::Production;
                                current = Some(watchdog);
                            }
                        }
                    }
                }
            }
            RuntimeGatewayOwnerWatchdogActionV1::RenewNow => {
                let request_started_at = Instant::now();
                let inflight = match watchdog.begin_renewal(config.lease_for, request_started_at) {
                    Ok(inflight) => inflight,
                    Err(error) => {
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            map_watchdog_error(error),
                            config.cleanup,
                        );
                    }
                };
                let safety_deadline = inflight.previous_schedule().safety_deadline();
                let request = inflight.request().clone();
                let renewal = port.renew_gateway_owner(request);
                tokio::pin!(renewal);
                let result = tokio::select! {
                    biased;
                    command = shutdown_commands.recv() => {
                        guard.invalidate_now();
                        let stop = receive_shutdown(
                            command,
                            &mut shutdown_acknowledgement,
                            config.cleanup,
                        );
                        let cleanup_deadline =
                            startup_cleanup_cap.limit(stop.cleanup_deadline);
                        let _joined_result = timeout_at(cleanup_deadline, &mut renewal).await;
                        break 'supervisor stop;
                    }
                    _ = sleep_until(TokioInstant::from_std(safety_deadline)) => {
                        guard.invalidate_now();
                        let stop = RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                            config.cleanup,
                        );
                        let cleanup_deadline =
                            startup_cleanup_cap.limit(stop.cleanup_deadline);
                        let _joined_result = timeout_at(cleanup_deadline, &mut renewal).await;
                        break 'supervisor stop;
                    }
                    result = &mut renewal => result,
                };
                let response_observed_at = Instant::now();
                match result {
                    Ok(outcome) => match inflight.complete(outcome, response_observed_at) {
                        Ok(RuntimeGatewayOwnerRenewalCompletionV1::Renewed(successor)) => {
                            current = Some(successor);
                        }
                        Ok(RuntimeGatewayOwnerRenewalCompletionV1::OwnershipLost(_)) => {
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                                config.cleanup,
                            );
                        }
                        Err(error) => {
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                map_watchdog_error(error),
                                config.cleanup,
                            );
                        }
                    },
                    Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied { .. }) => {
                        let restored = match inflight.definitely_not_applied(response_observed_at) {
                            Ok(restored) => restored,
                            Err(error) => {
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    map_watchdog_error(error),
                                    config.cleanup,
                                );
                            }
                        };
                        let retry_at = Instant::now()
                            .checked_add(config.retry_delay)
                            .map(|candidate| candidate.min(restored.schedule().safety_deadline()))
                            .unwrap_or(restored.schedule().safety_deadline());
                        tokio::select! {
                            biased;
                            command = shutdown_commands.recv() => {
                                break 'supervisor receive_shutdown(
                                    command,
                                    &mut shutdown_acknowledgement,
                                    config.cleanup,
                                );
                            }
                            command = closed_recovery_commands.recv() => {
                                let Some(command) = command else {
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                        config.cleanup,
                                    );
                                };
                                pending_closed_recovery_command = Some(command);
                                current = Some(restored);
                            }
                            command = supervisor_commands.recv() => {
                                let Some(command) = command else {
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                        config.cleanup,
                                    );
                                };
                                pending_supervisor_command = Some(command);
                                current = Some(restored);
                            }
                            _ = sleep_until(TokioInstant::from_std(retry_at)) => {
                                current = Some(restored);
                            }
                        }
                    }
                    Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown { .. }) => {
                        let _unknown = inflight.into_unknown();
                        guard.invalidate_now();
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown,
                            config.cleanup,
                        );
                    }
                }
            }
            RuntimeGatewayOwnerWatchdogActionV1::InvalidateNow => {
                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                    config.cleanup,
                );
            }
        }
    };
    guard.invalidate_now();
    let cleanup_deadline = startup_cleanup_cap.limit(stop.cleanup_deadline);
    let gateway_stopped =
        wait_for_emergency_gateway_shutdown_v1(guard.gateway_shutdown_watch(), cleanup_deadline)
            .await;
    let release = if gateway_stopped {
        release_gateway_owner_v1(&port, lease_id, cleanup_deadline).await
    } else {
        RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed
    };
    let exit = finalize_gateway_owner_exit_v1(stop.exit, release);
    guard.disarm();
    if let Some(response) = shutdown_acknowledgement {
        let _result = response.send(exit);
    }
    exit
}

fn reject_frozen_supervisor_command_v2(command: RuntimeGatewayOwnerSupervisorCommandV1) {
    match command {
        RuntimeGatewayOwnerSupervisorCommandV1::Observe { response } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation,
            ));
        }
        RuntimeGatewayOwnerSupervisorCommandV1::Promote { response } => {
            drop(response);
        }
    }
}

fn reject_frozen_closed_recovery_command_v2(command: RuntimeGatewayOwnerClosedRecoveryCommandV2) {
    match command {
        RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare { response } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation,
            ));
        }
        RuntimeGatewayOwnerClosedRecoveryCommandV2::Commit { response, .. } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation,
            ));
        }
    }
}

enum RuntimeGatewayOwnerClosedRecoveryPrepareStepV2 {
    Prepared {
        successor: Box<RuntimeGatewayOwnerWatchdogV1>,
        observation: RuntimeGatewayOwnerCurrentObservationV1,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
            >,
        >,
    },
    Stop(RuntimeGatewayOwnerStartupWatchdogStopV1),
}

async fn prepare_closed_recovery_owner_v2<P>(
    port: &P,
    watchdog: RuntimeGatewayOwnerWatchdogV1,
    mut response: oneshot::Sender<
        Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
        >,
    >,
    shutdown_commands: &mut mpsc::Receiver<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    shutdown_acknowledgement: &mut Option<
        oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>,
    >,
    guard: &mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
) -> RuntimeGatewayOwnerClosedRecoveryPrepareStepV2
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
    P::Error: Send,
{
    let request_started_at = Instant::now();
    let inflight = match watchdog.begin_current_observation(request_started_at) {
        Ok(inflight) => inflight,
        Err(error) => {
            return stop_after_closed_recovery_prepare_error_v2(
                response,
                map_closed_recovery_prepare_watchdog_error_v2(error),
                map_watchdog_error(error),
                guard,
                cleanup,
            );
        }
    };
    let safety_deadline = inflight.previous_schedule().safety_deadline();
    let request = inflight.request().clone();
    let observation = port.observe_gateway_owner(request);
    tokio::pin!(observation);
    let result = tokio::select! {
        biased;
        shutdown = shutdown_commands.recv() => {
            guard.invalidate_now();
            let stop = receive_shutdown(
                shutdown,
                shutdown_acknowledgement,
                cleanup,
            );
            let _result = response.send(Err(
                RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SupervisorUnavailable,
            ));
            return RuntimeGatewayOwnerClosedRecoveryPrepareStepV2::Stop(stop);
        }
        _ = response.closed() => {
            guard.invalidate_now();
            return RuntimeGatewayOwnerClosedRecoveryPrepareStepV2::Stop(
                RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                    cleanup,
                ),
            );
        }
        _ = sleep_until(TokioInstant::from_std(safety_deadline)) => {
            return stop_after_closed_recovery_prepare_error_v2(
                response,
                RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SafetyElapsed,
                RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                guard,
                cleanup,
            );
        }
        result = &mut observation => result,
    };
    let response_observed_at = Instant::now();
    match result {
        Ok(observation) => match inflight.complete(observation, response_observed_at) {
            Ok(RuntimeGatewayOwnerObservationCompletionV1::Current(successor)) => {
                let observation =
                    RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&successor);
                RuntimeGatewayOwnerClosedRecoveryPrepareStepV2::Prepared {
                    successor,
                    observation,
                    response,
                }
            }
            Ok(RuntimeGatewayOwnerObservationCompletionV1::OwnershipLost(_)) => {
                stop_after_closed_recovery_prepare_error_v2(
                    response,
                    RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup,
                )
            }
            Err(error) => stop_after_closed_recovery_prepare_error_v2(
                response,
                map_closed_recovery_prepare_watchdog_error_v2(error),
                map_watchdog_error(error),
                guard,
                cleanup,
            ),
        },
        Err(error) => match P::classify_observation_error(&error) {
            RuntimeGatewayOwnerObservationErrorClassV1::Retryable => {
                stop_after_closed_recovery_prepare_error_v2(
                    response,
                    RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ObservationUnavailable,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                    guard,
                    cleanup,
                )
            }
            RuntimeGatewayOwnerObservationErrorClassV1::OwnershipLost => {
                stop_after_closed_recovery_prepare_error_v2(
                    response,
                    RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup,
                )
            }
            RuntimeGatewayOwnerObservationErrorClassV1::ProtocolViolation => {
                stop_after_closed_recovery_prepare_error_v2(
                    response,
                    RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                    guard,
                    cleanup,
                )
            }
        },
    }
}

fn stop_after_closed_recovery_prepare_error_v2(
    response: oneshot::Sender<
        Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
        >,
    >,
    response_error: RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    guard: &mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
) -> RuntimeGatewayOwnerClosedRecoveryPrepareStepV2 {
    guard.invalidate_now();
    let _result = response.send(Err(response_error));
    RuntimeGatewayOwnerClosedRecoveryPrepareStepV2::Stop(
        RuntimeGatewayOwnerStartupWatchdogStopV1::new(exit, cleanup),
    )
}

fn map_closed_recovery_prepare_watchdog_error_v2(
    error: RuntimeGatewayOwnerWatchdogErrorV1,
) -> RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2 {
    match error {
        RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed,
        ) => RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SafetyElapsed,
        RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed
        | RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort
        | RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted
        | RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { .. }
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(_) => {
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation
        }
    }
}

enum RuntimeGatewayOwnerStartupObservationStepV1 {
    Continue {
        successor: Box<RuntimeGatewayOwnerWatchdogV1>,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerCurrentObservationErrorV1,
            >,
        >,
        result: Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerCurrentObservationErrorV1,
        >,
    },
    Stop(RuntimeGatewayOwnerStartupWatchdogStopV1),
}

async fn observe_current_gateway_owner_v1<P>(
    port: &P,
    watchdog: RuntimeGatewayOwnerWatchdogV1,
    command: RuntimeGatewayOwnerStartupObservationCommandV1,
    shutdown_commands: &mut mpsc::Receiver<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    shutdown_acknowledgement: &mut Option<
        oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>,
    >,
    guard: &mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
) -> RuntimeGatewayOwnerStartupObservationStepV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
    P::Error: Send,
{
    let request_started_at = Instant::now();
    let inflight = match watchdog.begin_current_observation(request_started_at) {
        Ok(inflight) => inflight,
        Err(error) => {
            return stop_after_observation_error_v1(
                command,
                map_current_observation_error(error),
                map_watchdog_error(error),
                guard,
                cleanup,
            );
        }
    };
    let renew_at = inflight.previous_schedule().renew_at();
    let safety_deadline = inflight.previous_schedule().safety_deadline();
    let request = inflight.request().clone();
    let observation = port.observe_gateway_owner(request);
    tokio::pin!(observation);
    let result = tokio::select! {
        biased;
        shutdown = shutdown_commands.recv() => {
            guard.invalidate_now();
            let stop = receive_shutdown(
                shutdown,
                shutdown_acknowledgement,
                cleanup,
            );
            let _result = command.response.send(Err(
                RuntimeGatewayOwnerCurrentObservationErrorV1::SupervisorUnavailable,
            ));
            return RuntimeGatewayOwnerStartupObservationStepV1::Stop(stop);
        }
        _ = sleep_until(TokioInstant::from_std(renew_at)) => {
            let response_observed_at = Instant::now();
            return match inflight.observation_failed(response_observed_at) {
                Ok(restored) => RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                    successor: Box::new(restored),
                    response: command.response,
                    result: Err(RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable),
                },
                Err(error) => stop_after_observation_error_v1(
                    command,
                    map_current_observation_error(error),
                    map_watchdog_error(error),
                    guard,
                    cleanup,
                ),
            };
        }
        _ = sleep_until(TokioInstant::from_std(safety_deadline)) => {
            guard.invalidate_now();
            let stop = RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                cleanup,
            );
            let _result = command.response.send(Err(
                RuntimeGatewayOwnerCurrentObservationErrorV1::SafetyElapsed,
            ));
            return RuntimeGatewayOwnerStartupObservationStepV1::Stop(stop);
        }
        result = &mut observation => result,
    };
    let response_observed_at = Instant::now();
    match result {
        Ok(observation) => match inflight.complete(observation, response_observed_at) {
            Ok(RuntimeGatewayOwnerObservationCompletionV1::Current(successor)) => {
                let projection = RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&successor);
                RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                    successor,
                    response: command.response,
                    result: Ok(projection),
                }
            }
            Ok(RuntimeGatewayOwnerObservationCompletionV1::OwnershipLost(_)) => {
                stop_after_observation_error_v1(
                    command,
                    RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup,
                )
            }
            Err(error) => stop_after_observation_error_v1(
                command,
                map_current_observation_error(error),
                map_watchdog_error(error),
                guard,
                cleanup,
            ),
        },
        Err(error) => match P::classify_observation_error(&error) {
            RuntimeGatewayOwnerObservationErrorClassV1::Retryable => {
                match inflight.observation_failed(response_observed_at) {
                    Ok(restored) => RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                        successor: Box::new(restored),
                        response: command.response,
                        result: Err(RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable),
                    },
                    Err(error) => stop_after_observation_error_v1(
                        command,
                        map_current_observation_error(error),
                        map_watchdog_error(error),
                        guard,
                        cleanup,
                    ),
                }
            }
            RuntimeGatewayOwnerObservationErrorClassV1::OwnershipLost => {
                stop_after_observation_error_v1(
                    command,
                    RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup,
                )
            }
            RuntimeGatewayOwnerObservationErrorClassV1::ProtocolViolation => {
                stop_after_observation_error_v1(
                    command,
                    RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                    guard,
                    cleanup,
                )
            }
        },
    }
}

fn stop_after_observation_error_v1(
    command: RuntimeGatewayOwnerStartupObservationCommandV1,
    response_error: RuntimeGatewayOwnerCurrentObservationErrorV1,
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    guard: &mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
) -> RuntimeGatewayOwnerStartupObservationStepV1 {
    guard.invalidate_now();
    let _result = command.response.send(Err(response_error));
    RuntimeGatewayOwnerStartupObservationStepV1::Stop(
        RuntimeGatewayOwnerStartupWatchdogStopV1::new(exit, cleanup),
    )
}

struct RuntimeGatewayOwnerStartupWatchdogStopV1 {
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    cleanup_deadline: TokioInstant,
}

impl RuntimeGatewayOwnerStartupWatchdogStopV1 {
    fn new(
        exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
        cleanup: RuntimeGatewayOwnerCleanupBoundV1,
    ) -> Self {
        Self {
            exit,
            cleanup_deadline: cleanup.deadline(),
        }
    }
}

fn receive_shutdown(
    command: Option<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    acknowledgement: &mut Option<oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
) -> RuntimeGatewayOwnerStartupWatchdogStopV1 {
    let mut cleanup = cleanup;
    if let Some(RuntimeGatewayOwnerStartupShutdownCommandV1 {
        response,
        cleanup_deadline,
    }) = command
    {
        *acknowledgement = Some(response);
        if let Some(cleanup_deadline) = cleanup_deadline {
            cleanup = cleanup.capped_at(cleanup_deadline);
        }
    }
    RuntimeGatewayOwnerStartupWatchdogStopV1::new(
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
        cleanup,
    )
}

fn map_current_observation_error(
    error: RuntimeGatewayOwnerWatchdogErrorV1,
) -> RuntimeGatewayOwnerCurrentObservationErrorV1 {
    match error {
        RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed,
        ) => RuntimeGatewayOwnerCurrentObservationErrorV1::SafetyElapsed,
        RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed
        | RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort
        | RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted
        | RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { .. }
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(_) => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation
        }
    }
}

fn map_watchdog_error(
    error: RuntimeGatewayOwnerWatchdogErrorV1,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
    match error {
        RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed,
        ) => RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
        RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed
        | RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort
        | RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted
        | RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { .. }
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(_) => {
            RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
        }
    }
}

fn finalize_gateway_owner_exit_v1(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    release: RuntimeGatewayOwnerReleaseStatusV1,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
    match release {
        RuntimeGatewayOwnerReleaseStatusV1::Confirmed => exit,
        RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed => {
            RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        }
        RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation => {
            RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
        }
    }
}

async fn release_gateway_owner_v1<P>(
    port: &P,
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
    cleanup_deadline: TokioInstant,
) -> RuntimeGatewayOwnerReleaseStatusV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    let request = RuntimeReleaseGatewayOwnerLeaseV1 { lease_id };
    for _attempt in 0..RELEASE_ATTEMPTS {
        if TokioInstant::now() >= cleanup_deadline {
            return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
        }
        let release = match timeout_at(
            cleanup_deadline,
            port.release_gateway_owner(request.clone()),
        )
        .await
        {
            Ok(release) => release,
            Err(_) => return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed,
        };
        if TokioInstant::now() >= cleanup_deadline {
            return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
        }
        match release {
            Ok(outcome) => match accept_gateway_owner_release_v1(&request, outcome) {
                Ok(RuntimeAcceptedGatewayOwnerReleaseV1::Released)
                | Ok(RuntimeAcceptedGatewayOwnerReleaseV1::NotHeld(_)) => {
                    return RuntimeGatewayOwnerReleaseStatusV1::Confirmed;
                }
                Err(_) => return RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation,
            },
            Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied { .. }) => {}
            Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown { .. }) => {
                let observation = match timeout_at(
                    cleanup_deadline,
                    port.observe_gateway_owner(
                        automation_runtime_controller::RuntimeObserveGatewayOwnerLeaseV1 {
                            gateway_shard_id: request.lease_id.gateway_shard_id.clone(),
                        },
                    ),
                )
                .await
                {
                    Ok(Ok(observation)) => observation,
                    Ok(Err(_)) | Err(_) => {
                        return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
                    }
                };
                if TokioInstant::now() >= cleanup_deadline {
                    return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
                }
                match classify_unknown_gateway_owner_release_v1(&request, observation) {
                    RuntimeGatewayOwnerReleaseRecoveryV1::ReplaySameRequest => {}
                    RuntimeGatewayOwnerReleaseRecoveryV1::CompleteWithoutOwnership(_) => {
                        return RuntimeGatewayOwnerReleaseStatusV1::Confirmed;
                    }
                    RuntimeGatewayOwnerReleaseRecoveryV1::ProtocolViolation => {
                        return RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation;
                    }
                }
            }
        }
    }
    RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed
}

pub(crate) async fn release_runtime_gateway_owner_until_v1<P>(
    port: &P,
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
    cleanup_deadline: Instant,
) -> RuntimeGatewayOwnerReleaseStatusV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    if Instant::now() >= cleanup_deadline {
        return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
    }
    release_gateway_owner_v1(port, lease_id, TokioInstant::from_std(cleanup_deadline)).await
}

struct RuntimeGatewayOwnerInvalidationLatchV1 {
    invalidator: Arc<dyn RuntimeGatewayOwnerEmergencyInvalidatorV1>,
    invalidated: AtomicBool,
}

impl RuntimeGatewayOwnerInvalidationLatchV1 {
    fn new(invalidator: impl RuntimeGatewayOwnerEmergencyInvalidatorV1) -> Self {
        Self {
            invalidator: Arc::new(invalidator),
            invalidated: AtomicBool::new(false),
        }
    }

    fn invalidate(&self) {
        if !self.invalidated.swap(true, Ordering::AcqRel) {
            self.invalidator.invalidate_gateway_ownership();
        }
    }

    fn gateway_shutdown_watch(&self) -> Option<watch::Receiver<bool>> {
        self.invalidator.gateway_shutdown_watch()
    }
}

struct RuntimeGatewayOwnerStartupWatchdogGuardV1 {
    invalidation: Arc<RuntimeGatewayOwnerInvalidationLatchV1>,
    armed: bool,
}

impl RuntimeGatewayOwnerStartupWatchdogGuardV1 {
    fn new(invalidation: Arc<RuntimeGatewayOwnerInvalidationLatchV1>) -> Self {
        Self {
            invalidation,
            armed: true,
        }
    }

    fn invalidate_now(&mut self) {
        self.invalidation.invalidate();
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn gateway_shutdown_watch(&self) -> Option<watch::Receiver<bool>> {
        self.invalidation.gateway_shutdown_watch()
    }
}

impl Drop for RuntimeGatewayOwnerStartupWatchdogGuardV1 {
    fn drop(&mut self) {
        if self.armed {
            self.invalidation.invalidate();
        }
    }
}

async fn wait_for_emergency_gateway_shutdown_v1(
    mut stopped: Option<watch::Receiver<bool>>,
    cleanup_deadline: TokioInstant,
) -> bool {
    let Some(stopped) = stopped.as_mut() else {
        return true;
    };
    loop {
        if *stopped.borrow() {
            return true;
        }
        if TokioInstant::now() >= cleanup_deadline {
            return false;
        }
        tokio::select! {
            biased;
            _ = sleep_until(cleanup_deadline) => return false,
            changed = stopped.changed() => {
                if changed.is_err() {
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
mod emergency_gateway_shutdown_tests {
    use std::time::Duration;

    use tokio::sync::watch;
    use tokio::time::Instant;

    use super::wait_for_emergency_gateway_shutdown_v1;

    #[tokio::test]
    async fn unattached_or_already_stopped_gateway_never_blocks_owner_cleanup() {
        assert!(
            wait_for_emergency_gateway_shutdown_v1(None, Instant::now() + Duration::from_secs(1))
                .await
        );
        let (_sender, receiver) = watch::channel(true);
        assert!(
            wait_for_emergency_gateway_shutdown_v1(
                Some(receiver),
                Instant::now() + Duration::from_secs(1)
            )
            .await
        );
    }

    #[tokio::test]
    async fn attached_gateway_must_publish_stopped_before_owner_cleanup_continues() {
        let (sender, receiver) = watch::channel(false);
        let waiter =
            tokio::runtime::Handle::current().spawn(wait_for_emergency_gateway_shutdown_v1(
                Some(receiver),
                Instant::now() + Duration::from_secs(1),
            ));
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        sender.send(true).unwrap();
        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn closed_or_expired_gateway_stop_evidence_never_authorizes_release() {
        let (sender, receiver) = watch::channel(false);
        drop(sender);
        assert!(
            !wait_for_emergency_gateway_shutdown_v1(
                Some(receiver),
                Instant::now() + Duration::from_secs(1)
            )
            .await
        );
        let (_sender, receiver) = watch::channel(false);
        assert!(!wait_for_emergency_gateway_shutdown_v1(Some(receiver), Instant::now()).await);
    }
}

#[cfg(test)]
#[path = "gateway_owner_startup_watchdog_handoff_tests.rs"]
mod handoff_tests;
