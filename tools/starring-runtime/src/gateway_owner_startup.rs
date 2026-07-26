use std::fmt::{Debug, Formatter};
use std::time::Instant;

use automation_runtime_controller::{
    RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeBuildRevisionV1, RuntimeObserveGatewayOwnerLeaseV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, accept_gateway_owner_observation_v1,
    classify_unknown_gateway_owner_acquire_v1, RuntimeAcceptedGatewayOwnerAcquireV1,
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeGatewayOwnerAcquireRecoveryV1,
    RuntimeGatewayOwnerLeasePortV1, RuntimeGatewayOwnerMutationErrorV1,
    RuntimeGatewayOwnerObservationErrorClassV1,
};
use tokio::time::{sleep_until, Instant as TokioInstant};

use crate::gateway::runtime_gateway_shard_id_v1;
use crate::gateway_owner_startup_watchdog::{
    release_runtime_gateway_owner_until_v1, RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2,
    RuntimeGatewayOwnerPreparedClosedRecoveryV2, RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
};
use crate::{
    GatewayOwnerTimingConfigV1, RuntimeGatewayBootstrapV1,
    RuntimeGatewayOwnerCurrentObservationErrorV1, RuntimeGatewayOwnerCurrentObservationV1,
    RuntimeGatewayOwnerReleaseStatusV1, RuntimeGatewayOwnerStartupWatchdogConfigErrorV1,
    RuntimeGatewayOwnerStartupWatchdogConfigV1, RuntimeGatewayOwnerStartupWatchdogExitV1,
    RuntimeGatewayOwnerStartupWatchdogHandleV1, RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerStartupAcquisitionErrorV1 {
    #[error("runtime gateway owner startup configuration failed")]
    Configuration(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1),
    #[error("runtime gateway owner startup operation deadline elapsed")]
    OperationDeadlineElapsed,
    #[error("runtime gateway owner startup acquisition is unavailable")]
    AcquireUnavailable,
    #[error("runtime gateway owner startup acquisition is unconfirmed")]
    AcquireUnconfirmed,
    #[error("runtime gateway owner startup acquisition is contended")]
    Contended,
    #[error("runtime gateway owner startup acquisition violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner startup cleanup is unconfirmed")]
    CleanupUnconfirmed,
    #[error("runtime gateway owner startup watchdog failed to start")]
    WatchdogStart(RuntimeGatewayOwnerStartupWatchdogStartErrorV1),
    #[error("runtime gateway owner startup watchdog observation failed")]
    WatchdogObservation(RuntimeGatewayOwnerCurrentObservationErrorV1),
    #[error("runtime gateway owner startup watchdog terminated")]
    WatchdogTerminated(RuntimeGatewayOwnerStartupWatchdogExitV1),
}

impl RuntimeGatewayOwnerStartupAcquisitionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Configuration(_) => "runtime_gateway_owner_startup_configuration",
            Self::OperationDeadlineElapsed => {
                "runtime_gateway_owner_startup_operation_deadline_elapsed"
            }
            Self::AcquireUnavailable => "runtime_gateway_owner_startup_acquire_unavailable",
            Self::AcquireUnconfirmed => "runtime_gateway_owner_startup_acquire_unconfirmed",
            Self::Contended => "runtime_gateway_owner_startup_contended",
            Self::ProtocolViolation => "runtime_gateway_owner_startup_protocol_violation",
            Self::CleanupUnconfirmed => "runtime_gateway_owner_startup_cleanup_unconfirmed",
            Self::WatchdogStart(_) => "runtime_gateway_owner_startup_watchdog_start",
            Self::WatchdogObservation(_) => "runtime_gateway_owner_startup_watchdog_observation",
            Self::WatchdogTerminated(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeGatewayOwnerStartupAcquisitionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerStartupAcquisitionErrorV1(<redacted>)")
    }
}

pub(crate) struct RuntimeAcquiredGatewayOwnerV1 {
    watchdog: RuntimeGatewayOwnerStartupWatchdogHandleV1,
    current_observation: RuntimeGatewayOwnerCurrentObservationV1,
}

impl RuntimeAcquiredGatewayOwnerV1 {
    pub(crate) fn terminal_status(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.watchdog.terminal_status()
    }

    pub(crate) async fn wait_terminal(&mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        self.watchdog.wait_terminal().await
    }

    pub(crate) async fn prepare_closed_recovery_in_place_v2(
        &mut self,
    ) -> Result<(), RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2> {
        self.watchdog.prepare_closed_recovery_in_place_v2().await
    }

    pub(crate) fn try_into_prepared_closed_recovery_v2(
        self,
    ) -> Result<RuntimeGatewayOwnerPreparedClosedRecoveryV2, Box<Self>> {
        let Self {
            watchdog,
            current_observation,
        } = self;
        match watchdog.try_into_prepared_closed_recovery_v2() {
            Ok(prepared) => {
                drop(current_observation);
                Ok(prepared)
            }
            Err(watchdog) => Err(Box::new(Self {
                watchdog: *watchdog,
                current_observation,
            })),
        }
    }

    pub(crate) async fn shutdown_until(
        self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        let Self {
            watchdog,
            current_observation,
        } = self;
        let result = watchdog.shutdown_until(cleanup_deadline).await;
        drop(current_observation);
        result
    }
}

impl Debug for RuntimeAcquiredGatewayOwnerV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcquiredGatewayOwnerV1(<redacted>)")
    }
}

struct RuntimeAcceptedGatewayOwnerStartupV1 {
    accepted: RuntimeAcceptedGatewayOwnerReceiptV1,
    request_started_at: Instant,
    response_observed_at: Instant,
}

enum RuntimeGatewayOwnerAcquireAttemptV1 {
    Accepted(RuntimeAcceptedGatewayOwnerStartupV1),
    ReplaySameRequest,
    OutcomeUnknown,
    Contended,
}

enum RuntimeGatewayOwnerUnknownResolutionV1 {
    Accepted(RuntimeAcceptedGatewayOwnerStartupV1),
    ReplaySameRequest,
    Contended,
}

pub(crate) async fn acquire_runtime_gateway_owner_startup_v1<P>(
    gateway: &mut RuntimeGatewayBootstrapV1,
    port: P,
    process_instance_id: &ProcessInstanceId,
    build_revision: &RuntimeBuildRevisionV1,
    timing: GatewayOwnerTimingConfigV1,
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
) -> Result<RuntimeAcquiredGatewayOwnerV1, RuntimeGatewayOwnerStartupAcquisitionErrorV1>
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
    P::Error: Send + 'static,
{
    let config = RuntimeGatewayOwnerStartupWatchdogConfigV1::from_runtime_config(timing)
        .map_err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::Configuration)?;
    let request = RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: runtime_gateway_shard_id_v1(),
        process_instance_id: process_instance_id.clone(),
        expected_build_revision: build_revision.clone(),
        lease_for: config.lease_for(),
    };
    let first =
        acquire_gateway_owner_once_v1(&port, &request, operation_cutoff, cleanup_deadline).await?;
    let accepted = match first {
        RuntimeGatewayOwnerAcquireAttemptV1::Accepted(accepted) => accepted,
        RuntimeGatewayOwnerAcquireAttemptV1::Contended => {
            return Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::Contended);
        }
        RuntimeGatewayOwnerAcquireAttemptV1::ReplaySameRequest => {
            acquire_gateway_owner_second_v1(&port, &request, operation_cutoff, cleanup_deadline)
                .await?
        }
        RuntimeGatewayOwnerAcquireAttemptV1::OutcomeUnknown => {
            match resolve_unknown_gateway_owner_acquire_v1(
                &port,
                &request,
                operation_cutoff,
                cleanup_deadline,
            )
            .await?
            {
                RuntimeGatewayOwnerUnknownResolutionV1::Accepted(accepted) => accepted,
                RuntimeGatewayOwnerUnknownResolutionV1::Contended => {
                    return Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::Contended);
                }
                RuntimeGatewayOwnerUnknownResolutionV1::ReplaySameRequest => {
                    acquire_gateway_owner_second_v1(
                        &port,
                        &request,
                        operation_cutoff,
                        cleanup_deadline,
                    )
                    .await?
                }
            }
        }
    };
    start_and_confirm_gateway_owner_watchdog_v1(
        gateway,
        port,
        accepted,
        config,
        operation_cutoff,
        cleanup_deadline,
    )
    .await
}

async fn acquire_gateway_owner_second_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
) -> Result<RuntimeAcceptedGatewayOwnerStartupV1, RuntimeGatewayOwnerStartupAcquisitionErrorV1>
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    match acquire_gateway_owner_once_v1(port, request, operation_cutoff, cleanup_deadline).await? {
        RuntimeGatewayOwnerAcquireAttemptV1::Accepted(accepted) => Ok(accepted),
        RuntimeGatewayOwnerAcquireAttemptV1::Contended => {
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::Contended)
        }
        RuntimeGatewayOwnerAcquireAttemptV1::ReplaySameRequest => {
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnavailable)
        }
        RuntimeGatewayOwnerAcquireAttemptV1::OutcomeUnknown => {
            match resolve_unknown_gateway_owner_acquire_v1(
                port,
                request,
                operation_cutoff,
                cleanup_deadline,
            )
            .await?
            {
                RuntimeGatewayOwnerUnknownResolutionV1::Accepted(accepted) => Ok(accepted),
                RuntimeGatewayOwnerUnknownResolutionV1::Contended => {
                    Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::Contended)
                }
                RuntimeGatewayOwnerUnknownResolutionV1::ReplaySameRequest => {
                    Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnconfirmed)
                }
            }
        }
    }
}

async fn acquire_gateway_owner_once_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
) -> Result<RuntimeGatewayOwnerAcquireAttemptV1, RuntimeGatewayOwnerStartupAcquisitionErrorV1>
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    if Instant::now() >= operation_cutoff {
        return Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed);
    }
    let request_started_at = Instant::now();
    let result = {
        let acquisition = port.acquire_gateway_owner(request.clone());
        tokio::pin!(acquisition);
        tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(operation_cutoff)) => None,
            result = &mut acquisition => Some(result),
        }
    };
    let response_observed_at = Instant::now();
    let Some(result) = result else {
        return Err(
            operation_deadline_after_unknown_acquire_v1(port, request, cleanup_deadline).await,
        );
    };
    if response_observed_at >= operation_cutoff {
        return Err(cleanup_completed_acquire_after_deadline_v1(
            port,
            request,
            result,
            cleanup_deadline,
        )
        .await);
    }
    match result {
        Ok(outcome) => match accept_gateway_owner_acquire_v1(request, outcome) {
            Ok(RuntimeAcceptedGatewayOwnerAcquireV1::Acquired(accepted)) => {
                if accepted.receipt().database_lease_duration() != Some(request.lease_for.get()) {
                    return Err(protocol_failure_with_known_owner_v1(
                        port,
                        accepted,
                        cleanup_deadline,
                    )
                    .await);
                }
                Ok(RuntimeGatewayOwnerAcquireAttemptV1::Accepted(
                    RuntimeAcceptedGatewayOwnerStartupV1 {
                        accepted,
                        request_started_at,
                        response_observed_at,
                    },
                ))
            }
            Ok(RuntimeAcceptedGatewayOwnerAcquireV1::Contended(_)) => {
                Ok(RuntimeGatewayOwnerAcquireAttemptV1::Contended)
            }
            Err(_) => {
                Err(
                    protocol_failure_after_unknown_acquire_v1(port, request, cleanup_deadline)
                        .await,
                )
            }
        },
        Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied { .. }) => {
            Ok(RuntimeGatewayOwnerAcquireAttemptV1::ReplaySameRequest)
        }
        Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown { .. }) => {
            Ok(RuntimeGatewayOwnerAcquireAttemptV1::OutcomeUnknown)
        }
    }
}

async fn resolve_unknown_gateway_owner_acquire_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
) -> Result<RuntimeGatewayOwnerUnknownResolutionV1, RuntimeGatewayOwnerStartupAcquisitionErrorV1>
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    if Instant::now() >= operation_cutoff {
        return Err(
            operation_deadline_after_unknown_acquire_v1(port, request, cleanup_deadline).await,
        );
    }
    let observation_request = RuntimeObserveGatewayOwnerLeaseV1 {
        gateway_shard_id: request.gateway_shard_id.clone(),
    };
    let request_started_at = Instant::now();
    let result = {
        let observation = port.observe_gateway_owner(observation_request.clone());
        tokio::pin!(observation);
        tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(operation_cutoff)) => None,
            result = &mut observation => Some(result),
        }
    };
    let response_observed_at = Instant::now();
    let Some(result) = result else {
        return Err(
            operation_deadline_after_unknown_acquire_v1(port, request, cleanup_deadline).await,
        );
    };
    if response_observed_at >= operation_cutoff {
        return Err(
            operation_deadline_after_unknown_acquire_v1(port, request, cleanup_deadline).await,
        );
    }
    let observation = match result {
        Ok(observation) => {
            match accept_gateway_owner_observation_v1(&observation_request, observation) {
                Ok(observation) => observation,
                Err(_) => {
                    return Err(protocol_failure_after_unknown_acquire_v1(
                        port,
                        request,
                        cleanup_deadline,
                    )
                    .await);
                }
            }
        }
        Err(error) => {
            let original = match P::classify_observation_error(&error) {
                RuntimeGatewayOwnerObservationErrorClassV1::ProtocolViolation => {
                    RuntimeGatewayOwnerStartupAcquisitionErrorV1::ProtocolViolation
                }
                RuntimeGatewayOwnerObservationErrorClassV1::Retryable
                | RuntimeGatewayOwnerObservationErrorClassV1::OwnershipLost => {
                    RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnconfirmed
                }
            };
            return Err(failure_after_unknown_acquire_v1(
                port,
                request,
                cleanup_deadline,
                original,
            )
            .await);
        }
    };
    match classify_unknown_gateway_owner_acquire_v1(request, observation) {
        RuntimeGatewayOwnerAcquireRecoveryV1::Adopt(accepted) => {
            Ok(RuntimeGatewayOwnerUnknownResolutionV1::Accepted(
                RuntimeAcceptedGatewayOwnerStartupV1 {
                    accepted,
                    request_started_at,
                    response_observed_at,
                },
            ))
        }
        RuntimeGatewayOwnerAcquireRecoveryV1::ReplaySameRequest => {
            Ok(RuntimeGatewayOwnerUnknownResolutionV1::ReplaySameRequest)
        }
        RuntimeGatewayOwnerAcquireRecoveryV1::Contended(_) => {
            Ok(RuntimeGatewayOwnerUnknownResolutionV1::Contended)
        }
        RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation => {
            Err(protocol_failure_after_unknown_acquire_v1(port, request, cleanup_deadline).await)
        }
    }
}

async fn start_and_confirm_gateway_owner_watchdog_v1<P>(
    gateway: &mut RuntimeGatewayBootstrapV1,
    port: P,
    accepted: RuntimeAcceptedGatewayOwnerStartupV1,
    config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
) -> Result<RuntimeAcquiredGatewayOwnerV1, RuntimeGatewayOwnerStartupAcquisitionErrorV1>
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
    P::Error: Send + 'static,
{
    if Instant::now() >= operation_cutoff {
        let release = release_runtime_gateway_owner_until_v1(
            &port,
            accepted.accepted.receipt().lease_id.clone(),
            cleanup_deadline,
        )
        .await;
        return Err(deadline_or_cleanup_error_v1(release));
    }
    let expected_lease_id = accepted.accepted.receipt().lease_id.clone();
    let watchdog = match gateway.start_bounded_gateway_owner_startup_watchdog_v1(
        port,
        accepted.accepted,
        accepted.request_started_at,
        accepted.response_observed_at,
        config,
        cleanup_deadline,
    ) {
        Ok(watchdog) => watchdog,
        Err(failure) => {
            let reason = failure.reason();
            let cleanup = failure.cleanup_until(cleanup_deadline).await;
            return Err(match cleanup {
                RuntimeGatewayOwnerReleaseStatusV1::Confirmed => {
                    RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogStart(reason)
                }
                RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed
                | RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation => {
                    RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed
                }
            });
        }
    };
    if Instant::now() >= operation_cutoff {
        return Err(shutdown_watchdog_after_deadline_v1(watchdog, cleanup_deadline).await);
    }
    let observation = {
        let observation = watchdog.observe_current_gateway_owner_v1();
        tokio::pin!(observation);
        tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(operation_cutoff)) => None,
            result = &mut observation => Some((result, Instant::now())),
        }
    };
    let Some((observation, response_observed_at)) = observation else {
        return Err(shutdown_watchdog_after_deadline_v1(watchdog, cleanup_deadline).await);
    };
    if response_observed_at >= operation_cutoff {
        return Err(shutdown_watchdog_after_deadline_v1(watchdog, cleanup_deadline).await);
    }
    let current_observation = match observation {
        Ok(observation) if observation.receipt().lease_id == expected_lease_id => observation,
        Ok(_) => {
            return Err(shutdown_watchdog_after_failure_v1(
                watchdog,
                cleanup_deadline,
                RuntimeGatewayOwnerStartupAcquisitionErrorV1::ProtocolViolation,
            )
            .await);
        }
        Err(error) => {
            return Err(shutdown_watchdog_after_failure_v1(
                watchdog,
                cleanup_deadline,
                RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogObservation(error),
            )
            .await);
        }
    };
    Ok(RuntimeAcquiredGatewayOwnerV1 {
        watchdog,
        current_observation,
    })
}

async fn cleanup_completed_acquire_after_deadline_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    result: Result<
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
        RuntimeGatewayOwnerMutationErrorV1<P::Error>,
    >,
    cleanup_deadline: Instant,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    match result {
        Ok(outcome) => match accept_gateway_owner_acquire_v1(request, outcome) {
            Ok(RuntimeAcceptedGatewayOwnerAcquireV1::Acquired(accepted)) => {
                deadline_or_cleanup_error_v1(
                    release_runtime_gateway_owner_until_v1(
                        port,
                        accepted.receipt().lease_id.clone(),
                        cleanup_deadline,
                    )
                    .await,
                )
            }
            Ok(RuntimeAcceptedGatewayOwnerAcquireV1::Contended(_)) => {
                RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed
            }
            Err(_) => {
                operation_deadline_after_unknown_acquire_v1(port, request, cleanup_deadline).await
            }
        },
        Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied { .. }) => {
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed
        }
        Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown { .. }) => {
            operation_deadline_after_unknown_acquire_v1(port, request, cleanup_deadline).await
        }
    }
}

async fn protocol_failure_with_known_owner_v1<P>(
    port: &P,
    accepted: RuntimeAcceptedGatewayOwnerReceiptV1,
    cleanup_deadline: Instant,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    match release_runtime_gateway_owner_until_v1(
        port,
        accepted.receipt().lease_id.clone(),
        cleanup_deadline,
    )
    .await
    {
        RuntimeGatewayOwnerReleaseStatusV1::Confirmed => {
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::ProtocolViolation
        }
        RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed
        | RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation => {
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed
        }
    }
}

async fn protocol_failure_after_unknown_acquire_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    cleanup_deadline: Instant,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    failure_after_unknown_acquire_v1(
        port,
        request,
        cleanup_deadline,
        RuntimeGatewayOwnerStartupAcquisitionErrorV1::ProtocolViolation,
    )
    .await
}

async fn operation_deadline_after_unknown_acquire_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    cleanup_deadline: Instant,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    failure_after_unknown_acquire_v1(
        port,
        request,
        cleanup_deadline,
        RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed,
    )
    .await
}

async fn failure_after_unknown_acquire_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    cleanup_deadline: Instant,
    confirmed_error: RuntimeGatewayOwnerStartupAcquisitionErrorV1,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    if cleanup_unknown_gateway_owner_acquire_v1(port, request, cleanup_deadline).await {
        confirmed_error
    } else {
        RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed
    }
}

async fn cleanup_unknown_gateway_owner_acquire_v1<P>(
    port: &P,
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    cleanup_deadline: Instant,
) -> bool
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    if Instant::now() >= cleanup_deadline {
        return false;
    }
    let observation_request = RuntimeObserveGatewayOwnerLeaseV1 {
        gateway_shard_id: request.gateway_shard_id.clone(),
    };
    let observation = port.observe_gateway_owner(observation_request.clone());
    tokio::pin!(observation);
    let result = tokio::select! {
        biased;
        _ = sleep_until(TokioInstant::from_std(cleanup_deadline)) => None,
        result = &mut observation => Some(result),
    };
    let Some(Ok(observation)) = result else {
        return false;
    };
    if Instant::now() >= cleanup_deadline {
        return false;
    }
    let Ok(observation) = accept_gateway_owner_observation_v1(&observation_request, observation)
    else {
        return false;
    };
    match classify_unknown_gateway_owner_acquire_v1(request, observation) {
        RuntimeGatewayOwnerAcquireRecoveryV1::Adopt(accepted) => matches!(
            release_runtime_gateway_owner_until_v1(
                port,
                accepted.receipt().lease_id.clone(),
                cleanup_deadline,
            )
            .await,
            RuntimeGatewayOwnerReleaseStatusV1::Confirmed
        ),
        RuntimeGatewayOwnerAcquireRecoveryV1::ReplaySameRequest
        | RuntimeGatewayOwnerAcquireRecoveryV1::Contended(_) => true,
        RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation => false,
    }
}

async fn shutdown_watchdog_after_deadline_v1(
    watchdog: RuntimeGatewayOwnerStartupWatchdogHandleV1,
    cleanup_deadline: Instant,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1 {
    match watchdog.shutdown_until(cleanup_deadline).await {
        Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed)
        | Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation)
        | Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped)
        | Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed) => {
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed
        }
        Ok(_) => RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed,
    }
}

async fn shutdown_watchdog_after_failure_v1(
    watchdog: RuntimeGatewayOwnerStartupWatchdogHandleV1,
    cleanup_deadline: Instant,
    original: RuntimeGatewayOwnerStartupAcquisitionErrorV1,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1 {
    match watchdog.shutdown_until(cleanup_deadline).await {
        Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown) => original,
        Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed)
        | Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation)
        | Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped)
        | Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed) => {
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed
        }
        Ok(exit) => RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogTerminated(exit),
    }
}

fn deadline_or_cleanup_error_v1(
    cleanup: RuntimeGatewayOwnerReleaseStatusV1,
) -> RuntimeGatewayOwnerStartupAcquisitionErrorV1 {
    match cleanup {
        RuntimeGatewayOwnerReleaseStatusV1::Confirmed => {
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed
        }
        RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed
        | RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation => {
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::{pending, Future};
    use std::num::NonZeroU64;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use automation_runtime_controller::{
        RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseObservationV1,
        RuntimeGatewayOwnerLeaseReceiptV1, RuntimeObservedGatewayOwnerLeaseV1,
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
        RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
    };
    use automation_runtime_worker::RuntimeGatewayClosedSnapshotV2;
    use chrono::{TimeDelta, Utc};

    use super::*;
    use crate::{compose_runtime_gateway_bootstrap_v1, GatewayResourceConfigV1};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FakePortErrorV1 {
        Transport,
    }

    #[derive(Clone, Copy)]
    enum FakeAcquireStepV1 {
        Acquired,
        Contended,
        DefinitelyNotApplied,
        OutcomeUnknown,
        PendingUnknown,
    }

    #[derive(Clone, Copy)]
    enum FakeObserveStepV1 {
        Current,
        Unowned,
        Foreign,
        Transport,
        Pending,
        Panic,
    }

    #[derive(Clone)]
    struct FakeGatewayOwnerPortV1 {
        state: Arc<FakeGatewayOwnerPortStateV1>,
    }

    struct FakeGatewayOwnerPortStateV1 {
        local_receipt: Mutex<RuntimeGatewayOwnerLeaseReceiptV1>,
        foreign_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        acquire_steps: Mutex<VecDeque<FakeAcquireStepV1>>,
        observe_steps: Mutex<VecDeque<FakeObserveStepV1>>,
        acquire_requests: Mutex<Vec<RuntimeAcquireGatewayOwnerLeaseV1>>,
        observe_requests: Mutex<Vec<RuntimeObserveGatewayOwnerLeaseV1>>,
        release_requests: Mutex<Vec<RuntimeReleaseGatewayOwnerLeaseV1>>,
        active_acquires: AtomicUsize,
        active_observations: AtomicUsize,
        active_releases: AtomicUsize,
        overlapping_cleanup: AtomicBool,
        block_releases: AtomicBool,
    }

    impl FakeGatewayOwnerPortV1 {
        fn new(
            local_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
            acquire_steps: impl IntoIterator<Item = FakeAcquireStepV1>,
            observe_steps: impl IntoIterator<Item = FakeObserveStepV1>,
        ) -> Self {
            let foreign_receipt = receipt(
                "runtime-process:foreign",
                local_receipt.lease_id.expected_build_revision.as_str(),
                local_receipt.database_lease_duration().unwrap(),
            );
            Self {
                state: Arc::new(FakeGatewayOwnerPortStateV1 {
                    local_receipt: Mutex::new(local_receipt),
                    foreign_receipt,
                    acquire_steps: Mutex::new(acquire_steps.into_iter().collect()),
                    observe_steps: Mutex::new(observe_steps.into_iter().collect()),
                    acquire_requests: Mutex::new(Vec::new()),
                    observe_requests: Mutex::new(Vec::new()),
                    release_requests: Mutex::new(Vec::new()),
                    active_acquires: AtomicUsize::new(0),
                    active_observations: AtomicUsize::new(0),
                    active_releases: AtomicUsize::new(0),
                    overlapping_cleanup: AtomicBool::new(false),
                    block_releases: AtomicBool::new(false),
                }),
            }
        }

        fn acquire_requests(&self) -> Vec<RuntimeAcquireGatewayOwnerLeaseV1> {
            self.state
                .acquire_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn observe_calls(&self) -> usize {
            self.state
                .observe_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }

        fn observe_requests(&self) -> Vec<RuntimeObserveGatewayOwnerLeaseV1> {
            self.state
                .observe_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn release_requests(&self) -> Vec<RuntimeReleaseGatewayOwnerLeaseV1> {
            self.state
                .release_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn overlapping_cleanup(&self) -> bool {
            self.state.overlapping_cleanup.load(Ordering::Acquire)
        }

        fn block_releases(&self) {
            self.state.block_releases.store(true, Ordering::Release);
        }

        fn active_releases(&self) -> usize {
            self.state.active_releases.load(Ordering::Acquire)
        }
    }

    #[derive(Clone, Copy)]
    enum FakeActiveOperationKindV1 {
        Acquire,
        Observe,
        Release,
    }

    struct FakeActiveOperationV1 {
        state: Arc<FakeGatewayOwnerPortStateV1>,
        kind: FakeActiveOperationKindV1,
    }

    impl FakeActiveOperationV1 {
        fn begin(state: Arc<FakeGatewayOwnerPortStateV1>, kind: FakeActiveOperationKindV1) -> Self {
            match kind {
                FakeActiveOperationKindV1::Acquire => {
                    state.active_acquires.fetch_add(1, Ordering::AcqRel);
                }
                FakeActiveOperationKindV1::Observe => {
                    state.active_observations.fetch_add(1, Ordering::AcqRel);
                }
                FakeActiveOperationKindV1::Release => {
                    state.active_releases.fetch_add(1, Ordering::AcqRel);
                }
            }
            Self { state, kind }
        }
    }

    impl Drop for FakeActiveOperationV1 {
        fn drop(&mut self) {
            match self.kind {
                FakeActiveOperationKindV1::Acquire => {
                    self.state.active_acquires.fetch_sub(1, Ordering::AcqRel);
                }
                FakeActiveOperationKindV1::Observe => {
                    self.state
                        .active_observations
                        .fetch_sub(1, Ordering::AcqRel);
                }
                FakeActiveOperationKindV1::Release => {
                    self.state.active_releases.fetch_sub(1, Ordering::AcqRel);
                }
            }
        }
    }

    impl RuntimeGatewayOwnerLeasePortV1 for FakeGatewayOwnerPortV1 {
        type Error = FakePortErrorV1;

        fn classify_observation_error(
            _error: &Self::Error,
        ) -> RuntimeGatewayOwnerObservationErrorClassV1 {
            RuntimeGatewayOwnerObservationErrorClassV1::Retryable
        }

        fn observe_gateway_owner(
            &self,
            request: RuntimeObserveGatewayOwnerLeaseV1,
        ) -> impl Future<Output = Result<RuntimeGatewayOwnerLeaseObservationV1, Self::Error>> + Send
        {
            let state = self.state.clone();
            async move {
                if state.active_acquires.load(Ordering::Acquire) != 0
                    || state.active_observations.load(Ordering::Acquire) != 0
                {
                    state.overlapping_cleanup.store(true, Ordering::Release);
                }
                let _active =
                    FakeActiveOperationV1::begin(state.clone(), FakeActiveOperationKindV1::Observe);
                state
                    .observe_requests
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(request);
                let step = state
                    .observe_steps
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .pop_front()
                    .unwrap_or(FakeObserveStepV1::Current);
                match step {
                    FakeObserveStepV1::Current => {
                        Ok(RuntimeGatewayOwnerLeaseObservationV1::Owned(observed(
                            &state
                                .local_receipt
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner),
                        )))
                    }
                    FakeObserveStepV1::Unowned => {
                        Ok(RuntimeGatewayOwnerLeaseObservationV1::Unowned {
                            gateway_shard_id: runtime_gateway_shard_id_v1(),
                            database_now: Utc::now(),
                        })
                    }
                    FakeObserveStepV1::Foreign => Ok(RuntimeGatewayOwnerLeaseObservationV1::Owned(
                        observed(&state.foreign_receipt),
                    )),
                    FakeObserveStepV1::Transport => Err(FakePortErrorV1::Transport),
                    FakeObserveStepV1::Pending => pending().await,
                    FakeObserveStepV1::Panic => panic!("fake gateway owner observation panic"),
                }
            }
        }

        fn acquire_gateway_owner(
            &self,
            request: RuntimeAcquireGatewayOwnerLeaseV1,
        ) -> impl Future<
            Output = Result<
                RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
                RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
            >,
        > + Send {
            let state = self.state.clone();
            async move {
                let _active =
                    FakeActiveOperationV1::begin(state.clone(), FakeActiveOperationKindV1::Acquire);
                state
                    .acquire_requests
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(request);
                let step = state
                    .acquire_steps
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .pop_front()
                    .unwrap_or(FakeAcquireStepV1::DefinitelyNotApplied);
                match step {
                    FakeAcquireStepV1::Acquired => {
                        Ok(RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(
                            state
                                .local_receipt
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone(),
                        ))
                    }
                    FakeAcquireStepV1::Contended => {
                        Ok(RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(
                            state.foreign_receipt.clone(),
                        ))
                    }
                    FakeAcquireStepV1::DefinitelyNotApplied => {
                        Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied {
                            source: FakePortErrorV1::Transport,
                        })
                    }
                    FakeAcquireStepV1::OutcomeUnknown => {
                        Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown {
                            source: FakePortErrorV1::Transport,
                        })
                    }
                    FakeAcquireStepV1::PendingUnknown => pending().await,
                }
            }
        }

        fn renew_gateway_owner(
            &self,
            request: RuntimeRenewGatewayOwnerLeaseV1,
        ) -> impl Future<
            Output = Result<
                RuntimeRenewGatewayOwnerLeaseOutcomeV1,
                RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
            >,
        > + Send {
            let state = self.state.clone();
            async move {
                let mut receipt = state
                    .local_receipt
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                receipt.owner_revision =
                    NonZeroU64::new(request.expected_owner_revision.get() + 1).unwrap();
                receipt.database_now = Utc::now();
                receipt.expires_at =
                    receipt.database_now + TimeDelta::from_std(request.lease_for.get()).unwrap();
                Ok(RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(
                    receipt.clone(),
                ))
            }
        }

        fn release_gateway_owner(
            &self,
            request: RuntimeReleaseGatewayOwnerLeaseV1,
        ) -> impl Future<
            Output = Result<
                RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
                RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
            >,
        > + Send {
            let state = self.state.clone();
            async move {
                let _active =
                    FakeActiveOperationV1::begin(state.clone(), FakeActiveOperationKindV1::Release);
                state
                    .release_requests
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(request.clone());
                if state.block_releases.load(Ordering::Acquire) {
                    pending().await
                }
                Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
                    lease_id: request.lease_id,
                    database_now: Utc::now(),
                })
            }
        }
    }

    fn process_instance_id(value: &str) -> ProcessInstanceId {
        ProcessInstanceId::parse(value).unwrap()
    }

    fn build_revision() -> RuntimeBuildRevisionV1 {
        RuntimeBuildRevisionV1::parse("0123456789abcdef0123456789abcdef01234567").unwrap()
    }

    fn timing() -> GatewayOwnerTimingConfigV1 {
        GatewayOwnerTimingConfigV1::from_parts_for_test_v1(
            Duration::from_secs(5),
            Duration::from_secs(2),
            Duration::from_millis(500),
        )
    }

    fn receipt(
        process: &str,
        revision: &str,
        lease_for: Duration,
    ) -> RuntimeGatewayOwnerLeaseReceiptV1 {
        let database_now = Utc::now();
        RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: runtime_gateway_shard_id_v1(),
                process_instance_id: process_instance_id(process),
                lease_epoch: NonZeroU64::new(1).unwrap(),
                expected_build_revision: RuntimeBuildRevisionV1::parse(revision).unwrap(),
            },
            owner_revision: NonZeroU64::new(1).unwrap(),
            database_now,
            expires_at: database_now + TimeDelta::from_std(lease_for).unwrap(),
        }
    }

    fn observed(receipt: &RuntimeGatewayOwnerLeaseReceiptV1) -> RuntimeObservedGatewayOwnerLeaseV1 {
        RuntimeObservedGatewayOwnerLeaseV1 {
            lease_id: receipt.lease_id.clone(),
            owner_revision: receipt.owner_revision,
            observed_database_now: receipt.database_now,
            expires_at: receipt.expires_at,
        }
    }

    fn local_receipt() -> RuntimeGatewayOwnerLeaseReceiptV1 {
        receipt(
            "runtime-process:owner-startup",
            build_revision().as_str(),
            timing().lease_for(),
        )
    }

    fn assert_exact_observation_shards(port: &FakeGatewayOwnerPortV1) {
        let requests = port.observe_requests();
        assert!(!requests.is_empty());
        assert!(requests
            .iter()
            .all(|request| request.gateway_shard_id == runtime_gateway_shard_id_v1()));
    }

    async fn acquire_with_port(
        gateway_process: &ProcessInstanceId,
        request_process: &ProcessInstanceId,
        port: FakeGatewayOwnerPortV1,
        operation_cutoff: Instant,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeAcquiredGatewayOwnerV1, RuntimeGatewayOwnerStartupAcquisitionErrorV1> {
        let mut gateway = compose_runtime_gateway_bootstrap_v1(
            gateway_process.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let result = acquire_runtime_gateway_owner_startup_v1(
            &mut gateway,
            port,
            request_process,
            &build_revision(),
            timing(),
            operation_cutoff,
            cleanup_deadline,
        )
        .await;
        assert!(matches!(
            gateway.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency { .. }
        ));
        result
    }

    #[tokio::test]
    async fn direct_acquire_observes_once_and_releases_the_exact_lease() {
        let process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [FakeAcquireStepV1::Acquired],
            [FakeObserveStepV1::Current],
        );
        let owner = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();

        let requests = port.acquire_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].gateway_shard_id, runtime_gateway_shard_id_v1());
        assert_eq!(requests[0].process_instance_id, process);
        assert_eq!(requests[0].expected_build_revision, build_revision());
        assert_eq!(requests[0].lease_for.get(), timing().lease_for());
        assert_eq!(port.observe_calls(), 1);
        assert_eq!(
            port.observe_requests(),
            [RuntimeObserveGatewayOwnerLeaseV1 {
                gateway_shard_id: runtime_gateway_shard_id_v1(),
            }]
        );
        assert_eq!(
            format!("{owner:?}"),
            "RuntimeAcquiredGatewayOwnerV1(<redacted>)"
        );
        assert_eq!(
            owner
                .shutdown_until(Instant::now() + Duration::from_secs(1))
                .await,
            Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown)
        );
        assert_eq!(
            port.release_requests()
                .into_iter()
                .map(|request| request.lease_id)
                .collect::<Vec<_>>(),
            [receipt.lease_id]
        );
    }

    #[tokio::test]
    async fn unknown_acquire_adopts_exact_local_observation_without_replay() {
        let process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [FakeAcquireStepV1::OutcomeUnknown],
            [FakeObserveStepV1::Current, FakeObserveStepV1::Current],
        );
        let owner = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();

        assert_eq!(port.acquire_requests().len(), 1);
        assert_eq!(port.observe_calls(), 2);
        assert_exact_observation_shards(&port);
        assert_eq!(
            owner
                .shutdown_until(Instant::now() + Duration::from_secs(1))
                .await,
            Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown)
        );
        assert_eq!(port.release_requests()[0].lease_id, receipt.lease_id);
    }

    #[tokio::test]
    async fn unknown_unowned_replays_the_identical_request_once() {
        let process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [
                FakeAcquireStepV1::OutcomeUnknown,
                FakeAcquireStepV1::Acquired,
            ],
            [FakeObserveStepV1::Unowned, FakeObserveStepV1::Current],
        );
        let owner = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();

        let requests = port.acquire_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        assert_eq!(
            owner
                .shutdown_until(Instant::now() + Duration::from_secs(1))
                .await,
            Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown)
        );
        assert_exact_observation_shards(&port);
        assert_eq!(port.release_requests()[0].lease_id, receipt.lease_id);
    }

    #[tokio::test]
    async fn second_unknown_unowned_stops_without_a_third_mutation_or_release() {
        let process = process_instance_id("runtime-process:owner-startup");
        let port = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [
                FakeAcquireStepV1::OutcomeUnknown,
                FakeAcquireStepV1::OutcomeUnknown,
            ],
            [FakeObserveStepV1::Unowned, FakeObserveStepV1::Unowned],
        );
        let result = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnconfirmed)
        ));
        assert_eq!(port.acquire_requests().len(), 2);
        assert_eq!(port.observe_calls(), 2);
        assert!(port.release_requests().is_empty());
    }

    #[tokio::test]
    async fn definitely_not_applied_replay_matrix_never_exceeds_two_mutations() {
        let process = process_instance_id("runtime-process:owner-startup");

        let acquired = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [
                FakeAcquireStepV1::DefinitelyNotApplied,
                FakeAcquireStepV1::Acquired,
            ],
            [FakeObserveStepV1::Current],
        );
        let owner = acquire_with_port(
            &process,
            &process,
            acquired.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(acquired.acquire_requests().len(), 2);
        assert_eq!(
            acquired.acquire_requests()[0],
            acquired.acquire_requests()[1]
        );
        assert_eq!(
            owner
                .shutdown_until(Instant::now() + Duration::from_secs(1))
                .await,
            Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown)
        );

        let unavailable = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [
                FakeAcquireStepV1::DefinitelyNotApplied,
                FakeAcquireStepV1::DefinitelyNotApplied,
            ],
            std::iter::empty(),
        );
        let result = acquire_with_port(
            &process,
            &process,
            unavailable.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await;
        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnavailable)
        ));
        assert_eq!(unavailable.acquire_requests().len(), 2);
        assert_eq!(
            unavailable.acquire_requests()[0],
            unavailable.acquire_requests()[1]
        );
        assert_eq!(unavailable.observe_calls(), 0);
        assert!(unavailable.release_requests().is_empty());

        let unconfirmed = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [
                FakeAcquireStepV1::DefinitelyNotApplied,
                FakeAcquireStepV1::OutcomeUnknown,
            ],
            [FakeObserveStepV1::Unowned],
        );
        let result = acquire_with_port(
            &process,
            &process,
            unconfirmed.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await;
        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnconfirmed)
        ));
        assert_eq!(unconfirmed.acquire_requests().len(), 2);
        assert_eq!(
            unconfirmed.acquire_requests()[0],
            unconfirmed.acquire_requests()[1]
        );
        assert_eq!(unconfirmed.observe_calls(), 1);
        assert!(unconfirmed.release_requests().is_empty());
    }

    #[tokio::test]
    async fn foreign_contention_never_releases_foreign_authority() {
        let process = process_instance_id("runtime-process:owner-startup");
        for (acquire, observe) in [
            (FakeAcquireStepV1::Contended, None),
            (
                FakeAcquireStepV1::OutcomeUnknown,
                Some(FakeObserveStepV1::Foreign),
            ),
        ] {
            let port = FakeGatewayOwnerPortV1::new(local_receipt(), [acquire], observe.into_iter());
            let result = acquire_with_port(
                &process,
                &process,
                port.clone(),
                Instant::now() + Duration::from_secs(1),
                Instant::now() + Duration::from_secs(2),
            )
            .await;

            assert!(matches!(
                result,
                Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::Contended)
            ));
            assert!(port.release_requests().is_empty());
        }
    }

    #[tokio::test]
    async fn canceled_acquire_exact_observes_and_releases_during_cleanup_tail() {
        let process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [FakeAcquireStepV1::PendingUnknown],
            [FakeObserveStepV1::Current],
        );
        let result = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_millis(10),
            Instant::now() + Duration::from_secs(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed)
        ));
        assert_eq!(port.observe_calls(), 1);
        assert_exact_observation_shards(&port);
        assert_eq!(port.release_requests().len(), 1);
        assert_eq!(port.release_requests()[0].lease_id, receipt.lease_id);
        assert!(!port.overlapping_cleanup());
    }

    #[tokio::test]
    async fn canceled_acquire_cleanup_never_releases_a_foreign_owner() {
        let process = process_instance_id("runtime-process:owner-startup");
        let port = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [FakeAcquireStepV1::PendingUnknown],
            [FakeObserveStepV1::Foreign],
        );
        let result = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_millis(10),
            Instant::now() + Duration::from_secs(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed)
        ));
        assert_eq!(port.observe_calls(), 1);
        assert_exact_observation_shards(&port);
        assert!(port.release_requests().is_empty());
        assert!(!port.overlapping_cleanup());
    }

    #[tokio::test]
    async fn canceled_unknown_observation_is_dropped_before_cleanup_observation() {
        let process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [FakeAcquireStepV1::OutcomeUnknown],
            [FakeObserveStepV1::Pending, FakeObserveStepV1::Current],
        );
        let result = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_millis(10),
            Instant::now() + Duration::from_secs(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed)
        ));
        assert_eq!(port.observe_calls(), 2);
        assert_exact_observation_shards(&port);
        assert_eq!(port.release_requests().len(), 1);
        assert_eq!(port.release_requests()[0].lease_id, receipt.lease_id);
        assert!(!port.overlapping_cleanup());
    }

    #[tokio::test]
    async fn watchdog_start_failure_releases_before_returning() {
        let gateway_process = process_instance_id("runtime-process:different-gateway");
        let request_process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [FakeAcquireStepV1::Acquired],
            std::iter::empty(),
        );
        let result = acquire_with_port(
            &gateway_process,
            &request_process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogStart(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ProcessMismatch,
            ))
        ));
        assert_eq!(port.release_requests()[0].lease_id, receipt.lease_id);
    }

    #[tokio::test]
    async fn watchdog_observation_failure_releases_before_returning() {
        let process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [FakeAcquireStepV1::Acquired],
            [FakeObserveStepV1::Transport],
        );
        let result = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await;

        assert!(matches!(
            result,
            Err(
                RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogObservation(
                    RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable,
                ),
            )
        ));
        assert_eq!(port.release_requests()[0].lease_id, receipt.lease_id);
    }

    #[tokio::test]
    async fn stopped_watchdog_task_is_cleanup_unconfirmed() {
        let process = process_instance_id("runtime-process:owner-startup");
        let port = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [FakeAcquireStepV1::Acquired],
            [FakeObserveStepV1::Panic],
        );
        let result = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed)
        ));
        assert!(port.release_requests().is_empty());
    }

    #[tokio::test]
    async fn absolute_cleanup_cap_terminates_a_blocked_watchdog_release() {
        let process = process_instance_id("runtime-process:owner-startup");
        let receipt = local_receipt();
        let port = FakeGatewayOwnerPortV1::new(
            receipt.clone(),
            [FakeAcquireStepV1::Acquired],
            [FakeObserveStepV1::Current],
        );
        let cleanup_deadline = Instant::now() + Duration::from_millis(300);
        let owner = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_millis(100),
            cleanup_deadline,
        )
        .await
        .unwrap();
        port.block_releases();

        let shutdown = tokio::time::timeout(
            Duration::from_secs(1),
            owner.shutdown_until(cleanup_deadline),
        )
        .await
        .unwrap();

        assert_eq!(
            shutdown,
            Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed)
        );
        assert_eq!(port.release_requests()[0].lease_id, receipt.lease_id);
    }

    #[tokio::test]
    async fn elapsed_shutdown_deadline_aborts_and_joins_the_watchdog_task() {
        let process = process_instance_id("runtime-process:owner-startup");
        let port = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [FakeAcquireStepV1::Acquired],
            [FakeObserveStepV1::Current],
        );
        let owner = acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();

        assert_eq!(
            owner.shutdown_until(Instant::now()).await,
            Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed)
        );
        tokio::task::yield_now().await;
        assert!(port.release_requests().is_empty());
    }

    #[tokio::test]
    async fn canceled_startup_observation_retains_the_absolute_cleanup_cap() {
        let process = process_instance_id("runtime-process:owner-startup");
        let port = FakeGatewayOwnerPortV1::new(
            local_receipt(),
            [FakeAcquireStepV1::Acquired],
            [FakeObserveStepV1::Pending],
        );
        port.block_releases();
        let cleanup_deadline = Instant::now() + Duration::from_millis(300);
        let mut acquisition = Box::pin(acquire_with_port(
            &process,
            &process,
            port.clone(),
            Instant::now() + Duration::from_millis(100),
            cleanup_deadline,
        ));
        tokio::time::timeout(Duration::from_millis(80), async {
            loop {
                tokio::select! {
                    result = &mut acquisition => {
                        panic!("startup acquisition completed before cancellation: {result:?}")
                    }
                    _ = tokio::time::sleep(Duration::from_millis(1)) => {
                        if port.observe_calls() == 1 {
                            break;
                        }
                    }
                }
            }
        })
        .await
        .unwrap();

        drop(acquisition);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if port.active_releases() == 0 && !port.release_requests().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(Instant::now() <= cleanup_deadline + Duration::from_millis(100));
        assert_eq!(port.release_requests().len(), 1);
    }

    #[test]
    fn public_errors_have_finite_codes_and_redacted_diagnostics() {
        let errors = [
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::Configuration(
                RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidPolicy,
            ),
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed,
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnavailable,
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::AcquireUnconfirmed,
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::Contended,
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::ProtocolViolation,
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed,
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogStart(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::SafetyElapsed,
            ),
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogObservation(
                RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost,
            ),
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::WatchdogTerminated(
                RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
            ),
        ];

        for error in errors {
            assert!(!error.code().is_empty());
            assert_eq!(error.context(), None);
            assert!(!error.to_string().is_empty());
            assert_eq!(
                format!("{error:?}"),
                "RuntimeGatewayOwnerStartupAcquisitionErrorV1(<redacted>)"
            );
            assert!(std::error::Error::source(&error).is_none());
        }
    }
}
