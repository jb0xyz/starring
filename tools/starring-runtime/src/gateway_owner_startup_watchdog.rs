use std::io::Write;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

use crate::closed_recovery::RuntimeGatewayOwnerAdmissionFrozenAuthorityV2;
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

    fn limit(&self, candidate: TokioInstant) -> TokioInstant {
        self.deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .map(TokioInstant::from_std)
            .map_or(candidate, |deadline| candidate.min(deadline))
    }

    fn clear(&self) {
        *self
            .deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
    current_observation: watch::Receiver<RuntimeGatewayOwnerCurrentObservationV1>,
    invalidation: Arc<RuntimeGatewayOwnerInvalidationLatchV1>,
    gateway_lifetime: Arc<AtomicBool>,
    process_generation: Arc<AtomicU64>,
    production_generation: Arc<AtomicU64>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for RuntimeGatewayOwnerSupervisorHandleV1 {
    fn drop(&mut self) {
        self.invalidation.invalidate();
    }
}

impl RuntimeGatewayOwnerSupervisorHandleV1 {
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

    fn current_observation_watch_v2(
        &self,
    ) -> watch::Receiver<RuntimeGatewayOwnerCurrentObservationV1> {
        self.current_observation.clone()
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

    async fn enter_admission_frozen_v2(
        &self,
        authority: RuntimeGatewayOwnerAdmissionFrozenAuthorityV2,
    ) -> Result<
        RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2,
    > {
        if Instant::now() >= authority.cutoff_v2() {
            self.invalidation.invalidate();
            return Err(RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::DeadlineElapsed);
        }
        let cutoff = authority.cutoff_v2();
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        if self
            .supervisor_commands
            .send(
                RuntimeGatewayOwnerSupervisorCommandV1::EnterAdmissionFrozen {
                    authority,
                    response,
                },
            )
            .await
            .is_err()
        {
            return Err(wait_for_admission_frozen_handoff_terminal_v2(terminal).await);
        }
        match acknowledgement.await {
            Ok(Ok(observation))
                if Instant::now() < cutoff && observation.safety_deadline() > Instant::now() =>
            {
                Ok(observation)
            }
            Ok(Ok(_)) => {
                self.invalidation.invalidate();
                Err(RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::DeadlineElapsed)
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err(wait_for_admission_frozen_handoff_terminal_v2(terminal).await),
        }
    }

    async fn activate_process_ownership_v2(
        &self,
        expected_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        process_generation: NonZeroU64,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerProcessActivationErrorV2>
    {
        let (response, acknowledgement) = oneshot::channel();
        if self
            .supervisor_commands
            .send(
                RuntimeGatewayOwnerSupervisorCommandV1::ActivateProcessOwnership {
                    expected_receipt,
                    process_generation,
                    response,
                },
            )
            .await
            .is_err()
        {
            return Err(RuntimeGatewayOwnerProcessActivationErrorV2::SupervisorUnavailable);
        }
        self.accept_process_activation_acknowledgement_v2(process_generation, acknowledgement)
            .await
    }

    async fn accept_process_activation_acknowledgement_v2(
        &self,
        process_generation: NonZeroU64,
        acknowledgement: oneshot::Receiver<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerProcessActivationErrorV2,
            >,
        >,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerProcessActivationErrorV2>
    {
        match acknowledgement.await {
            Ok(result) => result,
            Err(_)
                if self.process_generation.load(Ordering::Acquire) == process_generation.get() =>
            {
                self.observe_current_gateway_owner_v1().await.map_err(|_| {
                    RuntimeGatewayOwnerProcessActivationErrorV2::ObservationUnavailable
                })
            }
            Err(_) => Err(RuntimeGatewayOwnerProcessActivationErrorV2::SupervisorUnavailable),
        }
    }

    async fn start_process_renewal_v2(
        &self,
        expected_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        process_generation: NonZeroU64,
    ) -> Result<
        RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerProcessRenewalStartErrorV2,
    > {
        let (response, acknowledgement) = oneshot::channel();
        if self
            .supervisor_commands
            .send(
                RuntimeGatewayOwnerSupervisorCommandV1::StartProcessRenewal {
                    expected_receipt,
                    process_generation,
                    response,
                },
            )
            .await
            .is_err()
        {
            return Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::SupervisorUnavailable);
        }
        self.accept_process_renewal_acknowledgement_v2(process_generation, acknowledgement)
            .await
    }

    async fn accept_process_renewal_acknowledgement_v2(
        &self,
        process_generation: NonZeroU64,
        acknowledgement: oneshot::Receiver<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerProcessRenewalStartErrorV2,
            >,
        >,
    ) -> Result<
        RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerProcessRenewalStartErrorV2,
    > {
        match acknowledgement.await {
            Ok(result) => result,
            Err(_)
                if self.production_generation.load(Ordering::Acquire)
                    == process_generation.get() =>
            {
                Ok(self.current_observation.borrow().clone())
            }
            Err(_) => Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::SupervisorUnavailable),
        }
    }

    async fn freeze_certification_v2(
        &self,
        expected_observation: RuntimeGatewayOwnerCurrentObservationV1,
        process_generation: NonZeroU64,
        cutoff: Instant,
    ) -> Result<
        RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerCertificationFreezeErrorV2,
    > {
        if Instant::now() >= cutoff {
            self.invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed);
        }
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        match timeout_at(
            TokioInstant::from_std(cutoff),
            self.supervisor_commands.send(
                RuntimeGatewayOwnerSupervisorCommandV1::FreezeCertification {
                    expected_observation,
                    process_generation,
                    cutoff,
                    response,
                },
            ),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                self.invalidation.invalidate();
                return Err(
                    wait_for_certification_freeze_terminal_until_v2(terminal, cutoff).await,
                );
            }
            Err(_) => {
                self.invalidation.invalidate();
                return Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed);
            }
        }
        match timeout_at(TokioInstant::from_std(cutoff), acknowledgement).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.invalidation.invalidate();
                Err(wait_for_certification_freeze_terminal_until_v2(terminal, cutoff).await)
            }
            Err(_) => {
                self.invalidation.invalidate();
                Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed)
            }
        }
    }

    async fn thaw_certification_v2(
        &self,
        authority: RuntimeGatewayOwnerCertificationFrozenObservationV2,
        completion_deadline: Instant,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCertificationThawErrorV2>
    {
        let completion_deadline = authority.cutoff_v2().min(completion_deadline);
        if Instant::now() >= completion_deadline {
            self.invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed);
        }
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        match timeout_at(
            TokioInstant::from_std(completion_deadline),
            self.supervisor_commands.send(
                RuntimeGatewayOwnerSupervisorCommandV1::ThawCertification {
                    authority,
                    completion_deadline,
                    response,
                },
            ),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                self.invalidation.invalidate();
                return Err(wait_for_certification_thaw_terminal_until_v2(
                    terminal,
                    completion_deadline,
                )
                .await);
            }
            Err(_) => {
                self.invalidation.invalidate();
                return Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed);
            }
        }
        match timeout_at(TokioInstant::from_std(completion_deadline), acknowledgement).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.invalidation.invalidate();
                Err(
                    wait_for_certification_thaw_terminal_until_v2(terminal, completion_deadline)
                        .await,
                )
            }
            Err(_) => {
                self.invalidation.invalidate();
                Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed)
            }
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
            committed_closed_recovery_observation: None,
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
    committed_closed_recovery_observation: Option<RuntimeGatewayOwnerCurrentObservationV1>,
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

    pub(crate) async fn commit_closed_recovery_in_place_v2(
        &mut self,
        permit: &RuntimeClosedDrainRecoveryPermitV2,
    ) -> Result<(), RuntimeGatewayOwnerClosedRecoveryCommitErrorV2> {
        if self.committed_closed_recovery_observation.is_some() {
            self.committed_closed_recovery_observation = None;
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation);
        }
        if Instant::now() >= self.observation.safety_deadline() {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SafetyElapsed);
        }
        if permit.owner_receipt() != self.observation.receipt() {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::OwnerReceiptMismatch);
        }
        let acknowledged = match self
            .inner()
            .commit_closed_recovery_v2(permit.owner_receipt().clone())
            .await
        {
            Ok(acknowledged) => acknowledged,
            Err(error) => {
                self.inner().invalidation.invalidate();
                return Err(error);
            }
        };
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
        self.committed_closed_recovery_observation = Some(acknowledged);
        Ok(())
    }

    pub(crate) fn try_into_committed_closed_recovery_v2(
        mut self,
    ) -> Result<RuntimeGatewayOwnerClosedRecoverySupervisorV2, Box<Self>> {
        let Some(acknowledged) = self.committed_closed_recovery_observation.take() else {
            return Err(Box::new(self));
        };
        if accept_closed_recovery_commit_observation_v2(
            acknowledged.clone(),
            &self.observation,
            self.observation.receipt(),
            Instant::now(),
        )
        .is_err()
        {
            self.inner().invalidation.invalidate();
            return Err(Box::new(self));
        }
        let inner = self.take_inner();
        Ok(RuntimeGatewayOwnerClosedRecoverySupervisorV2 {
            inner: Some(inner),
            observation: acknowledged,
            admission_frozen_handoff: None,
        })
    }

    #[cfg(test)]
    pub(crate) async fn commit_closed_recovery_v2(
        mut self,
        permit: &RuntimeClosedDrainRecoveryPermitV2,
    ) -> Result<
        RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        RuntimeGatewayOwnerClosedRecoveryCommitErrorV2,
    > {
        self.commit_closed_recovery_in_place_v2(permit).await?;
        self.try_into_committed_closed_recovery_v2()
            .map_err(|owner| {
                owner.inner().invalidation.invalidate();
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation
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
    admission_frozen_handoff: Option<RuntimeGatewayOwnerAdmissionFrozenHandoffV2>,
}

struct RuntimeGatewayOwnerAdmissionFrozenHandoffV2 {
    observation: RuntimeGatewayOwnerCurrentObservationV1,
    cutoff: Instant,
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

    pub(crate) fn terminal_status_v2(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn terminal_observation_v2(
        &self,
    ) -> impl std::future::Future<Output = RuntimeGatewayOwnerStartupWatchdogExitV1> + Send + 'static
    {
        let mut terminal = self.inner().terminal.clone();
        async move {
            loop {
                if let Some(exit) = *terminal.borrow() {
                    return exit;
                }
                if terminal.changed().await.is_err() {
                    return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
                }
            }
        }
    }

    pub(crate) fn is_bound_to_gateway_lifetime_v2(&self, expected: &Arc<AtomicBool>) -> bool {
        self.inner().is_bound_to_gateway_lifetime_v2(expected)
    }

    pub(crate) async fn enter_admission_frozen_in_place_v2(
        &mut self,
        authority: RuntimeGatewayOwnerAdmissionFrozenAuthorityV2,
    ) -> Result<(), RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2> {
        if self.admission_frozen_handoff.is_some() {
            self.admission_frozen_handoff = None;
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation);
        }
        let cutoff = authority.cutoff_v2();
        let observation = self.inner().enter_admission_frozen_v2(authority).await?;
        self.admission_frozen_handoff = Some(RuntimeGatewayOwnerAdmissionFrozenHandoffV2 {
            observation,
            cutoff,
        });
        Ok(())
    }

    pub(crate) fn try_into_admission_frozen_v2(
        mut self,
    ) -> Result<RuntimeGatewayOwnerAdmissionFrozenSupervisorV2, Box<Self>> {
        let Some(handoff) = self.admission_frozen_handoff.take() else {
            return Err(Box::new(self));
        };
        let inner = self.take_inner();
        Ok(RuntimeGatewayOwnerAdmissionFrozenSupervisorV2 {
            inner: Some(Box::new(inner)),
            handoff_observation: Box::new(handoff.observation),
            handoff_cutoff: handoff.cutoff,
        })
    }

    pub(crate) async fn shutdown(mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.take_inner().shutdown().await
    }

    #[cfg_attr(test, allow(dead_code))]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2 {
    #[error("runtime gateway owner admission-frozen handoff deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime gateway owner admission-frozen handoff found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner admission-frozen handoff observation is unavailable")]
    ObservationUnavailable,
    #[error("runtime gateway owner admission-frozen handoff violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner admission-frozen handoff supervisor is unavailable")]
    SupervisorUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayOwnerProcessActivationErrorV2 {
    #[error("runtime gateway owner process activation deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime gateway owner process activation receipt mismatched")]
    OwnerReceiptMismatch,
    #[error("runtime gateway owner process activation observation is unavailable")]
    ObservationUnavailable,
    #[error("runtime gateway owner process activation violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner process activation supervisor is unavailable")]
    SupervisorUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayOwnerProcessRenewalStartErrorV2 {
    #[error("runtime gateway owner process renewal start receipt mismatched")]
    OwnerReceiptMismatch,
    #[error("runtime gateway owner process renewal start generation mismatched")]
    ProcessGenerationMismatch,
    #[error("runtime gateway owner process renewal start violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner process renewal start supervisor is unavailable")]
    SupervisorUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayOwnerStrictSuccessorErrorV2 {
    #[error("runtime gateway owner strict successor deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime gateway owner strict successor found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner strict successor violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner strict successor supervisor is unavailable")]
    SupervisorUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeGatewayOwnerCertificationFreezeErrorV2 {
    #[error("runtime gateway owner certification freeze deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime gateway owner certification freeze receipt mismatched")]
    OwnerReceiptMismatch,
    #[error("runtime gateway owner certification freeze process generation mismatched")]
    ProcessGenerationMismatch,
    #[error("runtime gateway owner certification freeze found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner certification freeze violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner certification freeze supervisor is unavailable")]
    SupervisorUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeGatewayOwnerCertificationThawErrorV2 {
    #[error("runtime gateway owner certification thaw deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime gateway owner certification thaw authority is stale")]
    StaleAuthority,
    #[error("runtime gateway owner certification thaw process generation mismatched")]
    ProcessGenerationMismatch,
    #[error("runtime gateway owner certification thaw found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner certification thaw violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner certification thaw supervisor is unavailable")]
    SupervisorUnavailable,
}

pub(crate) struct RuntimeGatewayOwnerAdmissionFrozenSupervisorV2 {
    inner: Option<Box<RuntimeGatewayOwnerSupervisorHandleV1>>,
    handoff_observation: Box<RuntimeGatewayOwnerCurrentObservationV1>,
    handoff_cutoff: Instant,
}

impl RuntimeGatewayOwnerAdmissionFrozenSupervisorV2 {
    pub(crate) fn handoff_observation_v2(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.handoff_observation
    }

    pub(crate) fn handoff_cutoff_v2(&self) -> Instant {
        self.handoff_cutoff
    }

    pub(crate) fn terminal_status_v2(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    pub(crate) async fn activate_process_ownership_in_place_v2(
        &mut self,
        process_generation: NonZeroU64,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerProcessActivationErrorV2>
    {
        let expected = self.handoff_observation.receipt().clone();
        let observation = self
            .inner()
            .activate_process_ownership_v2(expected, process_generation)
            .await?;
        if observation.receipt() != self.handoff_observation.receipt()
            || self.inner().process_generation.load(Ordering::Acquire) != process_generation.get()
        {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerProcessActivationErrorV2::ProtocolViolation);
        }
        Ok(observation)
    }

    pub(crate) fn try_into_process_frozen_v2(
        mut self,
    ) -> Result<RuntimeGatewayOwnerProcessFrozenSupervisorV2, Self> {
        if self.inner().process_generation.load(Ordering::Acquire) == 0 {
            return Err(self);
        }
        let observation = self.handoff_observation.clone();
        let process_generation =
            NonZeroU64::new(self.inner().process_generation.load(Ordering::Acquire))
                .expect("process-owned gateway owner generation");
        Ok(RuntimeGatewayOwnerProcessFrozenSupervisorV2 {
            inner: Some(Box::new(self.take_inner())),
            activation_observation: observation,
            process_generation,
            production_start_observation: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn terminal_observation_v2(
        &self,
    ) -> impl std::future::Future<Output = RuntimeGatewayOwnerStartupWatchdogExitV1> + Send + 'static
    {
        let mut terminal = self.inner().terminal.clone();
        async move {
            loop {
                if let Some(exit) = *terminal.borrow() {
                    return exit;
                }
                if terminal.changed().await.is_err() {
                    return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
                }
            }
        }
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

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_deref()
            .expect("admission-frozen gateway owner supervisor")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        *self
            .inner
            .take()
            .expect("admission-frozen gateway owner supervisor")
    }
}

pub(crate) struct RuntimeGatewayOwnerProcessFrozenSupervisorV2 {
    inner: Option<Box<RuntimeGatewayOwnerSupervisorHandleV1>>,
    activation_observation: Box<RuntimeGatewayOwnerCurrentObservationV1>,
    process_generation: NonZeroU64,
    production_start_observation: Option<Box<RuntimeGatewayOwnerCurrentObservationV1>>,
}

impl RuntimeGatewayOwnerProcessFrozenSupervisorV2 {
    pub(crate) fn activation_observation_v2(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.activation_observation
    }

    pub(crate) fn process_generation_v2(&self) -> NonZeroU64 {
        self.process_generation
    }

    pub(crate) fn terminal_status_v2(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    pub(crate) async fn observe_current_v2(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCurrentObservationErrorV1>
    {
        self.inner().observe_current_gateway_owner_v1().await
    }

    pub(crate) async fn start_production_renewal_in_place_v2(
        &mut self,
    ) -> Result<
        RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerProcessRenewalStartErrorV2,
    > {
        if self.production_start_observation.is_some() {
            self.production_start_observation = None;
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation);
        }
        let observation = self
            .inner()
            .start_process_renewal_v2(
                self.activation_observation.receipt().clone(),
                self.process_generation,
            )
            .await?;
        if observation.receipt().lease_id != self.activation_observation.receipt().lease_id
            || observation.receipt().owner_revision
                < self.activation_observation.receipt().owner_revision
            || self.inner().production_generation.load(Ordering::Acquire)
                != self.process_generation.get()
        {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation);
        }
        self.production_start_observation = Some(Box::new(observation.clone()));
        Ok(observation)
    }

    pub(crate) fn try_into_production_v2(
        mut self,
    ) -> Result<RuntimeGatewayOwnerProductionSupervisorV2, Self> {
        let Some(start_observation) = self.production_start_observation.take() else {
            return Err(self);
        };
        if self.inner().production_generation.load(Ordering::Acquire)
            != self.process_generation.get()
        {
            self.inner().invalidation.invalidate();
            return Err(self);
        }
        let process_generation = self.process_generation;
        Ok(RuntimeGatewayOwnerProductionSupervisorV2 {
            inner: Some(Box::new(self.take_inner())),
            process_generation,
            start_observation,
        })
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

    #[cfg(test)]
    pub(crate) fn terminal_observation_v2(
        &self,
    ) -> impl std::future::Future<Output = RuntimeGatewayOwnerStartupWatchdogExitV1> + Send + 'static
    {
        let mut terminal = self.inner().terminal.clone();
        async move {
            loop {
                if let Some(exit) = *terminal.borrow() {
                    return exit;
                }
                if terminal.changed().await.is_err() {
                    return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
                }
            }
        }
    }

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_deref()
            .expect("process-frozen gateway owner supervisor")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        *self
            .inner
            .take()
            .expect("process-frozen gateway owner supervisor")
    }
}

pub(crate) struct RuntimeGatewayOwnerProductionSupervisorV2 {
    inner: Option<Box<RuntimeGatewayOwnerSupervisorHandleV1>>,
    process_generation: NonZeroU64,
    start_observation: Box<RuntimeGatewayOwnerCurrentObservationV1>,
}

impl RuntimeGatewayOwnerProductionSupervisorV2 {
    pub(crate) fn process_generation_v2(&self) -> NonZeroU64 {
        self.process_generation
    }

    #[cfg(test)]
    pub(crate) fn start_observation_v2(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.start_observation
    }

    pub(crate) fn terminal_status_v2(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    pub(crate) fn terminal_observation_v2(
        &self,
    ) -> impl std::future::Future<Output = RuntimeGatewayOwnerStartupWatchdogExitV1> + Send + 'static
    {
        let mut terminal = self.inner().terminal.clone();
        async move {
            loop {
                if let Some(exit) = *terminal.borrow() {
                    return exit;
                }
                if terminal.changed().await.is_err() {
                    return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
                }
            }
        }
    }

    pub(crate) async fn observe_current_v2(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCurrentObservationErrorV1>
    {
        match self.inner().observe_current_gateway_owner_v1().await {
            Err(RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable) => {
                self.inner().observe_current_gateway_owner_v1().await
            }
            result => result,
        }
    }

    pub(crate) async fn wait_for_strict_successor_v2(
        &self,
        previous_revision: NonZeroU64,
        deadline: Instant,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerStrictSuccessorErrorV2>
    {
        let mut current = self.inner().current_observation_watch_v2();
        let mut terminal = self.inner().terminal.clone();
        let expected_lease_id = &self.start_observation.receipt().lease_id;
        loop {
            let observation = current.borrow().clone();
            if &observation.receipt().lease_id != expected_lease_id {
                self.inner().invalidation.invalidate();
                return Err(RuntimeGatewayOwnerStrictSuccessorErrorV2::OwnershipLost);
            }
            if observation.receipt().owner_revision > previous_revision {
                return Ok(observation);
            }
            if observation.receipt().owner_revision < previous_revision {
                self.inner().invalidation.invalidate();
                return Err(RuntimeGatewayOwnerStrictSuccessorErrorV2::ProtocolViolation);
            }
            if let Some(exit) = *terminal.borrow() {
                return Err(map_terminal_strict_successor_error_v2(exit));
            }
            if Instant::now() >= deadline {
                self.inner().invalidation.invalidate();
                return Err(RuntimeGatewayOwnerStrictSuccessorErrorV2::DeadlineElapsed);
            }
            tokio::select! {
                biased;
                _ = sleep_until(TokioInstant::from_std(deadline)) => {
                    self.inner().invalidation.invalidate();
                    return Err(RuntimeGatewayOwnerStrictSuccessorErrorV2::DeadlineElapsed);
                }
                changed = terminal.changed() => {
                    if changed.is_err() {
                        self.inner().invalidation.invalidate();
                        return Err(
                            RuntimeGatewayOwnerStrictSuccessorErrorV2::SupervisorUnavailable,
                        );
                    }
                }
                changed = current.changed() => {
                    if changed.is_err() {
                        self.inner().invalidation.invalidate();
                        return Err(
                            RuntimeGatewayOwnerStrictSuccessorErrorV2::SupervisorUnavailable,
                        );
                    }
                }
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn prepare_certification_freeze_v2(
        self,
        cutoff: Instant,
    ) -> Result<
        RuntimeGatewayOwnerCertificationFreezeAuthorityV2,
        RuntimeGatewayOwnerCertificationFreezeErrorV2,
    > {
        let now = Instant::now();
        let expected_observation = self.inner().current_observation.borrow().clone();
        if now >= cutoff || expected_observation.safety_deadline() <= cutoff {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed);
        }
        if self.inner().process_generation.load(Ordering::Acquire) != self.process_generation.get()
        {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::ProcessGenerationMismatch);
        }
        if self.inner().production_generation.load(Ordering::Acquire)
            != self.process_generation.get()
        {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::OwnershipLost);
        }
        let process_generation = self.process_generation;
        Ok(RuntimeGatewayOwnerCertificationFreezeAuthorityV2 {
            owner: Some(Box::new(self)),
            expected_observation: Box::new(expected_observation),
            process_generation,
            cutoff,
        })
    }

    pub(crate) async fn shutdown_until_v2(
        mut self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        self.take_inner().shutdown_until(cleanup_deadline).await
    }

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_deref()
            .expect("production gateway owner supervisor")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        *self
            .inner
            .take()
            .expect("production gateway owner supervisor")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeGatewayOwnerCertificationFreezeAuthorityV2 {
    owner: Option<Box<RuntimeGatewayOwnerProductionSupervisorV2>>,
    expected_observation: Box<RuntimeGatewayOwnerCurrentObservationV1>,
    process_generation: NonZeroU64,
    cutoff: Instant,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeGatewayOwnerCertificationFreezeAuthorityV2 {
    pub(crate) fn expected_observation_v2(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.expected_observation
    }

    pub(crate) fn process_generation_v2(&self) -> NonZeroU64 {
        self.process_generation
    }

    pub(crate) fn cutoff_v2(&self) -> Instant {
        self.cutoff
    }

    pub(crate) async fn freeze_v2(
        mut self,
    ) -> Result<
        (
            RuntimeGatewayOwnerCertificationFrozenSupervisorV2,
            RuntimeGatewayOwnerCertificationFrozenObservationV2,
        ),
        RuntimeGatewayOwnerCertificationFreezeErrorV2,
    > {
        let expected_observation = (*self.expected_observation).clone();
        let process_generation = self.process_generation;
        let cutoff = self.cutoff;
        let acknowledged = self
            .owner()
            .inner()
            .freeze_certification_v2(expected_observation.clone(), process_generation, cutoff)
            .await?;
        if Instant::now() >= cutoff {
            self.owner().inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed);
        }
        if !accept_certification_freeze_observation_v2(&expected_observation, &acknowledged, cutoff)
            || self
                .owner()
                .inner()
                .process_generation
                .load(Ordering::Acquire)
                != process_generation.get()
            || self
                .owner()
                .inner()
                .production_generation
                .load(Ordering::Acquire)
                != 0
        {
            self.owner().inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::ProtocolViolation);
        }
        let mut owner = self.take_owner();
        let inner = owner.take_inner();
        let frozen = RuntimeGatewayOwnerCertificationFrozenSupervisorV2 {
            inner: Some(Box::new(inner)),
            frozen_observation: Box::new(acknowledged.clone()),
            process_generation,
            cutoff,
        };
        let observation = RuntimeGatewayOwnerCertificationFrozenObservationV2 {
            observation: Box::new(acknowledged),
            process_generation,
            cutoff,
        };
        Ok((frozen, observation))
    }

    fn owner(&self) -> &RuntimeGatewayOwnerProductionSupervisorV2 {
        self.owner
            .as_deref()
            .expect("certification freeze authority")
    }

    fn take_owner(&mut self) -> RuntimeGatewayOwnerProductionSupervisorV2 {
        *self.owner.take().expect("certification freeze authority")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeGatewayOwnerCertificationFrozenObservationV2 {
    observation: Box<RuntimeGatewayOwnerCurrentObservationV1>,
    process_generation: NonZeroU64,
    cutoff: Instant,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeGatewayOwnerCertificationFrozenObservationV2 {
    pub(crate) fn observation_v2(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.observation
    }

    pub(crate) fn process_generation_v2(&self) -> NonZeroU64 {
        self.process_generation
    }

    pub(crate) fn cutoff_v2(&self) -> Instant {
        self.cutoff
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeGatewayOwnerCertificationFrozenSupervisorV2 {
    inner: Option<Box<RuntimeGatewayOwnerSupervisorHandleV1>>,
    frozen_observation: Box<RuntimeGatewayOwnerCurrentObservationV1>,
    process_generation: NonZeroU64,
    cutoff: Instant,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeGatewayOwnerCertificationFrozenSupervisorV2 {
    pub(crate) fn frozen_observation_v2(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        &self.frozen_observation
    }

    pub(crate) fn process_generation_v2(&self) -> NonZeroU64 {
        self.process_generation
    }

    pub(crate) fn cutoff_v2(&self) -> Instant {
        self.cutoff
    }

    pub(crate) fn terminal_status_v2(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.inner().terminal_status()
    }

    pub(crate) fn terminal_observation_v2(
        &self,
    ) -> impl std::future::Future<Output = RuntimeGatewayOwnerStartupWatchdogExitV1> + Send + 'static
    {
        let mut terminal = self.inner().terminal.clone();
        async move {
            loop {
                if let Some(exit) = *terminal.borrow() {
                    return exit;
                }
                if terminal.changed().await.is_err() {
                    return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
                }
            }
        }
    }

    pub(crate) async fn observe_current_v2(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCurrentObservationErrorV1>
    {
        self.inner().observe_current_gateway_owner_v1().await
    }

    pub(crate) async fn thaw_v2(
        mut self,
        authority: RuntimeGatewayOwnerCertificationFrozenObservationV2,
        successor_deadline: Instant,
    ) -> Result<
        (
            RuntimeGatewayOwnerProductionSupervisorV2,
            RuntimeGatewayOwnerCurrentObservationV1,
        ),
        RuntimeGatewayOwnerCertificationThawErrorV2,
    > {
        let now = Instant::now();
        let completion_deadline = self.cutoff.min(successor_deadline);
        if now >= completion_deadline {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed);
        }
        if authority.process_generation != self.process_generation {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationThawErrorV2::ProcessGenerationMismatch);
        }
        if authority.cutoff != self.cutoff
            || authority.observation.as_ref() != self.frozen_observation.as_ref()
        {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationThawErrorV2::StaleAuthority);
        }
        let previous_revision = authority.observation.receipt().owner_revision;
        let acknowledged = self
            .inner()
            .thaw_certification_v2(authority, completion_deadline)
            .await?;
        if Instant::now() >= completion_deadline {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed);
        }
        if acknowledged != *self.frozen_observation
            || self.inner().process_generation.load(Ordering::Acquire)
                != self.process_generation.get()
            || self.inner().production_generation.load(Ordering::Acquire)
                != self.process_generation.get()
        {
            self.inner().invalidation.invalidate();
            return Err(RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation);
        }
        let process_generation = self.process_generation;
        let production = RuntimeGatewayOwnerProductionSupervisorV2 {
            inner: Some(Box::new(self.take_inner())),
            process_generation,
            start_observation: Box::new(acknowledged),
        };
        match production
            .wait_for_strict_successor_v2(previous_revision, completion_deadline)
            .await
        {
            Ok(successor) => Ok((production, successor)),
            Err(error) => Err(map_strict_successor_thaw_error_v2(error)),
        }
    }

    pub(crate) async fn shutdown_until_v2(
        mut self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        self.take_inner().shutdown_until(cleanup_deadline).await
    }

    fn inner(&self) -> &RuntimeGatewayOwnerSupervisorHandleV1 {
        self.inner
            .as_deref()
            .expect("certification-frozen gateway owner supervisor")
    }

    fn take_inner(&mut self) -> RuntimeGatewayOwnerSupervisorHandleV1 {
        *self
            .inner
            .take()
            .expect("certification-frozen gateway owner supervisor")
    }
}

impl std::fmt::Debug for RuntimeGatewayOwnerCertificationFreezeAuthorityV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerCertificationFreezeAuthorityV2(<redacted>)")
    }
}

impl std::fmt::Debug for RuntimeGatewayOwnerCertificationFrozenObservationV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerCertificationFrozenObservationV2(<redacted>)")
    }
}

impl std::fmt::Debug for RuntimeGatewayOwnerCertificationFrozenSupervisorV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerCertificationFrozenSupervisorV2(<redacted>)")
    }
}

impl std::fmt::Debug for RuntimeGatewayOwnerProductionSupervisorV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerProductionSupervisorV2(<redacted>)")
    }
}

impl std::fmt::Debug for RuntimeGatewayOwnerProcessFrozenSupervisorV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerProcessFrozenSupervisorV2(<redacted>)")
    }
}

impl std::fmt::Debug for RuntimeGatewayOwnerAdmissionFrozenSupervisorV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerAdmissionFrozenSupervisorV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeGatewayOwnerSupervisorRoleV1 {
    Startup,
    PreparedClosedRecovery,
    ClosedRecovery,
    AdmissionFrozen,
    ProcessFrozen,
    CertificationFrozen,
    Production,
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
    EnterAdmissionFrozen {
        authority: RuntimeGatewayOwnerAdmissionFrozenAuthorityV2,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2,
            >,
        >,
    },
    ActivateProcessOwnership {
        expected_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        process_generation: NonZeroU64,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerProcessActivationErrorV2,
            >,
        >,
    },
    StartProcessRenewal {
        expected_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        process_generation: NonZeroU64,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerProcessRenewalStartErrorV2,
            >,
        >,
    },
    FreezeCertification {
        expected_observation: RuntimeGatewayOwnerCurrentObservationV1,
        process_generation: NonZeroU64,
        cutoff: Instant,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerCertificationFreezeErrorV2,
            >,
        >,
    },
    ThawCertification {
        authority: RuntimeGatewayOwnerCertificationFrozenObservationV2,
        completion_deadline: Instant,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerCertificationThawErrorV2,
            >,
        >,
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

async fn wait_for_admission_frozen_handoff_terminal_v2(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_admission_frozen_handoff_error_v2(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::SupervisorUnavailable;
        }
    }
}

async fn wait_for_certification_freeze_terminal_until_v2(
    terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
    cutoff: Instant,
) -> RuntimeGatewayOwnerCertificationFreezeErrorV2 {
    match timeout_at(
        TokioInstant::from_std(cutoff),
        wait_for_certification_freeze_terminal_v2(terminal),
    )
    .await
    {
        Ok(error) => error,
        Err(_) => RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed,
    }
}

async fn wait_for_certification_freeze_terminal_v2(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerCertificationFreezeErrorV2 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_certification_freeze_error_v2(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerCertificationFreezeErrorV2::SupervisorUnavailable;
        }
    }
}

async fn wait_for_certification_thaw_terminal_until_v2(
    terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
    cutoff: Instant,
) -> RuntimeGatewayOwnerCertificationThawErrorV2 {
    match timeout_at(
        TokioInstant::from_std(cutoff),
        wait_for_certification_thaw_terminal_v2(terminal),
    )
    .await
    {
        Ok(error) => error,
        Err(_) => RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed,
    }
}

async fn wait_for_certification_thaw_terminal_v2(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerCertificationThawErrorV2 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_certification_thaw_error_v2(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerCertificationThawErrorV2::SupervisorUnavailable;
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

fn map_terminal_admission_frozen_handoff_error_v2(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::DeadlineElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::SupervisorUnavailable
        }
    }
}

fn map_terminal_strict_successor_error_v2(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerStrictSuccessorErrorV2 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerStrictSuccessorErrorV2::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerStrictSuccessorErrorV2::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerStrictSuccessorErrorV2::SupervisorUnavailable
        }
    }
}

fn accept_certification_freeze_observation_v2(
    expected: &RuntimeGatewayOwnerCurrentObservationV1,
    current: &RuntimeGatewayOwnerCurrentObservationV1,
    cutoff: Instant,
) -> bool {
    if Instant::now() >= cutoff
        || expected.safety_deadline() <= cutoff
        || current.safety_deadline() <= cutoff
        || expected.receipt().database_lease_duration().is_none()
        || current.receipt().database_lease_duration().is_none()
        || current.receipt().lease_id != expected.receipt().lease_id
    {
        return false;
    }
    if current.receipt().owner_revision == expected.receipt().owner_revision
        && current.receipt().expires_at == expected.receipt().expires_at
        && current.receipt().database_now >= expected.receipt().database_now
        && current.safety_deadline() <= expected.safety_deadline()
    {
        return true;
    }
    expected.receipt().owner_revision.get().checked_add(1)
        == Some(current.receipt().owner_revision.get())
        && current.receipt().database_now > expected.receipt().database_now
        && current.receipt().expires_at > expected.receipt().expires_at
        && current.safety_deadline() > expected.safety_deadline()
}

fn map_terminal_certification_freeze_error_v2(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerCertificationFreezeErrorV2 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerCertificationFreezeErrorV2::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerCertificationFreezeErrorV2::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerCertificationFreezeErrorV2::SupervisorUnavailable
        }
    }
}

fn map_terminal_certification_thaw_error_v2(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerCertificationThawErrorV2 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerCertificationThawErrorV2::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerCertificationThawErrorV2::SupervisorUnavailable
        }
    }
}

fn map_strict_successor_thaw_error_v2(
    error: RuntimeGatewayOwnerStrictSuccessorErrorV2,
) -> RuntimeGatewayOwnerCertificationThawErrorV2 {
    match error {
        RuntimeGatewayOwnerStrictSuccessorErrorV2::DeadlineElapsed => {
            RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed
        }
        RuntimeGatewayOwnerStrictSuccessorErrorV2::OwnershipLost => {
            RuntimeGatewayOwnerCertificationThawErrorV2::OwnershipLost
        }
        RuntimeGatewayOwnerStrictSuccessorErrorV2::ProtocolViolation => {
            RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation
        }
        RuntimeGatewayOwnerStrictSuccessorErrorV2::SupervisorUnavailable => {
            RuntimeGatewayOwnerCertificationThawErrorV2::SupervisorUnavailable
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
    let initial_observation = RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&watchdog);
    let (current_observation_sender, current_observation) = watch::channel(initial_observation);
    let guard = RuntimeGatewayOwnerStartupWatchdogGuardV1::new(invalidation.clone());
    let startup_cleanup_cap =
        RuntimeGatewayOwnerStartupCleanupCapV1::new(initial_startup_cleanup_deadline);
    let actor_startup_cleanup_cap = startup_cleanup_cap.clone();
    let process_generation = Arc::new(AtomicU64::new(0));
    let actor_process_generation = process_generation.clone();
    let production_generation = Arc::new(AtomicU64::new(0));
    let actor_production_generation = production_generation.clone();
    let task = runtime.spawn(async move {
        let exit =
            run_gateway_owner_startup_watchdog_v1(RuntimeGatewayOwnerStartupWatchdogActorV1 {
                port,
                watchdog,
                config,
                receivers,
                guard,
                startup_cleanup_cap: actor_startup_cleanup_cap,
                process_generation: actor_process_generation,
                production_generation: actor_production_generation,
                current_observation_sender,
            })
            .await;
        let _result = terminal_sender.send(Some(exit));
        let _write_result = writeln!(
            std::io::stderr().lock(),
            "starring_runtime_component_status component=gateway_owner status={}",
            exit.code()
        );
    });
    Ok(RuntimeGatewayOwnerStartupWatchdogHandleV1 {
        inner: Some(RuntimeGatewayOwnerSupervisorHandleV1 {
            shutdown_commands,
            supervisor_commands,
            closed_recovery_commands,
            terminal,
            current_observation,
            invalidation,
            gateway_lifetime,
            process_generation,
            production_generation,
            task: Some(task),
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

struct RuntimeGatewayOwnerStartupWatchdogActorV1<P> {
    port: P,
    watchdog: RuntimeGatewayOwnerWatchdogV1,
    config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
    receivers: RuntimeGatewayOwnerStartupWatchdogReceiversV1,
    guard: RuntimeGatewayOwnerStartupWatchdogGuardV1,
    startup_cleanup_cap: RuntimeGatewayOwnerStartupCleanupCapV1,
    process_generation: Arc<AtomicU64>,
    production_generation: Arc<AtomicU64>,
    current_observation_sender: watch::Sender<RuntimeGatewayOwnerCurrentObservationV1>,
}

async fn run_gateway_owner_startup_watchdog_v1<P>(
    actor: RuntimeGatewayOwnerStartupWatchdogActorV1<P>,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
    P::Error: Send + 'static,
{
    let RuntimeGatewayOwnerStartupWatchdogActorV1 {
        port,
        watchdog,
        config,
        receivers,
        mut guard,
        startup_cleanup_cap,
        process_generation,
        production_generation,
        current_observation_sender,
    } = actor;
    let RuntimeGatewayOwnerStartupWatchdogReceiversV1 {
        mut shutdown_commands,
        mut supervisor_commands,
        mut closed_recovery_commands,
    } = receivers;
    let lease_id = watchdog.schedule().receipt().lease_id.clone();
    let mut current = Some(watchdog);
    let mut role = RuntimeGatewayOwnerSupervisorRoleV1::Startup;
    let mut admission_frozen_cutoff = None;
    let mut certification_frozen_cutoff = None;
    let mut pending_supervisor_command = None;
    let mut pending_closed_recovery_command = None;
    let mut shutdown_acknowledgement = None;
    let mut force_production_renewal = false;
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
                | RuntimeGatewayOwnerSupervisorRoleV1::AdmissionFrozen
                | RuntimeGatewayOwnerSupervisorRoleV1::ProcessFrozen
                | RuntimeGatewayOwnerSupervisorRoleV1::CertificationFrozen
        ) {
            let watchdog = current.take().expect("gateway owner watchdog state");
            let safety_deadline = match role {
                RuntimeGatewayOwnerSupervisorRoleV1::AdmissionFrozen => {
                    let Some(cutoff) = admission_frozen_cutoff else {
                        guard.invalidate_now();
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                            config.cleanup,
                        );
                    };
                    watchdog.schedule().safety_deadline().min(cutoff)
                }
                RuntimeGatewayOwnerSupervisorRoleV1::CertificationFrozen => {
                    let Some(cutoff) = certification_frozen_cutoff else {
                        guard.invalidate_now();
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                            config.cleanup,
                        );
                    };
                    watchdog.schedule().safety_deadline().min(cutoff)
                }
                _ => watchdog.schedule().safety_deadline(),
            };
            if Instant::now() >= safety_deadline {
                guard.invalidate_now();
                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                    config.cleanup,
                );
            }
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
                    match command {
                        RuntimeGatewayOwnerSupervisorCommandV1::EnterAdmissionFrozen {
                            authority,
                            response,
                        } if role == RuntimeGatewayOwnerSupervisorRoleV1::ClosedRecovery => {
                            match enter_admission_frozen_owner_v2(
                                &port,
                                watchdog,
                                authority,
                                response,
                                RuntimeGatewayOwnerAdmissionFrozenActorContextV2 {
                                    shutdown_commands: &mut shutdown_commands,
                                    shutdown_acknowledgement: &mut shutdown_acknowledgement,
                                    guard: &mut guard,
                                    cleanup: config.cleanup,
                                },
                            )
                            .await
                            {
                                RuntimeGatewayOwnerAdmissionFrozenStepV2::Frozen {
                                    successor,
                                    observation,
                                    cutoff,
                                    response,
                                } => {
                                    role = RuntimeGatewayOwnerSupervisorRoleV1::AdmissionFrozen;
                                    admission_frozen_cutoff = Some(cutoff);
                                    current = Some(*successor);
                                    if response.send(Ok(observation)).is_err() {
                                        guard.invalidate_now();
                                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                            RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                            config.cleanup,
                                        );
                                    }
                                }
                                RuntimeGatewayOwnerAdmissionFrozenStepV2::Stop(stop) => {
                                    break 'supervisor stop;
                                }
                            }
                            continue 'supervisor;
                        }
                        RuntimeGatewayOwnerSupervisorCommandV1::ActivateProcessOwnership {
                            expected_receipt,
                            process_generation: requested_process_generation,
                            response,
                        } if role == RuntimeGatewayOwnerSupervisorRoleV1::AdmissionFrozen => {
                            if response.is_closed() {
                                current = Some(watchdog);
                                continue 'supervisor;
                            }
                            if Instant::now() >= safety_deadline {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessActivationErrorV2::DeadlineElapsed,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                                    config.cleanup,
                                );
                            }
                            if watchdog.schedule().receipt() != &expected_receipt {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessActivationErrorV2::OwnerReceiptMismatch,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            let observation =
                                RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&watchdog);
                            if process_generation
                                .compare_exchange(
                                    0,
                                    requested_process_generation.get(),
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                )
                                .is_err()
                            {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessActivationErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            startup_cleanup_cap.clear();
                            role = RuntimeGatewayOwnerSupervisorRoleV1::ProcessFrozen;
                            admission_frozen_cutoff = None;
                            current = Some(watchdog);
                            let _result = response.send(Ok(observation));
                            continue 'supervisor;
                        }
                        RuntimeGatewayOwnerSupervisorCommandV1::Observe { response }
                            if matches!(
                                role,
                                RuntimeGatewayOwnerSupervisorRoleV1::ProcessFrozen
                                    | RuntimeGatewayOwnerSupervisorRoleV1::CertificationFrozen
                            ) =>
                        {
                            if response.is_closed() {
                                current = Some(watchdog);
                                continue 'supervisor;
                            }
                            let observation =
                                RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&watchdog);
                            current = Some(watchdog);
                            let _result = response.send(Ok(observation));
                            continue 'supervisor;
                        }
                        RuntimeGatewayOwnerSupervisorCommandV1::ThawCertification {
                            authority,
                            completion_deadline,
                            response,
                        } if role == RuntimeGatewayOwnerSupervisorRoleV1::CertificationFrozen => {
                            if response.is_closed() {
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            let Some(cutoff) = certification_frozen_cutoff else {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            };
                            if completion_deadline > cutoff {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            if Instant::now() >= completion_deadline {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                                    config.cleanup,
                                );
                            }
                            if process_generation.load(Ordering::Acquire)
                                != authority.process_generation.get()
                            {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::ProcessGenerationMismatch,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            let observation =
                                RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&watchdog);
                            if authority.cutoff != cutoff
                                || authority.observation.as_ref() != &observation
                            {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::StaleAuthority,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            if Instant::now() >= completion_deadline {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                                    config.cleanup,
                                );
                            }
                            if production_generation
                                .compare_exchange(
                                    0,
                                    authority.process_generation.get(),
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                )
                                .is_err()
                            {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            role = RuntimeGatewayOwnerSupervisorRoleV1::Production;
                            certification_frozen_cutoff = None;
                            force_production_renewal = true;
                            let _result = current_observation_sender.send(observation.clone());
                            current = Some(watchdog);
                            if response.send(Ok(observation)).is_err() {
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            continue 'supervisor;
                        }
                        RuntimeGatewayOwnerSupervisorCommandV1::StartProcessRenewal {
                            expected_receipt,
                            process_generation: requested_process_generation,
                            response,
                        } if role == RuntimeGatewayOwnerSupervisorRoleV1::ProcessFrozen => {
                            if response.is_closed() {
                                current = Some(watchdog);
                                continue 'supervisor;
                            }
                            if watchdog.schedule().receipt() != &expected_receipt {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessRenewalStartErrorV2::OwnerReceiptMismatch,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            if process_generation.load(Ordering::Acquire)
                                != requested_process_generation.get()
                            {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProcessGenerationMismatch,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            let observation =
                                RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&watchdog);
                            if production_generation
                                .compare_exchange(
                                    0,
                                    requested_process_generation.get(),
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                )
                                .is_err()
                            {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            role = RuntimeGatewayOwnerSupervisorRoleV1::Production;
                            force_production_renewal = true;
                            let _result = current_observation_sender.send(observation.clone());
                            current = Some(watchdog);
                            let _result = response.send(Ok(observation));
                            continue 'supervisor;
                        }
                        command => {
                            reject_frozen_supervisor_command_v2(command);
                            guard.invalidate_now();
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                config.cleanup,
                            );
                        }
                    }
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
        let action = if force_production_renewal && pending_supervisor_command.is_none() {
            force_production_renewal = false;
            RuntimeGatewayOwnerWatchdogActionV1::RenewNow
        } else {
            watchdog.action_at(Instant::now())
        };
        match action {
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
                                        let observation =
                                            RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(
                                                &successor,
                                            );
                                        let _result =
                                            current_observation_sender.send(observation);
                                        current = Some(*successor);
                                        let _result = response.send(result);
                                    }
                                    RuntimeGatewayOwnerStartupObservationStepV1::Stop(stop) => {
                                        break 'supervisor stop;
                                    }
                                }
                            }
                            RuntimeGatewayOwnerSupervisorCommandV1::EnterAdmissionFrozen {
                                response,
                                ..
                            } => {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            RuntimeGatewayOwnerSupervisorCommandV1::ActivateProcessOwnership {
                                response,
                                ..
                            } => {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessActivationErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            RuntimeGatewayOwnerSupervisorCommandV1::StartProcessRenewal {
                                response,
                                ..
                            } => {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            RuntimeGatewayOwnerSupervisorCommandV1::FreezeCertification {
                                expected_observation,
                                process_generation: requested_process_generation,
                                cutoff,
                                response,
                            } if role == RuntimeGatewayOwnerSupervisorRoleV1::Production => {
                                if response.is_closed() {
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                        config.cleanup,
                                    );
                                }
                                if Instant::now() >= cutoff {
                                    let _result = response.send(Err(
                                        RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed,
                                    ));
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                                        config.cleanup,
                                    );
                                }
                                if process_generation.load(Ordering::Acquire)
                                    != requested_process_generation.get()
                                {
                                    let _result = response.send(Err(
                                        RuntimeGatewayOwnerCertificationFreezeErrorV2::ProcessGenerationMismatch,
                                    ));
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                        config.cleanup,
                                    );
                                }
                                if production_generation.load(Ordering::Acquire)
                                    != requested_process_generation.get()
                                {
                                    let _result = response.send(Err(
                                        RuntimeGatewayOwnerCertificationFreezeErrorV2::OwnershipLost,
                                    ));
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                        config.cleanup,
                                    );
                                }
                                let observation =
                                    RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(
                                        &watchdog,
                                    );
                                if !accept_certification_freeze_observation_v2(
                                    &expected_observation,
                                    &observation,
                                    cutoff,
                                ) {
                                    let error = if observation.safety_deadline() <= cutoff {
                                        RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed
                                    } else {
                                        RuntimeGatewayOwnerCertificationFreezeErrorV2::OwnerReceiptMismatch
                                    };
                                    let _result = response.send(Err(error));
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                        config.cleanup,
                                    );
                                }
                                if production_generation
                                    .compare_exchange(
                                        requested_process_generation.get(),
                                        0,
                                        Ordering::AcqRel,
                                        Ordering::Acquire,
                                    )
                                    .is_err()
                                {
                                    let _result = response.send(Err(
                                        RuntimeGatewayOwnerCertificationFreezeErrorV2::ProtocolViolation,
                                    ));
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                        config.cleanup,
                                    );
                                }
                                role =
                                    RuntimeGatewayOwnerSupervisorRoleV1::CertificationFrozen;
                                certification_frozen_cutoff = Some(cutoff);
                                force_production_renewal = false;
                                current = Some(watchdog);
                                if response.send(Ok(observation)).is_err() {
                                    guard.invalidate_now();
                                    break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                        config.cleanup,
                                    );
                                }
                                continue 'supervisor;
                            }
                            RuntimeGatewayOwnerSupervisorCommandV1::FreezeCertification {
                                response,
                                ..
                            } => {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationFreezeErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
                            }
                            RuntimeGatewayOwnerSupervisorCommandV1::ThawCertification {
                                response,
                                ..
                            } => {
                                let _result = response.send(Err(
                                    RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation,
                                ));
                                guard.invalidate_now();
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                                    config.cleanup,
                                );
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
                            let observation =
                                RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&successor);
                            let _result = current_observation_sender.send(observation);
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
                        if role == RuntimeGatewayOwnerSupervisorRoleV1::Production {
                            force_production_renewal = true;
                        }
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
        RuntimeGatewayOwnerSupervisorCommandV1::EnterAdmissionFrozen { response, .. } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation,
            ));
        }
        RuntimeGatewayOwnerSupervisorCommandV1::ActivateProcessOwnership { response, .. } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerProcessActivationErrorV2::ProtocolViolation,
            ));
        }
        RuntimeGatewayOwnerSupervisorCommandV1::StartProcessRenewal { response, .. } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation,
            ));
        }
        RuntimeGatewayOwnerSupervisorCommandV1::FreezeCertification { response, .. } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerCertificationFreezeErrorV2::ProtocolViolation,
            ));
        }
        RuntimeGatewayOwnerSupervisorCommandV1::ThawCertification { response, .. } => {
            let _result = response.send(Err(
                RuntimeGatewayOwnerCertificationThawErrorV2::ProtocolViolation,
            ));
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

enum RuntimeGatewayOwnerAdmissionFrozenStepV2 {
    Frozen {
        successor: Box<RuntimeGatewayOwnerWatchdogV1>,
        observation: RuntimeGatewayOwnerCurrentObservationV1,
        cutoff: Instant,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2,
            >,
        >,
    },
    Stop(RuntimeGatewayOwnerStartupWatchdogStopV1),
}

struct RuntimeGatewayOwnerAdmissionFrozenActorContextV2<'a> {
    shutdown_commands: &'a mut mpsc::Receiver<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    shutdown_acknowledgement:
        &'a mut Option<oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
    guard: &'a mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
}

async fn enter_admission_frozen_owner_v2<P>(
    port: &P,
    watchdog: RuntimeGatewayOwnerWatchdogV1,
    authority: RuntimeGatewayOwnerAdmissionFrozenAuthorityV2,
    mut response: oneshot::Sender<
        Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2,
        >,
    >,
    context: RuntimeGatewayOwnerAdmissionFrozenActorContextV2<'_>,
) -> RuntimeGatewayOwnerAdmissionFrozenStepV2
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
    P::Error: Send,
{
    let RuntimeGatewayOwnerAdmissionFrozenActorContextV2 {
        shutdown_commands,
        shutdown_acknowledgement,
        guard,
        cleanup,
    } = context;
    let now = Instant::now();
    if now >= authority.cutoff_v2() || !authority.accepts_current_v2(watchdog.schedule().receipt())
    {
        return stop_after_admission_frozen_error_v2(
            response,
            if now >= authority.cutoff_v2() {
                RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::DeadlineElapsed
            } else {
                RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation
            },
            if now >= authority.cutoff_v2() {
                RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
            } else {
                RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
            },
            guard,
            cleanup,
        );
    }
    let inflight = match watchdog.begin_current_observation(now) {
        Ok(inflight) => inflight,
        Err(error) => {
            return stop_after_admission_frozen_error_v2(
                response,
                map_admission_frozen_watchdog_error_v2(error),
                map_watchdog_error(error),
                guard,
                cleanup,
            );
        }
    };
    let observation_deadline = inflight
        .previous_schedule()
        .safety_deadline()
        .min(authority.cutoff_v2());
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
                RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::SupervisorUnavailable,
            ));
            return RuntimeGatewayOwnerAdmissionFrozenStepV2::Stop(stop);
        }
        _ = response.closed() => {
            guard.invalidate_now();
            return RuntimeGatewayOwnerAdmissionFrozenStepV2::Stop(
                RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                    cleanup,
                ),
            );
        }
        _ = sleep_until(TokioInstant::from_std(observation_deadline)) => {
            return stop_after_admission_frozen_error_v2(
                response,
                RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::DeadlineElapsed,
                RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                guard,
                cleanup,
            );
        }
        result = &mut observation => result,
    };
    let response_observed_at = Instant::now();
    if response_observed_at >= authority.cutoff_v2() {
        return stop_after_admission_frozen_error_v2(
            response,
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::DeadlineElapsed,
            RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
            guard,
            cleanup,
        );
    }
    match result {
        Ok(observation) => match inflight.complete(observation, response_observed_at) {
            Ok(RuntimeGatewayOwnerObservationCompletionV1::Current(successor)) => {
                let observation =
                    RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&successor);
                if !authority.accepts_observed_v2(observation.receipt()) {
                    return stop_after_admission_frozen_error_v2(
                        response,
                        RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation,
                        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                        guard,
                        cleanup,
                    );
                }
                RuntimeGatewayOwnerAdmissionFrozenStepV2::Frozen {
                    successor,
                    observation,
                    cutoff: authority.cutoff_v2(),
                    response,
                }
            }
            Ok(RuntimeGatewayOwnerObservationCompletionV1::OwnershipLost(_)) => {
                stop_after_admission_frozen_error_v2(
                    response,
                    RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup,
                )
            }
            Err(error) => stop_after_admission_frozen_error_v2(
                response,
                map_admission_frozen_watchdog_error_v2(error),
                map_watchdog_error(error),
                guard,
                cleanup,
            ),
        },
        Err(error) => match P::classify_observation_error(&error) {
            RuntimeGatewayOwnerObservationErrorClassV1::Retryable => {
                stop_after_admission_frozen_error_v2(
                    response,
                    RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ObservationUnavailable,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                    guard,
                    cleanup,
                )
            }
            RuntimeGatewayOwnerObservationErrorClassV1::OwnershipLost => {
                stop_after_admission_frozen_error_v2(
                    response,
                    RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup,
                )
            }
            RuntimeGatewayOwnerObservationErrorClassV1::ProtocolViolation => {
                stop_after_admission_frozen_error_v2(
                    response,
                    RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                    guard,
                    cleanup,
                )
            }
        },
    }
}

fn stop_after_admission_frozen_error_v2(
    response: oneshot::Sender<
        Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2,
        >,
    >,
    response_error: RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2,
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    guard: &mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup: RuntimeGatewayOwnerCleanupBoundV1,
) -> RuntimeGatewayOwnerAdmissionFrozenStepV2 {
    guard.invalidate_now();
    let _result = response.send(Err(response_error));
    RuntimeGatewayOwnerAdmissionFrozenStepV2::Stop(RuntimeGatewayOwnerStartupWatchdogStopV1::new(
        exit, cleanup,
    ))
}

fn map_admission_frozen_watchdog_error_v2(
    error: RuntimeGatewayOwnerWatchdogErrorV1,
) -> RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2 {
    match error {
        RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed,
        ) => RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::DeadlineElapsed,
        RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed
        | RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort
        | RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted
        | RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { .. }
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(_) => {
            RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation
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
