use std::fmt::{Debug, Formatter};

use crate::gateway_owner_startup::{
    acquire_runtime_gateway_owner_startup_v1, RuntimeAcquiredGatewayOwnerV1,
    RuntimeGatewayOwnerStartupAcquisitionErrorV1,
};
use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1;
use crate::{RuntimeDatabasePoolShutdownErrorV1, RuntimeGatewayOwnerStartupWatchdogExitV1};

use super::RuntimeProcessFoundationV1;

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessGatewayOwnerTransitionErrorV1 {
    #[error("runtime process gateway owner transition failed")]
    GatewayOwner(RuntimeGatewayOwnerStartupAcquisitionErrorV1),
    #[error("runtime process gateway owner transition cleanup failed")]
    CleanupAfterGatewayOwner {
        transition: RuntimeGatewayOwnerStartupAcquisitionErrorV1,
        cleanup: RuntimeDatabasePoolShutdownErrorV1,
    },
}

impl RuntimeProcessGatewayOwnerTransitionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::GatewayOwner(error) => error.code(),
            Self::CleanupAfterGatewayOwner { .. } => {
                "runtime_process_gateway_owner_transition_cleanup"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::GatewayOwner(error) => error.context(),
            Self::CleanupAfterGatewayOwner { .. } => None,
        }
    }
}

impl Debug for RuntimeProcessGatewayOwnerTransitionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessGatewayOwnerTransitionErrorV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerShutdownFailureV1 {
    Exit(RuntimeGatewayOwnerStartupWatchdogExitV1),
    DeadlineElapsed,
}

impl RuntimeGatewayOwnerShutdownFailureV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Exit(exit) => exit.code(),
            Self::DeadlineElapsed => "runtime_gateway_owner_shutdown_deadline_elapsed",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeOwnerHeldProcessShutdownErrorV1 {
    #[error("runtime owner-held process gateway owner shutdown failed")]
    GatewayOwner(RuntimeGatewayOwnerShutdownFailureV1),
    #[error("runtime owner-held process database shutdown failed")]
    Database(RuntimeDatabasePoolShutdownErrorV1),
    #[error("runtime owner-held process gateway owner and database shutdown failed")]
    GatewayOwnerAndDatabase {
        owner: RuntimeGatewayOwnerShutdownFailureV1,
        database: RuntimeDatabasePoolShutdownErrorV1,
    },
}

impl RuntimeOwnerHeldProcessShutdownErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::GatewayOwner(error) => error.code(),
            Self::Database(RuntimeDatabasePoolShutdownErrorV1::TimedOut) => {
                "runtime_owner_held_process_database_shutdown_timed_out"
            }
            Self::GatewayOwnerAndDatabase { .. } => {
                "runtime_owner_held_process_gateway_owner_and_database_shutdown"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeOwnerHeldProcessShutdownErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeOwnerHeldProcessShutdownErrorV1(<redacted>)")
    }
}

pub(crate) struct RuntimeOwnerHeldProcessV1 {
    foundation: RuntimeProcessFoundationV1,
    owner: RuntimeAcquiredGatewayOwnerV1,
}

impl RuntimeOwnerHeldProcessV1 {
    pub(crate) async fn shutdown(self) -> Result<(), RuntimeOwnerHeldProcessShutdownErrorV1> {
        let Self { foundation, owner } = self;
        let cleanup_deadline = foundation.startup_budget.cleanup_deadline();
        let owner = owner.shutdown_until(cleanup_deadline).await;
        let database = foundation.shutdown().await;
        finish_runtime_owner_held_process_shutdown_v1(owner, database)
    }
}

impl Debug for RuntimeOwnerHeldProcessV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeOwnerHeldProcessV1(<redacted>)")
    }
}

impl RuntimeProcessFoundationV1 {
    pub(crate) async fn into_owner_held_v1(
        mut self,
    ) -> Result<RuntimeOwnerHeldProcessV1, RuntimeProcessGatewayOwnerTransitionErrorV1> {
        if !self.startup_budget.operation_is_open() {
            return Err(cleanup_runtime_gateway_owner_transition_failure_v1(
                self,
                RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed,
            )
            .await);
        }
        let port = self.databases.execution().clone();
        let transition = acquire_runtime_gateway_owner_startup_v1(
            &mut self.gateway,
            port,
            &self.process_instance_id,
            &self.build_revision,
            self.config.gateway_owner(),
            self.startup_budget.operation_cutoff(),
            self.startup_budget.cleanup_deadline(),
        )
        .await;
        let owner = match transition {
            Ok(owner) => owner,
            Err(error) => {
                return Err(cleanup_runtime_gateway_owner_transition_failure_v1(self, error).await);
            }
        };
        if !self.startup_budget.operation_is_open() {
            let owner_cleanup = owner
                .shutdown_until(self.startup_budget.cleanup_deadline())
                .await;
            let error = match owner_cleanup {
                Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed)
                | Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation)
                | Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped)
                | Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed) => {
                    RuntimeGatewayOwnerStartupAcquisitionErrorV1::CleanupUnconfirmed
                }
                Ok(_) => RuntimeGatewayOwnerStartupAcquisitionErrorV1::OperationDeadlineElapsed,
            };
            return Err(cleanup_runtime_gateway_owner_transition_failure_v1(self, error).await);
        }
        Ok(RuntimeOwnerHeldProcessV1 {
            foundation: self,
            owner,
        })
    }
}

async fn cleanup_runtime_gateway_owner_transition_failure_v1(
    foundation: RuntimeProcessFoundationV1,
    transition: RuntimeGatewayOwnerStartupAcquisitionErrorV1,
) -> RuntimeProcessGatewayOwnerTransitionErrorV1 {
    match foundation.shutdown().await {
        Ok(()) => RuntimeProcessGatewayOwnerTransitionErrorV1::GatewayOwner(transition),
        Err(cleanup) => RuntimeProcessGatewayOwnerTransitionErrorV1::CleanupAfterGatewayOwner {
            transition,
            cleanup,
        },
    }
}

fn finish_runtime_owner_held_process_shutdown_v1(
    owner: Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    >,
    database: Result<(), RuntimeDatabasePoolShutdownErrorV1>,
) -> Result<(), RuntimeOwnerHeldProcessShutdownErrorV1> {
    let owner = match owner {
        Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown) => None,
        Ok(exit) => Some(RuntimeGatewayOwnerShutdownFailureV1::Exit(exit)),
        Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed) => {
            Some(RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed)
        }
    };
    match (owner, database) {
        (None, Ok(())) => Ok(()),
        (Some(owner), Ok(())) => Err(RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwner(owner)),
        (None, Err(database)) => Err(RuntimeOwnerHeldProcessShutdownErrorV1::Database(database)),
        (Some(owner), Err(database)) => {
            Err(RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndDatabase { owner, database })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_classification_requires_confirmed_owner_release_and_database_close() {
        assert_eq!(
            finish_runtime_owner_held_process_shutdown_v1(
                Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown),
                Ok(()),
            ),
            Ok(())
        );
        assert_eq!(
            finish_runtime_owner_held_process_shutdown_v1(
                Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed),
                Ok(()),
            ),
            Err(RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwner(
                RuntimeGatewayOwnerShutdownFailureV1::Exit(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed,
                ),
            ))
        );
        assert_eq!(
            finish_runtime_owner_held_process_shutdown_v1(
                Err(RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed),
                Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut),
            ),
            Err(
                RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndDatabase {
                    owner: RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
                    database: RuntimeDatabasePoolShutdownErrorV1::TimedOut,
                },
            )
        );
    }

    #[test]
    fn public_errors_are_finite_and_redacted() {
        let transition = RuntimeProcessGatewayOwnerTransitionErrorV1::GatewayOwner(
            RuntimeGatewayOwnerStartupAcquisitionErrorV1::Contended,
        );
        let shutdown = RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwner(
            RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
        );

        assert_eq!(transition.code(), "runtime_gateway_owner_startup_contended");
        assert_eq!(transition.context(), None);
        assert_eq!(
            format!("{transition:?}"),
            "RuntimeProcessGatewayOwnerTransitionErrorV1(<redacted>)"
        );
        assert_eq!(
            shutdown.code(),
            "runtime_gateway_owner_shutdown_deadline_elapsed"
        );
        assert_eq!(shutdown.context(), None);
        assert_eq!(
            format!("{shutdown:?}"),
            "RuntimeOwnerHeldProcessShutdownErrorV1(<redacted>)"
        );
    }
}
