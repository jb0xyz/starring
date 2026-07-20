use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::Router;
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use product_control_http::ProductApiReadinessGate;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tower::ServiceExt;

pub const MAX_GRACEFUL_DRAIN_TIMEOUT: Duration = Duration::from_secs(120);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_secs(1);

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
    #[error("the readiness gate was not initially unready")]
    ReadinessGateAlreadyReady,
    #[error("the server bind address is not loopback")]
    NonLoopbackBindAddress,
    #[error("the graceful drain timeout is outside the supported range")]
    InvalidDrainTimeout,
    #[error("the loopback listener could not be bound")]
    BindFailed,
    #[error("an isolated connection task failed")]
    ConnectionTaskFailed,
    #[error("graceful connection drain exceeded its deadline")]
    DrainTimedOut,
}

pub async fn serve_verified_loopback<S>(
    bind_addr: SocketAddr,
    router: Router,
    readiness_gate: ProductApiReadinessGate,
    drain_timeout: Duration,
    shutdown: S,
) -> Result<LoopbackServeReportV1, LoopbackServeErrorV1>
where
    S: Future<Output = ()> + Send,
{
    let readiness = ReadinessReset::claim(readiness_gate)?;
    validate_server_parameters(bind_addr, drain_timeout)?;

    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|_| LoopbackServeErrorV1::BindFailed)?;
    let local_addr = listener
        .local_addr()
        .map_err(|_| LoopbackServeErrorV1::BindFailed)?;
    let (drain_sender, drain_receiver) = watch::channel(false);
    let mut connections = JoinSet::new();
    let mut shutdown = Box::pin(shutdown);

    readiness.mark_ready();

    let exit = loop {
        tokio::select! {
            biased;
            _ = shutdown.as_mut() => {
                break ServeExit::Shutdown;
            }
            (stream, _) = accept_resilient(&listener) => {
                spawn_connection(
                    &mut connections,
                    stream,
                    router.clone(),
                    drain_receiver.clone(),
                );
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if matches!(completed, Some(Err(_))) {
                    break ServeExit::ConnectionTaskFailed;
                }
            }
        }
    };

    readiness.mark_unready();
    drop(listener);

    match exit {
        ServeExit::Shutdown => {
            drain_connections(&mut connections, drain_sender, drain_timeout).await?;
            Ok(LoopbackServeReportV1 { local_addr })
        }
        ServeExit::ConnectionTaskFailed => {
            abort_connections(&mut connections).await;
            Err(LoopbackServeErrorV1::ConnectionTaskFailed)
        }
    }
}

enum ServeExit {
    Shutdown,
    ConnectionTaskFailed,
}

async fn accept_resilient(listener: &TcpListener) -> (TcpStream, SocketAddr) {
    loop {
        match listener.accept().await {
            Ok(accepted) => return accepted,
            Err(error) => {
                if let Some(delay) = accept_retry_delay(&error) {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

fn accept_retry_delay(error: &io::Error) -> Option<Duration> {
    match error.kind() {
        io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::Interrupted => None,
        _ => Some(ACCEPT_RETRY_DELAY),
    }
}

fn validate_server_parameters(
    bind_addr: SocketAddr,
    drain_timeout: Duration,
) -> Result<(), LoopbackServeErrorV1> {
    if !bind_addr.ip().is_loopback() {
        return Err(LoopbackServeErrorV1::NonLoopbackBindAddress);
    }
    if drain_timeout.is_zero() || drain_timeout > MAX_GRACEFUL_DRAIN_TIMEOUT {
        return Err(LoopbackServeErrorV1::InvalidDrainTimeout);
    }
    Ok(())
}

fn spawn_connection(
    connections: &mut JoinSet<()>,
    stream: TcpStream,
    router: Router,
    mut drain_receiver: watch::Receiver<bool>,
) {
    connections.spawn(async move {
        let service = router.map_request(|request: Request<Incoming>| request.map(Body::new));
        let service = TowerToHyperService::new(service);
        let builder = Builder::new(TokioExecutor::new());
        let connection = builder.serve_connection(TokioIo::new(stream), service);
        tokio::pin!(connection);

        tokio::select! {
            _ = connection.as_mut() => {}
            _ = drain_receiver.changed() => {
                connection.as_mut().graceful_shutdown();
                let _ = connection.await;
            }
        }
    });
}

async fn drain_connections(
    connections: &mut JoinSet<()>,
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

async fn abort_connections(connections: &mut JoinSet<()>) {
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

struct ReadinessReset {
    gate: ProductApiReadinessGate,
}

impl ReadinessReset {
    fn claim(gate: ProductApiReadinessGate) -> Result<Self, LoopbackServeErrorV1> {
        let was_ready = gate.is_ready();
        gate.mark_unready();
        if was_ready {
            return Err(LoopbackServeErrorV1::ReadinessGateAlreadyReady);
        }
        Ok(Self { gate })
    }

    fn mark_ready(&self) {
        self.gate.mark_ready();
    }

    fn mark_unready(&self) {
        self.gate.mark_unready();
    }
}

impl Drop for ReadinessReset {
    fn drop(&mut self) {
        self.gate.mark_unready();
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::future::pending;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use axum::extract::State;
    use axum::routing::get;
    use hyper::client::conn::http2;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::{oneshot, Notify};

    use super::*;

    #[tokio::test]
    async fn non_loopback_bind_is_rejected_and_gate_remains_unready() {
        let gate = ProductApiReadinessGate::initially_unready();
        let result = serve_verified_loopback(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            Router::new(),
            gate.clone(),
            Duration::from_secs(1),
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::NonLoopbackBindAddress));
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn already_ready_gate_is_rejected_and_reset() {
        let gate = ProductApiReadinessGate::initially_unready();
        gate.mark_ready();
        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            Duration::from_secs(1),
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::ReadinessGateAlreadyReady));
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn gate_stays_unready_when_bind_fails() {
        let occupied = TcpListener::bind(loopback_address(0)).await.unwrap();
        let occupied_address = occupied.local_addr().unwrap();
        let gate = ProductApiReadinessGate::initially_unready();
        let result = serve_verified_loopback(
            occupied_address,
            Router::new(),
            gate.clone(),
            Duration::from_secs(1),
            pending(),
        )
        .await;

        assert_eq!(result, Err(LoopbackServeErrorV1::BindFailed));
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn invalid_drain_bounds_are_rejected() {
        for drain_timeout in [
            Duration::ZERO,
            MAX_GRACEFUL_DRAIN_TIMEOUT + Duration::from_nanos(1),
        ] {
            let gate = ProductApiReadinessGate::initially_unready();
            let result = serve_verified_loopback(
                loopback_address(0),
                Router::new(),
                gate.clone(),
                drain_timeout,
                pending(),
            )
            .await;

            assert_eq!(result, Err(LoopbackServeErrorV1::InvalidDrainTimeout));
            assert!(!gate.is_ready());
        }
    }

    #[test]
    fn accept_errors_are_retried_without_terminating_the_server() {
        assert_eq!(
            accept_retry_delay(&io::Error::from(io::ErrorKind::ConnectionReset)),
            None
        );
        assert_eq!(
            accept_retry_delay(&io::Error::from(io::ErrorKind::Other)),
            Some(ACCEPT_RETRY_DELAY)
        );
    }

    #[tokio::test]
    async fn bound_server_accepts_http_and_shuts_down_cleanly() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new().route("/health", get(|| async { "alive" })),
                server_gate,
                Duration::from_secs(1),
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let response = request(address, "/health").await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("alive"));

        shutdown_sender.send(()).unwrap();
        let report = server.await.unwrap().unwrap();
        assert_eq!(report.local_addr(), address);
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn shutdown_marks_unready_before_waiting_for_active_request() {
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
                Duration::from_secs(1),
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let client = tokio::spawn(request(address, "/hold"));
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
                Duration::from_secs(1),
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let stream = TcpStream::connect(address).await.unwrap();
        let (mut sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
        let driver = tokio::spawn(connection);
        let uri = hyper::Uri::builder()
            .scheme("http")
            .authority(format!("localhost:{}", address.port()))
            .path_and_query("/hold")
            .build()
            .unwrap();
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = tokio::spawn(async move { sender.send_request(request).await });

        state.started.notified().await;
        shutdown_sender.send(()).unwrap();
        wait_until_unready(&gate).await;
        assert!(!server.is_finished());
        state.release.notify_one();

        assert_eq!(response.await.unwrap().unwrap().status(), 200);
        assert!(server.await.unwrap().is_ok());
        assert!(driver.await.unwrap().is_ok());
        assert!(!gate.is_ready());
    }

    #[tokio::test]
    async fn immediate_shutdown_race_never_leaves_readiness_open() {
        let gate = ProductApiReadinessGate::initially_unready();
        let result = serve_verified_loopback(
            loopback_address(0),
            Router::new(),
            gate.clone(),
            Duration::from_secs(1),
            async {},
        )
        .await
        .unwrap();

        assert!(!gate.is_ready());
        assert!(TcpStream::connect(result.local_addr()).await.is_err());
    }

    #[tokio::test]
    async fn cancellation_resets_readiness_and_closes_listener() {
        let address = available_loopback_address().await;
        let gate = ProductApiReadinessGate::initially_unready();
        let server_gate = gate.clone();
        let server = tokio::spawn(async move {
            serve_verified_loopback(
                address,
                Router::new(),
                server_gate,
                Duration::from_secs(1),
                pending(),
            )
            .await
        });

        wait_until_ready(&gate).await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        wait_until_unready(&gate).await;
        assert!(TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn drain_timeout_aborts_and_joins_active_request() {
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
                Duration::from_millis(25),
                async move {
                    let _ = shutdown_receiver.await;
                },
            )
            .await
        });

        wait_until_ready(&gate).await;
        let client = tokio::spawn(request(address, "/hold"));
        state.started.notified().await;
        shutdown_sender.send(()).unwrap();

        assert_eq!(
            server.await.unwrap(),
            Err(LoopbackServeErrorV1::DrainTimedOut)
        );
        assert!(!gate.is_ready());
        assert!(dropped.load(Ordering::Acquire));
        client.abort();
        let _ = client.await;
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

    async fn wait_until_ready(gate: &ProductApiReadinessGate) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !gate.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn wait_until_unready(gate: &ProductApiReadinessGate) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while gate.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn request(address: SocketAddr, path: &'static str) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        String::from_utf8(response).unwrap()
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
}
