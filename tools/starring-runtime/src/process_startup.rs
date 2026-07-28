use std::fmt::{Debug, Formatter};

use crate::build_revision::bootstrap_compiled_runtime_build_revision_v1;
use crate::process::{
    compose_runtime_process_foundation_v1, RuntimeOwnerHeldProcessShutdownErrorV1,
    RuntimeProcessFoundationCompositionErrorV1, RuntimeProcessGatewayOwnerTransitionErrorV1,
};
use crate::secret::{resolve_runtime_secrets_until_v1, RuntimeSecretsStartupResolutionErrorV1};
use crate::startup::{
    run_runtime_startup_sync_stage_v1, RuntimeStartupBudgetV1, RuntimeStartupSyncStageErrorV1,
};
use crate::{
    RuntimeBuildRevisionBootstrapErrorV1, RuntimeClosedRecoveryProcessCleanupFailureV2,
    RuntimeClosedRecoveryProcessShutdownErrorV2, RuntimeConfigErrorV1, RuntimeConfigV1,
    RuntimePausedConnectedProcessShutdownErrorV1, RuntimeProcessClosedRecoveryTransitionErrorV2,
    RuntimeProcessPausedConnectedTransitionErrorV1, RuntimeProcessProductionHandoffErrorV2,
    RuntimeProcessRecoveryPendingTransitionErrorV2,
    RuntimeProcessRecoveryReadinessTransitionErrorV2, RuntimeProcessStartupRecoveryLoopErrorV2,
    RuntimeRecoveryPendingProcessShutdownErrorV2, RuntimeSecretsResolutionErrorV1,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessStagingErrorV1 {
    #[error("runtime asynchronous executor is unavailable")]
    AsyncRuntimeUnavailable,
    #[error("runtime process startup operation deadline elapsed")]
    OperationDeadlineElapsed,
    #[error("runtime process configuration failed")]
    Configuration(RuntimeConfigErrorV1),
    #[error("runtime process secret resolution failed")]
    Secrets(RuntimeSecretsResolutionErrorV1),
    #[error("runtime process build revision bootstrap failed")]
    BuildRevision(RuntimeBuildRevisionBootstrapErrorV1),
    #[error("runtime process foundation composition failed")]
    Foundation(RuntimeProcessFoundationCompositionErrorV1),
    #[error("runtime process gateway owner transition failed")]
    GatewayOwner(RuntimeProcessGatewayOwnerTransitionErrorV1),
    #[error("runtime process paused-connected transition failed")]
    PausedConnected(RuntimeProcessPausedConnectedTransitionErrorV1),
    #[error("runtime process recovery-pending transition failed")]
    RecoveryPending(RuntimeProcessRecoveryPendingTransitionErrorV2),
    #[error("runtime process closed-recovery transition failed")]
    ClosedRecovery(RuntimeProcessClosedRecoveryTransitionErrorV2),
    #[error("runtime process recovery-readiness transition failed")]
    RecoveryReadiness(RuntimeProcessRecoveryReadinessTransitionErrorV2),
    #[error("runtime process startup recovery loop failed")]
    StartupRecovery(RuntimeProcessStartupRecoveryLoopErrorV2),
    #[error("runtime process production handoff failed")]
    ProductionHandoff(RuntimeProcessProductionHandoffErrorV2),
    #[error("runtime owner-held process shutdown failed")]
    OwnerHeldShutdown(RuntimeOwnerHeldProcessShutdownErrorV1),
    #[error("runtime paused-connected process shutdown failed")]
    PausedConnectedShutdown(RuntimePausedConnectedProcessShutdownErrorV1),
    #[error("runtime recovery-pending process shutdown failed")]
    RecoveryPendingShutdown(RuntimeRecoveryPendingProcessShutdownErrorV2),
    #[error("runtime closed-recovery process shutdown failed")]
    ClosedRecoveryShutdown(RuntimeClosedRecoveryProcessShutdownErrorV2),
    #[error("runtime startup recovery fixed-point process shutdown failed")]
    StartupRecoveryFixedPointShutdown(RuntimeClosedRecoveryProcessCleanupFailureV2),
}

impl RuntimeProcessStagingErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::AsyncRuntimeUnavailable => "runtime_async_runtime_unavailable",
            Self::OperationDeadlineElapsed => "runtime_process_startup_operation_deadline_elapsed",
            Self::Configuration(error) => error.code(),
            Self::Secrets(error) => error.code(),
            Self::BuildRevision(error) => error.code(),
            Self::Foundation(error) => error.code(),
            Self::GatewayOwner(error) => error.code(),
            Self::PausedConnected(error) => error.code(),
            Self::RecoveryPending(error) => error.code(),
            Self::ClosedRecovery(error) => error.code(),
            Self::RecoveryReadiness(error) => error.code(),
            Self::StartupRecovery(error) => error.code(),
            Self::ProductionHandoff(error) => error.code(),
            Self::OwnerHeldShutdown(error) => error.code(),
            Self::PausedConnectedShutdown(error) => error.code(),
            Self::RecoveryPendingShutdown(error) => error.code(),
            Self::ClosedRecoveryShutdown(error) => error.code(),
            Self::StartupRecoveryFixedPointShutdown(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::Configuration(error) => match error.field() {
                Some(field) => Some(field.code()),
                None => None,
            },
            Self::Secrets(error) => error.context(),
            Self::BuildRevision(error) => error.context(),
            Self::Foundation(error) => error.context(),
            Self::GatewayOwner(error) => error.context(),
            Self::PausedConnected(error) => error.context(),
            Self::RecoveryPending(error) => error.context(),
            Self::ClosedRecovery(error) => error.context(),
            Self::RecoveryReadiness(error) => error.context(),
            Self::StartupRecovery(error) => error.context(),
            Self::ProductionHandoff(error) => error.context(),
            Self::OwnerHeldShutdown(error) => error.context(),
            Self::PausedConnectedShutdown(error) => error.context(),
            Self::RecoveryPendingShutdown(error) => error.context(),
            Self::ClosedRecoveryShutdown(error) => error.context(),
            Self::StartupRecoveryFixedPointShutdown(error) => error.context(),
            Self::AsyncRuntimeUnavailable | Self::OperationDeadlineElapsed => None,
        }
    }

    pub const fn configuration_class(self) -> bool {
        matches!(
            self,
            Self::Configuration(_) | Self::Secrets(_) | Self::BuildRevision(_)
        )
    }

    pub const fn cleanup_class(self) -> bool {
        match self {
            Self::StartupRecovery(error) => error.cleanup_class(),
            Self::ProductionHandoff(error) => error.cleanup_class(),
            Self::OwnerHeldShutdown(_)
            | Self::PausedConnectedShutdown(_)
            | Self::RecoveryPendingShutdown(_)
            | Self::ClosedRecoveryShutdown(_)
            | Self::StartupRecoveryFixedPointShutdown(_) => true,
            Self::AsyncRuntimeUnavailable
            | Self::OperationDeadlineElapsed
            | Self::Configuration(_)
            | Self::Secrets(_)
            | Self::BuildRevision(_)
            | Self::Foundation(_)
            | Self::GatewayOwner(_)
            | Self::PausedConnected(_)
            | Self::RecoveryPending(_)
            | Self::ClosedRecovery(_)
            | Self::RecoveryReadiness(_) => false,
        }
    }
}

impl Debug for RuntimeProcessStagingErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessStagingErrorV1(<redacted>)")
    }
}

pub struct RuntimeProcessStagingOutcomeV1 {
    _private: (),
}

impl Debug for RuntimeProcessStagingOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessStagingOutcomeV1(<redacted>)")
    }
}

pub fn run_runtime_process_staging_from_environment_v1(
) -> Result<RuntimeProcessStagingOutcomeV1, RuntimeProcessStagingErrorV1> {
    let startup_budget = RuntimeStartupBudgetV1::begin();
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(RuntimeProcessStagingErrorV1::AsyncRuntimeUnavailable);
    }
    let runtime = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| RuntimeProcessStagingErrorV1::AsyncRuntimeUnavailable)
        },
    )
    .map_err(|error| map_sync_stage_error_v1(error, |error| error))?;
    runtime.block_on(stage_runtime_process_from_environment_v1(startup_budget))
}

async fn stage_runtime_process_from_environment_v1(
    startup_budget: RuntimeStartupBudgetV1,
) -> Result<RuntimeProcessStagingOutcomeV1, RuntimeProcessStagingErrorV1> {
    let config = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        RuntimeConfigV1::from_process_environment,
    )
    .map_err(|error| map_sync_stage_error_v1(error, RuntimeProcessStagingErrorV1::Configuration))?;
    let secrets = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        || resolve_runtime_secrets_until_v1(&config, &startup_budget),
    )
    .map_err(|error| {
        map_sync_stage_error_v1(error, |error| match error {
            RuntimeSecretsStartupResolutionErrorV1::OperationDeadlineElapsed => {
                RuntimeProcessStagingErrorV1::OperationDeadlineElapsed
            }
            RuntimeSecretsStartupResolutionErrorV1::Resolution(error) => {
                RuntimeProcessStagingErrorV1::Secrets(error)
            }
        })
    })?;
    let build_revision = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        bootstrap_compiled_runtime_build_revision_v1,
    )
    .map_err(|error| map_sync_stage_error_v1(error, RuntimeProcessStagingErrorV1::BuildRevision))?;
    let foundation =
        compose_runtime_process_foundation_v1(startup_budget, config, secrets, build_revision)
            .await
            .map_err(RuntimeProcessStagingErrorV1::Foundation)?;
    let owner_held: crate::process::RuntimeOwnerHeldProcessV1 = foundation
        .into_owner_held_v1()
        .await
        .map_err(RuntimeProcessStagingErrorV1::GatewayOwner)?;
    let mut discord_starting = match owner_held.begin_paused_discord_connection_v1().await {
        Ok(starting) => starting,
        Err(failure) => {
            return Err(RuntimeProcessStagingErrorV1::PausedConnected(
                failure.cleanup().await,
            ));
        }
    };
    let paused_gateway = match discord_starting.wait_for_paused_connected_v1().await {
        Ok(paused_gateway) => paused_gateway,
        Err(transition) => {
            return Err(RuntimeProcessStagingErrorV1::PausedConnected(
                discord_starting
                    .cleanup_after_transition_failure_v1(transition)
                    .await,
            ));
        }
    };
    let paused_connected = match discord_starting.into_paused_connected_v1(paused_gateway) {
        Ok(paused_connected) => paused_connected,
        Err(failure) => {
            return Err(RuntimeProcessStagingErrorV1::PausedConnected(
                failure.cleanup().await,
            ));
        }
    };
    let recovery_pending = paused_connected
        .into_recovery_pending_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::RecoveryPending)?;
    let closed_recovery = recovery_pending
        .into_closed_recovery_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::ClosedRecovery)?;
    let recovery_iteration_ready = closed_recovery
        .into_recovery_iteration_ready_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::RecoveryReadiness)?;
    let fixed_point = recovery_iteration_ready
        .into_startup_recovery_fixed_point_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::StartupRecovery)?;
    let paused_production = fixed_point
        .into_paused_production_handoff_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::ProductionHandoff)?;
    let process_bound = paused_production
        .into_process_bound_handoff_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::ProductionHandoff)?;
    let recovery_resume = process_bound
        .into_recovery_resume_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::ProductionHandoff)?;
    let admission_acknowledging = recovery_resume
        .resume_recovery_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::ProductionHandoff)?;
    let empty_open = admission_acknowledging
        .enter_empty_open_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::ProductionHandoff)?;
    empty_open
        .run_until_shutdown_v2()
        .await
        .map_err(RuntimeProcessStagingErrorV1::ProductionHandoff)?;
    Ok(RuntimeProcessStagingOutcomeV1 { _private: () })
}

fn map_sync_stage_error_v1<E>(
    error: RuntimeStartupSyncStageErrorV1<E>,
    map_error: impl FnOnce(E) -> RuntimeProcessStagingErrorV1,
) -> RuntimeProcessStagingErrorV1 {
    match error {
        RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed => {
            RuntimeProcessStagingErrorV1::OperationDeadlineElapsed
        }
        RuntimeStartupSyncStageErrorV1::Stage(error) => map_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DatabaseCapabilityV1, RuntimeClosedRecoveryProcessCleanupFailureV2,
        RuntimeConfigurationFieldV1, RuntimeDatabaseCompositionErrorV1,
        RuntimeDatabasePoolShutdownErrorV1, RuntimeDiscordGatewayShutdownFailureV1,
        RuntimeGatewayOwnerShutdownFailureV1, RuntimeProcessClosedRecoveryTransitionFailureV2,
        RuntimeProcessPausedConnectedTransitionFailureV1,
        RuntimeProcessRecoveryPendingTransitionFailureV2,
        RuntimeProcessRecoveryReadinessTransitionFailureV2,
        RuntimeProcessStartupRecoveryLoopFailureV2, RuntimeRecoveryPendingProcessCleanupFailureV2,
        RuntimeSecretResolutionErrorV1,
    };

    #[test]
    fn public_errors_have_finite_codes_context_and_redacted_diagnostics() {
        let errors = [
            RuntimeProcessStagingErrorV1::AsyncRuntimeUnavailable,
            RuntimeProcessStagingErrorV1::OperationDeadlineElapsed,
            RuntimeProcessStagingErrorV1::Configuration(RuntimeConfigErrorV1::Missing(
                RuntimeConfigurationFieldV1::HealthBindAddress,
            )),
            RuntimeProcessStagingErrorV1::Secrets(RuntimeSecretsResolutionErrorV1::Database {
                capability: DatabaseCapabilityV1::Panel,
                source: RuntimeSecretResolutionErrorV1::Missing,
            }),
            RuntimeProcessStagingErrorV1::BuildRevision(
                RuntimeBuildRevisionBootstrapErrorV1::Missing,
            ),
            RuntimeProcessStagingErrorV1::Foundation(
                RuntimeProcessFoundationCompositionErrorV1::Database(
                    RuntimeDatabaseCompositionErrorV1::Unavailable {
                        capability: DatabaseCapabilityV1::Interaction,
                    },
                ),
            ),
            RuntimeProcessStagingErrorV1::OwnerHeldShutdown(
                RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndDatabase {
                    owner: RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
                    database: RuntimeDatabasePoolShutdownErrorV1::TimedOut,
                },
            ),
            RuntimeProcessStagingErrorV1::PausedConnected(
                RuntimeProcessPausedConnectedTransitionErrorV1::Transition(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                ),
            ),
            RuntimeProcessStagingErrorV1::PausedConnectedShutdown(
                RuntimePausedConnectedProcessShutdownErrorV1::Discord(
                    RuntimeDiscordGatewayShutdownFailureV1::TaskStopped,
                ),
            ),
            RuntimeProcessStagingErrorV1::RecoveryPending(
                RuntimeProcessRecoveryPendingTransitionErrorV2::Transition(
                    RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed,
                ),
            ),
            RuntimeProcessStagingErrorV1::RecoveryPendingShutdown(
                RuntimeRecoveryPendingProcessShutdownErrorV2::Cleanup(
                    RuntimeRecoveryPendingProcessCleanupFailureV2::Discord(
                        RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed,
                    ),
                ),
            ),
            RuntimeProcessStagingErrorV1::ClosedRecovery(
                RuntimeProcessClosedRecoveryTransitionErrorV2::Transition(
                    RuntimeProcessClosedRecoveryTransitionFailureV2::OperationDeadlineElapsed,
                ),
            ),
            RuntimeProcessStagingErrorV1::RecoveryReadiness(
                RuntimeProcessRecoveryReadinessTransitionErrorV2::Transition(
                    RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed,
                ),
            ),
            RuntimeProcessStagingErrorV1::ClosedRecoveryShutdown(
                RuntimeClosedRecoveryProcessShutdownErrorV2::Cleanup(
                    RuntimeClosedRecoveryProcessCleanupFailureV2::Discord(
                        RuntimeDiscordGatewayShutdownFailureV1::UnexpectedExit,
                    ),
                ),
            ),
            RuntimeProcessStagingErrorV1::StartupRecovery(
                RuntimeProcessStartupRecoveryLoopErrorV2::Transition(
                    RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveRecoveryUnavailable,
                ),
            ),
            RuntimeProcessStagingErrorV1::StartupRecoveryFixedPointShutdown(
                RuntimeClosedRecoveryProcessCleanupFailureV2::Discord(
                    RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed,
                ),
            ),
        ];

        assert_eq!(errors[0].code(), "runtime_async_runtime_unavailable");
        assert_eq!(
            errors[1].code(),
            "runtime_process_startup_operation_deadline_elapsed"
        );
        assert_eq!(errors[2].code(), "runtime_config_missing");
        assert_eq!(errors[2].context(), Some("health_bind_address"));
        assert_eq!(errors[3].code(), "runtime_secret_missing");
        assert_eq!(errors[3].context(), Some("panel"));
        assert_eq!(errors[4].code(), "runtime_build_revision_missing");
        assert_eq!(errors[5].code(), "runtime_database_unavailable");
        assert_eq!(errors[5].context(), Some("interaction"));
        assert_eq!(
            errors[6].code(),
            "runtime_owner_held_process_gateway_owner_and_database_shutdown"
        );
        assert_eq!(
            errors[7].code(),
            "runtime_process_paused_connected_discord_terminated"
        );
        assert_eq!(
            errors[8].code(),
            "runtime_discord_gateway_shutdown_task_stopped"
        );
        assert_eq!(
            errors[9].code(),
            "runtime_process_recovery_pending_operation_deadline_elapsed"
        );
        assert_eq!(
            errors[10].code(),
            "runtime_discord_gateway_shutdown_close_deadline_elapsed"
        );
        assert_eq!(
            errors[11].code(),
            "runtime_process_closed_recovery_operation_deadline_elapsed"
        );
        assert_eq!(
            errors[12].code(),
            "runtime_process_recovery_readiness_operation_deadline_elapsed"
        );
        assert_eq!(
            errors[13].code(),
            "runtime_discord_gateway_shutdown_unexpected_exit"
        );
        assert_eq!(
            errors[14].code(),
            "runtime_process_startup_recovery_loop_stale_live_recovery_unavailable"
        );
        assert_eq!(
            errors[15].code(),
            "runtime_discord_gateway_shutdown_close_deadline_elapsed"
        );
        for error in errors {
            assert!(!error.code().is_empty());
            assert!(!error.to_string().is_empty());
            assert_eq!(
                format!("{error:?}"),
                "RuntimeProcessStagingErrorV1(<redacted>)"
            );
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn only_configuration_stages_use_the_configuration_exit_class() {
        assert!(RuntimeProcessStagingErrorV1::Configuration(
            RuntimeConfigErrorV1::AmbientDatabaseConfiguration,
        )
        .configuration_class());
        assert!(RuntimeProcessStagingErrorV1::Secrets(
            RuntimeSecretsResolutionErrorV1::DiscordBotToken {
                source: RuntimeSecretResolutionErrorV1::Missing,
            },
        )
        .configuration_class());
        assert!(RuntimeProcessStagingErrorV1::BuildRevision(
            RuntimeBuildRevisionBootstrapErrorV1::Invalid,
        )
        .configuration_class());
        assert!(!RuntimeProcessStagingErrorV1::OperationDeadlineElapsed.configuration_class());
    }

    #[test]
    fn cleanup_class_tracks_nested_loop_and_terminal_cleanup_failures() {
        assert!(!RuntimeProcessStagingErrorV1::StartupRecovery(
            RuntimeProcessStartupRecoveryLoopErrorV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable,
            ),
        )
        .cleanup_class());
        assert!(RuntimeProcessStagingErrorV1::StartupRecovery(
            RuntimeProcessStartupRecoveryLoopErrorV2::CleanupAfterTransition {
                transition:
                    RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable,
                cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2::Discord(
                    RuntimeDiscordGatewayShutdownFailureV1::TaskStopped,
                ),
            },
        )
        .cleanup_class());
        assert!(
            RuntimeProcessStagingErrorV1::StartupRecoveryFixedPointShutdown(
                RuntimeClosedRecoveryProcessCleanupFailureV2::Discord(
                    RuntimeDiscordGatewayShutdownFailureV1::TaskStopped,
                ),
            )
            .cleanup_class()
        );
    }

    #[test]
    fn an_existing_async_runtime_is_rejected_before_environment_access() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = runtime.block_on(async { run_runtime_process_staging_from_environment_v1() });

        assert!(matches!(
            result,
            Err(RuntimeProcessStagingErrorV1::AsyncRuntimeUnavailable)
        ));
    }

    #[test]
    fn staging_outcome_has_no_serializable_or_diagnostic_payload() {
        let outcome = RuntimeProcessStagingOutcomeV1 { _private: () };

        assert_eq!(
            format!("{outcome:?}"),
            "RuntimeProcessStagingOutcomeV1(<redacted>)"
        );
    }
}
