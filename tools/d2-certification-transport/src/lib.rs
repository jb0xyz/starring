mod config;
mod control;
mod gateway;
mod http_proxy;
mod state;

pub use config::{Config, ConfigError};

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::watch;

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
}

pub async fn run(config: Config) -> Result<(), RunError> {
    let state = Arc::new(SharedState::new(&config).map_err(|_| RunError::InstanceIdentity)?);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let gateway = gateway::serve(config.clone(), Arc::clone(&state), shutdown_rx.clone());
    let http = http_proxy::serve(config.clone(), Arc::clone(&state), shutdown_rx.clone());
    let control = control::serve(config, state, shutdown_tx, shutdown_rx);
    tokio::pin!(gateway);
    tokio::pin!(http);
    tokio::pin!(control);
    tokio::select! {
        result = &mut gateway => result.map_err(|_| RunError::Gateway),
        result = &mut http => result.map_err(|_| RunError::Http),
        result = &mut control => result.map_err(|_| RunError::Control),
    }
}
