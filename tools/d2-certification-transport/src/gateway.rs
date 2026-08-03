use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};

use crate::state::{DuplicateClaim, SharedState};
use crate::Config;

const MAX_GATEWAY_CONNECTIONS: usize = 4;
const MAX_GATEWAY_MESSAGE_BYTES: usize = 512 * 1024;
const MAX_GATEWAY_FRAME_BYTES: usize = 128 * 1024;
const GATEWAY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const GATEWAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const GATEWAY_CLOSE_ACK_TIMEOUT: Duration = Duration::from_millis(500);
const GATEWAY_RELAY_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
const GATEWAY_RELAY_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const GATEWAY_CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

enum GatewayConnectionTerminal {
    CleanClose,
    Partitioned,
    Shutdown,
}

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("gateway_bind_failed")]
    Bind,
    #[error("gateway_accept_failed")]
    Accept,
    #[error("gateway_connection_failed")]
    Connection,
    #[error("gateway_connection_drain_failed")]
    ConnectionDrain,
}

pub async fn serve(
    config: Config,
    state: Arc<SharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), GatewayError> {
    let listener = TcpListener::bind(config.gateway_listen())
        .await
        .map_err(|_| GatewayError::Bind)?;
    state.mark_gateway_listener_ready();
    let permits = Arc::new(Semaphore::new(MAX_GATEWAY_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                if gateway_task_failed(joined) {
                    return Err(GatewayError::Connection);
                }
            }
            accepted = listener.accept() => {
                let (stream, address) = accepted.map_err(|_| GatewayError::Accept)?;
                if !address.ip().is_loopback() || state.is_partitioned() {
                    continue;
                }
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    continue;
                };
                let config = config.clone();
                let connection_state = Arc::clone(&state);
                let connection = state.begin_gateway_connection();
                let connection_shutdown = shutdown_rx.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    match proxy_connection(
                        stream,
                        config,
                        connection_state,
                        connection_shutdown,
                    )
                    .await
                    {
                        Ok(GatewayConnectionTerminal::CleanClose) => {
                            connection.complete_clean_close();
                            Ok(())
                        }
                        Ok(GatewayConnectionTerminal::Partitioned | GatewayConnectionTerminal::Shutdown) => {
                            connection.complete();
                            Ok(())
                        }
                        Err(()) => {
                            connection.fail();
                            Err(())
                        }
                    }
                });
            }
        }
    }
    let drained = tokio::time::timeout(GATEWAY_CONNECTION_DRAIN_TIMEOUT, async {
        let mut failed = false;
        while let Some(joined) = connections.join_next().await {
            failed |= gateway_task_failed(Some(joined));
        }
        failed
    })
    .await;
    match drained {
        Ok(false) => Ok(()),
        Ok(true) => Err(GatewayError::Connection),
        Err(_) => {
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            Err(GatewayError::ConnectionDrain)
        }
    }
}

fn gateway_task_failed(joined: Option<Result<Result<(), ()>, tokio::task::JoinError>>) -> bool {
    !matches!(joined, Some(Ok(Ok(()))))
}

async fn proxy_connection(
    stream: TcpStream,
    config: Config,
    state: Arc<SharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<GatewayConnectionTerminal, ()> {
    let downstream = tokio::time::timeout(
        GATEWAY_HANDSHAKE_TIMEOUT,
        tokio_tungstenite::accept_async_with_config(stream, Some(websocket_config())),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    if state.is_partitioned() {
        return Err(());
    }
    let (upstream, _) = tokio::time::timeout(
        GATEWAY_CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(
            config.gateway_upstream(),
            Some(websocket_config()),
            false,
        ),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let mut partition_rx = state.subscribe_partition();
    let (mut downstream_write, mut downstream_read) = downstream.split();
    let (mut upstream_write, mut upstream_read) = upstream.split();
    loop {
        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    let (downstream, upstream) = tokio::join!(
                        tokio::time::timeout(
                            GATEWAY_RELAY_WRITE_TIMEOUT,
                            downstream_write.send(Message::Close(None)),
                        ),
                        tokio::time::timeout(
                            GATEWAY_RELAY_WRITE_TIMEOUT,
                            upstream_write.send(Message::Close(None)),
                        ),
                    );
                    if !matches!(downstream, Ok(Ok(()))) || !matches!(upstream, Ok(Ok(()))) {
                        return Err(());
                    }
                    return Ok(GatewayConnectionTerminal::Shutdown);
                }
            }
            _ = partition_rx.recv() => {
                let (downstream, upstream) = tokio::join!(
                    tokio::time::timeout(
                        GATEWAY_RELAY_WRITE_TIMEOUT,
                        downstream_write.send(Message::Close(None)),
                    ),
                    tokio::time::timeout(
                        GATEWAY_RELAY_WRITE_TIMEOUT,
                        upstream_write.send(Message::Close(None)),
                    ),
                );
                if !matches!(downstream, Ok(Ok(()))) || !matches!(upstream, Ok(Ok(()))) {
                    return Err(());
                }
                return Ok(GatewayConnectionTerminal::Partitioned);
            }
            message = downstream_read.next() => {
                let Some(message) = message else {
                    return Err(());
                };
                let message = message.map_err(|_| ())?;
                let terminal = message.is_close();
                if terminal {
                    tokio::time::timeout(
                        GATEWAY_CLOSE_ACK_TIMEOUT,
                        downstream_write.flush(),
                    )
                    .await
                    .map_err(|_| ())?
                    .map_err(|_| ())?;
                }
                tokio::time::timeout(
                    GATEWAY_RELAY_WRITE_TIMEOUT,
                    upstream_write.send(message),
                )
                .await
                .map_err(|_| ())?
                .map_err(|_| ())?;
                if terminal {
                    return Ok(GatewayConnectionTerminal::CleanClose);
                }
            }
            message = upstream_read.next() => {
                let Some(message) = message else {
                    return Err(());
                };
                let message = message.map_err(|_| ())?;
                let terminal = message.is_close();
                if terminal {
                    tokio::time::timeout(
                        GATEWAY_CLOSE_ACK_TIMEOUT,
                        upstream_write.flush(),
                    )
                    .await
                    .map_err(|_| ())?
                    .map_err(|_| ())?;
                }
                let (message, duplicate) = transform_upstream_message(message, &config, &state);
                if let Some(claim) = duplicate {
                    let delivered = tokio::time::timeout(GATEWAY_RELAY_WRITE_TIMEOUT, async {
                        downstream_write.send(message.clone()).await.map_err(|_| ())?;
                        if !state.record_duplicate_delivery(&claim) {
                            return Err(());
                        }
                        downstream_write.send(message).await.map_err(|_| ())?;
                        if !state.record_duplicate_delivery(&claim) {
                            return Err(());
                        }
                        Ok::<(), ()>(())
                    })
                    .await;
                    if !matches!(delivered, Ok(Ok(()))) {
                        let _ = state.abort_duplicate(claim);
                        return Err(());
                    }
                    if !state.finish_duplicate(claim) {
                        return Err(());
                    }
                } else {
                    tokio::time::timeout(
                        GATEWAY_RELAY_WRITE_TIMEOUT,
                        downstream_write.send(message),
                    )
                    .await
                    .map_err(|_| ())?
                    .map_err(|_| ())?;
                }
                if terminal {
                    return Ok(GatewayConnectionTerminal::CleanClose);
                }
            }
            _ = tokio::time::sleep(GATEWAY_RELAY_IDLE_TIMEOUT) => {
                return Err(());
            }
        }
    }
}

fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_GATEWAY_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_GATEWAY_FRAME_BYTES))
}

fn transform_upstream_message(
    message: Message,
    config: &Config,
    state: &SharedState,
) -> (Message, Option<DuplicateClaim>) {
    let Message::Text(text) = &message else {
        return (message, None);
    };
    let Ok(mut payload) = serde_json::from_str::<Value>(text.as_str()) else {
        return (message, None);
    };
    if is_dispatch(&payload, "READY") {
        let Some(resume_url) = payload
            .get_mut("d")
            .and_then(Value::as_object_mut)
            .and_then(|data| data.get_mut("resume_gateway_url"))
        else {
            return (message, None);
        };
        if !resume_url.is_string() {
            return (message, None);
        }
        *resume_url = Value::String(config.gateway_proxy_url());
        let Ok(encoded) = serde_json::to_string(&payload) else {
            return (message, None);
        };
        state.record_ready_rewrite();
        return (Message::Text(encoded.into()), None);
    }
    if !is_dispatch(&payload, "INTERACTION_CREATE") {
        return (message, None);
    }
    let interaction_id = payload.pointer("/d/id").and_then(Value::as_str);
    let guild_id = payload.pointer("/d/guild_id").and_then(Value::as_str);
    let actor_id = payload
        .pointer("/d/member/user/id")
        .or_else(|| payload.pointer("/d/user/id"))
        .and_then(Value::as_str);
    let (Some(interaction_id), Some(guild_id), Some(actor_id)) =
        (interaction_id, guild_id, actor_id)
    else {
        state.record_gateway_identity_rejection();
        return (message, None);
    };
    let duplicate = state.claim_duplicate(interaction_id, guild_id, actor_id);
    (message, duplicate)
}

fn is_dispatch(payload: &Value, event_type: &str) -> bool {
    payload.get("op").and_then(Value::as_u64) == Some(0)
        && payload.get("t").and_then(Value::as_str) == Some(event_type)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use paused_discord_gateway::{
        CloseFrame as DiscordCloseFrame, ConfigBuilder as DiscordConfigBuilder,
        Event as DiscordEvent, EventTypeFlags as DiscordEventTypeFlags, Intents as DiscordIntents,
        Shard as DiscordShard, ShardId as DiscordShardId, StreamExt as _,
    };
    use tempfile::TempDir;
    use tokio::sync::{oneshot, watch};
    use tokio_tungstenite::tungstenite::{
        protocol::{frame::coding::CloseCode, CloseFrame},
        Message,
    };

    use super::*;

    async fn reserve_address() -> SocketAddrV4 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        match address {
            std::net::SocketAddr::V4(address) => address,
            _ => panic!("expected IPv4"),
        }
    }

    async fn wait_for_gateway_listener(state: &SharedState) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !state.gateway_listener_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn wait_for_gateway_completion(state: &SharedState, completed: u64) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = serde_json::to_value(state.snapshot()).unwrap();
                if snapshot["gateway"]["active_connections"] == 0
                    && snapshot["gateway"]["completed_connections"] == completed
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    fn close_message(reason: &'static str) -> Message {
        Message::Close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: reason.into(),
        }))
    }

    #[tokio::test]
    async fn fake_gateway_rewrites_ready_duplicates_exact_interaction_and_partitions() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let http_address = reserve_address().await;
        let upstream_listener = TcpListener::bind(upstream_address).await.unwrap();
        let upstream = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let ready =
                r#"{"op":0,"t":"READY","d":{"resume_gateway_url":"wss://resume.discord.gg"}}"#;
            let interaction = r#"{"op":0,"t":"INTERACTION_CREATE","d":{"id":"9","guild_id":"7","member":{"user":{"id":"8"}}}}"#;
            websocket.send(Message::Text(ready.into())).await.unwrap();
            websocket
                .send(Message::Text(interaction.into()))
                .await
                .unwrap();
            while websocket.next().await.is_some() {}
        });
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            http_address,
            format!("ws://{upstream_address}"),
            format!("http://{http_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        assert_eq!(
            state.arm_next_duplicate("d2:duplicate:1"),
            crate::state::ArmOutcome::Armed
        );
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server =
            tokio::spawn(async move { serve(server_config, server_state, shutdown_rx).await });
        wait_for_gateway_listener(&state).await;
        let (mut downstream, _) = tokio_tungstenite::connect_async(config.gateway_proxy_url())
            .await
            .unwrap();
        let ready = downstream
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap();
        let ready: Value = serde_json::from_str(&ready).unwrap();
        assert_eq!(ready["d"]["resume_gateway_url"], config.gateway_proxy_url());
        let first = downstream
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap();
        let second = downstream
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap();
        assert_eq!(first, second);
        assert!(state.partition());
        let closed = tokio::time::timeout(Duration::from_secs(1), downstream.next())
            .await
            .unwrap();
        assert!(
            closed.is_none()
                || closed.is_some_and(|message| message.is_ok_and(|value| value.is_close()))
        );
        wait_for_gateway_completion(&state, 1).await;
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["ready_rewrites"], 1);
        assert_eq!(snapshot["gateway"]["duplicate_injections"], 1);
        assert_eq!(snapshot["gateway"]["duplicate_delivery_count"], 2);
        shutdown_tx.send(true).unwrap();
        server.await.unwrap().unwrap();
        upstream.abort();
    }

    #[tokio::test]
    async fn downstream_close_is_acknowledged_before_proxy_returns() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let http_address = reserve_address().await;
        let upstream_listener = TcpListener::bind(upstream_address).await.unwrap();
        let (forwarded_tx, forwarded_rx) = oneshot::channel();
        let upstream = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(message) = websocket.next().await {
                let message = message.unwrap();
                if message.is_close() {
                    forwarded_tx.send(message).unwrap();
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    return;
                }
            }
        });
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            http_address,
            format!("ws://{upstream_address}"),
            format!("http://{http_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server =
            tokio::spawn(async move { serve(server_config, server_state, shutdown_rx).await });
        wait_for_gateway_listener(&state).await;
        let (mut downstream, _) = tokio_tungstenite::connect_async(config.gateway_proxy_url())
            .await
            .unwrap();
        let expected = close_message("runtime-stop");
        downstream.send(expected.clone()).await.unwrap();
        let close = tokio::time::timeout(Duration::from_secs(1), downstream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(close, expected);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), forwarded_rx)
                .await
                .unwrap()
                .unwrap(),
            expected
        );
        wait_for_gateway_completion(&state, 1).await;
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["clean_close_relays"], 1);
        assert_eq!(snapshot["gateway"]["relay_failures"], 0);
        shutdown_tx.send(true).unwrap();
        server.await.unwrap().unwrap();
        upstream.abort();
    }

    #[tokio::test]
    async fn upstream_close_is_acknowledged_and_relayed_downstream() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let http_address = reserve_address().await;
        let upstream_listener = TcpListener::bind(upstream_address).await.unwrap();
        let upstream = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let expected = close_message("discord-stop");
            websocket.send(expected.clone()).await.unwrap();
            let close = tokio::time::timeout(Duration::from_secs(1), websocket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(close, expected);
        });
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            http_address,
            format!("ws://{upstream_address}"),
            format!("http://{http_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server = tokio::spawn(async move {
            serve(server_config, server_state, shutdown_rx)
                .await
                .unwrap()
        });
        wait_for_gateway_listener(&state).await;
        let (mut downstream, _) = tokio_tungstenite::connect_async(config.gateway_proxy_url())
            .await
            .unwrap();
        let close = tokio::time::timeout(Duration::from_secs(1), downstream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(close, close_message("discord-stop"));
        upstream.await.unwrap();
        wait_for_gateway_completion(&state, 1).await;
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["clean_close_relays"], 1);
        assert_eq!(snapshot["gateway"]["relay_failures"], 0);
        shutdown_tx.send(true).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn upstream_eof_is_recorded_as_a_relay_failure() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let http_address = reserve_address().await;
        let upstream_listener = TcpListener::bind(upstream_address).await.unwrap();
        let upstream = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
            drop(websocket);
        });
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            http_address,
            format!("ws://{upstream_address}"),
            format!("http://{http_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        state.mark_effect_http_listener_ready();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server =
            tokio::spawn(async move { serve(server_config, server_state, shutdown_rx).await });
        wait_for_gateway_listener(&state).await;
        let (downstream, _) = tokio_tungstenite::connect_async(config.gateway_proxy_url())
            .await
            .unwrap();
        drop(downstream);
        upstream.await.unwrap();
        wait_for_gateway_completion(&state, 1).await;
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["ready"], false);
        assert_eq!(snapshot["gateway"]["relay_failures"], 1);
        assert_eq!(snapshot["gateway"]["clean_close_relays"], 0);
        drop(shutdown_tx);
        assert!(matches!(
            server.await.unwrap(),
            Err(GatewayError::Connection)
        ));
    }

    #[tokio::test]
    async fn twilight_shard_close_is_acknowledged_without_reconnect() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let http_address = reserve_address().await;
        let upstream_listener = TcpListener::bind(upstream_address).await.unwrap();
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let upstream = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
            accepted_tx.send(()).unwrap();
            while let Some(message) = websocket.next().await {
                if message.unwrap().is_close() {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    return;
                }
            }
        });
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            http_address,
            format!("ws://{upstream_address}"),
            format!("http://{http_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server = tokio::spawn(async move {
            serve(server_config, server_state, shutdown_rx)
                .await
                .unwrap()
        });
        wait_for_gateway_listener(&state).await;
        let discord_config =
            DiscordConfigBuilder::new("test-token".to_owned(), DiscordIntents::empty())
                .proxy_url(config.gateway_proxy_url())
                .build();
        let mut shard = DiscordShard::with_config(DiscordShardId::ONE, discord_config);
        let sender = shard.sender();
        let next_event =
            tokio::spawn(async move { shard.next_event(DiscordEventTypeFlags::empty()).await });
        tokio::time::timeout(Duration::from_secs(3), accepted_rx)
            .await
            .unwrap()
            .unwrap();
        sender.close(DiscordCloseFrame::NORMAL).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), next_event)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(event, DiscordEvent::GatewayClose(_)));
        wait_for_gateway_completion(&state, 1).await;
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["connections"], 1);
        assert_eq!(snapshot["gateway"]["clean_close_relays"], 1);
        assert_eq!(snapshot["gateway"]["relay_failures"], 0);
        shutdown_tx.send(true).unwrap();
        server.await.unwrap();
        upstream.abort();
    }

    #[tokio::test]
    async fn gateway_shutdown_drains_an_active_connection() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let http_address = reserve_address().await;
        let upstream_listener = TcpListener::bind(upstream_address).await.unwrap();
        let upstream = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let close = tokio::time::timeout(Duration::from_secs(1), websocket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(close.is_close());
            websocket.flush().await.unwrap();
        });
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            http_address,
            format!("ws://{upstream_address}"),
            format!("http://{http_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server =
            tokio::spawn(async move { serve(server_config, server_state, shutdown_rx).await });
        wait_for_gateway_listener(&state).await;
        let (mut downstream, _) = tokio_tungstenite::connect_async(config.gateway_proxy_url())
            .await
            .unwrap();
        shutdown_tx.send(true).unwrap();
        let close = tokio::time::timeout(Duration::from_secs(1), downstream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(close.is_close());
        server.await.unwrap().unwrap();
        upstream.await.unwrap();
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["active_connections"], 0);
        assert_eq!(snapshot["gateway"]["completed_connections"], 1);
        assert_eq!(snapshot["gateway"]["relay_failures"], 0);
        assert_eq!(snapshot["gateway"]["connection_aborts"], 0);
    }

    #[test]
    fn close_ack_budget_precedes_the_runtime_deadline() {
        let runtime_deadline = Duration::from_secs(2);
        assert!(GATEWAY_RELAY_WRITE_TIMEOUT + GATEWAY_CLOSE_ACK_TIMEOUT < runtime_deadline);
    }

    #[test]
    fn wrong_identity_never_duplicates() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            "127.0.0.1:21001".parse().unwrap(),
            "127.0.0.1:21002".parse().unwrap(),
            "ws://127.0.0.1:22001".to_owned(),
            "http://127.0.0.1:22002".to_owned(),
        );
        let state = SharedState::new(&config).unwrap();
        assert_eq!(
            state.arm_next_duplicate("d2:duplicate:1"),
            crate::state::ArmOutcome::Armed
        );
        let message = Message::Text(
            r#"{"op":0,"t":"INTERACTION_CREATE","d":{"id":"9","guild_id":"7","member":{"user":{"id":"10"}}}}"#
                .into(),
        );
        let (_, duplicate) = transform_upstream_message(message, &config, &state);
        assert!(duplicate.is_none());
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["identity_rejections"], 1);
        assert_eq!(snapshot["gateway"]["duplicate_armed"], true);
    }
}
