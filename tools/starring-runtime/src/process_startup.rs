use std::fmt::{Debug, Formatter};

use crate::build_revision::bootstrap_compiled_runtime_build_revision_v1;
use crate::process::{
    compose_runtime_process_foundation_v1, RuntimeProcessFoundationCompositionErrorV1,
};
use crate::secret::{resolve_runtime_secrets_until_v1, RuntimeSecretsStartupResolutionErrorV1};
use crate::startup::{
    run_runtime_startup_sync_stage_v1, RuntimeStartupBudgetV1, RuntimeStartupSyncStageErrorV1,
};
use crate::{
    RuntimeBuildRevisionBootstrapErrorV1, RuntimeConfigErrorV1, RuntimeConfigV1,
    RuntimeDatabasePoolShutdownErrorV1, RuntimeSecretsResolutionErrorV1,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessFoundationStagingErrorV1 {
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
    #[error("runtime process foundation shutdown failed")]
    FoundationShutdown(RuntimeDatabasePoolShutdownErrorV1),
}

impl RuntimeProcessFoundationStagingErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::AsyncRuntimeUnavailable => "runtime_async_runtime_unavailable",
            Self::OperationDeadlineElapsed => "runtime_process_startup_operation_deadline_elapsed",
            Self::Configuration(error) => error.code(),
            Self::Secrets(error) => error.code(),
            Self::BuildRevision(error) => error.code(),
            Self::Foundation(error) => error.code(),
            Self::FoundationShutdown(RuntimeDatabasePoolShutdownErrorV1::TimedOut) => {
                "runtime_process_foundation_shutdown_timed_out"
            }
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
            Self::AsyncRuntimeUnavailable
            | Self::OperationDeadlineElapsed
            | Self::FoundationShutdown(_) => None,
        }
    }

    pub const fn configuration_class(self) -> bool {
        matches!(
            self,
            Self::Configuration(_) | Self::Secrets(_) | Self::BuildRevision(_)
        )
    }
}

impl Debug for RuntimeProcessFoundationStagingErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationStagingErrorV1(<redacted>)")
    }
}

pub struct RuntimeProcessFoundationStagingOutcomeV1 {
    _private: (),
}

impl Debug for RuntimeProcessFoundationStagingOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationStagingOutcomeV1(<redacted>)")
    }
}

pub fn run_runtime_process_foundation_staging_from_environment_v1(
) -> Result<RuntimeProcessFoundationStagingOutcomeV1, RuntimeProcessFoundationStagingErrorV1> {
    let startup_budget = RuntimeStartupBudgetV1::begin();
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(RuntimeProcessFoundationStagingErrorV1::AsyncRuntimeUnavailable);
    }
    let runtime = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| RuntimeProcessFoundationStagingErrorV1::AsyncRuntimeUnavailable)
        },
    )
    .map_err(|error| map_sync_stage_error_v1(error, |error| error))?;
    runtime.block_on(stage_runtime_process_foundation_from_environment_v1(
        startup_budget,
    ))
}

async fn stage_runtime_process_foundation_from_environment_v1(
    startup_budget: RuntimeStartupBudgetV1,
) -> Result<RuntimeProcessFoundationStagingOutcomeV1, RuntimeProcessFoundationStagingErrorV1> {
    let config = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        RuntimeConfigV1::from_process_environment,
    )
    .map_err(|error| {
        map_sync_stage_error_v1(error, RuntimeProcessFoundationStagingErrorV1::Configuration)
    })?;
    let secrets = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        || resolve_runtime_secrets_until_v1(&config, &startup_budget),
    )
    .map_err(|error| {
        map_sync_stage_error_v1(error, |error| match error {
            RuntimeSecretsStartupResolutionErrorV1::OperationDeadlineElapsed => {
                RuntimeProcessFoundationStagingErrorV1::OperationDeadlineElapsed
            }
            RuntimeSecretsStartupResolutionErrorV1::Resolution(error) => {
                RuntimeProcessFoundationStagingErrorV1::Secrets(error)
            }
        })
    })?;
    let build_revision = run_runtime_startup_sync_stage_v1(
        || startup_budget.operation_is_open(),
        bootstrap_compiled_runtime_build_revision_v1,
    )
    .map_err(|error| {
        map_sync_stage_error_v1(error, RuntimeProcessFoundationStagingErrorV1::BuildRevision)
    })?;
    let foundation =
        compose_runtime_process_foundation_v1(startup_budget, config, secrets, build_revision)
            .await
            .map_err(RuntimeProcessFoundationStagingErrorV1::Foundation)?;
    foundation
        .shutdown()
        .await
        .map_err(RuntimeProcessFoundationStagingErrorV1::FoundationShutdown)?;
    Ok(RuntimeProcessFoundationStagingOutcomeV1 { _private: () })
}

fn map_sync_stage_error_v1<E>(
    error: RuntimeStartupSyncStageErrorV1<E>,
    map_error: impl FnOnce(E) -> RuntimeProcessFoundationStagingErrorV1,
) -> RuntimeProcessFoundationStagingErrorV1 {
    match error {
        RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed => {
            RuntimeProcessFoundationStagingErrorV1::OperationDeadlineElapsed
        }
        RuntimeStartupSyncStageErrorV1::Stage(error) => map_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DatabaseCapabilityV1, RuntimeConfigurationFieldV1, RuntimeDatabaseCompositionErrorV1,
        RuntimeSecretResolutionErrorV1,
    };

    #[test]
    fn public_errors_have_finite_codes_context_and_redacted_diagnostics() {
        let errors = [
            RuntimeProcessFoundationStagingErrorV1::AsyncRuntimeUnavailable,
            RuntimeProcessFoundationStagingErrorV1::OperationDeadlineElapsed,
            RuntimeProcessFoundationStagingErrorV1::Configuration(RuntimeConfigErrorV1::Missing(
                RuntimeConfigurationFieldV1::HealthBindAddress,
            )),
            RuntimeProcessFoundationStagingErrorV1::Secrets(
                RuntimeSecretsResolutionErrorV1::Database {
                    capability: DatabaseCapabilityV1::Panel,
                    source: RuntimeSecretResolutionErrorV1::Missing,
                },
            ),
            RuntimeProcessFoundationStagingErrorV1::BuildRevision(
                RuntimeBuildRevisionBootstrapErrorV1::Missing,
            ),
            RuntimeProcessFoundationStagingErrorV1::Foundation(
                RuntimeProcessFoundationCompositionErrorV1::Database(
                    RuntimeDatabaseCompositionErrorV1::Unavailable {
                        capability: DatabaseCapabilityV1::Interaction,
                    },
                ),
            ),
            RuntimeProcessFoundationStagingErrorV1::FoundationShutdown(
                RuntimeDatabasePoolShutdownErrorV1::TimedOut,
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
            "runtime_process_foundation_shutdown_timed_out"
        );
        for error in errors {
            assert!(!error.code().is_empty());
            assert!(!error.to_string().is_empty());
            assert_eq!(
                format!("{error:?}"),
                "RuntimeProcessFoundationStagingErrorV1(<redacted>)"
            );
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn only_configuration_stages_use_the_configuration_exit_class() {
        assert!(RuntimeProcessFoundationStagingErrorV1::Configuration(
            RuntimeConfigErrorV1::AmbientDatabaseConfiguration,
        )
        .configuration_class());
        assert!(RuntimeProcessFoundationStagingErrorV1::Secrets(
            RuntimeSecretsResolutionErrorV1::DiscordBotToken {
                source: RuntimeSecretResolutionErrorV1::Missing,
            },
        )
        .configuration_class());
        assert!(RuntimeProcessFoundationStagingErrorV1::BuildRevision(
            RuntimeBuildRevisionBootstrapErrorV1::Invalid,
        )
        .configuration_class());
        assert!(
            !RuntimeProcessFoundationStagingErrorV1::OperationDeadlineElapsed.configuration_class()
        );
    }

    #[test]
    fn an_existing_async_runtime_is_rejected_before_environment_access() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = runtime
            .block_on(async { run_runtime_process_foundation_staging_from_environment_v1() });

        assert!(matches!(
            result,
            Err(RuntimeProcessFoundationStagingErrorV1::AsyncRuntimeUnavailable)
        ));
    }

    #[test]
    fn staging_outcome_has_no_serializable_or_diagnostic_payload() {
        let outcome = RuntimeProcessFoundationStagingOutcomeV1 { _private: () };

        assert_eq!(
            format!("{outcome:?}"),
            "RuntimeProcessFoundationStagingOutcomeV1(<redacted>)"
        );
    }
}
