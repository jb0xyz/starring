use std::future::{poll_fn, Future};
use std::io::Write;
use std::task::Poll;
use std::time::Duration;

use product_control_http::{
    product_control_router_with_operational_v2_and_lifecycle_v1_and_readiness_gate,
    ProductApiReadinessGate, ProductControlFacade,
};
use starring_api::{
    compose_production_service_v1, resolve_production_secrets_v1,
    serve_verified_loopback_with_runtime_readiness, ComposedProductionServiceV1, DatabaseRoleV1,
    LoopbackServeErrorV1, LoopbackServerConfigV1, ProductionCompositionErrorV1, ProductionConfigV1,
    ProductionReadinessPhaseV1,
};
use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};

const RUNTIME_READINESS_INTERVAL: Duration = Duration::from_secs(30);
const RUNTIME_READINESS_TIMEOUT: Duration = Duration::from_secs(10);

fn main() {
    install_panic_hook();
    let status = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run()),
        Err(_) => ExitStatusV1::RuntimeInitializationFailed,
    };
    emit_status(status);
    if !status.success() {
        std::process::exit(1);
    }
}

async fn run() -> ExitStatusV1 {
    let mut shutdown = match ShutdownSignalsV1::register() {
        Ok(shutdown) => shutdown,
        Err(()) => return ExitStatusV1::SignalRegistrationFailed,
    };
    let config = match ProductionConfigV1::from_process_environment() {
        Ok(config) => config,
        Err(_) => return ExitStatusV1::ConfigurationInvalid,
    };
    if shutdown.received_now().await {
        return ExitStatusV1::StartupCancelled;
    }
    let secrets = match resolve_production_secrets_v1(&config) {
        Ok(secrets) => secrets,
        Err(_) => return ExitStatusV1::SecretResolutionFailed,
    };
    if shutdown.received_now().await {
        return ExitStatusV1::StartupCancelled;
    }
    let composition = compose_production_service_v1(config, secrets);
    tokio::pin!(composition);
    let service = tokio::select! {
        biased;
        _ = shutdown.wait() => {
            return finish_cancelled_composition(composition.await).await;
        }
        result = &mut composition => {
            match result {
                Ok(service) => service,
                Err(error) => return classify_composition_error(error),
            }
        }
    };
    if shutdown.received_now().await {
        return close_cancelled_service(service).await;
    }
    serve(service, shutdown).await
}

async fn finish_cancelled_composition(
    composition: Result<ComposedProductionServiceV1, ProductionCompositionErrorV1>,
) -> ExitStatusV1 {
    match composition {
        Ok(service) => close_cancelled_service(service).await,
        Err(_) => ExitStatusV1::StartupCancelled,
    }
}

async fn close_cancelled_service(service: ComposedProductionServiceV1) -> ExitStatusV1 {
    let (facade, _, _, database_shutdown) = service.into_parts();
    drop(facade);
    match database_shutdown.close().await {
        Ok(()) => ExitStatusV1::StartupCancelled,
        Err(_) => ExitStatusV1::DatabasePoolShutdownTimedOut,
    }
}

async fn serve(
    service: ComposedProductionServiceV1,
    mut shutdown: ShutdownSignalsV1,
) -> ExitStatusV1 {
    let (facade, http_boundary, bind_addr, database_shutdown) = service.into_parts();
    let readiness_gate = ProductApiReadinessGate::initially_unready();
    let router = product_control_router_with_operational_v2_and_lifecycle_v1_and_readiness_gate(
        facade.clone(),
        http_boundary,
        readiness_gate.clone(),
    );
    let startup_facade = facade.clone();
    let startup_probe = async move { startup_facade.readiness().await.is_ok() };
    let runtime_facade = facade.clone();
    let runtime_readiness_failure = monitor_runtime_readiness(
        move || {
            let runtime_facade = runtime_facade.clone();
            async move { runtime_facade.readiness().await.is_ok() }
        },
        || sleep(RUNTIME_READINESS_INTERVAL),
        RUNTIME_READINESS_TIMEOUT,
    );
    let server_result = serve_verified_loopback_with_runtime_readiness(
        bind_addr,
        router,
        readiness_gate,
        LoopbackServerConfigV1::production_default(),
        startup_probe,
        runtime_readiness_failure,
        shutdown.wait(),
    )
    .await;
    drop(facade);
    let shutdown_result = database_shutdown.close().await;
    if shutdown_result.is_err() {
        return ExitStatusV1::DatabasePoolShutdownTimedOut;
    }
    match server_result {
        Ok(_) => ExitStatusV1::CleanShutdown,
        Err(error) => classify_server_error(error),
    }
}

async fn monitor_runtime_readiness<P, PF, W, WF>(mut probe: P, mut wait: W, probe_timeout: Duration)
where
    P: FnMut() -> PF,
    PF: Future<Output = bool> + Send + 'static,
    W: FnMut() -> WF,
    WF: Future<Output = ()>,
{
    loop {
        wait().await;
        if !run_runtime_readiness_probe(probe(), probe_timeout).await {
            return;
        }
    }
}

async fn run_runtime_readiness_probe<P>(probe: P, probe_timeout: Duration) -> bool
where
    P: Future<Output = bool> + Send + 'static,
{
    let mut probes = JoinSet::new();
    probes.spawn(probe);
    let outcome = timeout(probe_timeout, probes.join_next()).await;
    let ready = matches!(outcome, Ok(Some(Ok(true))));
    if !ready {
        probes.abort_all();
        while probes.join_next().await.is_some() {}
    }
    ready
}

struct ShutdownSignalsV1 {
    interrupt: Signal,
    terminate: Signal,
}

impl ShutdownSignalsV1 {
    fn register() -> Result<Self, ()> {
        let interrupt = signal(SignalKind::interrupt()).map_err(|_| ())?;
        let terminate = signal(SignalKind::terminate()).map_err(|_| ())?;
        Ok(Self {
            interrupt,
            terminate,
        })
    }

    async fn wait(&mut self) {
        tokio::select! {
            biased;
            _ = self.terminate.recv() => {}
            _ = self.interrupt.recv() => {}
        }
    }

    async fn received_now(&mut self) -> bool {
        poll_fn(|context| {
            if self.terminate.poll_recv(context).is_ready()
                || self.interrupt.poll_recv(context).is_ready()
            {
                Poll::Ready(true)
            } else {
                Poll::Ready(false)
            }
        })
        .await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitStatusV1 {
    CleanShutdown,
    StartupCancelled,
    RuntimeInitializationFailed,
    SignalRegistrationFailed,
    ConfigurationInvalid,
    SecretResolutionFailed,
    HttpBoundaryConfigurationFailed,
    ProductIdentityConfigurationFailed,
    DiscordConfigurationFailed,
    DatabaseConfigurationFailed(DatabaseRoleV1),
    DatabaseTransportRejected(DatabaseRoleV1),
    DatabaseUnavailable(DatabaseRoleV1),
    ProductPersistenceConfigurationFailed,
    FacadeConfigurationFailed,
    DependencyReadinessFailed(ProductionReadinessPhaseV1),
    DependencyReadinessTimedOut(ProductionReadinessPhaseV1),
    ServerReadinessGateFailed,
    ServerBindRejected,
    ServerBindFailed,
    ServerStartupReadinessFailed,
    ServerRuntimeReadinessFailed,
    ServerAcceptFailed,
    ServerConnectionFailed,
    ServerDrainTimedOut,
    DatabasePoolShutdownTimedOut,
}

impl ExitStatusV1 {
    fn success(self) -> bool {
        matches!(self, Self::CleanShutdown | Self::StartupCancelled)
    }

    fn code(self) -> &'static str {
        match self {
            Self::CleanShutdown => "clean_shutdown",
            Self::StartupCancelled => "startup_cancelled",
            Self::RuntimeInitializationFailed => "runtime_initialization_failed",
            Self::SignalRegistrationFailed => "signal_registration_failed",
            Self::ConfigurationInvalid => "configuration_invalid",
            Self::SecretResolutionFailed => "secret_resolution_failed",
            Self::HttpBoundaryConfigurationFailed => "http_boundary_configuration_failed",
            Self::ProductIdentityConfigurationFailed => "product_identity_configuration_failed",
            Self::DiscordConfigurationFailed => "discord_configuration_failed",
            Self::DatabaseConfigurationFailed(_) => "database_configuration_failed",
            Self::DatabaseTransportRejected(_) => "database_transport_rejected",
            Self::DatabaseUnavailable(_) => "database_unavailable",
            Self::ProductPersistenceConfigurationFailed => {
                "product_persistence_configuration_failed"
            }
            Self::FacadeConfigurationFailed => "facade_configuration_failed",
            Self::DependencyReadinessFailed(_) => "dependency_readiness_failed",
            Self::DependencyReadinessTimedOut(_) => "dependency_readiness_timed_out",
            Self::ServerReadinessGateFailed => "server_readiness_gate_failed",
            Self::ServerBindRejected => "server_bind_rejected",
            Self::ServerBindFailed => "server_bind_failed",
            Self::ServerStartupReadinessFailed => "server_startup_readiness_failed",
            Self::ServerRuntimeReadinessFailed => "server_runtime_readiness_failed",
            Self::ServerAcceptFailed => "server_accept_failed",
            Self::ServerConnectionFailed => "server_connection_failed",
            Self::ServerDrainTimedOut => "server_drain_timed_out",
            Self::DatabasePoolShutdownTimedOut => "database_pool_shutdown_timed_out",
        }
    }

    fn context(self) -> Option<&'static str> {
        match self {
            Self::DatabaseConfigurationFailed(role)
            | Self::DatabaseTransportRejected(role)
            | Self::DatabaseUnavailable(role) => Some(database_role_code(role)),
            Self::DependencyReadinessFailed(phase) | Self::DependencyReadinessTimedOut(phase) => {
                Some(readiness_phase_code(phase))
            }
            _ => None,
        }
    }
}

fn classify_composition_error(error: ProductionCompositionErrorV1) -> ExitStatusV1 {
    match error {
        ProductionCompositionErrorV1::HttpBoundaryConfiguration => {
            ExitStatusV1::HttpBoundaryConfigurationFailed
        }
        ProductionCompositionErrorV1::ProductIdentityConfiguration => {
            ExitStatusV1::ProductIdentityConfigurationFailed
        }
        ProductionCompositionErrorV1::DiscordConfiguration => {
            ExitStatusV1::DiscordConfigurationFailed
        }
        ProductionCompositionErrorV1::DatabaseConfiguration { role } => {
            ExitStatusV1::DatabaseConfigurationFailed(role)
        }
        ProductionCompositionErrorV1::UnsafeDatabaseTransport { role } => {
            ExitStatusV1::DatabaseTransportRejected(role)
        }
        ProductionCompositionErrorV1::DatabaseUnavailable { role } => {
            ExitStatusV1::DatabaseUnavailable(role)
        }
        ProductionCompositionErrorV1::ProductPersistenceConfiguration => {
            ExitStatusV1::ProductPersistenceConfigurationFailed
        }
        ProductionCompositionErrorV1::FacadeConfiguration => {
            ExitStatusV1::FacadeConfigurationFailed
        }
        ProductionCompositionErrorV1::ReadinessFailed { phase } => {
            ExitStatusV1::DependencyReadinessFailed(phase)
        }
        ProductionCompositionErrorV1::ReadinessTimedOut { phase } => {
            ExitStatusV1::DependencyReadinessTimedOut(phase)
        }
    }
}

fn classify_server_error(error: LoopbackServeErrorV1) -> ExitStatusV1 {
    match error {
        LoopbackServeErrorV1::ReadinessGateAlreadyClaimed
        | LoopbackServeErrorV1::ReadinessGateAlreadyReady
        | LoopbackServeErrorV1::ReadinessGateInvalidState => {
            ExitStatusV1::ServerReadinessGateFailed
        }
        LoopbackServeErrorV1::NonLoopbackBindAddress => ExitStatusV1::ServerBindRejected,
        LoopbackServeErrorV1::BindFailed => ExitStatusV1::ServerBindFailed,
        LoopbackServeErrorV1::StartupReadinessFailed => ExitStatusV1::ServerStartupReadinessFailed,
        LoopbackServeErrorV1::RuntimeReadinessFailed => ExitStatusV1::ServerRuntimeReadinessFailed,
        LoopbackServeErrorV1::AcceptFailed => ExitStatusV1::ServerAcceptFailed,
        LoopbackServeErrorV1::ConnectionTaskFailed => ExitStatusV1::ServerConnectionFailed,
        LoopbackServeErrorV1::DrainTimedOut => ExitStatusV1::ServerDrainTimedOut,
    }
}

fn database_role_code(role: DatabaseRoleV1) -> &'static str {
    match role {
        DatabaseRoleV1::OAuthFlowWriter => "oauth_flow_writer",
        DatabaseRoleV1::SessionIssuer => "session_issuer",
        DatabaseRoleV1::SessionApi => "session_api",
        DatabaseRoleV1::SecurityRevoker => "security_revoker",
        DatabaseRoleV1::InstallationAuthorityReader => "installation_authority_reader",
        DatabaseRoleV1::AuthorizedSnapshotReader => "authorized_snapshot_reader",
        DatabaseRoleV1::PromotionExecutor => "promotion_executor",
        DatabaseRoleV1::DecisionReader => "decision_reader",
        DatabaseRoleV1::ApprovalExecutor => "approval_executor",
        DatabaseRoleV1::RejectionExecutor => "rejection_executor",
        DatabaseRoleV1::ApplyExecutor => "apply_executor",
        DatabaseRoleV1::CancellationExecutor => "cancellation_executor",
        DatabaseRoleV1::DeploymentStatusReader => "deployment_status_reader",
        DatabaseRoleV1::OperationalDeploymentStatusReader => "operational_deployment_status_reader",
    }
}

fn readiness_phase_code(phase: ProductionReadinessPhaseV1) -> &'static str {
    match phase {
        ProductionReadinessPhaseV1::Aggregate => "aggregate",
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_| {
        let _write_result = std::io::stderr().write_all(b"starring_api_status=panic\n");
    }));
}

fn emit_status(status: ExitStatusV1) {
    let mut stderr = std::io::stderr().lock();
    if let Some(context) = status.context() {
        let _write_result = writeln!(
            stderr,
            "starring_api_status={} context={}",
            status.code(),
            context
        );
    } else {
        let _write_result = writeln!(stderr, "starring_api_status={}", status.code());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    #[test]
    fn composition_failures_keep_only_stable_role_and_phase_context() {
        assert_eq!(
            classify_composition_error(ProductionCompositionErrorV1::DatabaseUnavailable {
                role: DatabaseRoleV1::DecisionReader,
            }),
            ExitStatusV1::DatabaseUnavailable(DatabaseRoleV1::DecisionReader)
        );
        assert_eq!(
            classify_composition_error(ProductionCompositionErrorV1::ReadinessTimedOut {
                phase: ProductionReadinessPhaseV1::Aggregate,
            }),
            ExitStatusV1::DependencyReadinessTimedOut(ProductionReadinessPhaseV1::Aggregate)
        );
    }

    #[test]
    fn status_codes_and_context_are_finite_and_redacted() {
        let status = ExitStatusV1::DatabaseTransportRejected(DatabaseRoleV1::SessionApi);
        assert_eq!(status.code(), "database_transport_rejected");
        assert_eq!(status.context(), Some("session_api"));
        assert!(!status.success());
        assert!(ExitStatusV1::CleanShutdown.success());
        assert!(ExitStatusV1::StartupCancelled.success());
    }

    #[test]
    fn server_errors_map_without_source_formatting() {
        assert_eq!(
            classify_server_error(LoopbackServeErrorV1::StartupReadinessFailed),
            ExitStatusV1::ServerStartupReadinessFailed
        );
        assert_eq!(
            classify_server_error(LoopbackServeErrorV1::DrainTimedOut),
            ExitStatusV1::ServerDrainTimedOut
        );
        assert_eq!(
            classify_server_error(LoopbackServeErrorV1::RuntimeReadinessFailed),
            ExitStatusV1::ServerRuntimeReadinessFailed
        );
        assert_eq!(
            ExitStatusV1::ServerRuntimeReadinessFailed.code(),
            "server_runtime_readiness_failed"
        );
    }

    #[tokio::test]
    async fn runtime_probe_contains_false_panic_and_timeout() {
        assert!(
            run_runtime_readiness_probe(std::future::ready(true), Duration::from_secs(1)).await
        );
        assert!(
            !run_runtime_readiness_probe(std::future::ready(false), Duration::from_secs(1)).await
        );
        assert!(
            !run_runtime_readiness_probe(
                async { panic!("runtime readiness panic") },
                Duration::from_secs(1)
            )
            .await
        );
        assert!(!run_runtime_readiness_probe(std::future::pending::<bool>(), Duration::ZERO).await);
    }

    #[tokio::test]
    async fn runtime_monitor_stops_on_the_first_failed_probe() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = calls.clone();
        monitor_runtime_readiness(
            move || {
                let call = observed_calls.fetch_add(1, Ordering::AcqRel);
                std::future::ready(call == 0)
            },
            || std::future::ready(()),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(calls.load(Ordering::Acquire), 2);
    }
}
