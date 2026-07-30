use std::fmt::{Debug, Formatter};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{timeout, timeout_at, Instant as TokioInstant};

const RUNTIME_HEALTH_REQUEST_LIMIT: usize = 1_024;
const RUNTIME_HEALTH_CONNECTION_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeHealthStartErrorV1 {
    #[error("runtime health listener bind failed")]
    Bind,
    #[error("runtime health asynchronous executor is unavailable")]
    AsyncRuntimeUnavailable,
}

#[cfg(test)]
impl RuntimeHealthStartErrorV1 {
    const fn code(self) -> &'static str {
        match self {
            Self::Bind => "runtime_health_bind",
            Self::AsyncRuntimeUnavailable => "runtime_health_async_runtime_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeHealthShutdownErrorV1 {
    #[error("runtime health listener shutdown deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime health listener task stopped unexpectedly")]
    TaskStopped,
}

#[cfg(test)]
impl RuntimeHealthShutdownErrorV1 {
    const fn code(self) -> &'static str {
        match self {
            Self::DeadlineElapsed => "runtime_health_shutdown_deadline_elapsed",
            Self::TaskStopped => "runtime_health_task_stopped",
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeHealthReadinessHandleV1 {
    state: Arc<RuntimeHealthStateV1>,
}

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeHealthReadinessObserverV1 {
    state: Arc<RuntimeHealthStateV1>,
}

impl RuntimeHealthReadinessHandleV1 {
    pub(crate) fn seal_readiness(&self) {
        self.state.readiness_sealed.store(true, Ordering::Release);
        self.state.ready.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn is_ready(&self) -> bool {
        self.state.ready.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn is_sealed(&self) -> bool {
        self.state.readiness_sealed.load(Ordering::Acquire)
    }
}

impl RuntimeHealthReadinessObserverV1 {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_ready_v1(&self) -> bool {
        self.state.ready.load(Ordering::Acquire)
            && !self.state.readiness_sealed.load(Ordering::Acquire)
    }
}

impl Debug for RuntimeHealthReadinessHandleV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeHealthReadinessHandleV1(<redacted>)")
    }
}

impl Debug for RuntimeHealthReadinessObserverV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeHealthReadinessObserverV1(<redacted>)")
    }
}

struct RuntimeHealthStateV1 {
    live: AtomicBool,
    ready: AtomicBool,
    readiness_sealed: AtomicBool,
    stopped: watch::Sender<bool>,
}

pub(crate) struct RuntimeHealthReadinessPublisherV2 {
    state: Arc<RuntimeHealthStateV1>,
}

impl RuntimeHealthReadinessPublisherV2 {
    pub(crate) fn publish_ready_v2(&self) -> bool {
        if self.state.readiness_sealed.load(Ordering::Acquire) {
            return false;
        }
        self.state.ready.store(true, Ordering::Release);
        if self.state.readiness_sealed.load(Ordering::Acquire) {
            self.state.ready.store(false, Ordering::Release);
            return false;
        }
        true
    }

    pub(crate) fn remove_readiness_v2(&self) {
        self.state.ready.store(false, Ordering::Release);
    }
}

impl Debug for RuntimeHealthReadinessPublisherV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeHealthReadinessPublisherV2(<redacted>)")
    }
}

pub(crate) struct RuntimeHealthTerminalObserverV1 {
    stopped: watch::Receiver<bool>,
}

impl RuntimeHealthTerminalObserverV1 {
    pub(crate) async fn wait(&mut self) {
        loop {
            if *self.stopped.borrow_and_update() {
                return;
            }
            if self.stopped.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Debug for RuntimeHealthTerminalObserverV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeHealthTerminalObserverV1(<redacted>)")
    }
}

pub(crate) struct RuntimeHealthSupervisorV1 {
    state: Arc<RuntimeHealthStateV1>,
    stop: mpsc::Sender<()>,
    task: Option<JoinHandle<bool>>,
    readiness_publisher_available: bool,
    #[cfg(test)]
    bound_addr: SocketAddr,
}

impl RuntimeHealthSupervisorV1 {
    pub(crate) async fn start(bind_addr: SocketAddr) -> Result<Self, RuntimeHealthStartErrorV1> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| RuntimeHealthStartErrorV1::AsyncRuntimeUnavailable)?;
        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|_| RuntimeHealthStartErrorV1::Bind)?;
        #[cfg(test)]
        let bound_addr = listener
            .local_addr()
            .map_err(|_| RuntimeHealthStartErrorV1::Bind)?;
        #[cfg(not(test))]
        listener
            .local_addr()
            .map_err(|_| RuntimeHealthStartErrorV1::Bind)?;
        let (stopped, _) = watch::channel(false);
        let state = Arc::new(RuntimeHealthStateV1 {
            live: AtomicBool::new(true),
            ready: AtomicBool::new(false),
            readiness_sealed: AtomicBool::new(false),
            stopped,
        });
        let (stop, stop_receiver) = mpsc::channel(1);
        let task_state = state.clone();
        let task = runtime.spawn(run_runtime_health_listener_v1(
            listener,
            task_state,
            stop_receiver,
        ));
        Ok(Self {
            state,
            stop,
            task: Some(task),
            readiness_publisher_available: true,
            #[cfg(test)]
            bound_addr,
        })
    }

    pub(crate) fn readiness_handle(&self) -> RuntimeHealthReadinessHandleV1 {
        RuntimeHealthReadinessHandleV1 {
            state: self.state.clone(),
        }
    }

    pub(crate) fn readiness_observer_v1(&self) -> RuntimeHealthReadinessObserverV1 {
        RuntimeHealthReadinessObserverV1 {
            state: self.state.clone(),
        }
    }

    pub(crate) fn take_readiness_publisher_v2(
        &mut self,
    ) -> Option<RuntimeHealthReadinessPublisherV2> {
        if !self.readiness_publisher_available {
            return None;
        }
        self.readiness_publisher_available = false;
        Some(RuntimeHealthReadinessPublisherV2 {
            state: self.state.clone(),
        })
    }

    pub(crate) fn terminal_observer(&self) -> RuntimeHealthTerminalObserverV1 {
        RuntimeHealthTerminalObserverV1 {
            stopped: self.state.stopped.subscribe(),
        }
    }

    #[cfg(test)]
    fn bound_addr(&self) -> SocketAddr {
        self.bound_addr
    }

    #[cfg(test)]
    fn is_live(&self) -> bool {
        self.state.live.load(Ordering::Acquire)
    }

    pub(crate) async fn shutdown_until(
        mut self,
        deadline: Instant,
    ) -> Result<(), RuntimeHealthShutdownErrorV1> {
        self.readiness_handle().seal_readiness();
        if Instant::now() >= deadline {
            self.abort_and_join().await;
            return Err(RuntimeHealthShutdownErrorV1::DeadlineElapsed);
        }
        let _ = self.stop.try_send(());
        let Some(mut task) = self.task.take() else {
            return Err(RuntimeHealthShutdownErrorV1::TaskStopped);
        };
        match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
            Ok(Ok(true)) => Ok(()),
            Ok(_) => Err(RuntimeHealthShutdownErrorV1::TaskStopped),
            Err(_) => {
                task.abort();
                let _ = task.await;
                Err(RuntimeHealthShutdownErrorV1::DeadlineElapsed)
            }
        }
    }

    async fn abort_and_join(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for RuntimeHealthSupervisorV1 {
    fn drop(&mut self) {
        self.readiness_handle().seal_readiness();
        self.state.live.store(false, Ordering::Release);
        self.state.stopped.send_replace(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Debug for RuntimeHealthSupervisorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeHealthSupervisorV1(<redacted>)")
    }
}

struct RuntimeHealthActorGuardV1 {
    state: Arc<RuntimeHealthStateV1>,
}

impl Drop for RuntimeHealthActorGuardV1 {
    fn drop(&mut self) {
        self.state.readiness_sealed.store(true, Ordering::Release);
        self.state.ready.store(false, Ordering::Release);
        self.state.live.store(false, Ordering::Release);
        self.state.stopped.send_replace(true);
    }
}

async fn run_runtime_health_listener_v1(
    listener: TcpListener,
    state: Arc<RuntimeHealthStateV1>,
    mut stop: mpsc::Receiver<()>,
) -> bool {
    let guard = RuntimeHealthActorGuardV1 {
        state: state.clone(),
    };
    loop {
        tokio::select! {
            biased;
            command = stop.recv() => {
                drop(guard);
                return command.is_some();
            }
            accepted = listener.accept() => {
                let Ok((connection, _)) = accepted else {
                    drop(guard);
                    return false;
                };
                tokio::select! {
                    biased;
                    command = stop.recv() => {
                        drop(guard);
                        return command.is_some();
                    }
                    _ = handle_runtime_health_connection_v1(connection, &state) => {}
                }
            }
        }
    }
}

async fn handle_runtime_health_connection_v1(
    mut connection: TcpStream,
    state: &RuntimeHealthStateV1,
) {
    let request = async {
        let mut bytes = [0_u8; RUNTIME_HEALTH_REQUEST_LIMIT];
        let mut count = 0;
        while count < bytes.len() {
            let read = connection.read(&mut bytes[count..]).await?;
            if read == 0 {
                break;
            }
            count += read;
            if bytes[..count]
                .windows(4)
                .any(|window| window == b"\r\n\r\n")
            {
                break;
            }
        }
        let response = runtime_health_response_v1(&bytes[..count], state);
        connection.write_all(response).await?;
        connection.shutdown().await
    };
    let _ = timeout(RUNTIME_HEALTH_CONNECTION_TIMEOUT, request).await;
}

fn runtime_health_response_v1(request: &[u8], state: &RuntimeHealthStateV1) -> &'static [u8] {
    if !request.windows(4).any(|window| window == b"\r\n\r\n") {
        return b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_found";
    }
    let request_line = request
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|end| &request[..end]);
    if matches!(
        request_line,
        Some(line)
            if line == b"GET /health/live HTTP/1.1"
                || line == b"GET /health/live HTTP/1.0"
    ) {
        if state.live.load(Ordering::Acquire) {
            return b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlive";
        }
        return b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot_live";
    }
    if matches!(
        request_line,
        Some(line)
            if line == b"GET /health/ready HTTP/1.1"
                || line == b"GET /health/ready HTTP/1.0"
    ) {
        if state.ready.load(Ordering::Acquire) {
            return b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nready";
        }
        return b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_ready";
    }
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot_found"
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddrV4};

    use super::*;

    async fn request(addr: SocketAddr, path: &str) -> String {
        let mut connection = TcpStream::connect(addr).await.unwrap();
        connection
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        connection.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn listener_is_live_unready_redacted_and_stops_cleanly() {
        let mut supervisor = RuntimeHealthSupervisorV1::start(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            0,
        )))
        .await
        .unwrap();
        let addr = supervisor.bound_addr();
        let publisher = supervisor.take_readiness_publisher_v2().unwrap();
        let observer = supervisor.readiness_observer_v1();

        assert!(supervisor.is_live());
        assert!(!supervisor.readiness_handle().is_ready());
        assert!(!observer.is_ready_v1());
        assert!(request(addr, "/health/live")
            .await
            .starts_with("HTTP/1.1 200"));
        assert!(request(addr, "/health/ready")
            .await
            .starts_with("HTTP/1.1 503"));
        assert!(request(addr, "/unknown").await.starts_with("HTTP/1.1 404"));
        assert_eq!(
            format!("{supervisor:?}"),
            "RuntimeHealthSupervisorV1(<redacted>)"
        );
        assert_eq!(
            format!("{:?}", supervisor.readiness_handle()),
            "RuntimeHealthReadinessHandleV1(<redacted>)"
        );
        assert_eq!(
            format!("{observer:?}"),
            "RuntimeHealthReadinessObserverV1(<redacted>)"
        );
        assert_eq!(
            format!("{publisher:?}"),
            "RuntimeHealthReadinessPublisherV2(<redacted>)"
        );
        assert!(publisher.publish_ready_v2());
        assert!(observer.is_ready_v1());
        assert!(request(addr, "/health/ready")
            .await
            .starts_with("HTTP/1.1 200"));
        publisher.remove_readiness_v2();
        assert!(!observer.is_ready_v1());
        assert!(request(addr, "/health/ready")
            .await
            .starts_with("HTTP/1.1 503"));
        assert!(supervisor.take_readiness_publisher_v2().is_none());

        supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(TcpStream::connect(addr).await.is_err());
    }

    #[tokio::test]
    async fn elapsed_deadline_aborts_the_listener_and_removes_liveness() {
        let supervisor = RuntimeHealthSupervisorV1::start(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            0,
        )))
        .await
        .unwrap();
        let addr = supervisor.bound_addr();

        assert_eq!(
            supervisor.shutdown_until(Instant::now()).await,
            Err(RuntimeHealthShutdownErrorV1::DeadlineElapsed)
        );
        assert!(TcpStream::connect(addr).await.is_err());
    }

    #[test]
    fn health_errors_are_finite() {
        assert_eq!(
            RuntimeHealthStartErrorV1::Bind.code(),
            "runtime_health_bind"
        );
        assert_eq!(
            RuntimeHealthShutdownErrorV1::DeadlineElapsed.code(),
            "runtime_health_shutdown_deadline_elapsed"
        );
    }

    #[test]
    fn request_matching_requires_complete_exact_http_lines() {
        let (stopped, _) = watch::channel(false);
        let state = RuntimeHealthStateV1 {
            live: AtomicBool::new(true),
            ready: AtomicBool::new(false),
            readiness_sealed: AtomicBool::new(false),
            stopped,
        };

        assert!(runtime_health_response_v1(
            b"GET /health/live HTTP/1.1\r\nHost: localhost\r\n\r\n",
            &state,
        )
        .starts_with(b"HTTP/1.1 200"));
        assert!(
            runtime_health_response_v1(b"GET /health/live HTTP/1.1 extra\r\n\r\n", &state,)
                .starts_with(b"HTTP/1.1 404")
        );
        assert!(
            runtime_health_response_v1(b"GET /health/live HTTP/1.1\r\n", &state)
                .starts_with(b"HTTP/1.1 404")
        );
    }

    #[test]
    fn shutdown_seal_prevents_every_later_publish() {
        let (stopped, _) = watch::channel(false);
        let state = Arc::new(RuntimeHealthStateV1 {
            live: AtomicBool::new(true),
            ready: AtomicBool::new(false),
            readiness_sealed: AtomicBool::new(false),
            stopped,
        });
        let publisher = RuntimeHealthReadinessPublisherV2 {
            state: state.clone(),
        };
        let invalidator = RuntimeHealthReadinessHandleV1 {
            state: state.clone(),
        };
        let observer = RuntimeHealthReadinessObserverV1 {
            state: state.clone(),
        };

        assert!(publisher.publish_ready_v2());
        assert!(observer.is_ready_v1());
        publisher.remove_readiness_v2();
        assert!(!observer.is_ready_v1());
        assert!(publisher.publish_ready_v2());
        assert!(observer.is_ready_v1());
        invalidator.seal_readiness();

        assert!(invalidator.is_sealed());
        assert!(!invalidator.is_ready());
        assert!(!observer.is_ready_v1());
        assert!(!publisher.publish_ready_v2());
        assert!(!invalidator.is_ready());
        assert!(!observer.is_ready_v1());
    }

    #[test]
    fn concurrent_publish_and_shutdown_seal_finish_not_ready() {
        for _ in 0..512 {
            let (stopped, _) = watch::channel(false);
            let state = Arc::new(RuntimeHealthStateV1 {
                live: AtomicBool::new(true),
                ready: AtomicBool::new(false),
                readiness_sealed: AtomicBool::new(false),
                stopped,
            });
            let publisher = RuntimeHealthReadinessPublisherV2 {
                state: state.clone(),
            };
            let invalidator = RuntimeHealthReadinessHandleV1 {
                state: state.clone(),
            };
            let start = Arc::new(std::sync::Barrier::new(2));
            let publish_start = start.clone();
            let publish = std::thread::spawn(move || {
                publish_start.wait();
                publisher.publish_ready_v2()
            });

            start.wait();
            invalidator.seal_readiness();
            let _ = publish.join().unwrap();

            assert!(invalidator.is_sealed());
            assert!(!invalidator.is_ready());
        }
    }
}
