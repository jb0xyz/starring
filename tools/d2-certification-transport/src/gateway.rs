use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Semaphore};
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};

use crate::state::{DuplicateClaim, SharedState};
use crate::Config;

const MAX_GATEWAY_CONNECTIONS: usize = 4;
const MAX_GATEWAY_MESSAGE_BYTES: usize = 512 * 1024;
const MAX_GATEWAY_FRAME_BYTES: usize = 128 * 1024;
const GATEWAY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const GATEWAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const GATEWAY_RELAY_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const GATEWAY_RELAY_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("gateway_bind_failed")]
    Bind,
    #[error("gateway_accept_failed")]
    Accept,
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
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return Ok(());
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
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = proxy_connection(stream, config, state).await;
                });
            }
        }
    }
}

async fn proxy_connection(
    stream: TcpStream,
    config: Config,
    state: Arc<SharedState>,
) -> Result<(), ()> {
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
    state.record_gateway_connection();
    let mut partition_rx = state.subscribe_partition();
    let (mut downstream_write, mut downstream_read) = downstream.split();
    let (mut upstream_write, mut upstream_read) = upstream.split();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(GATEWAY_RELAY_IDLE_TIMEOUT) => {
                return Err(());
            }
            _ = partition_rx.recv() => {
                let _ = tokio::time::timeout(
                    GATEWAY_RELAY_WRITE_TIMEOUT,
                    downstream_write.send(Message::Close(None)),
                ).await;
                let _ = tokio::time::timeout(
                    GATEWAY_RELAY_WRITE_TIMEOUT,
                    upstream_write.send(Message::Close(None)),
                ).await;
                return Ok(());
            }
            message = downstream_read.next() => {
                let Some(message) = message else {
                    return Ok(());
                };
                let message = message.map_err(|_| ())?;
                let terminal = message.is_close();
                tokio::time::timeout(
                    GATEWAY_RELAY_WRITE_TIMEOUT,
                    upstream_write.send(message),
                )
                .await
                .map_err(|_| ())?
                .map_err(|_| ())?;
                if terminal {
                    return Ok(());
                }
            }
            message = upstream_read.next() => {
                let Some(message) = message else {
                    return Ok(());
                };
                let message = message.map_err(|_| ())?;
                let terminal = message.is_close();
                let (message, duplicate) = transform_upstream_message(message, &config, &state);
                if let Some(claim) = duplicate {
                    let first = tokio::time::timeout(
                        GATEWAY_RELAY_WRITE_TIMEOUT,
                        downstream_write.send(message.clone()),
                    )
                    .await;
                    if !matches!(first, Ok(Ok(()))) || !state.record_duplicate_delivery(&claim) {
                        let _ = state.abort_duplicate(claim);
                        return Err(());
                    }
                    let second = tokio::time::timeout(
                        GATEWAY_RELAY_WRITE_TIMEOUT,
                        downstream_write.send(message),
                    )
                    .await;
                    if !matches!(second, Ok(Ok(()))) || !state.record_duplicate_delivery(&claim) {
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
                    return Ok(());
                }
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

    use tempfile::TempDir;
    use tokio::sync::watch;
    use tokio_tungstenite::tungstenite::Message;

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
        let server = tokio::spawn(async move {
            serve(server_config, server_state, shutdown_rx)
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
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
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["ready_rewrites"], 1);
        assert_eq!(snapshot["gateway"]["duplicate_injections"], 1);
        assert_eq!(snapshot["gateway"]["duplicate_delivery_count"], 2);
        shutdown_tx.send(true).unwrap();
        server.await.unwrap();
        upstream.abort();
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
