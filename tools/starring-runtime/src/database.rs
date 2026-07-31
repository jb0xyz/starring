use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::net::IpAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use automation_runtime::{
    GatewayConnectionObserverV3, GatewayReadyLeaseV3, InstanceTeardownRetryExecutionFutureV1,
    InstanceTeardownRetryExecutionRequestV1, InstanceTeardownRetryScanFutureV1,
    InstanceTeardownRetryScanRequestV1, InstanceTeardownRetrySupervisorConfigV1,
    InstanceTeardownRetrySupervisorExitV1, InstanceTeardownRetrySupervisorPortV1,
    InstanceTeardownRetrySupervisorV1, OwnedSharedGatewayDispatchServicesCompositionErrorV3,
    OwnedSharedGatewayDispatchServicesV3, SharedGatewayAdmissionConfigV3,
    SharedGatewayInteractionEnvelopeV3, SharedGatewayInteractionReservationOutcomeV3,
    SharedGatewayReservedInteractionV3,
};
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
    RuntimeAuthorizedPendingDrainAcknowledgementV2, RuntimeAuthorizedPendingDrainClaimV2,
    RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3, RuntimeCapabilityReadinessKindV2,
    RuntimeCapabilityReadinessReceiptV2, RuntimeCapabilityReadinessSetV2,
    RuntimePendingDrainAcknowledgementExecutionPortV2, RuntimePendingDrainAcknowledgementReceiptV2,
    RuntimePendingDrainClaimExecutionPortV2, RuntimePendingDrainClaimReceiptV2,
    RuntimePendingDrainNoCandidateReceiptV2, RuntimePendingDrainNoCandidateRecorderPortV2,
    RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3,
    RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
    RuntimeSelectedPendingDrainNoCandidateV2,
};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use sqlx::ConnectOptions;
use tokio::time::{sleep_until, timeout, timeout_at, Instant as TokioInstant};
use zeroize::Zeroizing;

use crate::registry::RuntimeInteractionDispatchRegistryV1;
use crate::runtime_interaction_dispatch::{
    RuntimeInteractionDispatchFutureV1, RuntimeInteractionDispatchPortV1,
    RuntimeInteractionDispatchReservationOutcomeV1,
};
use crate::startup::RuntimeStartupBudgetV1;
use crate::{
    DatabaseCapabilityV1, DatabasePoolConfigV1, ResolvedRuntimeSecretsV1, RuntimeConfigV1,
    RuntimeDatabaseConnectionSecretV1, RuntimeDatabaseEndpointV1, RuntimeDatabaseSslModeV1,
    RuntimeDiscordBotTokenV1,
};

const PERIODIC_READINESS_TIMEOUT: Duration = Duration::from_secs(5);
const DATABASE_POOL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);
const INSTANCE_TEARDOWN_RETRY_CADENCE: Duration = Duration::from_secs(30);
const INSTANCE_TEARDOWN_RETRY_PAGE_LIMIT: usize = 32;
const INSTANCE_TEARDOWN_RETRY_CONCURRENCY: usize = 4;
const INSTANCE_TEARDOWN_RETRY_SCAN_TIMEOUT: Duration = Duration::from_secs(5);
const INSTANCE_TEARDOWN_RETRY_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) type RuntimeInteractionTeardownRetrySupervisorV1 = InstanceTeardownRetrySupervisorV1;
pub(crate) type RuntimeInteractionTeardownRetrySupervisorExitV1 =
    InstanceTeardownRetrySupervisorExitV1;

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
    #[error("runtime database startup cleanup timed out")]
    StartupCleanupTimedOut,
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
            Self::StartupCleanupTimedOut => "runtime_database_startup_cleanup_timed_out",
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
            | Self::StartupCleanupTimedOut
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

    pub(crate) async fn close_until(
        &self,
        deadline: StdInstant,
    ) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
        close_pool_refs_until(
            self.pools.each_ref().map(Some),
            TokioInstant::from_std(deadline),
        )
        .await
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

pub struct RuntimeDatabaseDependenciesV1 {
    execution: PostgresRuntimeExecutionV1,
    exact_target: PostgresRuntimeExactTargetReader,
    panel: PostgresRuntimePanelV1,
    serving: PostgresRuntimeServingLeaseV1,
    interaction: PostgresRuntimeInteractionV1,
    initial_readiness: RuntimeDatabaseReadinessV1,
    shutdown: RuntimeDatabasePoolShutdownV1,
}

#[derive(Clone)]
pub(crate) struct RuntimeDatabaseReadinessProbeV2 {
    execution: PostgresRuntimeExecutionV1,
    exact_target: PostgresRuntimeExactTargetReader,
    panel: PostgresRuntimePanelV1,
    serving: PostgresRuntimeServingLeaseV1,
    interaction: PostgresRuntimeInteractionV1,
}

impl RuntimeDatabaseReadinessProbeV2 {
    pub(crate) async fn verify_v2(
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
}

impl Debug for RuntimeDatabaseReadinessProbeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabaseReadinessProbeV2(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RuntimePendingDrainMutationDatabaseV3 {
    execution: PostgresRuntimeExecutionV1,
}

#[derive(Clone)]
pub(crate) struct RuntimeControllerDatabaseV2 {
    execution: PostgresRuntimeExecutionV1,
    exact_target: PostgresRuntimeExactTargetReader,
    panel: PostgresRuntimePanelV1,
    serving: PostgresRuntimeServingLeaseV1,
}

impl RuntimeControllerDatabaseV2 {
    pub(crate) fn execution(&self) -> &PostgresRuntimeExecutionV1 {
        &self.execution
    }

    pub(crate) fn exact_target(&self) -> &PostgresRuntimeExactTargetReader {
        &self.exact_target
    }

    pub(crate) fn panel(&self) -> &PostgresRuntimePanelV1 {
        &self.panel
    }

    pub(crate) fn serving(&self) -> &PostgresRuntimeServingLeaseV1 {
        &self.serving
    }
}

impl Debug for RuntimeControllerDatabaseV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeControllerDatabaseV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeInteractionDispatchCompositionErrorV1 {
    #[error("runtime interaction dispatch admission configuration is invalid")]
    AdmissionConfiguration,
    #[error("runtime interaction dispatch route configuration is invalid")]
    RouteConfiguration,
    #[error("runtime interaction dispatch service composition timed out")]
    TimedOut,
    #[error("runtime interaction dispatch role snapshot provider is unavailable")]
    SnapshotUnavailable,
}

impl RuntimeInteractionDispatchCompositionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::AdmissionConfiguration => "runtime_interaction_dispatch_admission_configuration",
            Self::RouteConfiguration => "runtime_interaction_dispatch_route_configuration",
            Self::TimedOut => "runtime_interaction_dispatch_composition_timed_out",
            Self::SnapshotUnavailable => "runtime_interaction_dispatch_snapshot_unavailable",
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeInteractionDispatchDatabasePortV1 {
    inner: Arc<OwnedSharedGatewayDispatchServicesV3<PostgresRuntimeInteractionV1>>,
}

impl Debug for RuntimeInteractionDispatchDatabasePortV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionDispatchDatabasePortV1(<redacted>)")
    }
}

impl RuntimeInteractionDispatchPortV1 for RuntimeInteractionDispatchDatabasePortV1 {
    type Reservation = SharedGatewayReservedInteractionV3;

    fn dispatch_capacity_v1(&self) -> std::num::NonZeroUsize {
        self.inner.dispatch_capacity_v3()
    }

    fn reserve_v1(
        &self,
        envelope: SharedGatewayInteractionEnvelopeV3,
        ready_lease: Option<GatewayReadyLeaseV3>,
        observer: &GatewayConnectionObserverV3,
    ) -> RuntimeInteractionDispatchReservationOutcomeV1<Self::Reservation> {
        match self.inner.reserve_v3(envelope, ready_lease, observer) {
            SharedGatewayInteractionReservationOutcomeV3::Reserved(reserved) => {
                RuntimeInteractionDispatchReservationOutcomeV1::Reserved(*reserved)
            }
            SharedGatewayInteractionReservationOutcomeV3::Ignored => {
                RuntimeInteractionDispatchReservationOutcomeV1::Ignored
            }
            SharedGatewayInteractionReservationOutcomeV3::Rejected { reason, envelope } => {
                RuntimeInteractionDispatchReservationOutcomeV1::Rejected { reason, envelope }
            }
        }
    }

    fn cancel_v1(&self, reservation: Self::Reservation) -> Box<SharedGatewayInteractionEnvelopeV3> {
        self.inner.cancel_v3(reservation)
    }

    fn dispatch_v1(
        self: Arc<Self>,
        reservation: Self::Reservation,
    ) -> RuntimeInteractionDispatchFutureV1 {
        Box::pin(async move { self.inner.dispatch_v3(reservation).await })
    }
}

struct RuntimeInteractionTeardownRetryDatabasePortV1 {
    inner: Arc<OwnedSharedGatewayDispatchServicesV3<PostgresRuntimeInteractionV1>>,
}

impl InstanceTeardownRetrySupervisorPortV1 for RuntimeInteractionTeardownRetryDatabasePortV1 {
    fn scan_retryable_v1(
        self: Arc<Self>,
        request: InstanceTeardownRetryScanRequestV1,
    ) -> InstanceTeardownRetryScanFutureV1 {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let (cursor, limit) = request.into_parts();
            inner.scan_teardown_retries_v1(&cursor, limit).await
        })
    }

    fn retry_teardown_v1(
        self: Arc<Self>,
        request: InstanceTeardownRetryExecutionRequestV1,
    ) -> InstanceTeardownRetryExecutionFutureV1 {
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let (guild_id, instance_id) = request.into_parts();
            inner.retry_teardown_v1(guild_id, instance_id).await
        })
    }
}

impl RuntimeInteractionDispatchDatabasePortV1 {
    pub(crate) fn start_teardown_retry_supervisor_v1(
        &self,
    ) -> RuntimeInteractionTeardownRetrySupervisorV1 {
        InstanceTeardownRetrySupervisorV1::start(
            RuntimeInteractionTeardownRetryDatabasePortV1 {
                inner: Arc::clone(&self.inner),
            },
            production_teardown_retry_config_v1(),
        )
    }
}

fn production_teardown_retry_config_v1() -> InstanceTeardownRetrySupervisorConfigV1 {
    InstanceTeardownRetrySupervisorConfigV1::new(
        INSTANCE_TEARDOWN_RETRY_CADENCE,
        NonZeroUsize::new(INSTANCE_TEARDOWN_RETRY_PAGE_LIMIT)
            .expect("instance teardown retry page limit is non-zero"),
        NonZeroUsize::new(INSTANCE_TEARDOWN_RETRY_CONCURRENCY)
            .expect("instance teardown retry concurrency is non-zero"),
        INSTANCE_TEARDOWN_RETRY_SCAN_TIMEOUT,
        INSTANCE_TEARDOWN_RETRY_TIMEOUT,
    )
    .expect("production instance teardown retry configuration is bounded")
}

impl RuntimePendingDrainMutationDatabaseV3 {
    pub(crate) async fn record_no_candidate_v3(
        &self,
        selection: &RuntimeSelectedPendingDrainNoCandidateV2,
        execution_cutoff: StdInstant,
    ) -> Result<RuntimePendingDrainNoCandidateReceiptV2, RuntimeExecutionPersistenceErrorV1> {
        self.execution
            .record_pending_drain_no_candidate(selection, execution_cutoff)
            .await
    }

    pub(crate) async fn execute_claim_v3(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainClaimV2,
        execution_cutoff: StdInstant,
    ) -> Result<RuntimePendingDrainClaimReceiptV2, RuntimeExecutionPersistenceErrorV1> {
        self.execution
            .execute_pending_drain_claim(authorization, execution_cutoff)
            .await
    }

    pub(crate) async fn execute_acknowledgement_v3(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainAcknowledgementV2,
        execution_cutoff: StdInstant,
    ) -> Result<RuntimePendingDrainAcknowledgementReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.execution
            .execute_pending_drain_acknowledgement(authorization, execution_cutoff)
            .await
    }

    pub(crate) async fn execute_succession_v3(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
        execution_cutoff: StdInstant,
    ) -> Result<
        RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
        RuntimeExecutionPersistenceErrorV1,
    > {
        self.execution
            .execute_pending_drain_succession_acknowledgement(authorization, execution_cutoff)
            .await
    }
}

impl Debug for RuntimePendingDrainMutationDatabaseV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainMutationDatabaseV3(<redacted>)")
    }
}

impl RuntimeDatabaseDependenciesV1 {
    pub fn execution(&self) -> &PostgresRuntimeExecutionV1 {
        &self.execution
    }

    pub(crate) fn pending_drain_mutation_v3(&self) -> RuntimePendingDrainMutationDatabaseV3 {
        RuntimePendingDrainMutationDatabaseV3 {
            execution: self.execution.clone(),
        }
    }

    pub(crate) fn runtime_controller_v2(&self) -> RuntimeControllerDatabaseV2 {
        RuntimeControllerDatabaseV2 {
            execution: self.execution.clone(),
            exact_target: self.exact_target.clone(),
            panel: self.panel.clone(),
            serving: self.serving.clone(),
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn compose_interaction_dispatch_port_v1(
        &self,
        registry: RuntimeInteractionDispatchRegistryV1,
        token: &RuntimeDiscordBotTokenV1,
        gateway: crate::GatewayResourceConfigV1,
        operation_deadline: StdInstant,
    ) -> Result<
        RuntimeInteractionDispatchDatabasePortV1,
        RuntimeInteractionDispatchCompositionErrorV1,
    > {
        let admission_config =
            SharedGatewayAdmissionConfigV3::new(gateway.global_admission_capacity())
                .map_err(|_| RuntimeInteractionDispatchCompositionErrorV1::AdmissionConfiguration)?
                .with_instance_lookup_timeout(gateway.instance_lookup_timeout())
                .map_err(|_| RuntimeInteractionDispatchCompositionErrorV1::RouteConfiguration)?;
        let inner = OwnedSharedGatewayDispatchServicesV3::compose_v3(
            Zeroizing::new(token.expose_secret().to_owned()),
            registry.into_registry_v1(),
            self.interaction.clone(),
            admission_config,
            operation_deadline,
        )
        .await
        .map_err(
            |error: OwnedSharedGatewayDispatchServicesCompositionErrorV3| match error {
                OwnedSharedGatewayDispatchServicesCompositionErrorV3::TimedOut => {
                    RuntimeInteractionDispatchCompositionErrorV1::TimedOut
                }
                OwnedSharedGatewayDispatchServicesCompositionErrorV3::SnapshotUnavailable => {
                    RuntimeInteractionDispatchCompositionErrorV1::SnapshotUnavailable
                }
            },
        )?;
        Ok(RuntimeInteractionDispatchDatabasePortV1 {
            inner: Arc::new(inner),
        })
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

    pub(crate) fn readiness_probe_v2(&self) -> RuntimeDatabaseReadinessProbeV2 {
        RuntimeDatabaseReadinessProbeV2 {
            execution: self.execution.clone(),
            exact_target: self.exact_target.clone(),
            panel: self.panel.clone(),
            serving: self.serving.clone(),
            interaction: self.interaction.clone(),
        }
    }

    pub async fn verify_readiness_v1(
        &self,
    ) -> Result<RuntimeDatabaseReadinessV1, RuntimeDatabaseCompositionErrorV1> {
        self.readiness_probe_v2().verify_v2().await
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) async fn verify_readiness_refresh_until_v2(
        &self,
        operation_cutoff: std::time::Instant,
    ) -> Result<RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseCompositionErrorV1> {
        let readiness = timeout_at(
            TokioInstant::from_std(operation_cutoff),
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

pub(crate) async fn compose_runtime_database_dependencies_v1(
    config: &RuntimeConfigV1,
    secrets: &ResolvedRuntimeSecretsV1,
    startup_budget: &RuntimeStartupBudgetV1,
) -> Result<RuntimeDatabaseDependenciesV1, RuntimeDatabaseCompositionErrorV1> {
    require_open_startup_operation_v1(startup_budget.operation_cutoff())?;
    let timeouts = RuntimeDatabaseTimeoutBundleV1::new(config)?;
    verify_expected_database_authority_v1(secrets)?;
    let pools = connect_database_pools_v1(
        secrets,
        config.database_pool(),
        startup_budget.operation_cutoff(),
        startup_budget.cleanup_deadline(),
    )
    .await?;
    if let Err(error) = require_open_startup_operation_v1(startup_budget.operation_cutoff()) {
        let cleanup = pools
            .close_until(TokioInstant::from_std(startup_budget.cleanup_deadline()))
            .await;
        return Err(map_database_startup_cleanup_result_v1(error, cleanup));
    }
    let build = build_verified_dependencies_v1(secrets, &pools, timeouts);
    let result = tokio::select! {
        biased;
        _ = sleep_until(TokioInstant::from_std(startup_budget.operation_cutoff())) => None,
        result = build => Some(result),
    };
    let operation_is_open = startup_budget.operation_is_open();
    let primary = match result {
        Some(Ok(dependencies)) if operation_is_open => return Ok(dependencies),
        Some(Ok(dependencies)) => {
            drop(dependencies);
            RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut
        }
        Some(Err(error)) if operation_is_open => error,
        Some(Err(_)) | None => RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
    };
    let cleanup = pools
        .close_until(TokioInstant::from_std(startup_budget.cleanup_deadline()))
        .await;
    Err(map_database_startup_cleanup_result_v1(primary, cleanup))
}

fn map_database_startup_cleanup_result_v1(
    primary: RuntimeDatabaseCompositionErrorV1,
    cleanup: Result<(), RuntimeDatabasePoolShutdownErrorV1>,
) -> RuntimeDatabaseCompositionErrorV1 {
    match cleanup {
        Ok(()) => primary,
        Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut) => {
            RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut
        }
    }
}

fn require_open_startup_operation_v1(
    operation_cutoff: StdInstant,
) -> Result<(), RuntimeDatabaseCompositionErrorV1> {
    if StdInstant::now() < operation_cutoff {
        Ok(())
    } else {
        Err(RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut)
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
        deadline: TokioInstant,
    ) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
        close_pool_refs_until(self.pools().map(Some), deadline).await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeDatabasePoolConnectErrorV1 {
    Configuration,
    UnsafeTransport,
    Unavailable,
    DeadlineElapsed,
    CleanupTimedOut,
}

async fn connect_database_pools_v1(
    secrets: &ResolvedRuntimeSecretsV1,
    config: DatabasePoolConfigV1,
    operation_cutoff: StdInstant,
    cleanup_deadline: StdInstant,
) -> Result<ConnectedRuntimeDatabasePoolsV1, RuntimeDatabaseCompositionErrorV1> {
    require_open_startup_operation_v1(operation_cutoff)?;
    let database_secrets = secrets.database_secrets();
    let (convergence, exact_target, panel, serving, interaction) = tokio::join!(
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Convergence),
            DatabaseCapabilityV1::Convergence,
            config,
            operation_cutoff,
            cleanup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::ExactTarget),
            DatabaseCapabilityV1::ExactTarget,
            config,
            operation_cutoff,
            cleanup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Panel),
            DatabaseCapabilityV1::Panel,
            config,
            operation_cutoff,
            cleanup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Serving),
            DatabaseCapabilityV1::Serving,
            config,
            operation_cutoff,
            cleanup_deadline,
        ),
        connect_pool_v1(
            database_secrets.database_url(DatabaseCapabilityV1::Interaction),
            DatabaseCapabilityV1::Interaction,
            config,
            operation_cutoff,
            cleanup_deadline,
        ),
    );
    let results = [&convergence, &exact_target, &panel, &serving, &interaction];
    let operation_is_open = StdInstant::now() < operation_cutoff;
    if results.iter().any(|result| result.is_err()) || !operation_is_open {
        let observed_error = results
            .iter()
            .any(|result| result.is_err())
            .then(|| first_database_error(results));
        let primary = match observed_error {
            Some(RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut) => {
                RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut
            }
            Some(error) if operation_is_open => error,
            Some(_) | None => RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
        };
        let cleanup = close_pool_refs_until(
            results.map(|result| result.as_ref().ok()),
            TokioInstant::from_std(cleanup_deadline),
        )
        .await;
        return Err(map_database_startup_cleanup_result_v1(primary, cleanup));
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
    if results.iter().any(|result| {
        matches!(
            result,
            Err(RuntimeDatabasePoolConnectErrorV1::CleanupTimedOut)
        )
    }) {
        return RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut;
    }
    if results.iter().any(|result| {
        matches!(
            result,
            Err(RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed)
        )
    }) {
        return RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut;
    }
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
        RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed => {
            RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut
        }
        RuntimeDatabasePoolConnectErrorV1::CleanupTimedOut => {
            RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut
        }
    }
}

async fn connect_pool_v1(
    database_url: &crate::RuntimeDatabaseUrlSecretV1,
    capability: DatabaseCapabilityV1,
    config: DatabasePoolConfigV1,
    operation_cutoff: StdInstant,
    cleanup_deadline: StdInstant,
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
    let started_at = StdInstant::now();
    let local_acquire_deadline = started_at + config.acquire_timeout();
    let (acquire_deadline, timeout_error) =
        select_database_acquire_deadline_v1(started_at, config.acquire_timeout(), operation_cutoff);
    let connect =
        begin_before_operation_cutoff_v1(operation_cutoff, || pool.connect_with(options))?;
    if let Some(error) = classify_database_connection_deadline_v1(
        StdInstant::now(),
        local_acquire_deadline,
        operation_cutoff,
    ) {
        return Err(error);
    }
    let result = tokio::select! {
        biased;
        _ = sleep_until(TokioInstant::from_std(acquire_deadline)) => {
            return Err(
                classify_database_connection_deadline_v1(
                    StdInstant::now(),
                    local_acquire_deadline,
                    operation_cutoff,
                )
                .unwrap_or(timeout_error)
            );
        }
        result = connect => result,
    };
    if let Some(error) = classify_database_connection_deadline_v1(
        StdInstant::now(),
        local_acquire_deadline,
        operation_cutoff,
    ) {
        return match result {
            Ok(pool) => Err(close_late_connected_pool_v1(pool, cleanup_deadline, error).await),
            Err(_) => Err(error),
        };
    }
    match result {
        Ok(pool) => Ok(pool),
        Err(_) => Err(RuntimeDatabasePoolConnectErrorV1::Unavailable),
    }
}

async fn close_late_connected_pool_v1(
    pool: PgPool,
    cleanup_deadline: StdInstant,
    primary: RuntimeDatabasePoolConnectErrorV1,
) -> RuntimeDatabasePoolConnectErrorV1 {
    let cleanup = close_pool_refs_until(
        [Some(&pool), None, None, None, None],
        TokioInstant::from_std(cleanup_deadline),
    )
    .await;
    drop(pool);
    match cleanup {
        Ok(()) => primary,
        Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut) => {
            RuntimeDatabasePoolConnectErrorV1::CleanupTimedOut
        }
    }
}

fn select_database_acquire_deadline_v1(
    started_at: StdInstant,
    acquire_timeout: Duration,
    operation_cutoff: StdInstant,
) -> (StdInstant, RuntimeDatabasePoolConnectErrorV1) {
    let acquire_deadline = started_at + acquire_timeout;
    if operation_cutoff <= acquire_deadline {
        (
            operation_cutoff,
            RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed,
        )
    } else {
        (
            acquire_deadline,
            RuntimeDatabasePoolConnectErrorV1::Unavailable,
        )
    }
}

fn classify_database_connection_deadline_v1(
    completed_at: StdInstant,
    local_acquire_deadline: StdInstant,
    operation_cutoff: StdInstant,
) -> Option<RuntimeDatabasePoolConnectErrorV1> {
    if completed_at >= operation_cutoff {
        Some(RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed)
    } else if completed_at >= local_acquire_deadline {
        Some(RuntimeDatabasePoolConnectErrorV1::Unavailable)
    } else {
        None
    }
}

fn begin_before_operation_cutoff_v1<T>(
    operation_cutoff: StdInstant,
    begin: impl FnOnce() -> T,
) -> Result<T, RuntimeDatabasePoolConnectErrorV1> {
    if StdInstant::now() < operation_cutoff {
        Ok(begin())
    } else {
        Err(RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed)
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
    deadline: TokioInstant,
) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
    let close = begin_pool_closures(pools);
    if TokioInstant::now() >= deadline {
        return Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut);
    }
    tokio::pin!(close);
    tokio::select! {
        biased;
        _ = sleep_until(deadline) => Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut),
        () = &mut close => {
            if TokioInstant::now() < deadline {
                Ok(())
            } else {
                Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut)
            }
        }
    }
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
    use std::cell::Cell;

    use super::*;

    #[test]
    fn teardown_retry_production_limits_are_exact_and_bounded() {
        let config = production_teardown_retry_config_v1();
        assert_eq!(config.cadence(), Duration::from_secs(30));
        assert_eq!(config.page_limit().get(), 32);
        assert_eq!(config.max_concurrency().get(), 4);
        assert_eq!(config.scan_timeout(), Duration::from_secs(5));
        assert_eq!(config.per_instance_timeout(), Duration::from_secs(60));
    }

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
        assert!(pools.iter().all(|pool| !pool.is_closed()));
        let result = close_pool_refs_until(pools.each_ref().map(Some), TokioInstant::now()).await;
        assert_eq!(result, Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut));
        assert!(pools.iter().all(PgPool::is_closed));
    }

    #[tokio::test]
    async fn late_connected_pool_cleanup_timeout_is_typed_and_fail_closed() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgresql://localhost/starring")
            .unwrap();
        let observation = pool.clone();

        assert_eq!(
            close_late_connected_pool_v1(
                pool,
                StdInstant::now(),
                RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed,
            )
            .await,
            RuntimeDatabasePoolConnectErrorV1::CleanupTimedOut
        );
        assert!(observation.is_closed());
    }

    #[test]
    fn periodic_probe_budget_remains_independent_and_bounded() {
        assert_eq!(PERIODIC_READINESS_TIMEOUT, Duration::from_secs(5));
    }

    #[test]
    fn acquire_deadline_uses_the_first_scoped_or_global_limit() {
        let started_at = StdInstant::now();
        let operation_cutoff = started_at + Duration::from_secs(20);
        let (scoped_deadline, scoped_error) = select_database_acquire_deadline_v1(
            started_at,
            Duration::from_secs(5),
            operation_cutoff,
        );
        let (global_deadline, global_error) = select_database_acquire_deadline_v1(
            started_at,
            Duration::from_secs(30),
            operation_cutoff,
        );
        let (equal_deadline, equal_error) = select_database_acquire_deadline_v1(
            started_at,
            Duration::from_secs(20),
            operation_cutoff,
        );

        assert_eq!(scoped_deadline, started_at + Duration::from_secs(5));
        assert_eq!(scoped_error, RuntimeDatabasePoolConnectErrorV1::Unavailable);
        assert_eq!(global_deadline, operation_cutoff);
        assert_eq!(
            global_error,
            RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed
        );
        assert_eq!(equal_deadline, operation_cutoff);
        assert_eq!(
            equal_error,
            RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed
        );
    }

    #[test]
    fn expired_operation_cutoff_does_not_create_connection_future() {
        let begin_called = Cell::new(false);
        let cutoff = StdInstant::now();
        let result = begin_before_operation_cutoff_v1(cutoff, || {
            begin_called.set(true);
        });

        assert_eq!(
            result,
            Err(RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed)
        );
        assert!(!begin_called.get());
    }

    #[test]
    fn actual_global_cutoff_preempts_a_nominally_earlier_acquire_timeout() {
        let started_at = StdInstant::now();
        let local_acquire_deadline = started_at + Duration::from_secs(5);
        let operation_cutoff = local_acquire_deadline + Duration::from_nanos(1);

        assert_eq!(
            classify_database_connection_deadline_v1(
                local_acquire_deadline,
                local_acquire_deadline,
                operation_cutoff,
            ),
            Some(RuntimeDatabasePoolConnectErrorV1::Unavailable)
        );
        assert_eq!(
            classify_database_connection_deadline_v1(
                operation_cutoff,
                local_acquire_deadline,
                operation_cutoff,
            ),
            Some(RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed)
        );
    }

    #[test]
    fn startup_cleanup_timeout_has_a_stable_public_failure_class() {
        let primary = RuntimeDatabaseCompositionErrorV1::Unavailable {
            capability: DatabaseCapabilityV1::Panel,
        };

        assert_eq!(
            map_database_startup_cleanup_result_v1(primary, Ok(())),
            primary
        );
        let cleanup = map_database_startup_cleanup_result_v1(
            primary,
            Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut),
        );
        assert_eq!(
            cleanup,
            RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut
        );
        assert_eq!(cleanup.code(), "runtime_database_startup_cleanup_timed_out");
        assert_eq!(cleanup.context(), None);
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
    fn global_operation_deadline_preempts_capability_scoped_connection_errors() {
        let results = [
            Err(RuntimeDatabasePoolConnectErrorV1::Unavailable),
            Err(RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed),
            Err(RuntimeDatabasePoolConnectErrorV1::Configuration),
            Ok(()),
            Ok(()),
        ];
        let references = std::array::from_fn(|index| &results[index]);

        assert_eq!(
            first_database_error(references),
            RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut
        );

        let cleanup_results = [
            Err(RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed),
            Err(RuntimeDatabasePoolConnectErrorV1::CleanupTimedOut),
            Err(RuntimeDatabasePoolConnectErrorV1::Configuration),
            Ok(()),
            Ok(()),
        ];
        let cleanup_references = std::array::from_fn(|index| &cleanup_results[index]);
        assert_eq!(
            first_database_error(cleanup_references),
            RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut
        );
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
