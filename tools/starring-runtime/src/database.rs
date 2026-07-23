use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use automation_runtime_convergence_postgres::{
    PostgresRuntimeExactTargetReader, RuntimeConvergenceStoreError,
    RuntimeExactTargetDatabaseExpectationV1, RuntimeExactTargetDatabaseReadinessV1,
    RuntimeExactTargetDatabaseTimeoutsV1,
};
use automation_runtime_execution_postgres::{
    observe_runtime_execution_database_identity_with_timeouts_v1, PostgresRuntimeExecutionV1,
    RuntimeExecutionDatabaseExpectationV1, RuntimeExecutionDatabaseReadinessV1,
    RuntimeExecutionDatabaseTimeoutsV1, RuntimeExecutionPersistenceErrorV1,
};
use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionDatabaseExpectationV1,
    RuntimeInteractionDatabaseReadinessV1, RuntimeInteractionDatabaseTimeoutsV1,
    RuntimeInteractionPersistenceErrorV1, RuntimeInteractionRouteTimeoutV1,
};
use automation_runtime_panel_postgres::{
    PostgresRuntimePanelV1, RuntimePanelDatabaseExpectationV1, RuntimePanelDatabaseReadinessV1,
    RuntimePanelDatabaseTimeoutsV1, RuntimePanelPersistenceErrorV1,
};
use automation_runtime_serving_postgres::{
    PostgresRuntimeServingLeaseV1, RuntimeServingDatabaseExpectationV1,
    RuntimeServingDatabaseReadinessV1, RuntimeServingDatabaseTimeoutsV1,
    RuntimeServingPersistenceErrorV1,
};
use automation_runtime_worker::{
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2,
};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use sqlx::ConnectOptions;
use tokio::time::{timeout, timeout_at, Instant};

use crate::{
    DatabaseCapabilityV1, DatabasePoolConfigV1, ResolvedRuntimeSecretsV1, RuntimeConfigV1,
    RuntimeDatabaseConnectionSecretV1, RuntimeDatabaseEndpointV1, RuntimeDatabaseSslModeV1,
};

const STARTUP_READINESS_TIMEOUT: Duration = Duration::from_secs(45);
const PERIODIC_READINESS_TIMEOUT: Duration = Duration::from_secs(5);
const DATABASE_POOL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDatabaseCompositionErrorV1 {
    #[error("runtime database configuration is invalid")]
    InvalidConfiguration,
    #[error("runtime database connection configuration is invalid")]
    ConnectionConfiguration { capability: DatabaseCapabilityV1 },
    #[error("runtime database connection transport is unsafe")]
    UnsafeTransport { capability: DatabaseCapabilityV1 },
    #[error("runtime database connection is unavailable")]
    Unavailable { capability: DatabaseCapabilityV1 },
    #[error("runtime database identity verification failed")]
    IdentityVerification,
    #[error("runtime database readiness authority does not match")]
    ReadinessAuthorityMismatch { capability: DatabaseCapabilityV1 },
    #[error("runtime database readiness is unavailable")]
    ReadinessUnavailable { capability: DatabaseCapabilityV1 },
    #[error("runtime database readiness contract was rejected")]
    ReadinessRejected { capability: DatabaseCapabilityV1 },
    #[error("runtime database readiness timed out")]
    ReadinessTimedOut,
    #[error("runtime database aggregate authority does not match")]
    AuthorityMismatch,
}

impl RuntimeDatabaseCompositionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "runtime_database_invalid_configuration",
            Self::ConnectionConfiguration { .. } => "runtime_database_connection_configuration",
            Self::UnsafeTransport { .. } => "runtime_database_unsafe_transport",
            Self::Unavailable { .. } => "runtime_database_unavailable",
            Self::IdentityVerification => "runtime_database_identity_verification",
            Self::ReadinessAuthorityMismatch { .. } => {
                "runtime_database_readiness_authority_mismatch"
            }
            Self::ReadinessUnavailable { .. } => "runtime_database_readiness_unavailable",
            Self::ReadinessRejected { .. } => "runtime_database_readiness_rejected",
            Self::ReadinessTimedOut => "runtime_database_readiness_timed_out",
            Self::AuthorityMismatch => "runtime_database_authority_mismatch",
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::ConnectionConfiguration { capability }
            | Self::UnsafeTransport { capability }
            | Self::Unavailable { capability }
            | Self::ReadinessAuthorityMismatch { capability }
            | Self::ReadinessUnavailable { capability }
            | Self::ReadinessRejected { capability } => Some(capability.code()),
            Self::InvalidConfiguration
            | Self::IdentityVerification
            | Self::ReadinessTimedOut
            | Self::AuthorityMismatch => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDatabasePoolShutdownErrorV1 {
    #[error("runtime database pool shutdown timed out")]
    TimedOut,
}

impl Debug for RuntimeDatabasePoolShutdownErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabasePoolShutdownErrorV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeDatabaseReadinessV1 {
    execution: RuntimeExecutionDatabaseReadinessV1,
    exact_target: RuntimeExactTargetDatabaseReadinessV1,
    panel: RuntimePanelDatabaseReadinessV1,
    serving: RuntimeServingDatabaseReadinessV1,
    interaction: RuntimeInteractionDatabaseReadinessV1,
    capability_receipts: RuntimeCapabilityReadinessSetV2,
}

impl RuntimeDatabaseReadinessV1 {
    pub const fn is_verified(&self) -> bool {
        true
    }

    pub fn exact_capability_receipts(&self) -> &RuntimeCapabilityReadinessSetV2 {
        &self.capability_receipts
    }
}

pub(crate) struct RuntimeDatabaseReadinessRefreshV2 {
    readiness: RuntimeDatabaseReadinessV1,
}

impl RuntimeDatabaseReadinessRefreshV2 {
    pub(crate) fn into_exact_capability_receipts(self) -> RuntimeCapabilityReadinessSetV2 {
        self.readiness.capability_receipts
    }
}

impl Debug for RuntimeDatabaseReadinessRefreshV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabaseReadinessRefreshV2(<redacted>)")
    }
}

#[cfg(test)]
pub(crate) fn runtime_database_readiness_for_test_v1() -> RuntimeDatabaseReadinessV1 {
    runtime_database_readiness_for_test_at_v1(1_000_000)
}

#[cfg(test)]
fn runtime_database_readiness_for_test_at_v1(checked_at_millis: i64) -> RuntimeDatabaseReadinessV1 {
    let checked_at = chrono::DateTime::from_timestamp_millis(checked_at_millis).unwrap();
    aggregate_readiness_v1(
        RuntimeExecutionDatabaseReadinessV1 {
            database_identity: "01234567-89ab-cdef-8123-456789abcdef".to_string(),
            database_name: "starring".to_string(),
            executor_role: "role_a".to_string(),
            checked_at,
        },
        RuntimeExactTargetDatabaseReadinessV1 {
            database_identity: "01234567-89ab-cdef-8123-456789abcdef".to_string(),
            database_name: "starring".to_string(),
            executor_role: "role_b".to_string(),
            checked_at,
        },
        RuntimePanelDatabaseReadinessV1 {
            database_identity: "01234567-89ab-cdef-8123-456789abcdef".to_string(),
            database_name: "starring".to_string(),
            executor_role: "role_c".to_string(),
            checked_at,
        },
        RuntimeServingDatabaseReadinessV1 {
            database_identity: "01234567-89ab-cdef-8123-456789abcdef".to_string(),
            database_name: "starring".to_string(),
            executor_role: "role_d".to_string(),
            checked_at,
        },
        RuntimeInteractionDatabaseReadinessV1 {
            database_identity: "01234567-89ab-cdef-8123-456789abcdef".to_string(),
            database_name: "starring".to_string(),
            executor_role: "role_e".to_string(),
            checked_at,
        },
    )
    .unwrap()
}

#[cfg(test)]
pub(crate) fn runtime_database_readiness_refresh_for_test_v2() -> RuntimeDatabaseReadinessRefreshV2
{
    runtime_database_readiness_refresh_at_for_test_v2(2_000_000)
}

#[cfg(test)]
pub(crate) fn runtime_database_readiness_refresh_at_for_test_v2(
    checked_at_millis: i64,
) -> RuntimeDatabaseReadinessRefreshV2 {
    RuntimeDatabaseReadinessRefreshV2 {
        readiness: runtime_database_readiness_for_test_at_v1(checked_at_millis),
    }
}

impl Debug for RuntimeDatabaseReadinessV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabaseReadinessV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct RuntimeDatabasePoolShutdownV1 {
    pools: Arc<[PgPool; 5]>,
}

impl RuntimeDatabasePoolShutdownV1 {
    pub async fn close(&self) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
        close_pool_refs_with_deadline(self.pools.each_ref().map(Some)).await
    }

    pub fn is_closed(&self) -> bool {
        self.pools.iter().all(PgPool::is_closed)
    }
}

impl Debug for RuntimeDatabasePoolShutdownV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabasePoolShutdownV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct RuntimeDatabaseDependenciesV1 {
    execution: PostgresRuntimeExecutionV1,
    exact_target: PostgresRuntimeExactTargetReader,
    panel: PostgresRuntimePanelV1,
    serving: PostgresRuntimeServingLeaseV1,
    interaction: PostgresRuntimeInteractionV1,
    initial_readiness: RuntimeDatabaseReadinessV1,
    shutdown: RuntimeDatabasePoolShutdownV1,
}

impl RuntimeDatabaseDependenciesV1 {
    pub fn execution(&self) -> &PostgresRuntimeExecutionV1 {
        &self.execution
    }

    pub fn exact_target(&self) -> &PostgresRuntimeExactTargetReader {
        &self.exact_target
    }

    pub fn panel(&self) -> &PostgresRuntimePanelV1 {
        &self.panel
    }

    pub fn serving(&self) -> &PostgresRuntimeServingLeaseV1 {
        &self.serving
    }

    pub fn interaction(&self) -> &PostgresRuntimeInteractionV1 {
        &self.interaction
    }

    pub fn initial_readiness(&self) -> &RuntimeDatabaseReadinessV1 {
        &self.initial_readiness
    }

    pub fn shutdown(&self) -> RuntimeDatabasePoolShutdownV1 {
        self.shutdown.clone()
    }

    pub async fn verify_readiness_v1(
        &self,
    ) -> Result<RuntimeDatabaseReadinessV1, RuntimeDatabaseCompositionErrorV1> {
        let readiness = async {
            let (execution, exact_target, panel, serving, interaction) = tokio::join!(
                self.execution.verify_database_v1(),
                self.exact_target.verify_database_v1(),
                self.panel.verify_database_v1(),
                self.serving.verify_database_v1(),
                self.interaction.verify_database_v1(),
            );
            verified_readiness_from_results(execution, exact_target, panel, serving, interaction)
        };
        timeout(PERIODIC_READINESS_TIMEOUT, readiness)
            .await
            .map_err(|_| RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut)?
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) async fn verify_readiness_refresh_until_v2(
        &self,
        operation_cutoff: std::time::Instant,
    ) -> Result<RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseCompositionErrorV1> {
        let readiness = timeout_at(
            Instant::from_std(operation_cutoff),
            self.verify_readiness_v1(),
        )
        .await
        .map_err(|_| RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut)??;
        Ok(RuntimeDatabaseReadinessRefreshV2 { readiness })
    }
}

impl Debug for RuntimeDatabaseDependenciesV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabaseDependenciesV1(<redacted>)")
    }
}

pub async fn compose_runtime_database_dependencies_v1(
    config: &RuntimeConfigV1,
    secrets: &ResolvedRuntimeSecretsV1,
) -> Result<RuntimeDatabaseDependenciesV1, RuntimeDatabaseCompositionErrorV1> {
    let startup_deadline = Instant::now() + STARTUP_READINESS_TIMEOUT;
    let timeouts = RuntimeDatabaseTimeoutBundleV1::new(config)?;
    verify_expected_database_authority_v1(secrets)?;
    let pools =
        connect_database_pools_v1(secrets, config.database_pool(), startup_deadline).await?;
    let build = build_verified_dependencies_v1(secrets, &pools, timeouts);
    let result = timeout_at(startup_deadline, build).await;
    match result {
        Ok(Ok(dependencies)) => Ok(dependencies),
        Ok(Err(error)) => {
            let _shutdown_result = pools.close_until(startup_deadline).await;
            Err(error)
        }
        Err(_) => {
            let _shutdown_result = pools.close_until(startup_deadline).await;
            Err(RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut)
        }
    }
}

fn verify_expected_database_authority_v1(
    secrets: &ResolvedRuntimeSecretsV1,
) -> Result<(), RuntimeDatabaseCompositionErrorV1> {
    let database_secrets = secrets.database_secrets();
    let expectations = DatabaseCapabilityV1::ALL.map(|capability| {
        let secret = database_secrets
            .database_url(capability)
            .connection_secret();
        (secret.database(), secret.username())
    });
    verify_expected_database_names_and_roles_v1(expectations)
}

fn verify_expected_database_names_and_roles_v1(
    expectations: [(&str, &str); 5],
) -> Result<(), RuntimeDatabaseCompositionErrorV1> {
    let expected_database = expectations[0].0;
    if expectations
        .iter()
        .any(|(database, _)| *database != expected_database)
    {
        return Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch);
    }
    let roles = expectations
        .iter()
        .map(|(_, role)| *role)
        .collect::<BTreeSet<_>>();
    if roles.len() != expectations.len() {
        return Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct RuntimeDatabaseTimeoutBundleV1 {
    execution: RuntimeExecutionDatabaseTimeoutsV1,
    exact_target: RuntimeExactTargetDatabaseTimeoutsV1,
    panel: RuntimePanelDatabaseTimeoutsV1,
    serving: RuntimeServingDatabaseTimeoutsV1,
    interaction: RuntimeInteractionDatabaseTimeoutsV1,
    interaction_route: RuntimeInteractionRouteTimeoutV1,
}

impl RuntimeDatabaseTimeoutBundleV1 {
    fn new(config: &RuntimeConfigV1) -> Result<Self, RuntimeDatabaseCompositionErrorV1> {
        let operation = config.database_operation();
        let statement = operation.statement_timeout();
        let lock = operation.lock_timeout();
        Ok(Self {
            execution: RuntimeExecutionDatabaseTimeoutsV1::new(statement, lock)
                .map_err(|_| RuntimeDatabaseCompositionErrorV1::InvalidConfiguration)?,
            exact_target: RuntimeExactTargetDatabaseTimeoutsV1::new(statement, lock)
                .map_err(|_| RuntimeDatabaseCompositionErrorV1::InvalidConfiguration)?,
            panel: RuntimePanelDatabaseTimeoutsV1::new(statement, lock)
                .map_err(|_| RuntimeDatabaseCompositionErrorV1::InvalidConfiguration)?,
            serving: RuntimeServingDatabaseTimeoutsV1::new(statement, lock)
                .map_err(|_| RuntimeDatabaseCompositionErrorV1::InvalidConfiguration)?,
            interaction: RuntimeInteractionDatabaseTimeoutsV1::new(statement, lock)
                .map_err(|_| RuntimeDatabaseCompositionErrorV1::InvalidConfiguration)?,
            interaction_route: RuntimeInteractionRouteTimeoutV1::new(
                config.gateway().instance_lookup_timeout(),
            )
            .map_err(|_| RuntimeDatabaseCompositionErrorV1::InvalidConfiguration)?,
        })
    }
}

async fn build_verified_dependencies_v1(
    secrets: &ResolvedRuntimeSecretsV1,
    pools: &ConnectedRuntimeDatabasePoolsV1,
    timeouts: RuntimeDatabaseTimeoutBundleV1,
) -> Result<RuntimeDatabaseDependenciesV1, RuntimeDatabaseCompositionErrorV1> {
    let database_secrets = secrets.database_secrets();
    let convergence_secret = database_secrets
        .database_url(DatabaseCapabilityV1::Convergence)
        .connection_secret();
    let identity = observe_runtime_execution_database_identity_with_timeouts_v1(
        &pools.convergence,
        convergence_secret.database(),
        convergence_secret.username(),
        timeouts.execution,
    )
    .await
    .map_err(|_| RuntimeDatabaseCompositionErrorV1::IdentityVerification)?;
    let database_identity = identity.database_identity();
    let execution_expectation = RuntimeExecutionDatabaseExpectationV1::new(
        database_identity,
        convergence_secret.database(),
        convergence_secret.username(),
    )
    .map_err(|_| RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)?;
    let exact_secret = database_secrets
        .database_url(DatabaseCapabilityV1::ExactTarget)
        .connection_secret();
    let exact_expectation = RuntimeExactTargetDatabaseExpectationV1::new(
        database_identity,
        exact_secret.database(),
        exact_secret.username(),
    )
    .map_err(|_| RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)?;
    let panel_secret = database_secrets
        .database_url(DatabaseCapabilityV1::Panel)
        .connection_secret();
    let panel_expectation = RuntimePanelDatabaseExpectationV1::new(
        database_identity,
        panel_secret.database(),
        panel_secret.username(),
    )
    .map_err(|_| RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)?;
    let serving_secret = database_secrets
        .database_url(DatabaseCapabilityV1::Serving)
        .connection_secret();
    let serving_expectation = RuntimeServingDatabaseExpectationV1::new(
        database_identity,
        serving_secret.database(),
        serving_secret.username(),
    )
    .map_err(|_| RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)?;
    let interaction_secret = database_secrets
        .database_url(DatabaseCapabilityV1::Interaction)
        .connection_secret();
    let interaction_expectation = RuntimeInteractionDatabaseExpectationV1::new(
        database_identity,
        interaction_secret.database(),
        interaction_secret.username(),
    )
    .map_err(|_| RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)?;
    let (execution, exact_target, panel, serving, interaction) = tokio::join!(
        PostgresRuntimeExecutionV1::connect_verified(
            pools.convergence.clone(),
            execution_expectation,
            timeouts.execution,
        ),
        PostgresRuntimeExactTargetReader::connect_verified(
            pools.exact_target.clone(),
            exact_expectation,
            timeouts.exact_target,
        ),
        PostgresRuntimePanelV1::connect_verified(
            pools.panel.clone(),
            panel_expectation,
            timeouts.panel,
        ),
        PostgresRuntimeServingLeaseV1::connect_verified(
            pools.serving.clone(),
            serving_expectation,
            timeouts.serving,
        ),
        PostgresRuntimeInteractionV1::connect_verified_with_route_timeout(
            pools.interaction.clone(),
            interaction_expectation,
            timeouts.interaction,
            timeouts.interaction_route,
        ),
    );
    let execution = execution.map_err(execution_readiness_error)?;
    let exact_target = exact_target.map_err(exact_target_readiness_error)?;
    let panel = panel.map_err(panel_readiness_error)?;
    let serving = serving.map_err(serving_readiness_error)?;
    let interaction = interaction.map_err(interaction_readiness_error)?;
    let initial_readiness = aggregate_readiness_v1(
        execution.initial_readiness().clone(),
        exact_target.initial_readiness().clone(),
        panel.initial_readiness().clone(),
        serving.initial_readiness().clone(),
        interaction.initial_readiness().clone(),
    )?;
    Ok(RuntimeDatabaseDependenciesV1 {
        execution,
        exact_target,
        panel,
        serving,
        interaction,
        initial_readiness,
        shutdown: pools.shutdown(),
    })
}

fn verified_readiness_from_results(
    execution: Result<RuntimeExecutionDatabaseReadinessV1, RuntimeExecutionPersistenceErrorV1>,
    exact_target: Result<RuntimeExactTargetDatabaseReadinessV1, RuntimeConvergenceStoreError>,
    panel: Result<RuntimePanelDatabaseReadinessV1, RuntimePanelPersistenceErrorV1>,
    serving: Result<RuntimeServingDatabaseReadinessV1, RuntimeServingPersistenceErrorV1>,
    interaction: Result<
        RuntimeInteractionDatabaseReadinessV1,
        RuntimeInteractionPersistenceErrorV1,
    >,
) -> Result<RuntimeDatabaseReadinessV1, RuntimeDatabaseCompositionErrorV1> {
    let execution = execution.map_err(execution_readiness_error)?;
    let exact_target = exact_target.map_err(exact_target_readiness_error)?;
    let panel = panel.map_err(panel_readiness_error)?;
    let serving = serving.map_err(serving_readiness_error)?;
    let interaction = interaction.map_err(interaction_readiness_error)?;
    aggregate_readiness_v1(execution, exact_target, panel, serving, interaction)
}

fn readiness_authority_mismatch(
    capability: DatabaseCapabilityV1,
) -> RuntimeDatabaseCompositionErrorV1 {
    RuntimeDatabaseCompositionErrorV1::ReadinessAuthorityMismatch { capability }
}

fn readiness_unavailable(capability: DatabaseCapabilityV1) -> RuntimeDatabaseCompositionErrorV1 {
    RuntimeDatabaseCompositionErrorV1::ReadinessUnavailable { capability }
}

fn readiness_rejected(capability: DatabaseCapabilityV1) -> RuntimeDatabaseCompositionErrorV1 {
    RuntimeDatabaseCompositionErrorV1::ReadinessRejected { capability }
}

fn execution_readiness_error(
    error: RuntimeExecutionPersistenceErrorV1,
) -> RuntimeDatabaseCompositionErrorV1 {
    match error {
        RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch => {
            readiness_authority_mismatch(DatabaseCapabilityV1::Convergence)
        }
        RuntimeExecutionPersistenceErrorV1::Timeout
        | RuntimeExecutionPersistenceErrorV1::Concurrency
        | RuntimeExecutionPersistenceErrorV1::Unavailable
        | RuntimeExecutionPersistenceErrorV1::Indeterminate => {
            readiness_unavailable(DatabaseCapabilityV1::Convergence)
        }
        _ => readiness_rejected(DatabaseCapabilityV1::Convergence),
    }
}

fn exact_target_readiness_error(
    error: RuntimeConvergenceStoreError,
) -> RuntimeDatabaseCompositionErrorV1 {
    match error {
        RuntimeConvergenceStoreError::DatabaseAuthorityMismatch => {
            readiness_authority_mismatch(DatabaseCapabilityV1::ExactTarget)
        }
        RuntimeConvergenceStoreError::DatabaseTimeout
        | RuntimeConvergenceStoreError::DatabaseConcurrency
        | RuntimeConvergenceStoreError::DatabaseUnavailable => {
            readiness_unavailable(DatabaseCapabilityV1::ExactTarget)
        }
        _ => readiness_rejected(DatabaseCapabilityV1::ExactTarget),
    }
}

fn panel_readiness_error(
    error: RuntimePanelPersistenceErrorV1,
) -> RuntimeDatabaseCompositionErrorV1 {
    match error {
        RuntimePanelPersistenceErrorV1::InvalidAuthority
        | RuntimePanelPersistenceErrorV1::AuthorityChanged => {
            readiness_authority_mismatch(DatabaseCapabilityV1::Panel)
        }
        RuntimePanelPersistenceErrorV1::Timeout
        | RuntimePanelPersistenceErrorV1::Unavailable
        | RuntimePanelPersistenceErrorV1::Indeterminate => {
            readiness_unavailable(DatabaseCapabilityV1::Panel)
        }
        _ => readiness_rejected(DatabaseCapabilityV1::Panel),
    }
}

fn serving_readiness_error(
    error: RuntimeServingPersistenceErrorV1,
) -> RuntimeDatabaseCompositionErrorV1 {
    match error {
        RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch
        | RuntimeServingPersistenceErrorV1::AuthorityChanged => {
            readiness_authority_mismatch(DatabaseCapabilityV1::Serving)
        }
        RuntimeServingPersistenceErrorV1::Timeout
        | RuntimeServingPersistenceErrorV1::Concurrency
        | RuntimeServingPersistenceErrorV1::Unavailable
        | RuntimeServingPersistenceErrorV1::Indeterminate => {
            readiness_unavailable(DatabaseCapabilityV1::Serving)
        }
        _ => readiness_rejected(DatabaseCapabilityV1::Serving),
    }
}

fn interaction_readiness_error(
    error: RuntimeInteractionPersistenceErrorV1,
) -> RuntimeDatabaseCompositionErrorV1 {
    match error {
        RuntimeInteractionPersistenceErrorV1::InvalidAuthority => {
            readiness_authority_mismatch(DatabaseCapabilityV1::Interaction)
        }
        RuntimeInteractionPersistenceErrorV1::Timeout
        | RuntimeInteractionPersistenceErrorV1::Unavailable
        | RuntimeInteractionPersistenceErrorV1::Indeterminate => {
            readiness_unavailable(DatabaseCapabilityV1::Interaction)
        }
        _ => readiness_rejected(DatabaseCapabilityV1::Interaction),
    }
}

struct RuntimeDatabaseAuthorityObservationV1<'a> {
    database_identity: &'a str,
    database_name: &'a str,
    executor_role: &'a str,
}

fn execution_authority(
    readiness: &RuntimeExecutionDatabaseReadinessV1,
) -> RuntimeDatabaseAuthorityObservationV1<'_> {
    authority(
        &readiness.database_identity,
        &readiness.database_name,
        &readiness.executor_role,
    )
}

fn exact_target_authority(
    readiness: &RuntimeExactTargetDatabaseReadinessV1,
) -> RuntimeDatabaseAuthorityObservationV1<'_> {
    authority(
        &readiness.database_identity,
        &readiness.database_name,
        &readiness.executor_role,
    )
}

fn panel_authority(
    readiness: &RuntimePanelDatabaseReadinessV1,
) -> RuntimeDatabaseAuthorityObservationV1<'_> {
    authority(
        &readiness.database_identity,
        &readiness.database_name,
        &readiness.executor_role,
    )
}

fn serving_authority(
    readiness: &RuntimeServingDatabaseReadinessV1,
) -> RuntimeDatabaseAuthorityObservationV1<'_> {
    authority(
        &readiness.database_identity,
        &readiness.database_name,
        &readiness.executor_role,
    )
}

fn interaction_authority(
    readiness: &RuntimeInteractionDatabaseReadinessV1,
) -> RuntimeDatabaseAuthorityObservationV1<'_> {
    authority(
        &readiness.database_identity,
        &readiness.database_name,
        &readiness.executor_role,
    )
}

fn authority<'a>(
    database_identity: &'a str,
    database_name: &'a str,
    executor_role: &'a str,
) -> RuntimeDatabaseAuthorityObservationV1<'a> {
    RuntimeDatabaseAuthorityObservationV1 {
        database_identity,
        database_name,
        executor_role,
    }
}

fn validate_readiness_authorities_v1(
    observations: [RuntimeDatabaseAuthorityObservationV1<'_>; 5],
) -> Result<(), RuntimeDatabaseCompositionErrorV1> {
    let expected_identity = observations[0].database_identity;
    let expected_name = observations[0].database_name;
    if observations.iter().any(|observation| {
        observation.database_identity != expected_identity
            || observation.database_name != expected_name
    }) {
        return Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch);
    }
    let roles = observations
        .iter()
        .map(|observation| observation.executor_role)
        .collect::<BTreeSet<_>>();
    if roles.len() != observations.len() {
        return Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch);
    }
    Ok(())
}

fn aggregate_readiness_v1(
    execution: RuntimeExecutionDatabaseReadinessV1,
    exact_target: RuntimeExactTargetDatabaseReadinessV1,
    panel: RuntimePanelDatabaseReadinessV1,
    serving: RuntimeServingDatabaseReadinessV1,
    interaction: RuntimeInteractionDatabaseReadinessV1,
) -> Result<RuntimeDatabaseReadinessV1, RuntimeDatabaseCompositionErrorV1> {
    validate_readiness_authorities_v1([
        execution_authority(&execution),
        exact_target_authority(&exact_target),
        panel_authority(&panel),
        serving_authority(&serving),
        interaction_authority(&interaction),
    ])?;
    let normalize =
        |kind, database_identity: &str, database_name: &str, executor_role: &str, checked_at| {
            RuntimeCapabilityReadinessReceiptV2::new(
                kind,
                database_identity,
                database_name,
                executor_role,
                checked_at,
            )
            .map_err(|_| RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)
        };
    let capability_receipts = RuntimeCapabilityReadinessSetV2::new(
        normalize(
            RuntimeCapabilityReadinessKindV2::Convergence,
            &execution.database_identity,
            &execution.database_name,
            &execution.executor_role,
            execution.checked_at,
        )?,
        normalize(
            RuntimeCapabilityReadinessKindV2::ExactTarget,
            &exact_target.database_identity,
            &exact_target.database_name,
            &exact_target.executor_role,
            exact_target.checked_at,
        )?,
        normalize(
            RuntimeCapabilityReadinessKindV2::Panel,
            &panel.database_identity,
            &panel.database_name,
            &panel.executor_role,
            panel.checked_at,
        )?,
        normalize(
            RuntimeCapabilityReadinessKindV2::Serving,
            &serving.database_identity,
            &serving.database_name,
            &serving.executor_role,
            serving.checked_at,
        )?,
        normalize(
            RuntimeCapabilityReadinessKindV2::Interaction,
            &interaction.database_identity,
            &interaction.database_name,
            &interaction.executor_role,
            interaction.checked_at,
        )?,
    )
    .map_err(|_| RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)?;
    Ok(RuntimeDatabaseReadinessV1 {
        execution,
        exact_target,
        panel,
        serving,
        interaction,
        capability_receipts,
    })
}

struct ConnectedRuntimeDatabasePoolsV1 {
    convergence: PgPool,
    exact_target: PgPool,
    panel: PgPool,
    serving: PgPool,
    interaction: PgPool,
}

impl ConnectedRuntimeDatabasePoolsV1 {
    fn pools(&self) -> [&PgPool; 5] {
        [
            &self.convergence,
            &self.exact_target,
            &self.panel,
            &self.serving,
            &self.interaction,
        ]
    }

    fn shutdown(&self) -> RuntimeDatabasePoolShutdownV1 {
        RuntimeDatabasePoolShutdownV1 {
            pools: Arc::new([
                self.convergence.clone(),
                self.exact_target.clone(),
                self.panel.clone(),
                self.serving.clone(),
                self.interaction.clone(),
            ]),
        }
    }

    async fn close_until(
        &self,
        deadline: Instant,
    ) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
        close_pool_refs_until(self.pools().map(Some), deadline).await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeDatabasePoolConnectErrorV1 {
    Configuration,
    UnsafeTransport,
    Unavailable,
}

async fn connect_database_pools_v1(
    secrets: &ResolvedRuntimeSecretsV1,
    config: DatabasePoolConfigV1,
    startup_deadline: Instant,
) -> Result<ConnectedRuntimeDatabasePoolsV1, RuntimeDatabaseCompositionErrorV1> {
    let database_secrets = secrets.database_secrets();
    let (convergence, exact_target, panel, serving, interaction) = tokio::join!(
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Convergence),
            DatabaseCapabilityV1::Convergence,
            config,
            startup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::ExactTarget),
            DatabaseCapabilityV1::ExactTarget,
            config,
            startup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Panel),
            DatabaseCapabilityV1::Panel,
            config,
            startup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Serving),
            DatabaseCapabilityV1::Serving,
            config,
            startup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Interaction),
            DatabaseCapabilityV1::Interaction,
            config,
            startup_deadline,
        ),
    );
    let results = [&convergence, &exact_target, &panel, &serving, &interaction];
    if results.iter().any(|result| result.is_err()) {
        let error = first_database_error(results);
        let _shutdown_result =
            close_pool_refs_until(results.map(|result| result.as_ref().ok()), startup_deadline)
                .await;
        return Err(error);
    }
    Ok(ConnectedRuntimeDatabasePoolsV1 {
        convergence: convergence.expect("runtime database results were checked"),
        exact_target: exact_target.expect("runtime database results were checked"),
        panel: panel.expect("runtime database results were checked"),
        serving: serving.expect("runtime database results were checked"),
        interaction: interaction.expect("runtime database results were checked"),
    })
}

fn first_database_error<T>(
    results: [&Result<T, RuntimeDatabasePoolConnectErrorV1>; 5],
) -> RuntimeDatabaseCompositionErrorV1 {
    DatabaseCapabilityV1::ALL
        .into_iter()
        .zip(results)
        .find_map(|(capability, result)| {
            result
                .as_ref()
                .err()
                .copied()
                .map(|error| map_database_connect_error(capability, error))
        })
        .expect("runtime database results contain a checked failure")
}

fn map_database_connect_error(
    capability: DatabaseCapabilityV1,
    error: RuntimeDatabasePoolConnectErrorV1,
) -> RuntimeDatabaseCompositionErrorV1 {
    match error {
        RuntimeDatabasePoolConnectErrorV1::Configuration => {
            RuntimeDatabaseCompositionErrorV1::ConnectionConfiguration { capability }
        }
        RuntimeDatabasePoolConnectErrorV1::UnsafeTransport => {
            RuntimeDatabaseCompositionErrorV1::UnsafeTransport { capability }
        }
        RuntimeDatabasePoolConnectErrorV1::Unavailable => {
            RuntimeDatabaseCompositionErrorV1::Unavailable { capability }
        }
    }
}

async fn connect_pool_v1(
    database_url: &crate::RuntimeDatabaseUrlSecretV1,
    capability: DatabaseCapabilityV1,
    config: DatabasePoolConfigV1,
    startup_deadline: Instant,
) -> Result<PgPool, RuntimeDatabasePoolConnectErrorV1> {
    let options = database_connect_options_v1(database_url.connection_secret(), capability);
    validate_database_transport_v1(&options)?;
    let pool = PgPoolOptions::new()
        .min_connections(0)
        .max_connections(config.max_connections_per_capability().get())
        .acquire_timeout(config.acquire_timeout())
        .idle_timeout(Some(config.idle_timeout()))
        .max_lifetime(Some(config.max_lifetime()))
        .test_before_acquire(true);
    let acquire_deadline = (Instant::now() + config.acquire_timeout()).min(startup_deadline);
    match timeout_at(acquire_deadline, pool.connect_with(options)).await {
        Ok(Ok(pool)) => Ok(pool),
        Ok(Err(_)) | Err(_) => Err(RuntimeDatabasePoolConnectErrorV1::Unavailable),
    }
}

fn database_connect_options_v1(
    secret: &RuntimeDatabaseConnectionSecretV1,
    capability: DatabaseCapabilityV1,
) -> PgConnectOptions {
    let ssl_mode = match secret.ssl_mode() {
        RuntimeDatabaseSslModeV1::Disable => PgSslMode::Disable,
        RuntimeDatabaseSslModeV1::VerifyFull => PgSslMode::VerifyFull,
    };
    let mut options = PgConnectOptions::new_without_pgpass()
        .port(secret.port())
        .username(secret.username())
        .password(secret.password().expose_secret())
        .database(secret.database())
        .ssl_mode(ssl_mode)
        .application_name(database_application_name(capability))
        .disable_statement_logging();
    options = match secret.endpoint() {
        RuntimeDatabaseEndpointV1::Network(host) => options.host(host),
        RuntimeDatabaseEndpointV1::Socket(path) => options.socket(path),
    };
    if let Some(root_cert) = secret.ssl_root_cert() {
        options = options.ssl_root_cert(root_cert);
    }
    options
}

fn database_application_name(capability: DatabaseCapabilityV1) -> &'static str {
    match capability {
        DatabaseCapabilityV1::Convergence => "starring-runtime-convergence",
        DatabaseCapabilityV1::ExactTarget => "starring-runtime-exact-target",
        DatabaseCapabilityV1::Panel => "starring-runtime-panel",
        DatabaseCapabilityV1::Serving => "starring-runtime-serving",
        DatabaseCapabilityV1::Interaction => "starring-runtime-interaction",
    }
}

fn validate_database_transport_v1(
    options: &PgConnectOptions,
) -> Result<(), RuntimeDatabasePoolConnectErrorV1> {
    if options.get_options().is_some() {
        return Err(RuntimeDatabasePoolConnectErrorV1::Configuration);
    }
    let local = options.get_socket().is_some() || database_host_is_loopback(options.get_host());
    if !local && !matches!(options.get_ssl_mode(), PgSslMode::VerifyFull) {
        return Err(RuntimeDatabasePoolConnectErrorV1::UnsafeTransport);
    }
    if options.get_socket().is_some() && !matches!(options.get_ssl_mode(), PgSslMode::Disable) {
        return Err(RuntimeDatabasePoolConnectErrorV1::UnsafeTransport);
    }
    Ok(())
}

fn database_host_is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

async fn close_pool_refs_with_deadline(
    pools: [Option<&PgPool>; 5],
) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
    await_pool_shutdown_with_timeout(begin_pool_closures(pools), DATABASE_POOL_SHUTDOWN_TIMEOUT)
        .await
}

async fn close_pool_refs_until(
    pools: [Option<&PgPool>; 5],
    deadline: Instant,
) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
    timeout_at(deadline, begin_pool_closures(pools))
        .await
        .map_err(|_| RuntimeDatabasePoolShutdownErrorV1::TimedOut)
}

fn begin_pool_closures<'a>(pools: [Option<&'a PgPool>; 5]) -> impl Future<Output = ()> + 'a {
    let [convergence, exact_target, panel, serving, interaction] = pools;
    let convergence = convergence.map(PgPool::close);
    let exact_target = exact_target.map(PgPool::close);
    let panel = panel.map(PgPool::close);
    let serving = serving.map(PgPool::close);
    let interaction = interaction.map(PgPool::close);
    async move {
        tokio::join!(
            await_optional_pool_close(convergence),
            await_optional_pool_close(exact_target),
            await_optional_pool_close(panel),
            await_optional_pool_close(serving),
            await_optional_pool_close(interaction),
        );
    }
}

async fn await_optional_pool_close<F>(close: Option<F>)
where
    F: Future<Output = ()>,
{
    if let Some(close) = close {
        close.await;
    }
}

async fn await_pool_shutdown_with_timeout<F>(
    close: F,
    deadline: Duration,
) -> Result<(), RuntimeDatabasePoolShutdownErrorV1>
where
    F: Future<Output = ()>,
{
    timeout(deadline, close)
        .await
        .map_err(|_| RuntimeDatabasePoolShutdownErrorV1::TimedOut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_authority_is_checked_before_connecting() {
        let valid = [
            ("starring", "role_a"),
            ("starring", "role_b"),
            ("starring", "role_c"),
            ("starring", "role_d"),
            ("starring", "role_e"),
        ];
        assert_eq!(verify_expected_database_names_and_roles_v1(valid), Ok(()));
        let duplicate_role = [
            ("starring", "role_a"),
            ("starring", "role_b"),
            ("starring", "role_c"),
            ("starring", "role_d"),
            ("starring", "role_a"),
        ];
        assert_eq!(
            verify_expected_database_names_and_roles_v1(duplicate_role),
            Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)
        );
        let different_database = [
            ("starring", "role_a"),
            ("starring", "role_b"),
            ("other", "role_c"),
            ("starring", "role_d"),
            ("starring", "role_e"),
        ];
        assert_eq!(
            verify_expected_database_names_and_roles_v1(different_database),
            Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)
        );
    }

    #[test]
    fn aggregate_requires_one_database_and_five_distinct_roles() {
        let observations = [
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_a"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_b"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_c"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_d"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_e"),
        ];
        assert_eq!(validate_readiness_authorities_v1(observations), Ok(()));
        let duplicate = [
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_a"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_b"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_c"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_d"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_a"),
        ];
        assert_eq!(
            validate_readiness_authorities_v1(duplicate),
            Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)
        );
    }

    #[test]
    fn aggregate_rejects_database_identity_or_name_mix() {
        let identity_mix = [
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_a"),
            authority("11234567-89ab-cdef-8123-456789abcdef", "starring", "role_b"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_c"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_d"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_e"),
        ];
        assert_eq!(
            validate_readiness_authorities_v1(identity_mix),
            Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)
        );
        let name_mix = [
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_a"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "other", "role_b"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_c"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_d"),
            authority("01234567-89ab-cdef-8123-456789abcdef", "starring", "role_e"),
        ];
        assert_eq!(
            validate_readiness_authorities_v1(name_mix),
            Err(RuntimeDatabaseCompositionErrorV1::AuthorityMismatch)
        );
    }

    #[test]
    fn aggregate_preserves_all_five_exact_readiness_receipts() {
        let identity = "01234567-89ab-cdef-8123-456789abcdef";
        let database = "starring";
        let execution = RuntimeExecutionDatabaseReadinessV1 {
            database_identity: identity.to_string(),
            database_name: database.to_string(),
            executor_role: "role_a".to_string(),
            checked_at: chrono::DateTime::from_timestamp(1, 0).unwrap(),
        };
        let exact_target = RuntimeExactTargetDatabaseReadinessV1 {
            database_identity: identity.to_string(),
            database_name: database.to_string(),
            executor_role: "role_b".to_string(),
            checked_at: chrono::DateTime::from_timestamp(2, 0).unwrap(),
        };
        let panel = RuntimePanelDatabaseReadinessV1 {
            database_identity: identity.to_string(),
            database_name: database.to_string(),
            executor_role: "role_c".to_string(),
            checked_at: chrono::DateTime::from_timestamp(3, 0).unwrap(),
        };
        let serving = RuntimeServingDatabaseReadinessV1 {
            database_identity: identity.to_string(),
            database_name: database.to_string(),
            executor_role: "role_d".to_string(),
            checked_at: chrono::DateTime::from_timestamp(4, 0).unwrap(),
        };
        let interaction = RuntimeInteractionDatabaseReadinessV1 {
            database_identity: identity.to_string(),
            database_name: database.to_string(),
            executor_role: "role_e".to_string(),
            checked_at: chrono::DateTime::from_timestamp(5, 0).unwrap(),
        };

        let readiness = aggregate_readiness_v1(
            execution.clone(),
            exact_target.clone(),
            panel.clone(),
            serving.clone(),
            interaction.clone(),
        )
        .unwrap();

        assert!(readiness.is_verified());
        assert_eq!(readiness.execution, execution);
        assert_eq!(readiness.exact_target, exact_target);
        assert_eq!(readiness.panel, panel);
        assert_eq!(readiness.serving, serving);
        assert_eq!(readiness.interaction, interaction);
        assert_eq!(
            readiness.exact_capability_receipts().checked_at_bounds(),
            (
                chrono::DateTime::from_timestamp(1, 0).unwrap(),
                chrono::DateTime::from_timestamp(5, 0).unwrap(),
            )
        );
        assert_eq!(
            format!("{readiness:?}"),
            "RuntimeDatabaseReadinessV1(<redacted>)"
        );
    }

    #[test]
    fn transport_requires_authenticated_remote_tls() {
        let insecure_remote = PgConnectOptions::new_without_pgpass()
            .host("database.example")
            .ssl_mode(PgSslMode::Require);
        let authenticated_remote = PgConnectOptions::new_without_pgpass()
            .host("database.example")
            .ssl_mode(PgSslMode::VerifyFull);
        assert_eq!(
            validate_database_transport_v1(&insecure_remote),
            Err(RuntimeDatabasePoolConnectErrorV1::UnsafeTransport)
        );
        assert_eq!(
            validate_database_transport_v1(&authenticated_remote),
            Ok(())
        );
    }

    #[test]
    fn transport_allows_loopback_and_local_socket() {
        let loopback = PgConnectOptions::new_without_pgpass()
            .host("127.0.0.1")
            .ssl_mode(PgSslMode::Disable);
        let socket = PgConnectOptions::new_without_pgpass()
            .socket("/private/tmp")
            .ssl_mode(PgSslMode::Disable);
        assert_eq!(validate_database_transport_v1(&loopback), Ok(()));
        assert_eq!(validate_database_transport_v1(&socket), Ok(()));
    }

    #[tokio::test]
    async fn shutdown_is_concurrent_idempotent_and_redacted() {
        let shutdown = RuntimeDatabasePoolShutdownV1 {
            pools: Arc::new(std::array::from_fn(|_| {
                PgPoolOptions::new()
                    .connect_lazy("postgresql://localhost/starring")
                    .unwrap()
            })),
        };
        assert_eq!(
            format!("{shutdown:?}"),
            "RuntimeDatabasePoolShutdownV1(<redacted>)"
        );
        let close = begin_pool_closures(shutdown.pools.each_ref().map(Some));
        assert!(shutdown.is_closed());
        close.await;
        assert_eq!(shutdown.close().await, Ok(()));
        assert_eq!(shutdown.close().await, Ok(()));
    }

    #[tokio::test]
    async fn shutdown_timeout_is_typed_and_redacted() {
        let result = await_pool_shutdown_with_timeout(
            std::future::pending::<()>(),
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(result, Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut));
        assert_eq!(
            format!("{:?}", RuntimeDatabasePoolShutdownErrorV1::TimedOut),
            "RuntimeDatabasePoolShutdownErrorV1(<redacted>)"
        );
    }

    #[tokio::test]
    async fn expired_startup_cleanup_still_closes_every_pool() {
        let pools: [PgPool; 5] = std::array::from_fn(|_| {
            PgPoolOptions::new()
                .connect_lazy("postgresql://localhost/starring")
                .unwrap()
        });
        let _result = close_pool_refs_until(pools.each_ref().map(Some), Instant::now()).await;
        assert!(pools.iter().all(PgPool::is_closed));
    }

    #[test]
    fn periodic_probe_budget_is_separate_and_shorter_than_startup() {
        assert!(PERIODIC_READINESS_TIMEOUT < STARTUP_READINESS_TIMEOUT);
    }

    #[test]
    fn first_connection_failure_is_stable_and_capability_scoped() {
        let results = [
            Ok(()),
            Err(RuntimeDatabasePoolConnectErrorV1::Unavailable),
            Err(RuntimeDatabasePoolConnectErrorV1::Configuration),
            Ok(()),
            Ok(()),
        ];
        let references = std::array::from_fn(|index| &results[index]);
        let error = first_database_error(references);
        assert_eq!(
            error,
            RuntimeDatabaseCompositionErrorV1::Unavailable {
                capability: DatabaseCapabilityV1::ExactTarget,
            }
        );
        assert_eq!(error.code(), "runtime_database_unavailable");
        assert_eq!(error.context(), Some("exact_target"));
    }

    #[test]
    fn readiness_failures_preserve_authority_transient_and_rejected_classes() {
        let authority = execution_readiness_error(
            RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch,
        );
        assert_eq!(
            authority,
            RuntimeDatabaseCompositionErrorV1::ReadinessAuthorityMismatch {
                capability: DatabaseCapabilityV1::Convergence,
            }
        );
        assert_eq!(
            authority.code(),
            "runtime_database_readiness_authority_mismatch"
        );
        let transient = panel_readiness_error(RuntimePanelPersistenceErrorV1::Timeout);
        assert_eq!(
            transient,
            RuntimeDatabaseCompositionErrorV1::ReadinessUnavailable {
                capability: DatabaseCapabilityV1::Panel,
            }
        );
        assert_eq!(transient.context(), Some("panel"));
        let rejected =
            interaction_readiness_error(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        assert_eq!(
            rejected,
            RuntimeDatabaseCompositionErrorV1::ReadinessRejected {
                capability: DatabaseCapabilityV1::Interaction,
            }
        );
    }
}
