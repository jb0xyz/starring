use std::collections::VecDeque;
use std::future::pending;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1, RuntimeLiveAttestationDigestV2,
    RuntimeServingIdentityV2, RuntimeServingReceiptV2,
};
use automation_runtime_convergence::{
    DeploymentId, InstallationId, ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_serving_postgres::{
    RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1,
};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::json;

use super::{
    execute_runtime_serving_heartbeat_v2, start_runtime_serving_heartbeat_monitor_v2,
    start_runtime_serving_heartbeat_monitor_with_ports_v2, RuntimeServingHeartbeatDatabaseFutureV2,
    RuntimeServingHeartbeatDatabasePortV2, RuntimeServingHeartbeatExternalObserversV2,
    RuntimeServingHeartbeatFailureV2, RuntimeServingHeartbeatMonitorConfigErrorV2,
    RuntimeServingHeartbeatMonitorConfigV2, RuntimeServingHeartbeatMonitorExitV2,
    RuntimeServingHeartbeatMonitorOutcomeV2, RuntimeServingHeartbeatMonitorPhaseV2,
    RuntimeServingHeartbeatMonitorReadyV2, RuntimeServingHeartbeatMonitorV2,
    RuntimeServingHeartbeatRegistryPortV2, RuntimeServingHeartbeatRetainedStateV2,
    RuntimeServingHeartbeatRetainedV2, RuntimeServingHeartbeatStartFailureV2,
    RuntimeServingHeartbeatTerminalStatusV2,
};
use crate::registry::RuntimeRegistryBarrierBServingErrorV2;
use crate::{RuntimeShutdownCauseV1, RuntimeShutdownSignalLatchV1};

const CONTENT_HASH: &str = "9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6";
const BINDING_FINGERPRINT: &str =
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";

struct MockDatabaseV2 {
    observations:
        Mutex<VecDeque<Result<RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1>>>,
    heartbeats: Mutex<VecDeque<Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1>>>,
    observed_revisions: Mutex<Vec<u64>>,
    heartbeat_revisions: Mutex<Vec<u64>>,
}

impl MockDatabaseV2 {
    fn new_v2(
        observations: Vec<Result<RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1>>,
        heartbeats: Vec<Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1>>,
    ) -> Self {
        Self {
            observations: Mutex::new(observations.into()),
            heartbeats: Mutex::new(heartbeats.into()),
            observed_revisions: Mutex::new(Vec::new()),
            heartbeat_revisions: Mutex::new(Vec::new()),
        }
    }
}

impl RuntimeServingHeartbeatDatabasePortV2 for MockDatabaseV2 {
    fn observe_serving_v2<'a>(
        &'a self,
        identity: &'a RuntimeServingIdentityV2,
    ) -> RuntimeServingHeartbeatDatabaseFutureV2<
        'a,
        Result<RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1>,
    > {
        Box::pin(async move {
            self.observed_revisions
                .lock()
                .unwrap()
                .push(identity.revision.get());
            self.observations.lock().unwrap().pop_front().unwrap()
        })
    }

    fn heartbeat_serving_v2<'a>(
        &'a self,
        identity: &'a RuntimeServingIdentityV2,
        _lease_for: Duration,
    ) -> RuntimeServingHeartbeatDatabaseFutureV2<
        'a,
        Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1>,
    > {
        Box::pin(async move {
            self.heartbeat_revisions
                .lock()
                .unwrap()
                .push(identity.revision.get());
            self.heartbeats.lock().unwrap().pop_front().unwrap()
        })
    }
}

struct MockRegistryV2 {
    observations: Arc<Mutex<VecDeque<bool>>>,
}

impl MockRegistryV2 {
    fn exact_v2(count: usize) -> (Self, Arc<Mutex<VecDeque<bool>>>) {
        let observations = Arc::new(Mutex::new(VecDeque::from(vec![true; count])));
        (
            Self {
                observations: observations.clone(),
            },
            observations,
        )
    }
}

impl RuntimeServingHeartbeatRegistryPortV2 for MockRegistryV2 {
    fn observe_exact_serving_v2(&self) -> Result<(), RuntimeRegistryBarrierBServingErrorV2> {
        match self.observations.lock().unwrap().pop_front() {
            Some(true) => Ok(()),
            Some(false) | None => Err(RuntimeRegistryBarrierBServingErrorV2::ExactServingLost),
        }
    }
}

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn process_identity() -> RuntimeProcessIdentityV1 {
    let target: RuntimeDeploymentTargetV1 = serde_json::from_value(json!({
        "guild_id": "42",
        "ruleset_key": "studyroom",
        "version": 1,
        "content_hash": CONTENT_HASH,
        "binding_revision": 1,
        "binding_fingerprint": BINDING_FINGERPRINT
    }))
    .unwrap();
    RuntimeProcessIdentityV1 {
        target,
        runtime_generation: RuntimeGeneration::new(1).unwrap(),
        process_instance_id: ProcessInstanceId::parse("runtime-process:heartbeat").unwrap(),
    }
}

fn identity(revision: u64) -> RuntimeServingIdentityV2 {
    RuntimeServingIdentityV2 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse("tenant:heartbeat").unwrap(),
            installation_id: InstallationId::parse("installation:heartbeat").unwrap(),
            deployment_id: DeploymentId::parse("deployment:heartbeat").unwrap(),
        },
        operation_id: RuntimeCertificationOperationIdV2::parse("00112233445566778899aabbccddeeff")
            .unwrap(),
        attestation_digest: RuntimeLiveAttestationDigestV2::parse("e".repeat(64)).unwrap(),
        process_identity: process_identity(),
        lease_epoch: non_zero(9),
        revision: non_zero(revision),
    }
}

fn receipt(
    revision: u64,
    heartbeat: DateTime<Utc>,
    lease_for: Duration,
) -> RuntimeServingReceiptV2 {
    RuntimeServingReceiptV2 {
        identity: identity(revision),
        acquired_at: heartbeat - TimeDelta::seconds(1),
        last_heartbeat_at: heartbeat,
        expires_at: heartbeat + TimeDelta::from_std(lease_for).unwrap(),
        connected: true,
        serving: true,
    }
}

fn observation(receipt: RuntimeServingReceiptV2) -> RuntimeServingObservationV2 {
    RuntimeServingObservationV2::Current {
        observed_at: receipt.last_heartbeat_at,
        serving: Box::new(receipt),
    }
}

fn test_config() -> RuntimeServingHeartbeatMonitorConfigV2 {
    RuntimeServingHeartbeatMonitorConfigV2::new_v2(
        Duration::from_millis(100),
        Duration::from_secs(2),
        Duration::from_millis(100),
    )
    .unwrap()
}

#[test]
fn production_timing_is_fifteen_second_heartbeat_and_forty_five_second_lease() {
    let config = RuntimeServingHeartbeatMonitorConfigV2::production_v2();

    assert_eq!(config.interval_v2(), Duration::from_secs(15));
    assert_eq!(config.lease_for_v2(), Duration::from_secs(45));
    assert_eq!(config.operation_timeout_v2(), Duration::from_secs(5));
}

#[test]
fn timing_requires_millisecond_alignment_and_strict_lease_runway() {
    assert_eq!(
        RuntimeServingHeartbeatMonitorConfigV2::new_v2(
            Duration::ZERO,
            Duration::from_secs(45),
            Duration::from_secs(5),
        ),
        Err(RuntimeServingHeartbeatMonitorConfigErrorV2::InvalidInterval)
    );
    assert_eq!(
        RuntimeServingHeartbeatMonitorConfigV2::new_v2(
            Duration::from_secs(15),
            Duration::from_millis(999),
            Duration::from_secs(5),
        ),
        Err(RuntimeServingHeartbeatMonitorConfigErrorV2::InvalidLease)
    );
    assert_eq!(
        RuntimeServingHeartbeatMonitorConfigV2::new_v2(
            Duration::from_secs(15),
            Duration::from_secs(20),
            Duration::from_secs(5),
        ),
        Err(RuntimeServingHeartbeatMonitorConfigErrorV2::InsufficientRunway)
    );
}

#[tokio::test]
async fn exact_start_handshake_returns_ready_and_commanded_stop_returns_authority() {
    let current = receipt(17, Utc::now(), Duration::from_secs(2));
    let database = MockDatabaseV2::new_v2(vec![Ok(observation(current.clone()))], vec![]);
    let (registry, remaining_registry_observations) = MockRegistryV2::exact_v2(2);
    let latch = RuntimeShutdownSignalLatchV1::create();
    let ready = start_runtime_serving_heartbeat_monitor_with_ports_v2(
        current.clone(),
        database,
        registry,
        latch.trigger(),
        latch.observer(),
        RuntimeServingHeartbeatExternalObserversV2::without_gateway_v2(pending()),
        test_config(),
    )
    .await
    .unwrap();

    assert_eq!(
        ready.health_v2().phase_v2(),
        RuntimeServingHeartbeatMonitorPhaseV2::Ready
    );
    assert!(ready.health_v2().lease_deadline_v2() > Instant::now());
    assert!(ready.health_v2().last_confirmed_at_v2() <= Instant::now());
    assert!(remaining_registry_observations.lock().unwrap().is_empty());

    let monitor = ready.into_monitor_v2();
    assert_eq!(
        monitor.health_v2().phase_v2(),
        RuntimeServingHeartbeatMonitorPhaseV2::Ready
    );
    let exit = monitor
        .stop_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    let RuntimeServingHeartbeatMonitorExitV2::Commanded(retained) = exit else {
        panic!("expected commanded stop");
    };
    assert_eq!(retained.last_confirmed_receipt_v2(), &current);
    let (returned_receipt, _registry) = retained.into_parts_v2();
    assert_eq!(returned_receipt, current);
    assert!(latch.observed().is_none());
}

#[tokio::test]
async fn monitor_drop_trips_shutdown_and_aborts_owned_actor() {
    let current = receipt(17, Utc::now(), Duration::from_secs(2));
    let database = MockDatabaseV2::new_v2(vec![Ok(observation(current.clone()))], vec![]);
    let (registry, _) = MockRegistryV2::exact_v2(2);
    let latch = RuntimeShutdownSignalLatchV1::create();
    let ready = start_runtime_serving_heartbeat_monitor_with_ports_v2(
        current,
        database,
        registry,
        latch.trigger(),
        latch.observer(),
        RuntimeServingHeartbeatExternalObserversV2::without_gateway_v2(pending()),
        test_config(),
    )
    .await
    .unwrap();

    drop(ready.into_monitor_v2());
    tokio::task::yield_now().await;

    assert_eq!(
        latch.observed().unwrap().cause(),
        RuntimeShutdownCauseV1::SupervisorFailure
    );
}

#[tokio::test]
async fn process_shutdown_wait_returns_retained_exact_state() {
    let current = receipt(17, Utc::now(), Duration::from_secs(2));
    let database = MockDatabaseV2::new_v2(vec![Ok(observation(current.clone()))], vec![]);
    let (registry, _) = MockRegistryV2::exact_v2(2);
    let latch = RuntimeShutdownSignalLatchV1::create();
    let trigger = latch.trigger();
    let ready = start_runtime_serving_heartbeat_monitor_with_ports_v2(
        current.clone(),
        database,
        registry,
        trigger.clone(),
        latch.observer(),
        RuntimeServingHeartbeatExternalObserversV2::without_gateway_v2(pending()),
        test_config(),
    )
    .await
    .unwrap();

    let monitor = ready.into_monitor_v2();
    let mut terminal = monitor.terminal_observer_v2();
    assert_eq!(terminal.current_v2(), None);
    trigger.trip(RuntimeShutdownCauseV1::Explicit);
    assert_eq!(
        terminal.wait_v2().await,
        RuntimeServingHeartbeatTerminalStatusV2::ProcessShutdown
    );
    let exit = monitor.wait_v2().await;
    let RuntimeServingHeartbeatMonitorExitV2::ProcessShutdown(retained) = exit else {
        panic!("expected process shutdown");
    };
    assert_eq!(retained.last_confirmed_receipt_v2(), &current);
}

#[tokio::test]
async fn owner_loss_is_terminal_fail_closed_and_returns_retained_state() {
    let current = receipt(17, Utc::now(), Duration::from_secs(2));
    let database = MockDatabaseV2::new_v2(vec![Ok(observation(current.clone()))], vec![]);
    let (registry, _) = MockRegistryV2::exact_v2(2);
    let latch = RuntimeShutdownSignalLatchV1::create();
    let (owner_loss, owner_lost) = tokio::sync::oneshot::channel();
    let ready = start_runtime_serving_heartbeat_monitor_with_ports_v2(
        current.clone(),
        database,
        registry,
        latch.trigger(),
        latch.observer(),
        RuntimeServingHeartbeatExternalObserversV2::without_gateway_v2(async move {
            let _lost = owner_lost.await;
        }),
        test_config(),
    )
    .await
    .unwrap();
    let monitor = ready.into_monitor_v2();
    let mut terminal = monitor.terminal_observer_v2();

    owner_loss.send(()).unwrap();
    assert_eq!(
        terminal.wait_v2().await,
        RuntimeServingHeartbeatTerminalStatusV2::FailedClosed(
            RuntimeServingHeartbeatFailureV2::OwnerLost
        )
    );
    let exit = monitor.wait_v2().await;
    assert_eq!(
        exit.failure_v2(),
        Some(RuntimeServingHeartbeatFailureV2::OwnerLost)
    );
    assert_eq!(
        exit.into_retained_v2().unwrap().last_confirmed_receipt_v2(),
        &current
    );
    assert_eq!(
        latch.observed().unwrap().cause(),
        RuntimeShutdownCauseV1::GatewayOwnerTerminal
    );
}

#[tokio::test]
async fn product_authority_change_requests_controlled_process_restart() {
    let current = receipt(17, Utc::now(), Duration::from_secs(2));
    let database = MockDatabaseV2::new_v2(
        vec![Ok(observation(current.clone()))],
        vec![Err(RuntimeServingPersistenceErrorV1::AuthorityChanged)],
    );
    let (registry, remaining_registry_observations) = MockRegistryV2::exact_v2(3);
    let latch = RuntimeShutdownSignalLatchV1::create();
    let ready = start_runtime_serving_heartbeat_monitor_with_ports_v2(
        current.clone(),
        database,
        registry,
        latch.trigger(),
        latch.observer(),
        RuntimeServingHeartbeatExternalObserversV2::without_gateway_v2(pending()),
        test_config(),
    )
    .await
    .unwrap();
    let monitor = ready.into_monitor_v2();
    let mut terminal = monitor.terminal_observer_v2();

    assert_eq!(
        terminal.wait_v2().await,
        RuntimeServingHeartbeatTerminalStatusV2::FailedClosed(
            RuntimeServingHeartbeatFailureV2::ProductAuthorityChanged
        )
    );
    let exit = monitor.wait_v2().await;
    assert_eq!(
        exit.failure_v2(),
        Some(RuntimeServingHeartbeatFailureV2::ProductAuthorityChanged)
    );
    assert_eq!(
        exit.into_retained_v2().unwrap().last_confirmed_receipt_v2(),
        &current
    );
    assert!(remaining_registry_observations.lock().unwrap().is_empty());
    assert_eq!(
        latch.observed().unwrap().cause(),
        RuntimeShutdownCauseV1::ProductAuthorityChanged
    );
}

#[tokio::test]
async fn unknown_heartbeat_adopts_only_the_exact_one_step_successor() {
    let config = test_config();
    let current = receipt(17, Utc::now(), config.lease_for_v2());
    let successor_heartbeat = current.last_heartbeat_at + TimeDelta::milliseconds(1);
    let successor = RuntimeServingReceiptV2 {
        identity: identity(18),
        acquired_at: current.acquired_at,
        last_heartbeat_at: successor_heartbeat,
        expires_at: successor_heartbeat + TimeDelta::from_std(config.lease_for_v2()).unwrap(),
        connected: true,
        serving: true,
    };
    let database = MockDatabaseV2::new_v2(
        vec![
            Ok(RuntimeServingObservationV2::Diverged {
                observed_at: Utc::now(),
            }),
            Ok(observation(successor.clone())),
        ],
        vec![Err(RuntimeServingPersistenceErrorV1::Indeterminate)],
    );

    let result = execute_runtime_serving_heartbeat_v2(
        &database,
        &current,
        config,
        Instant::now() + Duration::from_secs(1),
    )
    .await
    .unwrap();

    assert_eq!(result, successor);
    assert_eq!(*database.heartbeat_revisions.lock().unwrap(), vec![17]);
    assert_eq!(*database.observed_revisions.lock().unwrap(), vec![17, 18]);
}

#[tokio::test]
async fn unknown_heartbeat_preserves_exact_old_receipt_when_not_applied() {
    let config = test_config();
    let current = receipt(17, Utc::now(), config.lease_for_v2());
    let database = MockDatabaseV2::new_v2(
        vec![Ok(observation(current.clone()))],
        vec![Err(RuntimeServingPersistenceErrorV1::Indeterminate)],
    );

    let result = execute_runtime_serving_heartbeat_v2(
        &database,
        &current,
        config,
        Instant::now() + Duration::from_secs(1),
    )
    .await
    .unwrap();

    assert_eq!(result, current);
    assert_eq!(*database.observed_revisions.lock().unwrap(), vec![17]);
}

#[tokio::test]
async fn unknown_heartbeat_rejects_foreign_or_more_than_one_step_state() {
    let config = test_config();
    let current = receipt(17, Utc::now(), config.lease_for_v2());
    let database = MockDatabaseV2::new_v2(
        vec![
            Ok(RuntimeServingObservationV2::Diverged {
                observed_at: Utc::now(),
            }),
            Ok(RuntimeServingObservationV2::Diverged {
                observed_at: Utc::now(),
            }),
        ],
        vec![Err(RuntimeServingPersistenceErrorV1::Indeterminate)],
    );

    let result = execute_runtime_serving_heartbeat_v2(
        &database,
        &current,
        config,
        Instant::now() + Duration::from_secs(1),
    )
    .await;

    assert_eq!(
        result,
        Err(RuntimeServingHeartbeatFailureV2::DatabaseServingDiverged)
    );
    assert_eq!(*database.observed_revisions.lock().unwrap(), vec![17, 18]);
}

#[tokio::test]
async fn stale_ingress_acknowledgement_fails_closed_without_retry() {
    let config = test_config();
    let current = receipt(17, Utc::now(), config.lease_for_v2());
    let database = MockDatabaseV2::new_v2(
        vec![],
        vec![Err(RuntimeServingPersistenceErrorV1::RetryNotReady)],
    );

    let result = execute_runtime_serving_heartbeat_v2(
        &database,
        &current,
        config,
        Instant::now() + Duration::from_secs(1),
    )
    .await;

    assert_eq!(
        result,
        Err(RuntimeServingHeartbeatFailureV2::IngressAcknowledgementLost)
    );
    assert!(database.observed_revisions.lock().unwrap().is_empty());
}

#[test]
fn monitor_surface_is_affine_typed_and_redacted() {
    fn assert_send<T: Send>() {}

    assert_send::<RuntimeServingHeartbeatMonitorV2>();
    assert_send::<RuntimeServingHeartbeatMonitorReadyV2>();
    assert_send::<RuntimeServingHeartbeatRetainedV2>();
    assert_send::<RuntimeServingHeartbeatMonitorOutcomeV2>();
    let _start = start_runtime_serving_heartbeat_monitor_v2;
    let _gateway_constructor = RuntimeServingHeartbeatExternalObserversV2::with_exact_gateway_v2::<
        std::future::Pending<()>,
    >;
    assert_eq!(
        RuntimeServingHeartbeatStartFailureV2::GatewayLost.code_v2(),
        "runtime_serving_heartbeat_start_gateway_lost"
    );
    assert_eq!(
        RuntimeServingHeartbeatFailureV2::GatewayLost.code_v2(),
        "runtime_serving_heartbeat_gateway_lost"
    );
    let current = receipt(17, Utc::now(), Duration::from_secs(2));
    let (registry, _) = MockRegistryV2::exact_v2(0);
    let exit = RuntimeServingHeartbeatMonitorExitV2::FailedClosed {
        failure: RuntimeServingHeartbeatFailureV2::GatewayLost,
        retained: RuntimeServingHeartbeatRetainedStateV2 {
            last_confirmed_receipt: current,
            registry,
        },
    };
    assert_eq!(
        exit.failure_v2(),
        Some(RuntimeServingHeartbeatFailureV2::GatewayLost)
    );
    assert!(exit.into_retained_v2().is_some());
}
