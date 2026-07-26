use std::io::Write;
use std::process::ExitCode;

use starring_runtime::{
    run_runtime_process_staging_from_environment_v1, RuntimeProcessStagingErrorV1,
};

#[derive(Clone, Copy)]
enum RuntimeProcessExitStatusV1 {
    ClosedRecoveryAndClosed,
    Failed(RuntimeProcessStagingErrorV1),
}

impl RuntimeProcessExitStatusV1 {
    const fn code(self) -> &'static str {
        match self {
            Self::ClosedRecoveryAndClosed => "runtime_staging_closed_recovery_and_closed",
            Self::Failed(error) => error.code(),
        }
    }

    const fn context(self) -> Option<&'static str> {
        match self {
            Self::ClosedRecoveryAndClosed => None,
            Self::Failed(error) => error.context(),
        }
    }

    fn exit_code(self) -> ExitCode {
        match self {
            Self::ClosedRecoveryAndClosed => ExitCode::from(70),
            Self::Failed(error) if error.configuration_class() => ExitCode::from(78),
            Self::Failed(RuntimeProcessStagingErrorV1::AsyncRuntimeUnavailable)
            | Self::Failed(RuntimeProcessStagingErrorV1::OwnerHeldShutdown(_))
            | Self::Failed(RuntimeProcessStagingErrorV1::PausedConnectedShutdown(_))
            | Self::Failed(RuntimeProcessStagingErrorV1::RecoveryPendingShutdown(_))
            | Self::Failed(RuntimeProcessStagingErrorV1::ClosedRecoveryShutdown(_)) => {
                ExitCode::from(70)
            }
            Self::Failed(_) => ExitCode::from(69),
        }
    }
}

fn main() -> ExitCode {
    install_panic_hook();
    let status = match run_runtime_process_staging_from_environment_v1() {
        Ok(_) => RuntimeProcessExitStatusV1::ClosedRecoveryAndClosed,
        Err(error) => RuntimeProcessExitStatusV1::Failed(error),
    };
    emit_status(status.code(), status.context());
    status.exit_code()
}

fn emit_status(status: &str, context: Option<&str>) {
    let mut stderr = std::io::stderr().lock();
    if let Some(context) = context {
        let _write_result = writeln!(stderr, "starring_runtime_status={status} context={context}");
    } else {
        let _write_result = writeln!(stderr, "starring_runtime_status={status}");
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_| {
        let _write_result = std::io::stderr().write_all(b"starring_runtime_status=panic\n");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use starring_runtime::{
        RuntimeBuildRevisionBootstrapErrorV1, RuntimeClosedRecoveryProcessCleanupFailureV2,
        RuntimeClosedRecoveryProcessShutdownErrorV2, RuntimeConfigErrorV1,
        RuntimeConfigurationFieldV1, RuntimeDatabasePoolShutdownErrorV1,
        RuntimeDiscordGatewayShutdownFailureV1, RuntimeGatewayOwnerShutdownFailureV1,
        RuntimeOwnerHeldProcessShutdownErrorV1, RuntimePausedConnectedProcessShutdownErrorV1,
        RuntimeRecoveryPendingProcessCleanupFailureV2,
        RuntimeRecoveryPendingProcessShutdownErrorV2,
    };

    #[test]
    fn status_codes_context_and_exit_classes_are_finite() {
        let staged = RuntimeProcessExitStatusV1::ClosedRecoveryAndClosed;
        let configuration =
            RuntimeProcessExitStatusV1::Failed(RuntimeProcessStagingErrorV1::Configuration(
                RuntimeConfigErrorV1::Missing(RuntimeConfigurationFieldV1::HealthBindAddress),
            ));
        let build =
            RuntimeProcessExitStatusV1::Failed(RuntimeProcessStagingErrorV1::BuildRevision(
                RuntimeBuildRevisionBootstrapErrorV1::Invalid,
            ));
        let runtime = RuntimeProcessExitStatusV1::Failed(
            RuntimeProcessStagingErrorV1::AsyncRuntimeUnavailable,
        );
        let shutdown =
            RuntimeProcessExitStatusV1::Failed(RuntimeProcessStagingErrorV1::OwnerHeldShutdown(
                RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndDatabase {
                    owner: RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
                    database: RuntimeDatabasePoolShutdownErrorV1::TimedOut,
                },
            ));
        let paused_shutdown = RuntimeProcessExitStatusV1::Failed(
            RuntimeProcessStagingErrorV1::PausedConnectedShutdown(
                RuntimePausedConnectedProcessShutdownErrorV1::Discord(
                    RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed,
                ),
            ),
        );
        let recovery_pending_shutdown = RuntimeProcessExitStatusV1::Failed(
            RuntimeProcessStagingErrorV1::RecoveryPendingShutdown(
                RuntimeRecoveryPendingProcessShutdownErrorV2::Cleanup(
                    RuntimeRecoveryPendingProcessCleanupFailureV2::Discord(
                        RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed,
                    ),
                ),
            ),
        );
        let closed_recovery_shutdown = RuntimeProcessExitStatusV1::Failed(
            RuntimeProcessStagingErrorV1::ClosedRecoveryShutdown(
                RuntimeClosedRecoveryProcessShutdownErrorV2::Cleanup(
                    RuntimeClosedRecoveryProcessCleanupFailureV2::Discord(
                        RuntimeDiscordGatewayShutdownFailureV1::UnexpectedExit,
                    ),
                ),
            ),
        );

        assert_eq!(staged.code(), "runtime_staging_closed_recovery_and_closed");
        assert_eq!(staged.context(), None);
        assert_eq!(staged.exit_code(), ExitCode::from(70));
        assert_eq!(configuration.code(), "runtime_config_missing");
        assert_eq!(configuration.context(), Some("health_bind_address"));
        assert_eq!(configuration.exit_code(), ExitCode::from(78));
        assert_eq!(build.exit_code(), ExitCode::from(78));
        assert_eq!(runtime.exit_code(), ExitCode::from(70));
        assert_eq!(shutdown.exit_code(), ExitCode::from(70));
        assert_eq!(paused_shutdown.exit_code(), ExitCode::from(70));
        assert_eq!(recovery_pending_shutdown.exit_code(), ExitCode::from(70));
        assert_eq!(closed_recovery_shutdown.exit_code(), ExitCode::from(70));
    }
}
