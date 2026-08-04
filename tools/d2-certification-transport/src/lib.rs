mod config;
mod control;
mod gateway;
mod http_proxy;
mod state;

pub use config::{Config, ConfigError};

use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::watch;
use tokio::task::{JoinError, JoinSet};

use state::SharedState;

#[derive(Debug, Error)]
pub enum RunError {
    #[error("transport_bind_failed")]
    Bind,
    #[error("transport_control_failed")]
    Control,
    #[error("transport_gateway_failed")]
    Gateway,
    #[error("transport_http_failed")]
    Http,
    #[error("transport_instance_identity_failed")]
    InstanceIdentity,
    #[error("transport_supervisor_failed")]
    Supervisor,
    #[error("transport_shutdown_failed")]
    Shutdown,
}

const TRANSPORT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(6);

pub async fn run(config: Config) -> Result<(), RunError> {
    let state = Arc::new(SharedState::new(&config).map_err(|_| RunError::InstanceIdentity)?);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let supervisor_shutdown_rx = shutdown_rx.clone();
    let mut services = JoinSet::new();
    let gateway_config = config.clone();
    let gateway_state = Arc::clone(&state);
    let gateway_shutdown = shutdown_rx.clone();
    services.spawn(async move {
        gateway::serve(gateway_config, gateway_state, gateway_shutdown)
            .await
            .map_err(|_| RunError::Gateway)
    });
    let http_config = config.clone();
    let http_state = Arc::clone(&state);
    let http_shutdown = shutdown_rx.clone();
    services.spawn(async move {
        http_proxy::serve(http_config, http_state, http_shutdown)
            .await
            .map_err(|_| RunError::Http)
    });
    let control_shutdown_tx = shutdown_tx.clone();
    services.spawn(async move {
        control::serve(config, state, control_shutdown_tx, shutdown_rx)
            .await
            .map_err(|_| RunError::Control)
    });
    let mut failure = None;
    match services.join_next().await {
        Some(result) => {
            record_initial_service_result(result, *supervisor_shutdown_rx.borrow(), &mut failure)
        }
        None => return Err(RunError::Supervisor),
    }
    let _ = shutdown_tx.send(true);
    let drained = tokio::time::timeout(TRANSPORT_SHUTDOWN_TIMEOUT, async {
        while let Some(result) = services.join_next().await {
            record_service_result(result, &mut failure);
        }
    })
    .await;
    if drained.is_err() {
        services.abort_all();
        while services.join_next().await.is_some() {}
        return Err(failure.unwrap_or(RunError::Shutdown));
    }
    failure.map_or(Ok(()), Err)
}

fn record_initial_service_result(
    result: Result<Result<(), RunError>, JoinError>,
    shutdown_requested: bool,
    failure: &mut Option<RunError>,
) {
    record_service_result(result, failure);
    if failure.is_none() && !shutdown_requested {
        *failure = Some(RunError::Supervisor);
    }
}

fn record_service_result(
    result: Result<Result<(), RunError>, JoinError>,
    failure: &mut Option<RunError>,
) {
    let observed = match result {
        Ok(result) => result.err(),
        Err(_) => Some(RunError::Supervisor),
    };
    if failure.is_none() {
        *failure = observed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unrequested_clean_service_exit_is_a_supervisor_failure() {
        let mut failure = None;
        record_initial_service_result(Ok(Ok(())), false, &mut failure);
        assert!(matches!(failure, Some(RunError::Supervisor)));
    }

    #[test]
    fn a_requested_clean_service_exit_stays_successful() {
        let mut failure = None;
        record_initial_service_result(Ok(Ok(())), true, &mut failure);
        assert!(failure.is_none());
    }
}
