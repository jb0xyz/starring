use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map, Value};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

use crate::state::{valid_operation_id, ArmOutcome, SharedState};
use crate::Config;

const MAX_CONTROL_REQUEST_BYTES: usize = 4096;
const MAX_CONTROL_CONNECTIONS: usize = 4;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("control_bind_failed")]
    Bind,
    #[error("control_accept_failed")]
    Accept,
}

struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

enum CommandOutcome {
    Continue(Value),
    Shutdown(Value),
}

pub async fn serve(
    config: Config,
    state: Arc<SharedState>,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), ControlError> {
    let path = config.control_socket();
    validate_socket_path(config.root(), &path).map_err(|_| ControlError::Bind)?;
    if fs::symlink_metadata(&path).is_ok() {
        return Err(ControlError::Bind);
    }
    unsafe {
        libc::umask(0o077);
    }
    let listener = UnixListener::bind(&path).map_err(|_| ControlError::Bind)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|_| ControlError::Bind)?;
    let _guard = SocketGuard(path);
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONTROL_CONNECTIONS));
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return Ok(());
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|_| ControlError::Accept)?;
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    continue;
                };
                let state = Arc::clone(&state);
                let shutdown_tx = shutdown_tx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = tokio::time::timeout(
                        CONTROL_TIMEOUT,
                        handle_connection(stream, state, shutdown_tx),
                    )
                    .await;
                });
            }
        }
    }
}

fn validate_socket_path(root: &Path, path: &Path) -> io::Result<()> {
    if path.parent() != Some(root) || !path.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path"));
    }
    Ok(())
}

async fn handle_connection(
    stream: UnixStream,
    state: Arc<SharedState>,
    shutdown_tx: watch::Sender<bool>,
) -> io::Result<()> {
    if peer_effective_uid(&stream)? != unsafe { libc::geteuid() } {
        return Ok(());
    }
    let mut reader = BufReader::new(stream);
    let mut request = Vec::new();
    while request.len() <= MAX_CONTROL_REQUEST_BYTES {
        let byte = match reader.read_u8().await {
            Ok(byte) => byte,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        };
        request.push(byte);
        if byte == b'\n' {
            break;
        }
    }
    if request.is_empty() || request.len() > MAX_CONTROL_REQUEST_BYTES || !request.ends_with(b"\n")
    {
        write_response(reader.get_mut(), error_response("request_invalid")).await?;
        return Ok(());
    }
    request.pop();
    if request.ends_with(b"\r") {
        request.pop();
    }
    let outcome = match serde_json::from_slice::<Value>(&request) {
        Ok(value) => execute(value, &state),
        Err(_) => Err("request_invalid"),
    };
    match outcome {
        Ok(CommandOutcome::Continue(response)) => {
            write_response(reader.get_mut(), response).await?;
        }
        Ok(CommandOutcome::Shutdown(response)) => {
            write_response(reader.get_mut(), response).await?;
            let _ = shutdown_tx.send(true);
        }
        Err(code) => {
            write_response(reader.get_mut(), error_response(code)).await?;
        }
    }
    Ok(())
}

async fn write_response(stream: &mut UnixStream, response: Value) -> io::Result<()> {
    let mut encoded = serde_json::to_vec(&response).map_err(io::Error::other)?;
    encoded.push(b'\n');
    stream.write_all(&encoded).await?;
    stream.shutdown().await
}

fn execute(value: Value, state: &SharedState) -> Result<CommandOutcome, &'static str> {
    let object = value.as_object().ok_or("request_invalid")?;
    let command = string_field(object, "command")?;
    let expected = match command {
        "arm_next_duplicate" | "arm_next_create_role_indeterminate" => &[
            "version",
            "command",
            "run_id",
            "guild_id",
            "actor_id",
            "bot_user_id",
            "operation_id",
        ][..],
        "disarm_duplicate"
        | "disarm_indeterminate"
        | "partition_gateway"
        | "heal_gateway"
        | "snapshot"
        | "shutdown" => &[
            "version",
            "command",
            "run_id",
            "guild_id",
            "actor_id",
            "bot_user_id",
        ][..],
        _ => return Err("command_unsupported"),
    };
    require_exact_fields(object, expected)?;
    if object.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("version_unsupported");
    }
    let run_id = string_field(object, "run_id")?;
    let guild_id = string_field(object, "guild_id")?;
    let actor_id = string_field(object, "actor_id")?;
    let bot_user_id = string_field(object, "bot_user_id")?;
    if !state.identities_match(run_id, guild_id, actor_id, bot_user_id) {
        return Err("identity_mismatch");
    }
    let response = match command {
        "arm_next_duplicate" => {
            let operation_id = string_field(object, "operation_id")?;
            if !valid_operation_id(operation_id) {
                return Err("target_invalid");
            }
            arm_response(state.arm_next_duplicate(operation_id))
        }
        "arm_next_create_role_indeterminate" => {
            let operation_id = string_field(object, "operation_id")?;
            if !valid_operation_id(operation_id) {
                return Err("target_invalid");
            }
            arm_response(state.arm_next_indeterminate(operation_id))
        }
        "disarm_duplicate" => changed_response(state.disarm_duplicate()),
        "disarm_indeterminate" => changed_response(state.disarm_indeterminate()),
        "partition_gateway" => changed_response(state.partition()),
        "heal_gateway" => changed_response(state.heal()),
        "snapshot" => json!({"ok": true, "snapshot": state.snapshot()}),
        "shutdown" => return Ok(CommandOutcome::Shutdown(json!({"ok": true}))),
        _ => return Err("command_unsupported"),
    };
    Ok(CommandOutcome::Continue(response))
}

fn require_exact_fields(
    object: &Map<String, Value>,
    expected: &[&str],
) -> Result<(), &'static str> {
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err("request_invalid")
    }
}

fn string_field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, &'static str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or("request_invalid")
}

fn changed_response(changed: bool) -> Value {
    json!({"ok": true, "changed": changed})
}

fn arm_response(outcome: ArmOutcome) -> Value {
    match outcome {
        ArmOutcome::Armed => json!({"ok": true, "changed": true, "disposition": "armed"}),
        ArmOutcome::Replayed => {
            json!({"ok": true, "changed": false, "disposition": "replayed"})
        }
        ArmOutcome::Busy => json!({"ok": true, "changed": false, "disposition": "busy"}),
    }
}

fn error_response(code: &'static str) -> Value {
    json!({"ok": false, "error": {"code": code}})
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
fn peer_effective_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    let mut effective_uid = 0;
    let mut effective_gid = 0;
    let result =
        unsafe { libc::getpeereid(stream.as_raw_fd(), &mut effective_uid, &mut effective_gid) };
    if result == 0 {
        Ok(effective_uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn peer_effective_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    let credentials = stream.peer_cred()?;
    Ok(credentials.uid())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::time::Duration;

    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;
    use tokio::sync::watch;

    use super::*;

    fn state() -> (TempDir, SharedState) {
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
        (root, SharedState::new(&config).unwrap())
    }

    fn command(command: &str) -> Map<String, Value> {
        json!({
            "version": 1,
            "command": command,
            "run_id": "d2-test-run",
            "guild_id": "7",
            "actor_id": "8",
            "bot_user_id": "6"
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn command_contract_is_exact_and_identity_bound() {
        let (_root, state) = state();
        let mut request = command("arm_next_duplicate");
        request.insert("operation_id".to_owned(), json!("d2:duplicate:1"));
        assert!(execute(Value::Object(request.clone()), &state).is_ok());
        request.insert("unexpected".to_owned(), json!(true));
        assert_eq!(
            execute(Value::Object(request), &state).err(),
            Some("request_invalid")
        );
        let mut mismatch = command("snapshot");
        mismatch.insert("actor_id".to_owned(), json!("9"));
        assert_eq!(
            execute(Value::Object(mismatch), &state).err(),
            Some("identity_mismatch")
        );
    }

    #[test]
    fn unsupported_and_noncanonical_arm_commands_fail_closed() {
        let (_root, state) = state();
        assert_eq!(
            execute(Value::Object(command("arm_duplicate")), &state).err(),
            Some("command_unsupported")
        );
        let mut request = command("arm_next_create_role_indeterminate");
        request.insert("operation_id".to_owned(), json!("d2:indeterminate:1"));
        request.insert("audit_reason".to_owned(), json!("forbidden"));
        assert_eq!(
            execute(Value::Object(request), &state).err(),
            Some("request_invalid")
        );
    }

    #[tokio::test]
    async fn unix_control_is_private_peer_bound_bounded_and_shutdown_capable() {
        let (root, state) = state();
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
        let socket = config.control_socket();
        let state = Arc::new(state);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server = tokio::spawn(serve(config, Arc::clone(&state), shutdown_tx, shutdown_rx));
        for _ in 0..20 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let metadata = fs::metadata(&socket).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let snapshot = json!({
            "version": 1,
            "command": "snapshot",
            "run_id": "d2-test-run",
            "guild_id": "7",
            "actor_id": "8",
            "bot_user_id": "6"
        });
        stream
            .write_all(format!("{snapshot}\n").as_bytes())
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let response: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["snapshot"]["ready"], false);
        assert_eq!(response["snapshot"]["version"], 2);
        assert_eq!(response["snapshot"]["hub_channel_id"], "5");
        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let shutdown = json!({
            "version": 1,
            "command": "shutdown",
            "run_id": "d2-test-run",
            "guild_id": "7",
            "actor_id": "8",
            "bot_user_id": "6"
        });
        stream
            .write_all(format!("{shutdown}\n").as_bytes())
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&response).unwrap()["ok"],
            true
        );
        server.await.unwrap().unwrap();
        assert!(!socket.exists());
    }
}
