use std::future::{poll_fn, Future};
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use axum::body::Body;
use axum::Router;
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use product_control_http::{
    ProductApiReadinessClaimErrorV1, ProductApiReadinessGate, ProductApiReadinessLeaseV1,
};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};
use tower::ServiceExt;

pub const MAX_GRACEFUL_DRAIN_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CONNECTIONS: usize = 4_096;
const MAX_H1_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_H1_HEADERS: usize = 256;
const MIN_H1_BUFFER_SIZE: usize = 8 * 1_024;
const MAX_H1_BUFFER_SIZE: usize = 1_024 * 1_024;
const MAX_H2_CONCURRENT_STREAMS: u32 = 1_024;
const MAX_H2_HEADER_LIST_SIZE: u32 = 1_024 * 1_024;
const MAX_H2_SEND_BUFFER_SIZE: usize = 1_024 * 1_024;
const MAX_H2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(300);
const MAX_H2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_STARTUP_READINESS_TIMEOUT: Duration = Duration::from_secs(60);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_CONSECUTIVE_ACCEPT_FAILURES: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopbackServerConfigInputV1 {
    pub max_connections: usize,
    pub h1_header_read_timeout: Duration,
    pub h1_max_headers: usize,
    pub h1_max_buffer_size: usize,
    pub h2_max_concurrent_streams: u32,
    pub h2_max_header_list_size: u32,
    pub h2_max_send_buffer_size: usize,
    pub h2_keep_alive_interval: Duration,
    pub h2_keep_alive_timeout: Duration,
    pub startup_readiness_timeout: Duration,
    pub graceful_drain_timeout: Duration,
}

impl Default for LoopbackServerConfigInputV1 {
    fn default() -> Self {
        LoopbackServerConfigV1::production_default().into_input()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopbackServerConfigV1 {
    input: LoopbackServerConfigInputV1,
}

impl LoopbackServerConfigV1 {
    pub fn try_new(
        input: LoopbackServerConfigInputV1,
    ) -> Result<Self, LoopbackServerConfigErrorV1> {
        validate_positive_bounded_usize(
            input.max_connections,
            MAX_CONNECTIONS,
            LoopbackServerConfigErrorV1::InvalidMaxConnections,
        )?;
        validate_positive_bounded_duration(
            input.h1_header_read_timeout,
            MAX_H1_HEADER_READ_TIMEOUT,
            LoopbackServerConfigErrorV1::InvalidH1HeaderReadTimeout,
        )?;
        validate_positive_bounded_usize(
            input.h1_max_headers,
            MAX_H1_HEADERS,
            LoopbackServerConfigErrorV1::InvalidH1MaxHeaders,
        )?;
        if !(MIN_H1_BUFFER_SIZE..=MAX_H1_BUFFER_SIZE).contains(&input.h1_max_buffer_size) {
            return Err(LoopbackServerConfigErrorV1::InvalidH1MaxBufferSize);
        }
        if input.h2_max_concurrent_streams == 0
            || input.h2_max_concurrent_streams > MAX_H2_CONCURRENT_STREAMS
        {
            return Err(LoopbackServerConfigErrorV1::InvalidH2MaxConcurrentStreams);
        }
        if input.h2_max_header_list_size == 0
            || input.h2_max_header_list_size > MAX_H2_HEADER_LIST_SIZE
        {
            return Err(LoopbackServerConfigErrorV1::InvalidH2MaxHeaderListSize);
        }
        validate_positive_bounded_usize(
            input.h2_max_send_buffer_size,
            MAX_H2_SEND_BUFFER_SIZE,
            LoopbackServerConfigErrorV1::InvalidH2MaxSendBufferSize,
        )?;
        validate_positive_bounded_duration(
            input.h2_keep_alive_interval,
            MAX_H2_KEEP_ALIVE_INTERVAL,
            LoopbackServerConfigErrorV1::InvalidH2KeepAliveInterval,
        )?;
        validate_positive_bounded_duration(
            input.h2_keep_alive_timeout,
            MAX_H2_KEEP_ALIVE_TIMEOUT,
            LoopbackServerConfigErrorV1::InvalidH2KeepAliveTimeout,
        )?;
        validate_positive_bounded_duration(
            input.startup_readiness_timeout,
            MAX_STARTUP_READINESS_TIMEOUT,
            LoopbackServerConfigErrorV1::InvalidStartupReadinessTimeout,
        )?;
        validate_positive_bounded_duration(
            input.graceful_drain_timeout,
            MAX_GRACEFUL_DRAIN_TIMEOUT,
            LoopbackServerConfigErrorV1::InvalidGracefulDrainTimeout,
        )?;
        Ok(Self { input })
    }

    pub fn production_default() -> Self {
        Self {
            input: LoopbackServerConfigInputV1 {
                max_connections: 512,
                h1_header_read_timeout: Duration::from_secs(10),
                h1_max_headers: 64,
                h1_max_buffer_size: 64 * 1_024,
                h2_max_concurrent_streams: 64,
                h2_max_header_list_size: 16 * 1_024,
                h2_max_send_buffer_size: 64 * 1_024,
                h2_keep_alive_interval: Duration::from_secs(30),
                h2_keep_alive_timeout: Duration::from_secs(10),
                startup_readiness_timeout: Duration::from_secs(10),
                graceful_drain_timeout: Duration::from_secs(15),
            },
        }
    }

    pub fn into_input(self) -> LoopbackServerConfigInputV1 {
        self.input
    }
}

impl Default for LoopbackServerConfigV1 {
    fn default() -> Self {
        Self::production_default()
    }
}

impl TryFrom<LoopbackServerConfigInputV1> for LoopbackServerConfigV1 {
    type Error = LoopbackServerConfigErrorV1;

    fn try_from(input: LoopbackServerConfigInputV1) -> Result<Self, Self::Error> {
        Self::try_new(input)
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum LoopbackServerConfigErrorV1 {
    #[error("the maximum connection count is outside the supported range")]
    InvalidMaxConnections,
    #[error("the HTTP/1 header read timeout is outside the supported range")]
    InvalidH1HeaderReadTimeout,
    #[error("the HTTP/1 maximum header count is outside the supported range")]
    InvalidH1MaxHeaders,
    #[error("the HTTP/1 maximum buffer size is outside the supported range")]
    InvalidH1MaxBufferSize,
    #[error("the HTTP/2 maximum concurrent stream count is outside the supported range")]
    InvalidH2MaxConcurrentStreams,
    #[error("the HTTP/2 maximum header list size is outside the supported range")]
    InvalidH2MaxHeaderListSize,
    #[error("the HTTP/2 maximum send buffer size is outside the supported range")]
    InvalidH2MaxSendBufferSize,
    #[error("the HTTP/2 keep-alive interval is outside the supported range")]
    InvalidH2KeepAliveInterval,
    #[error("the HTTP/2 keep-alive acknowledgement timeout is outside the supported range")]
    InvalidH2KeepAliveTimeout,
    #[error("the startup readiness timeout is outside the supported range")]
    InvalidStartupReadinessTimeout,
    #[error("the graceful drain timeout is outside the supported range")]
    InvalidGracefulDrainTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopbackServeReportV1 {
    local_addr: SocketAddr,
}

impl LoopbackServeReportV1 {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum LoopbackServeErrorV1 {
    #[error("the readiness gate already has an owner")]
    ReadinessGateAlreadyClaimed,
    #[error("the readiness gate is already ready")]
    ReadinessGateAlreadyReady,
    #[error("the readiness gate is in an invalid state")]
    ReadinessGateInvalidState,
    #[error("the server bind address is not loopback")]
    NonLoopbackBindAddress,
    #[error("the loopback listener could not be bound")]
    BindFailed,
    #[error("the startup readiness probe failed or exceeded its deadline")]
    StartupReadinessFailed,
    #[error("the loopback listener repeatedly failed to accept connections")]
    AcceptFailed,
    #[error("an isolated connection task failed")]
    ConnectionTaskFailed,
    #[error("graceful connection drain exceeded its deadline")]
    DrainTimedOut,
}

pub async fn serve_verified_loopback<P, S>(
    bind_addr: SocketAddr,
    router: Router,
    readiness_gate: ProductApiReadinessGate,
    config: LoopbackServerConfigV1,
    startup_probe: P,
    shutdown: S,
) -> Result<LoopbackServeReportV1, LoopbackServeErrorV1>
where
    P: Future<Output = bool> + Send + 'static,
    S: Future<Output = ()> + Send,
{
    let readiness = readiness_gate.claim().map_err(map_claim_error)?;
    if !bind_addr.ip().is_loopback() {
        return Err(LoopbackServeErrorV1::NonLoopbackBindAddress);
    }

    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|_| LoopbackServeErrorV1::BindFailed)?;
    let local_addr = listener
        .local_addr()
        .map_err(|_| LoopbackServeErrorV1::BindFailed)?;
    let mut shutdown = Box::pin(shutdown);

    match wait_for_startup_readiness(
        startup_probe,
        shutdown.as_mut(),
        config.input.startup_readiness_timeout,
    )
    .await?
    {
        StartupReadinessOutcome::Shutdown => {
            drop(listener);
            return Ok(LoopbackServeReportV1 { local_addr });
        }
        StartupReadinessOutcome::Ready => {}
    }
    if shutdown_completed_now(shutdown.as_mut()).await {
        drop(listener);
        return Ok(LoopbackServeReportV1 { local_addr });
    }

    let (drain_sender, drain_receiver) = watch::channel(false);
    let connection_permits = Arc::new(Semaphore::new(config.input.max_connections));
    let mut connections = JoinSet::new();
    let mut consecutive_accept_failures = 0_u8;
    readiness.mark_ready();

    let exit = loop {
        let next_connection = acquire_permit_and_accept(&listener, connection_permits.clone());
        tokio::pin!(next_connection);

        tokio::select! {
            biased;
            _ = shutdown.as_mut() => {
                break ServeExit::Shutdown;
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if matches!(completed, Some(Err(_))) {
                    break ServeExit::ConnectionTaskFailed;
                }
            }
            accepted = next_connection.as_mut() => {
                match accepted {
                    Err(()) => break ServeExit::ConnectionTaskFailed,
                    Ok((permit, Ok((stream, _)))) => {
                        consecutive_accept_failures = 0;
                        readiness.mark_ready();
                        spawn_connection(
                            &mut connections,
                            stream,
                            permit,
                            router.clone(),
                            drain_receiver.clone(),
                            config,
                        );
                    }
                    Ok((_permit, Err(_))) => {
                        let delay = match register_accept_failure(
                            &readiness,
                            &mut consecutive_accept_failures,
                        ) {
                            Ok(delay) => delay,
                            Err(error) => break error,
                        };
                        if let Some(backoff_exit) = wait_for_accept_backoff(
                            shutdown.as_mut(),
                            &mut connections,
                            delay,
                        ).await {
                            break backoff_exit;
                        }
                    }
                }
            }
        }
    };

    readiness.mark_unready();
    drop(listener);

    match exit {
        ServeExit::Shutdown => {
            drain_connections(
                &mut connections,
                drain_sender,
                config.input.graceful_drain_timeout,
            )
            .await?;
            Ok(LoopbackServeReportV1 { local_addr })
        }
        ServeExit::AcceptFailed => {
            drain_connections(
                &mut connections,
                drain_sender,
                config.input.graceful_drain_timeout,
            )
            .await?;
            Err(LoopbackServeErrorV1::AcceptFailed)
        }
        ServeExit::ConnectionTaskFailed => {
            abort_connections(&mut connections).await;
            Err(LoopbackServeErrorV1::ConnectionTaskFailed)
        }
    }
}

async fn wait_for_startup_readiness<P, S>(
    startup_probe: P,
    mut shutdown: Pin<&mut S>,
    readiness_timeout: Duration,
) -> Result<StartupReadinessOutcome, LoopbackServeErrorV1>
where
    P: Future<Output = bool> + Send + 'static,
    S: Future<Output = ()>,
{
    let mut probe = JoinSet::new();
    probe.spawn(startup_probe);
    let deadline = sleep(readiness_timeout);
    tokio::pin!(deadline);
    tokio::select! {
        biased;
        _ = shutdown.as_mut() => Ok(StartupReadinessOutcome::Shutdown),
        completed = probe.join_next() => {
            match completed {
                Some(Ok(true)) => Ok(StartupReadinessOutcome::Ready),
                Some(Ok(false)) | Some(Err(_)) | None => {
                    Err(LoopbackServeErrorV1::StartupReadinessFailed)
                }
            }
        }
        _ = deadline.as_mut() => Err(LoopbackServeErrorV1::StartupReadinessFailed),
    }
}

fn map_claim_error(error: ProductApiReadinessClaimErrorV1) -> LoopbackServeErrorV1 {
    match error {
        ProductApiReadinessClaimErrorV1::AlreadyClaimed => {
            LoopbackServeErrorV1::ReadinessGateAlreadyClaimed
        }
        ProductApiReadinessClaimErrorV1::AlreadyReady => {
            LoopbackServeErrorV1::ReadinessGateAlreadyReady
        }
        ProductApiReadinessClaimErrorV1::InvalidState => {
            LoopbackServeErrorV1::ReadinessGateInvalidState
        }
    }
}

async fn shutdown_completed_now<S>(mut shutdown: Pin<&mut S>) -> bool
where
    S: Future<Output = ()>,
{
    poll_fn(|context| Poll::Ready(matches!(shutdown.as_mut().poll(context), Poll::Ready(())))).await
}

async fn acquire_permit_and_accept(
    listener: &TcpListener,
    permits: Arc<Semaphore>,
) -> Result<(OwnedSemaphorePermit, io::Result<(TcpStream, SocketAddr)>), ()> {
    let permit = permits.acquire_owned().await.map_err(|_| ())?;
    let accepted = listener.accept().await;
    Ok((permit, accepted))
}

fn register_accept_failure(
    readiness: &ProductApiReadinessLeaseV1,
    consecutive_failures: &mut u8,
) -> Result<Duration, ServeExit> {
    readiness.mark_unready();
    *consecutive_failures = consecutive_failures.saturating_add(1);
    if *consecutive_failures >= MAX_CONSECUTIVE_ACCEPT_FAILURES {
        return Err(ServeExit::AcceptFailed);
    }
    Ok(ACCEPT_RETRY_DELAY)
}

async fn wait_for_accept_backoff<S>(
    mut shutdown: Pin<&mut S>,
    connections: &mut JoinSet<OwnedSemaphorePermit>,
    delay: Duration,
) -> Option<ServeExit>
where
    S: Future<Output = ()>,
{
    let backoff = sleep(delay);
    tokio::pin!(backoff);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.as_mut() => return Some(ServeExit::Shutdown),
            completed = connections.join_next(), if !connections.is_empty() => {
                if matches!(completed, Some(Err(_))) {
                    return Some(ServeExit::ConnectionTaskFailed);
                }
            }
            _ = backoff.as_mut() => return None,
        }
    }
}

fn spawn_connection(
    connections: &mut JoinSet<OwnedSemaphorePermit>,
    stream: TcpStream,
    permit: OwnedSemaphorePermit,
    router: Router,
    mut drain_receiver: watch::Receiver<bool>,
    config: LoopbackServerConfigV1,
) {
    connections.spawn(async move {
        let service = router.map_request(|request: Request<Incoming>| request.map(Body::new));
        let service = TowerToHyperService::new(service);
        let mut builder = Builder::new(TokioExecutor::new());
        builder
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(config.input.h1_header_read_timeout)
            .max_headers(config.input.h1_max_headers)
            .max_buf_size(config.input.h1_max_buffer_size);
        builder
            .http2()
            .timer(TokioTimer::new())
            .max_concurrent_streams(config.input.h2_max_concurrent_streams)
            .max_header_list_size(config.input.h2_max_header_list_size)
            .max_send_buf_size(config.input.h2_max_send_buffer_size)
            .keep_alive_interval(config.input.h2_keep_alive_interval)
            .keep_alive_timeout(config.input.h2_keep_alive_timeout);
        let connection = builder.serve_connection(TokioIo::new(stream), service);
        tokio::pin!(connection);

        tokio::select! {
            _ = connection.as_mut() => {}
            _ = drain_receiver.changed() => {
                connection.as_mut().graceful_shutdown();
                let _ = connection.await;
            }
        }
        permit
    });
}

async fn drain_connections(
    connections: &mut JoinSet<OwnedSemaphorePermit>,
    drain_sender: watch::Sender<bool>,
    drain_timeout: Duration,
) -> Result<(), LoopbackServeErrorV1> {
    let _ = drain_sender.send(true);
    let drained = timeout(drain_timeout, async {
        while let Some(completed) = connections.join_next().await {
            if completed.is_err() {
                return Err(LoopbackServeErrorV1::ConnectionTaskFailed);
            }
        }
        Ok(())
    })
    .await;

    match drained {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            abort_connections(connections).await;
            Err(error)
        }
        Err(_) => {
            abort_connections(connections).await;
            Err(LoopbackServeErrorV1::DrainTimedOut)
        }
    }
}

async fn abort_connections(connections: &mut JoinSet<OwnedSemaphorePermit>) {
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

fn validate_positive_bounded_usize(
    value: usize,
    maximum: usize,
    error: LoopbackServerConfigErrorV1,
) -> Result<(), LoopbackServerConfigErrorV1> {
    if value == 0 || value > maximum {
        return Err(error);
    }
    Ok(())
}

fn validate_positive_bounded_duration(
    value: Duration,
    maximum: Duration,
    error: LoopbackServerConfigErrorV1,
) -> Result<(), LoopbackServerConfigErrorV1> {
    if value.is_zero() || value > maximum {
        return Err(error);
    }
    Ok(())
}

enum ServeExit {
    Shutdown,
    AcceptFailed,
    ConnectionTaskFailed,
}

enum StartupReadinessOutcome {
    Ready,
    Shutdown,
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::future::pending;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};

    use axum::extract::State;
    use axum::routing::get;
    use hyper::client::conn::http2;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{oneshot, Notify, Semaphore};
    use tokio::task::JoinHandle;

    use super::*;

    #[test]
    fn production_defaults_are_exact_and_validated() {
        let input = LoopbackServerConfigV1::production_default().into_input();
        assert_eq!(input.max_connections, 512);
        assert_eq!(input.h1_header_read_timeout, Duration::from_secs(10));
        assert_eq!(input.h1_max_headers, 64);
        assert_eq!(input.h1_max_buffer_size, 64 * 1_024);
        assert_eq!(input.h2_max_concurrent_streams, 64);
        assert_eq!(input.h2_max_header_list_size, 16 * 1_024);
        assert_eq!(input.h2_max_send_buffer_size, 64 * 1_024);
        assert_eq!(input.h2_keep_alive_interval, Duration::from_secs(30));
        assert_eq!(input.h2_keep_alive_timeout, Duration::from_secs(10));
        assert_eq!(input.startup_readiness_timeout, Duration::from_secs(10));
        assert_eq!(input.graceful_drain_timeout, Duration::from_secs(15));
        assert_eq!(
            LoopbackServerConfigV1::try_new(input).unwrap(),
            LoopbackServerConfigV1::default()
        );
    }

    #[test]
    fn every_server_limit_rejects_zero_or_excessive_values() {
        assert_invalid_config(
            |input| input.max_connections = 0,
            LoopbackServerConfigErrorV1::InvalidMaxConnections,
        );
        assert_invalid_config(
            |input| input.max_connections = MAX_CONNECTIONS + 1,
            LoopbackServerConfigErrorV1::InvalidMaxConnections,
        );
        assert_invalid_config(
            |input| input.h1_header_read_timeout = Duration::ZERO,
            LoopbackServerConfigErrorV1::InvalidH1HeaderReadTimeout,
        );
        assert_invalid_config(
            |input| {
                input.h1_header_read_timeout = MAX_H1_HEADER_READ_TIMEOUT + Duration::from_nanos(1)
            },
            LoopbackServerConfigErrorV1::InvalidH1HeaderReadTimeout,
        );
        assert_invalid_config(
            |input| input.h1_max_headers = 0,
            LoopbackServerConfigErrorV1::InvalidH1MaxHeaders,
        );
        assert_invalid_config(
            |input| input.h1_max_headers = MAX_H1_HEADERS + 1,
            LoopbackServerConfigErrorV1::InvalidH1MaxHeaders,
        );
        assert_invalid_config(
            |input| input.h1_max_buffer_size = MIN_H1_BUFFER_SIZE - 1,
            LoopbackServerConfigErrorV1::InvalidH1MaxBufferSize,
        );
        assert_invalid_config(
            |input| input.h1_max_buffer_size = MAX_H1_BUFFER_SIZE + 1,
            LoopbackServerConfigErrorV1::InvalidH1MaxBufferSize,
        );
        assert_invalid_config(
            |input| input.h2_max_concurrent_streams = 0,
            LoopbackServerConfigErrorV1::InvalidH2MaxConcurrentStreams,
        );
        assert_invalid_config(
            |input| input.h2_max_concurrent_streams = MAX_H2_CONCURRENT_STREAMS + 1,
            LoopbackServerConfigErrorV1::InvalidH2MaxConcurrentStreams,
        );
        assert_invalid_config(
            |input| input.h2_max_header_list_size = 0,
            LoopbackServerConfigErrorV1::InvalidH2MaxHeaderListSize,
        );
        assert_invalid_config(
            |input| input.h2_max_header_list_size = MAX_H2_HEADER_LIST_SIZE + 1,
            LoopbackServerConfigErrorV1::InvalidH2MaxHeaderListSize,
        );
        assert_invalid_config(
            |input| input.h2_max_send_buffer_size = 0,
            LoopbackServerConfigErrorV1::InvalidH2MaxSendBufferSize,
        );
        assert_invalid_config(
            |input| input.h2_max_send_buffer_size = MAX_H2_SEND_BUFFER_SIZE + 1,
            LoopbackServerConfigErrorV1::InvalidH2MaxSendBufferSize,
        );
        assert_invalid_config(
            |input| input.h2_keep_alive_interval = Duration::ZERO,
            LoopbackServerConfigErrorV1::InvalidH2KeepAliveInterval,
        );
        assert_invalid_config(
            |input| {
                input.h2_keep_alive_interval = MAX_H2_KEEP_ALIVE_INTERVAL + Duration::from_nanos(1)
            },
            LoopbackServerConfigErrorV1::InvalidH2KeepAliveInterval,
        );
        assert_invalid_config(
            |input| input.h2_keep_alive_timeout = Duration::ZERO,
            LoopbackServerConfigErrorV1::InvalidH2KeepAliveTimeout,
        );
        assert_invalid_config(
            |input| {
                input.h2_keep_alive_timeout = MAX_H2_KEEP_ALIVE_TIMEOUT + Duration::from_nanos(1)
            },
            LoopbackServerConfigErrorV1::InvalidH2KeepAliveTimeout,
        );
        assert_invalid_config(
            |input| input.startup_readiness_timeout = Duration::ZERO,
            LoopbackServerConfigErrorV1::InvalidStartupReadinessTimeout,
        );
        assert_invalid_config(
            |input| {
                input.startup_readiness_timeout =
                    MAX_STARTUP_READINESS_TIMEOUT + Duration::from_nanos(1)
            },
            LoopbackServerConfigErrorV1::InvalidStartupReadinessTimeout,
        );
        assert_invalid_config(
            |input| input.graceful_drain_timeout = Duration::ZERO,
            LoopbackServerConfigErrorV1::InvalidGracefulDrainTimeout,
        );
        assert_invalid_config(
            |input| {
                input.graceful_drain_timeout = MAX_GRACEFUL_DRAIN_TIMEOUT + Duration::from_nanos(1)
            },
            LoopbackServerConfigErrorV1::InvalidGracefulDrainTimeout,
        );
    }

    #[tokio::test]
    async fn non_loopback_bind_is_rejected_and_claim_is_released() {
        let gate = ProductApiReadinessGate::initially_unready();
        let result = serve_verified_loopback(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async { true },
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::NonLoopbackBindAddress));
        assert!(!gate.is_ready());
        assert!(gate.claim().is_ok());
    }

    #[tokio::test]
    async fn claimed_ready_gate_is_rejected_without_destroying_owner_state() {
        let gate = ProductApiReadinessGate::initially_unready();
        let owner = gate.claim().unwrap();
        owner.mark_ready();

        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async { true },
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::ReadinessGateAlreadyReady));
        assert!(gate.is_ready());
        drop(owner);
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn claimed_unready_gate_rejects_a_second_server() {
        let gate = ProductApiReadinessGate::initially_unready();
        let owner = gate.claim().unwrap();

        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async { true },
            pending(),
        )
        .await;

        assert_eq!(
            result,
            Err(LoopbackServeErrorV1::ReadinessGateAlreadyClaimed)
        );
        assert!(!gate.is_ready());
        drop(owner);
        assert!(gate.claim().is_ok());
    }

    #[tokio::test]
    async fn concurrent_server_claim_cannot_disrupt_the_owner() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let server_gate = gate.clone();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new(),
                server_gate,
                LoopbackServerConfigV1::default(),
                async { true },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let competing = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async { true },
            pending(),
        )
        .await;
        assert_eq!(
            competing,
            Err(LoopbackServeErrorV1::ReadinessGateAlreadyReady)
        );
        assert!(gate.is_ready());

        shutdown_sender.send(()).unwrap();
        assert!(server.await.unwrap().is_ok());
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn gate_stays_unready_and_claim_is_released_when_bind_fails() {
        let occupied = TcpListener::bind(loopback_address(0)).await.unwrap();
        let occupied_address = occupied.local_addr().unwrap();
        let gate = ProductApiReadinessGate::initially_unready();
        let result = serve_verified_loopback(
            occupied_address,
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async { true },
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::BindFailed));
        assert!(!gate.is_ready());
        assert!(gate.claim().is_ok());
    }

    #[test]
    fn accept_failure_closes_readiness_and_fails_on_fifth_consecutive_error() {
        let gate = ProductApiReadinessGate::initially_unready();
        let lease = gate.claim().unwrap();
        lease.mark_ready();
        let mut failures = 0;

        for _ in 1..MAX_CONSECUTIVE_ACCEPT_FAILURES {
            assert!(matches!(
                register_accept_failure(&lease, &mut failures),
                Ok(ACCEPT_RETRY_DELAY)
            ));
            assert!(!gate.is_ready());
        }
        assert!(matches!(
            register_accept_failure(&lease, &mut failures),
            Err(ServeExit::AcceptFailed)
        ));
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn startup_probe_stays_unready_until_true_then_opens_readiness() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let probe_polled = Arc::new(Notify::new());
        let probe_observer = probe_polled.clone();
        let (probe_sender, probe_receiver) = oneshot::channel();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new(),
                server_gate,
                LoopbackServerConfigV1::default(),
                async move {
                    probe_observer.notify_one();
                    probe_receiver.await.unwrap_or(false)
                },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        probe_polled.notified().await;
        assert!(!gate.is_ready());
        probe_sender.send(true).unwrap();
        wait_until_ready(&gate).await;

        shutdown_sender.send(()).unwrap();
        assert!(server.await.unwrap().is_ok());
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn false_startup_probe_fails_without_ready_transition() {
        let gate = ProductApiReadinessGate::initially_unready();
        let observed_ready_polls = Arc::new(AtomicUsize::new(0));
        let probe_gate = gate.clone();
        let probe_observations = observed_ready_polls.clone();
        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async move {
                if probe_gate.is_ready() {
                    probe_observations.fetch_add(1, Ordering::AcqRel);
                }
                false
            },
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::StartupReadinessFailed));
        assert_eq!(observed_ready_polls.load(Ordering::Acquire), 0);
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn startup_probe_timeout_fails_closed() {
        let gate = ProductApiReadinessGate::initially_unready();
        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            config_with_startup_timeout(Duration::from_millis(10)),
            pending(),
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::StartupReadinessFailed));
        assert!(!gate.is_ready());
        assert!(gate.claim().is_ok());
    }

    #[tokio::test]
    async fn panicking_startup_probe_fails_typed_without_ready_transition() {
        let gate = ProductApiReadinessGate::initially_unready();
        let observed_ready_polls = Arc::new(AtomicUsize::new(0));
        let probe_gate = gate.clone();
        let probe_observations = observed_ready_polls.clone();
        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async move {
                if probe_gate.is_ready() {
                    probe_observations.fetch_add(1, Ordering::AcqRel);
                }
                panic!("startup probe panic")
            },
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::StartupReadinessFailed));
        assert_eq!(observed_ready_polls.load(Ordering::Acquire), 0);
        assert!(!gate.is_ready());
        assert!(gate.claim().is_ok());
    }

    #[tokio::test]
    async fn completed_shutdown_wins_simultaneously_failed_probe_without_transition() {
        let gate = ProductApiReadinessGate::initially_unready();
        let observed_ready_polls = Arc::new(AtomicUsize::new(0));
        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            LoopbackServerConfigV1::default(),
            async { false },
            ObserveReadyOnShutdownPoll {
                gate: gate.clone(),
                observed_ready_polls: observed_ready_polls.clone(),
            },
        )
        .await
        .unwrap();

        assert_eq!(observed_ready_polls.load(Ordering::Acquire), 0);
        assert!(!gate.is_ready());
        assert!(TcpStream::connect(result.local_addr()).await.is_err());
    }

    #[tokio::test]
    async fn bound_server_accepts_http1_and_shuts_down_cleanly() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new().route("/health", get(|| async { "alive" })),
                server_gate,
                LoopbackServerConfigV1::default(),
                async { true },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let response = request_http1(address, "/health").await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("alive"));

        shutdown_sender.send(()).unwrap();
        let report = server.await.unwrap().unwrap();
        assert_eq!(report.local_addr(), address);
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn connection_limit_is_acquired_before_accept() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let state = LimitedBlockingState::new();
        let router_state = state.clone();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new()
                    .route("/hold", get(limited_blocking_request))
                    .with_state(router_state),
                server_gate,
                config_with_max_connections(1),
                async { true },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let first = tokio::spawn(request_http1(address, "/hold"));
        wait_until_started_count(&state, 1).await;
        let second = tokio::spawn(request_http1(address, "/hold"));
        sleep(Duration::from_millis(25)).await;
        assert_eq!(state.started.load(Ordering::Acquire), 1);

        state.release.add_permits(1);
        wait_until_started_count(&state, 2).await;
        state.release.add_permits(1);
        assert!(first.await.unwrap().starts_with("HTTP/1.1 200 OK"));
        assert!(second.await.unwrap().starts_with("HTTP/1.1 200 OK"));

        shutdown_sender.send(()).unwrap();
        assert!(server.await.unwrap().is_ok());
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn unresponsive_idle_http2_peer_releases_its_connection_permit() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new().route("/health", get(|| async { "alive" })),
                server_gate,
                config_with_idle_h2_recovery(),
                async { true },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let mut idle_peer = TcpStream::connect(address).await.unwrap();
        idle_peer
            .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0")
            .await
            .unwrap();
        sleep(Duration::from_millis(5)).await;

        let response = timeout(Duration::from_secs(1), request_http1(address, "/health"))
            .await
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));

        shutdown_sender.send(()).unwrap();
        assert!(server.await.unwrap().is_ok());
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn http1_shutdown_closes_readiness_before_active_request_drain() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let state = BlockingState::new();
        let router_state = state.clone();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new()
                    .route("/hold", get(blocking_request))
                    .with_state(router_state),
                server_gate,
                config_with_drain_timeout(Duration::from_secs(1)),
                async { true },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let client = tokio::spawn(request_http1(address, "/hold"));
        state.started.notified().await;
        shutdown_sender.send(()).unwrap();
        wait_until_unready(&gate).await;
        assert!(!server.is_finished());
        assert!(TcpStream::connect(address).await.is_err());
        state.release.notify_one();

        assert!(server.await.unwrap().is_ok());
        assert!(client.await.unwrap().starts_with("HTTP/1.1 200 OK"));
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn http2_connection_drains_after_readiness_closes() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let state = BlockingState::new();
        let router_state = state.clone();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new()
                    .route("/hold", get(blocking_request))
                    .with_state(router_state),
                server_gate,
                config_with_drain_timeout(Duration::from_secs(1)),
                async { true },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let client = start_http2_request(address, "/hold").await;
        state.started.notified().await;
        shutdown_sender.send(()).unwrap();
        wait_until_unready(&gate).await;
        assert!(!server.is_finished());
        state.release.notify_one();

        assert_eq!(client.response.await.unwrap().unwrap().status(), 200);
        assert!(server.await.unwrap().is_ok());
        assert!(client.driver.await.unwrap().is_ok());
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn http2_pending_handler_is_aborted_after_forced_drain_timeout() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let state = BlockingState::new();
        let dropped = state.dropped.clone();
        let router_state = state.clone();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new()
                    .route("/hold", get(blocking_request))
                    .with_state(router_state),
                server_gate,
                config_with_drain_timeout(Duration::from_millis(25)),
                async { true },
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let client = start_http2_request(address, "/hold").await;
        state.started.notified().await;
        shutdown_sender.send(()).unwrap();

        assert_eq!(
            server.await.unwrap(),
            Err(LoopbackServeErrorV1::DrainTimedOut)
        );
        assert!(!gate.is_ready());
        wait_until_dropped(&dropped).await;
        finish_http2_client(client).await;
    }

    #[tokio::test]
    async fn serve_future_cancellation_drops_pending_http2_handler() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let state = BlockingState::new();
        let dropped = state.dropped.clone();
        let router_state = state.clone();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new()
                    .route("/hold", get(blocking_request))
                    .with_state(router_state),
                server_gate,
                LoopbackServerConfigV1::default(),
                async { true },
                pending(),
            )
            .await
        });

        wait_until_ready(&gate).await;
        let client = start_http2_request(address, "/hold").await;
        state.started.notified().await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        wait_until_unready(&gate).await;
        wait_until_dropped(&dropped).await;
        assert!(TcpStream::connect(address).await.is_err());
        finish_http2_client(client).await;
    }

    fn assert_invalid_config(
        mutate: impl FnOnce(&mut LoopbackServerConfigInputV1),
        expected: LoopbackServerConfigErrorV1,
    ) {
        let mut input = LoopbackServerConfigInputV1::default();
        mutate(&mut input);
        assert_eq!(LoopbackServerConfigV1::try_new(input), Err(expected));
    }

    fn loopback_address(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    async fn available_loopback_address() -> SocketAddr {
        let listener = TcpListener::bind(loopback_address(0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        address
    }

    fn config_with_drain_timeout(timeout: Duration) -> LoopbackServerConfigV1 {
        let input = LoopbackServerConfigInputV1 {
            graceful_drain_timeout: timeout,
            ..LoopbackServerConfigInputV1::default()
        };
        LoopbackServerConfigV1::try_new(input).unwrap()
    }

    fn config_with_max_connections(max_connections: usize) -> LoopbackServerConfigV1 {
        let input = LoopbackServerConfigInputV1 {
            max_connections,
            ..LoopbackServerConfigInputV1::default()
        };
        LoopbackServerConfigV1::try_new(input).unwrap()
    }

    fn config_with_startup_timeout(timeout: Duration) -> LoopbackServerConfigV1 {
        let input = LoopbackServerConfigInputV1 {
            startup_readiness_timeout: timeout,
            ..LoopbackServerConfigInputV1::default()
        };
        LoopbackServerConfigV1::try_new(input).unwrap()
    }

    fn config_with_idle_h2_recovery() -> LoopbackServerConfigV1 {
        let input = LoopbackServerConfigInputV1 {
            max_connections: 1,
            h2_keep_alive_interval: Duration::from_millis(10),
            h2_keep_alive_timeout: Duration::from_millis(10),
            ..LoopbackServerConfigInputV1::default()
        };
        LoopbackServerConfigV1::try_new(input).unwrap()
    }

    async fn wait_until_ready(gate: &ProductApiReadinessGate) {
        timeout(Duration::from_secs(1), async {
            while !gate.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn wait_until_unready(gate: &ProductApiReadinessGate) {
        timeout(Duration::from_secs(1), async {
            while gate.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn wait_until_dropped(dropped: &AtomicBool) {
        timeout(Duration::from_secs(1), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn wait_until_started_count(state: &LimitedBlockingState, expected: usize) {
        timeout(Duration::from_secs(1), async {
            loop {
                let changed = state.changed.notified();
                if state.started.load(Ordering::Acquire) >= expected {
                    return;
                }
                changed.await;
            }
        })
        .await
        .unwrap();
    }

    async fn request_http1(address: SocketAddr, path: &'static str) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        String::from_utf8(response).unwrap()
    }

    async fn start_http2_request(address: SocketAddr, path: &'static str) -> Http2ClientRequest {
        let stream = TcpStream::connect(address).await.unwrap();
        let (mut sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
        let driver = tokio::spawn(connection);
        let uri = hyper::Uri::builder()
            .scheme("http")
            .authority(format!("localhost:{}", address.port()))
            .path_and_query(path)
            .build()
            .unwrap();
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = tokio::spawn(async move { sender.send_request(request).await });
        Http2ClientRequest { driver, response }
    }

    async fn finish_http2_client(mut client: Http2ClientRequest) {
        if timeout(Duration::from_secs(1), &mut client.response)
            .await
            .is_err()
        {
            client.response.abort();
            let _ = client.response.await;
        }
        if timeout(Duration::from_secs(1), &mut client.driver)
            .await
            .is_err()
        {
            client.driver.abort();
            let _ = client.driver.await;
        }
    }

    struct Http2ClientRequest {
        driver: JoinHandle<Result<(), hyper::Error>>,
        response: JoinHandle<Result<hyper::Response<Incoming>, hyper::Error>>,
    }

    struct ObserveReadyOnShutdownPoll {
        gate: ProductApiReadinessGate,
        observed_ready_polls: Arc<AtomicUsize>,
    }

    impl Future for ObserveReadyOnShutdownPoll {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
            if self.gate.is_ready() {
                self.observed_ready_polls.fetch_add(1, Ordering::AcqRel);
            }
            Poll::Ready(())
        }
    }

    #[derive(Clone)]
    struct BlockingState {
        started: Arc<Notify>,
        release: Arc<Notify>,
        dropped: Arc<AtomicBool>,
    }

    impl BlockingState {
        fn new() -> Self {
            Self {
                started: Arc::new(Notify::new()),
                release: Arc::new(Notify::new()),
                dropped: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    async fn blocking_request(
        State(state): State<BlockingState>,
    ) -> Result<&'static str, Infallible> {
        let _drop = DropMarker(state.dropped.clone());
        state.started.notify_one();
        state.release.notified().await;
        Ok("released")
    }

    struct DropMarker(Arc<AtomicBool>);

    impl Drop for DropMarker {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[derive(Clone)]
    struct LimitedBlockingState {
        started: Arc<AtomicUsize>,
        changed: Arc<Notify>,
        release: Arc<Semaphore>,
    }

    impl LimitedBlockingState {
        fn new() -> Self {
            Self {
                started: Arc::new(AtomicUsize::new(0)),
                changed: Arc::new(Notify::new()),
                release: Arc::new(Semaphore::new(0)),
            }
        }
    }

    async fn limited_blocking_request(
        State(state): State<LimitedBlockingState>,
    ) -> Result<&'static str, Infallible> {
        state.started.fetch_add(1, Ordering::AcqRel);
        state.changed.notify_waiters();
        if let Ok(permit) = state.release.acquire().await {
            permit.forget();
        }
        Ok("released")
    }
}
