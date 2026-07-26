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
    RuntimeBuildRevisionBootstrapErrorV1, RuntimeConfigErrorV1, RuntimeConfigV1,
    RuntimePausedConnectedProcessShutdownErrorV1, RuntimeProcessPausedConnectedTransitionErrorV1,
    RuntimeSecretsResolutionErrorV1,
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
    #[error("runtime owner-held process shutdown failed")]
    OwnerHeldShutdown(RuntimeOwnerHeldProcessShutdownErrorV1),
    #[error("runtime paused-connected process shutdown failed")]
    PausedConnectedShutdown(RuntimePausedConnectedProcessShutdownErrorV1),
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
            Self::OwnerHeldShutdown(error) => error.code(),
            Self::PausedConnectedShutdown(error) => error.code(),
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
            Self::OwnerHeldShutdown(error) => error.context(),
            Self::PausedConnectedShutdown(error) => error.context(),
            Self::AsyncRuntimeUnavailable | Self::OperationDeadlineElapsed => None,
        }
    }

    pub const fn configuration_class(self) -> bool {
        matches!(
            self,
            Self::Configuration(_) | Self::Secrets(_) | Self::BuildRevision(_)
        )
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
    paused_connected
        .shutdown()
        .await
        .map_err(RuntimeProcessStagingErrorV1::PausedConnectedShutdown)?;
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
        DatabaseCapabilityV1, RuntimeConfigurationFieldV1, RuntimeDatabaseCompositionErrorV1,
        RuntimeDatabasePoolShutdownErrorV1, RuntimeDiscordGatewayShutdownFailureV1,
        RuntimeGatewayOwnerShutdownFailureV1, RuntimeProcessPausedConnectedTransitionFailureV1,
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
