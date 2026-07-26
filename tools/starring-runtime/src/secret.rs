use std::fmt::{Debug, Display, Formatter};
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::io::Read;
use std::net::IpAddr;
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::process::{Child, Command, Stdio};
#[cfg(all(test, unix))]
use std::sync::atomic::AtomicUsize;
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{channel, Sender},
    Arc,
};
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::thread;
use std::time::{Duration, Instant};

use zeroize::{Zeroize, Zeroizing};

use crate::startup::{
    run_runtime_startup_sync_stage_v1, RuntimeStartupBudgetV1, RuntimeStartupSyncStageErrorV1,
};
use crate::{
    DatabaseCapabilityV1, RuntimeConfigV1, RuntimeSecretReferenceV1, RuntimeSecretReferencesV1,
};

const MAX_SECRET_BYTES: usize = 32 * 1024;
const MAX_DATABASE_URL_BYTES: usize = 8 * 1024;
const MIN_DISCORD_TOKEN_BYTES: usize = 32;
const MAX_DISCORD_TOKEN_BYTES: usize = 512;
#[cfg(target_os = "macos")]
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(target_os = "macos")]
const KEYCHAIN_CLEANUP_WINDOW: Duration = Duration::from_secs(1);
#[cfg(any(target_os = "macos", all(test, unix)))]
const KEYCHAIN_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(any(target_os = "macos", all(test, unix)))]
const KEYCHAIN_CAPTURE_BYTES: usize = MAX_SECRET_BYTES + 2;
#[cfg(all(test, unix))]
static KEYCHAIN_REAPER_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnvironmentSecretReadErrorV1 {
    InvalidEncoding,
}

trait EnvironmentSecretReaderV1 {
    fn read_secret(
        &self,
        name: &str,
    ) -> Result<Option<Zeroizing<String>>, EnvironmentSecretReadErrorV1>;
}

#[derive(Clone, Copy, Debug, Default)]
struct ProcessEnvironmentSecretReaderV1;

impl EnvironmentSecretReaderV1 for ProcessEnvironmentSecretReaderV1 {
    fn read_secret(
        &self,
        name: &str,
    ) -> Result<Option<Zeroizing<String>>, EnvironmentSecretReadErrorV1> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(EnvironmentSecretReadErrorV1::InvalidEncoding)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeychainSecretReadErrorV1 {
    Unavailable,
    #[cfg(any(target_os = "macos", test))]
    Timeout,
    #[cfg(any(target_os = "macos", test))]
    CleanupTimedOut,
    #[cfg(any(target_os = "macos", test))]
    OutputTooLarge,
}

trait KeychainSecretReaderV1 {
    fn read_secret(
        &self,
        service: &str,
        account: &str,
        operation_cutoff: Instant,
        cleanup_deadline: Instant,
    ) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1>;
}

#[derive(Clone, Copy, Debug, Default)]
struct MacOsKeychainSecretReaderV1;

impl KeychainSecretReaderV1 for MacOsKeychainSecretReaderV1 {
    fn read_secret(
        &self,
        service: &str,
        account: &str,
        operation_cutoff: Instant,
        cleanup_deadline: Instant,
    ) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
        #[cfg(target_os = "macos")]
        {
            read_macos_keychain_secret(service, account, operation_cutoff, cleanup_deadline)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (service, account, operation_cutoff, cleanup_deadline);
            Err(KeychainSecretReadErrorV1::Unavailable)
        }
    }
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_secret(
    service: &str,
    account: &str,
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    let mut command = Command::new("/usr/bin/security");
    command
        .args(["find-generic-password", "-w", "-s", service, "-a", account])
        .env_clear();
    let local_deadline = Instant::now()
        .checked_add(KEYCHAIN_TIMEOUT)
        .ok_or(KeychainSecretReadErrorV1::Timeout)?;
    let command_deadline = local_deadline.min(operation_cutoff);
    let command_cleanup_deadline = command_deadline
        .checked_add(KEYCHAIN_CLEANUP_WINDOW)
        .unwrap_or(cleanup_deadline)
        .min(cleanup_deadline);
    run_keychain_command_until(command, command_deadline, command_cleanup_deadline)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn run_keychain_command_until(
    mut command: Command,
    operation_deadline: Instant,
    cleanup_deadline: Instant,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    let now = Instant::now();
    if now >= operation_deadline {
        return Err(KeychainSecretReadErrorV1::Timeout);
    }
    if now >= cleanup_deadline {
        return Err(KeychainSecretReadErrorV1::CleanupTimedOut);
    }
    configure_keychain_command(&mut command);
    let reaper = KeychainReaperDispatchV1::start()?;
    let now = Instant::now();
    if now >= operation_deadline {
        return Err(KeychainSecretReadErrorV1::Timeout);
    }
    if now >= cleanup_deadline {
        return Err(KeychainSecretReadErrorV1::CleanupTimedOut);
    }
    let child = command
        .spawn()
        .map_err(|_| KeychainSecretReadErrorV1::Unavailable)?;
    capture_keychain_child_until(child, operation_deadline, cleanup_deadline, reaper)
}

#[cfg(all(test, unix))]
fn run_keychain_command(
    command: Command,
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    let operation_deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(KeychainSecretReadErrorV1::Timeout)?;
    let cleanup_deadline = operation_deadline
        .checked_add(Duration::from_secs(1))
        .ok_or(KeychainSecretReadErrorV1::CleanupTimedOut)?;
    run_keychain_command_until(command, operation_deadline, cleanup_deadline)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn configure_keychain_command(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn capture_keychain_child_until(
    mut child: Child,
    operation_deadline: Instant,
    cleanup_deadline: Instant,
    reaper: KeychainReaperDispatchV1,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    let cancellation = Arc::new(AtomicBool::new(false));
    if Instant::now() >= operation_deadline {
        return cancel_keychain_child_until(
            child,
            None,
            None,
            cancellation,
            cleanup_deadline,
            reaper,
            KeychainSecretReadErrorV1::Timeout,
        );
    }
    let Some(stdout) = child.stdout.take() else {
        return cancel_keychain_child_until(
            child,
            None,
            None,
            cancellation,
            cleanup_deadline,
            reaper,
            KeychainSecretReadErrorV1::Unavailable,
        );
    };
    let Some(stderr) = child.stderr.take() else {
        drop(stdout);
        return cancel_keychain_child_until(
            child,
            None,
            None,
            cancellation,
            cleanup_deadline,
            reaper,
            KeychainSecretReadErrorV1::Unavailable,
        );
    };
    if Instant::now() >= operation_deadline {
        drop((stdout, stderr));
        return cancel_keychain_child_until(
            child,
            None,
            None,
            cancellation,
            cleanup_deadline,
            reaper,
            KeychainSecretReadErrorV1::Timeout,
        );
    }
    let stdout_cancellation = cancellation.clone();
    let stdout_capture = match thread::Builder::new()
        .name("starring-runtime-keychain-stdout".to_string())
        .spawn(move || capture_bounded_until_cancelled(stdout, stdout_cancellation))
    {
        Ok(capture) => capture,
        Err(_) => {
            return cancel_keychain_child_until(
                child,
                None,
                None,
                cancellation,
                cleanup_deadline,
                reaper,
                KeychainSecretReadErrorV1::Unavailable,
            );
        }
    };
    if Instant::now() >= operation_deadline {
        drop(stderr);
        return cancel_keychain_child_until(
            child,
            Some(stdout_capture),
            None,
            cancellation,
            cleanup_deadline,
            reaper,
            KeychainSecretReadErrorV1::Timeout,
        );
    }
    let stderr_cancellation = cancellation.clone();
    let stderr_capture = match thread::Builder::new()
        .name("starring-runtime-keychain-stderr".to_string())
        .spawn(move || capture_bounded_until_cancelled(stderr, stderr_cancellation))
    {
        Ok(capture) => capture,
        Err(_) => {
            return cancel_keychain_child_until(
                child,
                Some(stdout_capture),
                None,
                cancellation,
                cleanup_deadline,
                reaper,
                KeychainSecretReadErrorV1::Unavailable,
            );
        }
    };
    if Instant::now() >= operation_deadline {
        return cancel_keychain_child_until(
            child,
            Some(stdout_capture),
            Some(stderr_capture),
            cancellation,
            cleanup_deadline,
            reaper,
            KeychainSecretReadErrorV1::Timeout,
        );
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < operation_deadline => {
                sleep_until_keychain_deadline(operation_deadline);
            }
            Ok(None) => {
                return cancel_keychain_child_until(
                    child,
                    Some(stdout_capture),
                    Some(stderr_capture),
                    cancellation,
                    cleanup_deadline,
                    reaper,
                    KeychainSecretReadErrorV1::Timeout,
                );
            }
            Err(_) => {
                return cancel_keychain_child_until(
                    child,
                    Some(stdout_capture),
                    Some(stderr_capture),
                    cancellation,
                    cleanup_deadline,
                    reaper,
                    KeychainSecretReadErrorV1::Unavailable,
                );
            }
        }
    };
    while !stdout_capture.is_finished() || !stderr_capture.is_finished() {
        if Instant::now() >= operation_deadline {
            return cancel_keychain_child_until(
                child,
                Some(stdout_capture),
                Some(stderr_capture),
                cancellation,
                cleanup_deadline,
                reaper,
                KeychainSecretReadErrorV1::Timeout,
            );
        }
        sleep_until_keychain_deadline(operation_deadline);
    }
    let captures = join_keychain_captures(stdout_capture, stderr_capture);
    if Instant::now() >= operation_deadline {
        return Err(KeychainSecretReadErrorV1::Timeout);
    }
    let (stdout, stderr) = captures?;
    if stdout.overflowed || stderr.overflowed {
        return Err(KeychainSecretReadErrorV1::OutputTooLarge);
    }
    if !status.success() {
        return Err(KeychainSecretReadErrorV1::Unavailable);
    }
    let mut value = stdout.bytes;
    if value.last() == Some(&b'\n') {
        value.pop();
        if value.last() == Some(&b'\r') {
            value.pop();
        }
    }
    Ok(value)
}

#[cfg(all(test, unix))]
fn capture_keychain_child(
    child: Child,
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    let operation_deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(KeychainSecretReadErrorV1::Timeout)?;
    let cleanup_deadline = operation_deadline
        .checked_add(Duration::from_secs(1))
        .ok_or(KeychainSecretReadErrorV1::CleanupTimedOut)?;
    let reaper = KeychainReaperDispatchV1::start()?;
    capture_keychain_child_until(child, operation_deadline, cleanup_deadline, reaper)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn cancel_keychain_child_until(
    mut child: Child,
    stdout_capture: Option<KeychainCaptureHandleV1>,
    stderr_capture: Option<KeychainCaptureHandleV1>,
    cancellation: Arc<AtomicBool>,
    cleanup_deadline: Instant,
    reaper: KeychainReaperDispatchV1,
    primary: KeychainSecretReadErrorV1,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    cancellation.store(true, Ordering::Release);
    let _ = child.kill();
    loop {
        let child_finished = matches!(child.try_wait(), Ok(Some(_)));
        let captures_finished =
            capture_finished(&stdout_capture) && capture_finished(&stderr_capture);
        if Instant::now() >= cleanup_deadline {
            if child_finished && captures_finished {
                join_optional_keychain_captures(stdout_capture, stderr_capture);
            } else {
                reaper.dispatch(child, stdout_capture, stderr_capture);
            }
            return Err(KeychainSecretReadErrorV1::CleanupTimedOut);
        }
        if child_finished && captures_finished {
            join_optional_keychain_captures(stdout_capture, stderr_capture);
            return if Instant::now() < cleanup_deadline {
                Err(primary)
            } else {
                Err(KeychainSecretReadErrorV1::CleanupTimedOut)
            };
        }
        sleep_until_keychain_deadline(cleanup_deadline);
    }
}

#[cfg(any(target_os = "macos", all(test, unix)))]
type KeychainCaptureHandleV1 =
    thread::JoinHandle<Result<BoundedCaptureV1, KeychainSecretReadErrorV1>>;

#[cfg(any(target_os = "macos", all(test, unix)))]
fn capture_finished(capture: &Option<KeychainCaptureHandleV1>) -> bool {
    capture.as_ref().is_none_or(thread::JoinHandle::is_finished)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn join_optional_keychain_captures(
    stdout_capture: Option<KeychainCaptureHandleV1>,
    stderr_capture: Option<KeychainCaptureHandleV1>,
) {
    if let Some(capture) = stdout_capture {
        let _ = capture.join();
    }
    if let Some(capture) = stderr_capture {
        let _ = capture.join();
    }
}

#[cfg(any(target_os = "macos", all(test, unix)))]
struct KeychainReaperPayloadV1 {
    child: Child,
    stdout_capture: Option<KeychainCaptureHandleV1>,
    stderr_capture: Option<KeychainCaptureHandleV1>,
}

#[cfg(any(target_os = "macos", all(test, unix)))]
struct KeychainReaperDispatchV1 {
    sender: Sender<KeychainReaperPayloadV1>,
}

#[cfg(any(target_os = "macos", all(test, unix)))]
impl KeychainReaperDispatchV1 {
    fn start() -> Result<Self, KeychainSecretReadErrorV1> {
        let (sender, receiver) = channel::<KeychainReaperPayloadV1>();
        thread::Builder::new()
            .name("starring-runtime-keychain-reaper".to_string())
            .spawn(move || {
                let Ok(mut payload) = receiver.recv() else {
                    return;
                };
                let _ = payload.child.kill();
                let _ = payload.child.wait();
                join_optional_keychain_captures(payload.stdout_capture, payload.stderr_capture);
                #[cfg(all(test, unix))]
                KEYCHAIN_REAPER_COUNT.fetch_sub(1, Ordering::AcqRel);
            })
            .map_err(|_| KeychainSecretReadErrorV1::Unavailable)?;
        Ok(Self { sender })
    }

    fn dispatch(
        self,
        child: Child,
        stdout_capture: Option<KeychainCaptureHandleV1>,
        stderr_capture: Option<KeychainCaptureHandleV1>,
    ) {
        let payload = KeychainReaperPayloadV1 {
            child,
            stdout_capture,
            stderr_capture,
        };
        #[cfg(all(test, unix))]
        KEYCHAIN_REAPER_COUNT.fetch_add(1, Ordering::AcqRel);
        if self.sender.send(payload).is_err() {
            std::process::abort();
        }
    }
}

#[cfg(all(test, unix))]
fn keychain_reaper_is_idle() -> bool {
    KEYCHAIN_REAPER_COUNT.load(Ordering::Acquire) == 0
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn sleep_until_keychain_deadline(deadline: Instant) {
    thread::sleep(KEYCHAIN_POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn join_keychain_captures(
    stdout_capture: thread::JoinHandle<Result<BoundedCaptureV1, KeychainSecretReadErrorV1>>,
    stderr_capture: thread::JoinHandle<Result<BoundedCaptureV1, KeychainSecretReadErrorV1>>,
) -> Result<(BoundedCaptureV1, BoundedCaptureV1), KeychainSecretReadErrorV1> {
    let stdout = stdout_capture.join();
    let stderr = stderr_capture.join();
    let stdout = stdout.map_err(|_| KeychainSecretReadErrorV1::Unavailable)??;
    let stderr = stderr.map_err(|_| KeychainSecretReadErrorV1::Unavailable)??;
    Ok((stdout, stderr))
}

#[cfg(any(target_os = "macos", all(test, unix)))]
struct BoundedCaptureV1 {
    bytes: Zeroizing<Vec<u8>>,
    overflowed: bool,
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn capture_bounded_until_cancelled(
    mut reader: impl Read,
    cancellation: Arc<AtomicBool>,
) -> Result<BoundedCaptureV1, KeychainSecretReadErrorV1> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(KEYCHAIN_CAPTURE_BYTES));
    let mut overflowed = false;
    let mut buffer = Zeroizing::new([0_u8; 1024]);
    loop {
        if cancellation.load(Ordering::Acquire) {
            bytes.zeroize();
            return Err(KeychainSecretReadErrorV1::Timeout);
        }
        let read = reader
            .read(&mut *buffer)
            .map_err(|_| KeychainSecretReadErrorV1::Unavailable)?;
        if cancellation.load(Ordering::Acquire) {
            buffer[..read].zeroize();
            bytes.zeroize();
            return Err(KeychainSecretReadErrorV1::Timeout);
        }
        if read == 0 {
            break;
        }
        let remaining = KEYCHAIN_CAPTURE_BYTES.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&buffer[..retained]);
        overflowed |= retained != read;
        buffer[..read].zeroize();
    }
    Ok(BoundedCaptureV1 { bytes, overflowed })
}

#[cfg(all(test, unix))]
fn capture_bounded(reader: impl Read) -> Result<BoundedCaptureV1, KeychainSecretReadErrorV1> {
    capture_bounded_until_cancelled(reader, Arc::new(AtomicBool::new(false)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSecretResolutionErrorV1 {
    Missing,
    InvalidEncoding,
    InvalidReference,
    InvalidSecret,
    TooLarge,
    KeychainUnavailable,
    KeychainTimeout,
    KeychainCleanupTimedOut,
}

impl RuntimeSecretResolutionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Missing => "runtime_secret_missing",
            Self::InvalidEncoding => "runtime_secret_invalid_encoding",
            Self::InvalidReference => "runtime_secret_invalid_reference",
            Self::InvalidSecret => "runtime_secret_invalid",
            Self::TooLarge => "runtime_secret_too_large",
            Self::KeychainUnavailable => "runtime_secret_keychain_unavailable",
            Self::KeychainTimeout => "runtime_secret_keychain_timeout",
            Self::KeychainCleanupTimedOut => "runtime_secret_keychain_cleanup_timed_out",
        }
    }
}

impl Display for RuntimeSecretResolutionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "referenced runtime secret is missing",
            Self::InvalidEncoding => "referenced runtime secret encoding is invalid",
            Self::InvalidReference => "runtime secret reference is invalid",
            Self::InvalidSecret => "referenced runtime secret is invalid",
            Self::TooLarge => "referenced runtime secret exceeds the supported size",
            Self::KeychainUnavailable => "runtime Keychain secret lookup is unavailable",
            Self::KeychainTimeout => "runtime Keychain secret lookup timed out",
            Self::KeychainCleanupTimedOut => "runtime Keychain secret cleanup timed out",
        })
    }
}

impl std::error::Error for RuntimeSecretResolutionErrorV1 {}

struct ResolvedSecretV1(Zeroizing<String>);

impl ResolvedSecretV1 {
    fn from_zeroizing(value: Zeroizing<String>) -> Result<Self, RuntimeSecretResolutionErrorV1> {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(RuntimeSecretResolutionErrorV1::InvalidSecret);
        }
        if value.len() > MAX_SECRET_BYTES {
            return Err(RuntimeSecretResolutionErrorV1::TooLarge);
        }
        Ok(Self(value))
    }

    fn take_secret(self) -> Zeroizing<String> {
        self.0
    }
}

impl Debug for ResolvedSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResolvedSecretV1(<redacted>)")
    }
}

pub struct RuntimeDatabaseUrlSecretV1(RuntimeDatabaseConnectionSecretV1);

pub struct RuntimeDatabaseConnectionSecretV1 {
    username: String,
    password: RuntimeDatabasePasswordV1,
    database: String,
    endpoint: RuntimeDatabaseEndpointV1,
    port: u16,
    ssl_mode: RuntimeDatabaseSslModeV1,
    ssl_root_cert: Option<String>,
}

pub struct RuntimeDatabasePasswordV1(Zeroizing<String>);

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeDatabaseEndpointV1 {
    Network(String),
    Socket(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDatabaseSslModeV1 {
    Disable,
    VerifyFull,
}

impl RuntimeDatabaseConnectionSecretV1 {
    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &RuntimeDatabasePasswordV1 {
        &self.password
    }

    pub fn database(&self) -> &str {
        &self.database
    }

    pub fn endpoint(&self) -> &RuntimeDatabaseEndpointV1 {
        &self.endpoint
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn ssl_mode(&self) -> RuntimeDatabaseSslModeV1 {
        self.ssl_mode
    }

    pub fn ssl_root_cert(&self) -> Option<&str> {
        self.ssl_root_cert.as_deref()
    }
}

impl Debug for RuntimeDatabaseConnectionSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabaseConnectionSecretV1(<redacted>)")
    }
}

impl RuntimeDatabasePasswordV1 {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl Debug for RuntimeDatabasePasswordV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabasePasswordV1(<redacted>)")
    }
}

impl RuntimeDatabaseUrlSecretV1 {
    fn parse(secret: ResolvedSecretV1) -> Result<Self, RuntimeSecretResolutionErrorV1> {
        let value = secret.take_secret();
        if value.len() > MAX_DATABASE_URL_BYTES {
            return Err(RuntimeSecretResolutionErrorV1::InvalidSecret);
        }
        parse_database_connection_secret(&value)
            .map(Self)
            .ok_or(RuntimeSecretResolutionErrorV1::InvalidSecret)
    }

    pub fn connection_secret(&self) -> &RuntimeDatabaseConnectionSecretV1 {
        &self.0
    }
}

impl Debug for RuntimeDatabaseUrlSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabaseUrlSecretV1(<redacted>)")
    }
}

impl RuntimeDatabaseUrlSecretV1 {
    fn has_same_identity(&self, other: &Self) -> bool {
        self.0.username == other.0.username
            && self.0.database == other.0.database
            && self.0.endpoint == other.0.endpoint
            && self.0.port == other.0.port
    }
}

pub struct RuntimeDiscordBotTokenV1(Zeroizing<String>);

impl RuntimeDiscordBotTokenV1 {
    fn parse(secret: ResolvedSecretV1) -> Result<Self, RuntimeSecretResolutionErrorV1> {
        let value = secret.take_secret();
        if !(MIN_DISCORD_TOKEN_BYTES..=MAX_DISCORD_TOKEN_BYTES).contains(&value.len())
            || !value.is_ascii()
            || value.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            return Err(RuntimeSecretResolutionErrorV1::InvalidSecret);
        }
        Ok(Self(value))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl Debug for RuntimeDiscordBotTokenV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordBotTokenV1(<redacted>)")
    }
}

struct SecretResolverV1<E, K> {
    environment: E,
    keychain: K,
}

impl<E, K> SecretResolverV1<E, K> {
    fn new(environment: E, keychain: K) -> Self {
        Self {
            environment,
            keychain,
        }
    }
}

impl Default for SecretResolverV1<ProcessEnvironmentSecretReaderV1, MacOsKeychainSecretReaderV1> {
    fn default() -> Self {
        Self::new(
            ProcessEnvironmentSecretReaderV1,
            MacOsKeychainSecretReaderV1,
        )
    }
}

impl<E: EnvironmentSecretReaderV1, K: KeychainSecretReaderV1> SecretResolverV1<E, K> {
    fn resolve(
        &self,
        reference: &RuntimeSecretReferenceV1,
        operation_cutoff: Instant,
        cleanup_deadline: Instant,
    ) -> Result<ResolvedSecretV1, RuntimeSecretResolutionErrorV1> {
        let value = if let Some(name) = reference.environment_name() {
            self.environment
                .read_secret(name)
                .map_err(|_| RuntimeSecretResolutionErrorV1::InvalidEncoding)?
                .ok_or(RuntimeSecretResolutionErrorV1::Missing)?
        } else if let Some((service, account)) = reference.keychain_identity() {
            let bytes = self
                .keychain
                .read_secret(service, account, operation_cutoff, cleanup_deadline)
                .map_err(map_keychain_error)?;
            zeroizing_utf8(bytes)?
        } else {
            return Err(RuntimeSecretResolutionErrorV1::InvalidReference);
        };
        ResolvedSecretV1::from_zeroizing(value)
    }

    fn resolve_database_url(
        &self,
        reference: &RuntimeSecretReferenceV1,
        operation_cutoff: Instant,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeDatabaseUrlSecretV1, RuntimeSecretResolutionErrorV1> {
        RuntimeDatabaseUrlSecretV1::parse(self.resolve(
            reference,
            operation_cutoff,
            cleanup_deadline,
        )?)
    }

    fn resolve_discord_bot_token(
        &self,
        reference: &RuntimeSecretReferenceV1,
        operation_cutoff: Instant,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeDiscordBotTokenV1, RuntimeSecretResolutionErrorV1> {
        RuntimeDiscordBotTokenV1::parse(self.resolve(
            reference,
            operation_cutoff,
            cleanup_deadline,
        )?)
    }
}

impl<E, K> Debug for SecretResolverV1<E, K> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretResolverV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSecretsResolutionErrorV1 {
    Database {
        capability: DatabaseCapabilityV1,
        source: RuntimeSecretResolutionErrorV1,
    },
    DiscordBotToken {
        source: RuntimeSecretResolutionErrorV1,
    },
    DuplicateDatabaseIdentity {
        capability: DatabaseCapabilityV1,
    },
}

impl RuntimeSecretsResolutionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Database { source, .. } | Self::DiscordBotToken { source } => source.code(),
            Self::DuplicateDatabaseIdentity { .. } => "runtime_secret_database_identity_duplicate",
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::Database { capability, .. } => Some(capability.code()),
            Self::DiscordBotToken { .. } => Some("discord_bot_token"),
            Self::DuplicateDatabaseIdentity { capability } => Some(capability.code()),
        }
    }
}

impl Display for RuntimeSecretsResolutionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Database { .. } => "runtime database secret resolution failed",
            Self::DiscordBotToken { .. } => "Discord bot token resolution failed",
            Self::DuplicateDatabaseIdentity { .. } => {
                "runtime database secret identities must be unique"
            }
        })
    }
}

impl std::error::Error for RuntimeSecretsResolutionErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database { source, .. } | Self::DiscordBotToken { source } => Some(source),
            Self::DuplicateDatabaseIdentity { .. } => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeSecretsStartupResolutionErrorV1 {
    OperationDeadlineElapsed,
    Resolution(RuntimeSecretsResolutionErrorV1),
}

impl Debug for RuntimeSecretsStartupResolutionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSecretsStartupResolutionErrorV1(<redacted>)")
    }
}

pub struct RuntimeDatabaseSecretsByCapabilityV1 {
    convergence: RuntimeDatabaseUrlSecretV1,
    exact_target: RuntimeDatabaseUrlSecretV1,
    panel: RuntimeDatabaseUrlSecretV1,
    serving: RuntimeDatabaseUrlSecretV1,
    interaction: RuntimeDatabaseUrlSecretV1,
}

impl RuntimeDatabaseSecretsByCapabilityV1 {
    pub fn database_url(&self, capability: DatabaseCapabilityV1) -> &RuntimeDatabaseUrlSecretV1 {
        match capability {
            DatabaseCapabilityV1::Convergence => &self.convergence,
            DatabaseCapabilityV1::ExactTarget => &self.exact_target,
            DatabaseCapabilityV1::Panel => &self.panel,
            DatabaseCapabilityV1::Serving => &self.serving,
            DatabaseCapabilityV1::Interaction => &self.interaction,
        }
    }

    fn duplicate_capability(&self) -> Option<DatabaseCapabilityV1> {
        let entries =
            DatabaseCapabilityV1::ALL.map(|capability| (capability, self.database_url(capability)));
        entries
            .iter()
            .enumerate()
            .find_map(|(index, (_, candidate))| {
                entries
                    .iter()
                    .skip(index + 1)
                    .find(|(_, other)| candidate.has_same_identity(other))
                    .map(|(capability, _)| *capability)
            })
    }
}

impl Debug for RuntimeDatabaseSecretsByCapabilityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDatabaseSecretsByCapabilityV1(<redacted>)")
    }
}

pub struct ResolvedRuntimeSecretsV1 {
    database_urls: RuntimeDatabaseSecretsByCapabilityV1,
    discord_bot_token: RuntimeDiscordBotTokenV1,
}

pub(crate) fn resolve_runtime_secrets_until_v1(
    config: &RuntimeConfigV1,
    startup_budget: &RuntimeStartupBudgetV1,
) -> Result<ResolvedRuntimeSecretsV1, RuntimeSecretsStartupResolutionErrorV1> {
    ResolvedRuntimeSecretsV1::resolve_until(
        config.secret_references(),
        &SecretResolverV1::default(),
        startup_budget.operation_cutoff(),
        startup_budget.cleanup_deadline(),
    )
}

impl ResolvedRuntimeSecretsV1 {
    #[cfg(test)]
    fn resolve<E: EnvironmentSecretReaderV1, K: KeychainSecretReaderV1>(
        references: &RuntimeSecretReferencesV1,
        resolver: &SecretResolverV1<E, K>,
    ) -> Result<Self, RuntimeSecretsResolutionErrorV1> {
        let operation_cutoff = Instant::now().checked_add(Duration::from_secs(60)).unwrap();
        let cleanup_deadline = operation_cutoff
            .checked_add(Duration::from_secs(1))
            .unwrap();
        match Self::resolve_until(references, resolver, operation_cutoff, cleanup_deadline) {
            Ok(resolved) => Ok(resolved),
            Err(RuntimeSecretsStartupResolutionErrorV1::Resolution(error)) => Err(error),
            Err(RuntimeSecretsStartupResolutionErrorV1::OperationDeadlineElapsed) => {
                panic!("test secret resolution deadline elapsed")
            }
        }
    }

    fn resolve_until<E: EnvironmentSecretReaderV1, K: KeychainSecretReaderV1>(
        references: &RuntimeSecretReferencesV1,
        resolver: &SecretResolverV1<E, K>,
        operation_cutoff: Instant,
        cleanup_deadline: Instant,
    ) -> Result<Self, RuntimeSecretsStartupResolutionErrorV1> {
        let resolve_database_url = |capability| {
            map_startup_secret_stage_v1(
                run_runtime_startup_sync_stage_v1(
                    || Instant::now() < operation_cutoff,
                    || {
                        resolver.resolve_database_url(
                            references.database_url(capability),
                            operation_cutoff,
                            cleanup_deadline,
                        )
                    },
                ),
                |source| RuntimeSecretsResolutionErrorV1::Database { capability, source },
            )
        };
        let database_urls = RuntimeDatabaseSecretsByCapabilityV1 {
            convergence: resolve_database_url(DatabaseCapabilityV1::Convergence)?,
            exact_target: resolve_database_url(DatabaseCapabilityV1::ExactTarget)?,
            panel: resolve_database_url(DatabaseCapabilityV1::Panel)?,
            serving: resolve_database_url(DatabaseCapabilityV1::Serving)?,
            interaction: resolve_database_url(DatabaseCapabilityV1::Interaction)?,
        };
        if let Some(capability) = database_urls.duplicate_capability() {
            return Err(RuntimeSecretsStartupResolutionErrorV1::Resolution(
                RuntimeSecretsResolutionErrorV1::DuplicateDatabaseIdentity { capability },
            ));
        }
        let discord_bot_token = map_startup_secret_stage_v1(
            run_runtime_startup_sync_stage_v1(
                || Instant::now() < operation_cutoff,
                || {
                    resolver.resolve_discord_bot_token(
                        references.discord_bot_token(),
                        operation_cutoff,
                        cleanup_deadline,
                    )
                },
            ),
            |source| RuntimeSecretsResolutionErrorV1::DiscordBotToken { source },
        )?;
        Ok(Self {
            database_urls,
            discord_bot_token,
        })
    }

    pub fn database_secrets(&self) -> &RuntimeDatabaseSecretsByCapabilityV1 {
        &self.database_urls
    }

    pub fn discord_bot_token(&self) -> &RuntimeDiscordBotTokenV1 {
        &self.discord_bot_token
    }
}

fn map_startup_secret_stage_v1<T>(
    result: Result<T, RuntimeStartupSyncStageErrorV1<RuntimeSecretResolutionErrorV1>>,
    map_error: impl FnOnce(RuntimeSecretResolutionErrorV1) -> RuntimeSecretsResolutionErrorV1,
) -> Result<T, RuntimeSecretsStartupResolutionErrorV1> {
    match result {
        Ok(value) => Ok(value),
        Err(RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed) => {
            Err(RuntimeSecretsStartupResolutionErrorV1::OperationDeadlineElapsed)
        }
        Err(RuntimeStartupSyncStageErrorV1::Stage(error)) => Err(
            RuntimeSecretsStartupResolutionErrorV1::Resolution(map_error(error)),
        ),
    }
}

impl Debug for ResolvedRuntimeSecretsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResolvedRuntimeSecretsV1(<redacted>)")
    }
}

fn parse_database_connection_secret(value: &str) -> Option<RuntimeDatabaseConnectionSecretV1> {
    if !value.is_ascii() || value.contains(['%', '#']) {
        return None;
    }
    let payload = value
        .strip_prefix("postgresql://")
        .or_else(|| value.strip_prefix("postgres://"))?;
    let (authority, path_and_query) = payload.split_once('/')?;
    let (database, query) = path_and_query.split_once('?')?;
    if query.contains('?') || !valid_database_identifier(database) {
        return None;
    }
    let (user_info, host_and_port) = authority.split_once('@')?;
    if user_info.contains('@') || host_and_port.contains('@') {
        return None;
    }
    let (username, password) = user_info.split_once(':')?;
    if !valid_database_identifier(username) || !valid_database_password(password) {
        return None;
    }
    let (authority_host, authority_port) = parse_database_authority(host_and_port)?;
    let query = parse_database_query(query)?;
    if authority_port.is_some() && query.port.is_some() {
        return None;
    }
    let port = authority_port.or(query.port)?;
    let endpoint = if let Some(socket) = query.socket {
        if !authority_host.is_localhost_name()
            || query.ssl_mode != RuntimeDatabaseSslModeV1::Disable
            || query.ssl_root_cert.is_some()
        {
            return None;
        }
        ParsedDatabaseEndpointV1::Socket(socket)
    } else {
        if (!authority_host.is_loopback() && query.ssl_mode != RuntimeDatabaseSslModeV1::VerifyFull)
            || (authority_host.is_loopback()
                && !matches!(
                    query.ssl_mode,
                    RuntimeDatabaseSslModeV1::Disable | RuntimeDatabaseSslModeV1::VerifyFull
                ))
            || (query.ssl_mode == RuntimeDatabaseSslModeV1::Disable
                && query.ssl_root_cert.is_some())
        {
            return None;
        }
        ParsedDatabaseEndpointV1::Network(authority_host)
    };
    Some(
        ParsedDatabaseConnectionV1 {
            username,
            password,
            database,
            endpoint,
            port,
            ssl_mode: query.ssl_mode,
            ssl_root_cert: query.ssl_root_cert,
        }
        .into_secret(),
    )
}

struct ParsedDatabaseConnectionV1<'a> {
    username: &'a str,
    password: &'a str,
    database: &'a str,
    endpoint: ParsedDatabaseEndpointV1<'a>,
    port: u16,
    ssl_mode: RuntimeDatabaseSslModeV1,
    ssl_root_cert: Option<&'a str>,
}

impl ParsedDatabaseConnectionV1<'_> {
    fn into_secret(self) -> RuntimeDatabaseConnectionSecretV1 {
        let password = RuntimeDatabasePasswordV1(Zeroizing::new(self.password.to_string()));
        let endpoint = match self.endpoint {
            ParsedDatabaseEndpointV1::Network(host) => {
                RuntimeDatabaseEndpointV1::Network(host.into_canonical_string())
            }
            ParsedDatabaseEndpointV1::Socket(path) => {
                RuntimeDatabaseEndpointV1::Socket(path.to_string())
            }
        };
        RuntimeDatabaseConnectionSecretV1 {
            username: self.username.to_string(),
            password,
            database: self.database.to_string(),
            endpoint,
            port: self.port,
            ssl_mode: self.ssl_mode,
            ssl_root_cert: self.ssl_root_cert.map(str::to_string),
        }
    }
}

enum ParsedDatabaseEndpointV1<'a> {
    Network(ParsedDatabaseHostV1<'a>),
    Socket(&'a str),
}

#[derive(Clone, Copy)]
enum ParsedDatabaseHostV1<'a> {
    Name(&'a str),
    Address(IpAddr),
}

impl ParsedDatabaseHostV1<'_> {
    fn is_localhost_name(self) -> bool {
        matches!(self, Self::Name(name) if name.eq_ignore_ascii_case("localhost"))
    }

    fn is_loopback(self) -> bool {
        self.is_localhost_name() || matches!(self, Self::Address(address) if address.is_loopback())
    }

    fn into_canonical_string(self) -> String {
        match self {
            Self::Name(name) => name.to_ascii_lowercase(),
            Self::Address(address) => address.to_string(),
        }
    }
}

struct ParsedDatabaseQueryV1<'a> {
    ssl_mode: RuntimeDatabaseSslModeV1,
    socket: Option<&'a str>,
    port: Option<u16>,
    ssl_root_cert: Option<&'a str>,
}

fn parse_database_authority(value: &str) -> Option<(ParsedDatabaseHostV1<'_>, Option<u16>)> {
    if let Some(ipv6) = value.strip_prefix('[') {
        let (address, remainder) = ipv6.split_once(']')?;
        let address = address.parse::<std::net::Ipv6Addr>().ok()?;
        let port = if remainder.is_empty() {
            None
        } else {
            Some(parse_database_port(remainder.strip_prefix(':')?)?)
        };
        return Some((ParsedDatabaseHostV1::Address(IpAddr::V6(address)), port));
    }
    let (host, port) = match value.rsplit_once(':') {
        Some((host, port)) => (host, Some(parse_database_port(port)?)),
        None => (value, None),
    };
    if host.contains(':') {
        return None;
    }
    Some((parse_database_host(host)?, port))
}

fn parse_database_host(value: &str) -> Option<ParsedDatabaseHostV1<'_>> {
    if let Ok(address) = value.parse::<IpAddr>() {
        return Some(ParsedDatabaseHostV1::Address(address));
    }
    if !valid_database_host_name(value) {
        return None;
    }
    Some(ParsedDatabaseHostV1::Name(value))
}

fn parse_database_query(value: &str) -> Option<ParsedDatabaseQueryV1<'_>> {
    let mut ssl_mode = None;
    let mut socket = None;
    let mut port = None;
    let mut ssl_root_cert = None;
    for pair in value.split('&') {
        let (key, value) = pair.split_once('=')?;
        if value.contains('=') {
            return None;
        }
        match key {
            "sslmode" if ssl_mode.is_none() => {
                ssl_mode = Some(RuntimeDatabaseSslModeV1::parse(value)?);
            }
            "host" if socket.is_none() && valid_absolute_database_path(value) => {
                socket = Some(value);
            }
            "port" if port.is_none() => {
                port = Some(parse_database_port(value)?);
            }
            "sslrootcert" if ssl_root_cert.is_none() && valid_absolute_database_path(value) => {
                ssl_root_cert = Some(value);
            }
            _ => return None,
        }
    }
    Some(ParsedDatabaseQueryV1 {
        ssl_mode: ssl_mode?,
        socket,
        port,
        ssl_root_cert,
    })
}

fn parse_database_port(value: &str) -> Option<u16> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse::<u16>().ok().filter(|port| *port != 0)
}

impl RuntimeDatabaseSslModeV1 {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "disable" => Some(Self::Disable),
            "verify-full" => Some(Self::VerifyFull),
            _ => None,
        }
    }
}

fn valid_database_host_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_database_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_database_password(value: &str) -> bool {
    (24..=512).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'~'))
}

fn valid_absolute_database_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 1_024
        && value.is_ascii()
        && value.split('/').skip(1).all(|segment| {
            !segment.is_empty()
                && !matches!(segment, "." | "..")
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
}

fn zeroizing_utf8(
    mut bytes: Zeroizing<Vec<u8>>,
) -> Result<Zeroizing<String>, RuntimeSecretResolutionErrorV1> {
    let moved = std::mem::take(&mut *bytes);
    match String::from_utf8(moved) {
        Ok(value) => Ok(Zeroizing::new(value)),
        Err(error) => {
            let mut invalid = error.into_bytes();
            invalid.zeroize();
            Err(RuntimeSecretResolutionErrorV1::InvalidEncoding)
        }
    }
}

fn map_keychain_error(error: KeychainSecretReadErrorV1) -> RuntimeSecretResolutionErrorV1 {
    match error {
        KeychainSecretReadErrorV1::Unavailable => {
            RuntimeSecretResolutionErrorV1::KeychainUnavailable
        }
        #[cfg(any(target_os = "macos", test))]
        KeychainSecretReadErrorV1::Timeout => RuntimeSecretResolutionErrorV1::KeychainTimeout,
        #[cfg(any(target_os = "macos", test))]
        KeychainSecretReadErrorV1::CleanupTimedOut => {
            RuntimeSecretResolutionErrorV1::KeychainCleanupTimedOut
        }
        #[cfg(any(target_os = "macos", test))]
        KeychainSecretReadErrorV1::OutputTooLarge => RuntimeSecretResolutionErrorV1::TooLarge,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct FakeEnvironmentV1 {
        values: BTreeMap<String, String>,
        invalid: bool,
    }

    impl EnvironmentSecretReaderV1 for FakeEnvironmentV1 {
        fn read_secret(
            &self,
            name: &str,
        ) -> Result<Option<Zeroizing<String>>, EnvironmentSecretReadErrorV1> {
            if self.invalid {
                return Err(EnvironmentSecretReadErrorV1::InvalidEncoding);
            }
            Ok(self.values.get(name).cloned().map(Zeroizing::new))
        }
    }

    #[derive(Default)]
    struct FakeKeychainV1 {
        values: BTreeMap<(String, String), Vec<u8>>,
        error: Option<KeychainSecretReadErrorV1>,
    }

    struct CountingEnvironmentV1 {
        calls: Rc<Cell<usize>>,
    }

    impl EnvironmentSecretReaderV1 for CountingEnvironmentV1 {
        fn read_secret(
            &self,
            _name: &str,
        ) -> Result<Option<Zeroizing<String>>, EnvironmentSecretReadErrorV1> {
            self.calls.set(self.calls.get() + 1);
            Ok(None)
        }
    }

    impl KeychainSecretReaderV1 for FakeKeychainV1 {
        fn read_secret(
            &self,
            service: &str,
            account: &str,
            operation_cutoff: Instant,
            cleanup_deadline: Instant,
        ) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
            assert!(Instant::now() < operation_cutoff);
            assert!(operation_cutoff < cleanup_deadline);
            if let Some(error) = self.error {
                return Err(error);
            }
            self.values
                .get(&(service.to_string(), account.to_string()))
                .cloned()
                .map(Zeroizing::new)
                .ok_or(KeychainSecretReadErrorV1::Unavailable)
        }
    }

    fn resolver(
        environment: FakeEnvironmentV1,
        keychain: FakeKeychainV1,
    ) -> SecretResolverV1<FakeEnvironmentV1, FakeKeychainV1> {
        SecretResolverV1::new(environment, keychain)
    }

    #[cfg(unix)]
    fn process_command(program: &str) -> Command {
        let mut command = Command::new(program);
        command.env_clear();
        command
    }

    #[cfg(unix)]
    fn process_is_alive(process_id: u32) -> bool {
        process_command("/bin/kill")
            .args(["-0", &process_id.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    #[test]
    fn keychain_process_runner_returns_secret_and_trims_one_security_newline() {
        let mut command = process_command("/usr/bin/printf");
        command.arg("secret-value\r\n");
        let value = run_keychain_command(command, Duration::from_secs(1)).unwrap();
        assert_eq!(value.as_slice(), b"secret-value");
    }

    #[cfg(unix)]
    #[test]
    fn keychain_process_runner_closes_nonzero_exit() {
        let command = process_command("/usr/bin/false");
        assert_eq!(
            run_keychain_command(command, Duration::from_secs(1)).unwrap_err(),
            KeychainSecretReadErrorV1::Unavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn expired_absolute_keychain_deadline_skips_process_spawn() {
        let command = process_command("/__starring_runtime_missing_keychain_program");
        let operation_deadline = Instant::now();
        let cleanup_deadline = operation_deadline
            .checked_add(Duration::from_secs(1))
            .unwrap();

        assert_eq!(
            run_keychain_command_until(command, operation_deadline, cleanup_deadline).unwrap_err(),
            KeychainSecretReadErrorV1::Timeout
        );
    }

    #[cfg(unix)]
    #[test]
    fn keychain_process_runner_kills_and_reaps_on_timeout() {
        let mut command = process_command("/bin/sleep");
        command.arg("10");
        configure_keychain_command(&mut command);
        let child = command.spawn().unwrap();
        let process_id = child.id();
        let started_at = Instant::now();
        assert_eq!(
            capture_keychain_child(child, Duration::from_millis(30)).unwrap_err(),
            KeychainSecretReadErrorV1::Timeout
        );
        assert!(started_at.elapsed() < Duration::from_secs(2));
        assert!(!process_is_alive(process_id));
    }

    #[cfg(unix)]
    #[test]
    fn keychain_cleanup_cutoff_returns_before_late_capture_and_reaper_finishes() {
        let mut command = process_command("/bin/sh");
        command.args(["-c", "(/bin/sleep 1) & /usr/bin/printf late-secret-value"]);
        let operation_deadline = Instant::now()
            .checked_add(Duration::from_millis(40))
            .unwrap();
        let cleanup_deadline = operation_deadline
            .checked_add(Duration::from_millis(40))
            .unwrap();
        let started_at = Instant::now();

        assert_eq!(
            run_keychain_command_until(command, operation_deadline, cleanup_deadline).unwrap_err(),
            KeychainSecretReadErrorV1::CleanupTimedOut
        );
        assert!(started_at.elapsed() < Duration::from_millis(500));
        assert!(!keychain_reaper_is_idle());
        let reaper_deadline = Instant::now().checked_add(Duration::from_secs(2)).unwrap();
        while !keychain_reaper_is_idle() && Instant::now() < reaper_deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(keychain_reaper_is_idle());
    }

    #[cfg(unix)]
    #[test]
    fn keychain_process_runner_caps_stdout_and_stderr() {
        let mut stdout = process_command("/usr/bin/printf");
        stdout.arg("x".repeat(KEYCHAIN_CAPTURE_BYTES + 1));
        assert_eq!(
            run_keychain_command(stdout, Duration::from_secs(2)).unwrap_err(),
            KeychainSecretReadErrorV1::OutputTooLarge
        );
        let mut stderr = process_command("/usr/bin/find");
        let paths = (0..512)
            .map(|index| format!("/__starring_runtime_missing_{index:04}_{}", "x".repeat(64)))
            .collect::<Vec<_>>();
        stderr.args(paths);
        assert_eq!(
            run_keychain_command(stderr, Duration::from_secs(2)).unwrap_err(),
            KeychainSecretReadErrorV1::OutputTooLarge
        );
        let capture = capture_bounded(std::io::Cursor::new(vec![b'x'; 8 * 1024])).unwrap();
        assert!(capture.bytes.capacity() >= KEYCHAIN_CAPTURE_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn keychain_process_runner_reaps_when_capture_pipe_is_missing() {
        let mut command = process_command("/bin/sleep");
        command
            .arg("10")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = command.spawn().unwrap();
        let process_id = child.id();
        assert_eq!(
            capture_keychain_child(child, Duration::from_secs(1)).unwrap_err(),
            KeychainSecretReadErrorV1::Unavailable
        );
        assert!(!process_is_alive(process_id));
    }

    #[test]
    fn fake_readers_resolve_exact_capability_set_and_redact_every_container() {
        let mut environment = FakeEnvironmentV1::default();
        let database_references = DatabaseCapabilityV1::ALL.map(|capability| {
            let name = format!("STARRING_RUNTIME_TEST_DATABASE_{}", capability.index());
            environment
                .values
                .insert(name.clone(), database_url(capability));
            RuntimeSecretReferenceV1::parse(&format!("env:{name}")).unwrap()
        });
        let token_reference =
            RuntimeSecretReferenceV1::parse("keychain:starring.runtime.test:discord.bot.test")
                .unwrap();
        let mut keychain = FakeKeychainV1::default();
        keychain.values.insert(
            (
                "starring.runtime.test".to_string(),
                "discord.bot.test".to_string(),
            ),
            b"opaque.discord_bot-token_1234567890abcdef".to_vec(),
        );
        let references =
            RuntimeSecretReferencesV1::from_parts(database_references, token_reference);
        let resolved =
            ResolvedRuntimeSecretsV1::resolve(&references, &resolver(environment, keychain))
                .unwrap();
        assert_eq!(
            format!("{resolved:?}"),
            "ResolvedRuntimeSecretsV1(<redacted>)"
        );
        let databases = resolved.database_secrets();
        assert_eq!(
            format!("{databases:?}"),
            "RuntimeDatabaseSecretsByCapabilityV1(<redacted>)"
        );
        let database_url = databases.database_url(DatabaseCapabilityV1::Convergence);
        assert_eq!(
            format!("{database_url:?}"),
            "RuntimeDatabaseUrlSecretV1(<redacted>)"
        );
        let connection = database_url.connection_secret();
        assert_eq!(
            format!("{connection:?}"),
            "RuntimeDatabaseConnectionSecretV1(<redacted>)"
        );
        assert_eq!(connection.username(), "runtime_convergence");
        assert_eq!(connection.password().expose_secret(), database_password());
        assert_eq!(connection.database(), "starring");
        assert_eq!(
            connection.endpoint(),
            &RuntimeDatabaseEndpointV1::Network("db.example".to_string())
        );
        assert_eq!(connection.port(), 5432);
        assert_eq!(connection.ssl_mode(), RuntimeDatabaseSslModeV1::VerifyFull);
        assert_eq!(connection.ssl_root_cert(), None);
        let token = resolved.discord_bot_token();
        assert_eq!(format!("{token:?}"), "RuntimeDiscordBotTokenV1(<redacted>)");
        assert_eq!(
            token.expose_secret(),
            "opaque.discord_bot-token_1234567890abcdef"
        );
        for rendered in [
            format!("{:?}", connection.password()),
            format!("{connection:?}"),
            format!("{database_url:?}"),
            format!("{databases:?}"),
            format!("{token:?}"),
            format!("{resolved:?}"),
        ] {
            assert!(!rendered.contains(database_password()));
            assert!(!rendered.contains("opaque.discord_bot-token_1234567890abcdef"));
        }
        for capability in DatabaseCapabilityV1::ALL {
            assert_eq!(
                databases
                    .database_url(capability)
                    .connection_secret()
                    .username(),
                format!("runtime_{}", capability.code())
            );
        }
    }

    #[test]
    fn expired_startup_cutoff_skips_every_secret_reader() {
        let calls = Rc::new(Cell::new(0));
        let database_references = DatabaseCapabilityV1::ALL.map(|capability| {
            RuntimeSecretReferenceV1::parse(&format!(
                "env:STARRING_RUNTIME_TEST_DATABASE_{}",
                capability.index()
            ))
            .unwrap()
        });
        let references = RuntimeSecretReferencesV1::from_parts(
            database_references,
            RuntimeSecretReferenceV1::parse("env:STARRING_RUNTIME_TEST_DISCORD_TOKEN").unwrap(),
        );
        let resolver = SecretResolverV1::new(
            CountingEnvironmentV1 {
                calls: calls.clone(),
            },
            FakeKeychainV1::default(),
        );

        let operation_cutoff = Instant::now();
        let cleanup_deadline = operation_cutoff
            .checked_add(Duration::from_secs(1))
            .unwrap();
        let result = ResolvedRuntimeSecretsV1::resolve_until(
            &references,
            &resolver,
            operation_cutoff,
            cleanup_deadline,
        );

        assert!(matches!(
            result,
            Err(RuntimeSecretsStartupResolutionErrorV1::OperationDeadlineElapsed)
        ));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn duplicate_resolved_database_identity_is_rejected_with_capability_context() {
        let mut environment = FakeEnvironmentV1::default();
        let database_references = DatabaseCapabilityV1::ALL.map(|capability| {
            let name = format!("STARRING_RUNTIME_TEST_DATABASE_{}", capability.index());
            let password = format!(
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef0123456789_{}",
                capability.index()
            );
            environment.values.insert(
                name.clone(),
                format!(
                    "postgresql:{}{}runtime_shared:{}@DB.EXAMPLE:5432/starring?sslmode=verify-full",
                    "/", "/", password
                ),
            );
            RuntimeSecretReferenceV1::parse(&format!("env:{name}")).unwrap()
        });
        let token_name = "STARRING_RUNTIME_TEST_DISCORD_TOKEN";
        environment.values.insert(
            token_name.to_string(),
            "opaque.discord_bot-token_1234567890abcdef".to_string(),
        );
        let references = RuntimeSecretReferencesV1::from_parts(
            database_references,
            RuntimeSecretReferenceV1::parse(&format!("env:{token_name}")).unwrap(),
        );
        let error = ResolvedRuntimeSecretsV1::resolve(
            &references,
            &resolver(environment, FakeKeychainV1::default()),
        )
        .unwrap_err();
        assert_eq!(
            error,
            RuntimeSecretsResolutionErrorV1::DuplicateDatabaseIdentity {
                capability: DatabaseCapabilityV1::ExactTarget
            }
        );
        assert_eq!(error.code(), "runtime_secret_database_identity_duplicate");
        assert_eq!(error.context(), Some("exact_target"));
        assert!(!format!("{error:?}").contains(database_password()));
        assert!(!error.to_string().contains(database_password()));
    }

    #[test]
    fn resolution_errors_are_finite_and_secret_values_are_never_formatted() {
        let missing = resolver(FakeEnvironmentV1::default(), FakeKeychainV1::default());
        let reference = RuntimeSecretReferenceV1::parse("env:MISSING_RUNTIME_SECRET").unwrap();
        let operation_cutoff = Instant::now().checked_add(Duration::from_secs(1)).unwrap();
        let cleanup_deadline = operation_cutoff
            .checked_add(Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            missing
                .resolve(&reference, operation_cutoff, cleanup_deadline)
                .unwrap_err(),
            RuntimeSecretResolutionErrorV1::Missing
        );
        let invalid = resolver(
            FakeEnvironmentV1 {
                invalid: true,
                ..FakeEnvironmentV1::default()
            },
            FakeKeychainV1::default(),
        );
        assert_eq!(
            invalid
                .resolve(&reference, operation_cutoff, cleanup_deadline)
                .unwrap_err(),
            RuntimeSecretResolutionErrorV1::InvalidEncoding
        );
        let marker = "runtime-secret-marker";
        let resolved =
            ResolvedSecretV1::from_zeroizing(Zeroizing::new(marker.to_string())).unwrap();
        assert_eq!(format!("{resolved:?}"), "ResolvedSecretV1(<redacted>)");
        let error = RuntimeSecretsResolutionErrorV1::Database {
            capability: DatabaseCapabilityV1::Panel,
            source: RuntimeSecretResolutionErrorV1::InvalidSecret,
        };
        assert!(!format!("{error:?}").contains(marker));
        assert!(!error.to_string().contains(marker));
        assert_eq!(error.code(), "runtime_secret_invalid");
        assert_eq!(error.context(), Some("panel"));
    }

    #[test]
    fn database_parser_accepts_only_complete_unambiguous_connection_parts() {
        let socket = resolved_secret(format!(
            "postgresql:{}{}runtime_panel:{}@localhost:5432/starring?host=/private/tmp&sslmode=disable",
            "/",
            "/",
            database_password()
        ));
        let socket = RuntimeDatabaseUrlSecretV1::parse(socket).unwrap();
        let connection = socket.connection_secret();
        assert_eq!(connection.username(), "runtime_panel");
        assert_eq!(connection.password().expose_secret(), database_password());
        assert_eq!(connection.database(), "starring");
        assert_eq!(
            connection.endpoint(),
            &RuntimeDatabaseEndpointV1::Socket("/private/tmp".to_string())
        );
        assert_eq!(connection.port(), 5432);
        assert_eq!(connection.ssl_mode(), RuntimeDatabaseSslModeV1::Disable);
        assert_eq!(connection.ssl_root_cert(), None);

        for invalid in [
            format!("postgresql:{}{}db.example:5432/starring?sslmode=verify-full", "/", "/"),
            format!("postgresql:{}{}runtime_panel@db.example:5432/starring?sslmode=verify-full", "/", "/"),
            format!("postgresql:{}{}runtime_panel:short@db.example:5432/starring?sslmode=verify-full", "/", "/"),
            format!("postgresql:{}{}runtime_panel:{}@db.example/starring?sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/?sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=verify-full&sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?options=-c&sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=verify-full#fragment", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@localhost:5432/starring?host=/private/tmp&sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=verify-full&sslrootcert=relative.pem", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=disable", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=allow", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=prefer", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=require", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=verify-ca", "/", "/", database_password()),
            format!("POSTGRESQL:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@bad_host:5432/starring?sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@::1:5432/starring?sslmode=disable", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:05432/starring?sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}runtime_panel:{}@db.example:5432/starring?sslmode=verify-full?port=5432", "/", "/", database_password()),
        ] {
            assert_eq!(
                RuntimeDatabaseUrlSecretV1::parse(resolved_secret(invalid)).unwrap_err(),
                RuntimeSecretResolutionErrorV1::InvalidSecret
            );
        }
    }

    #[test]
    fn controlled_database_parser_accepts_canonical_network_forms() {
        let ipv6 = resolved_secret(format!(
            "postgres:{}{}runtime_serving:{}@[::1]/starring?port=5432&sslmode=disable",
            "/",
            "/",
            database_password()
        ));
        let ipv6 = RuntimeDatabaseUrlSecretV1::parse(ipv6).unwrap();
        let ipv6 = ipv6.connection_secret();
        assert_eq!(
            ipv6.endpoint(),
            &RuntimeDatabaseEndpointV1::Network("::1".to_string())
        );
        assert_eq!(ipv6.port(), 5432);
        assert_eq!(ipv6.ssl_mode(), RuntimeDatabaseSslModeV1::Disable);

        let verified = resolved_secret(format!(
            "postgresql:{}{}runtime_serving:{}@DB.EXAMPLE:5432/starring?sslrootcert=/etc/ssl/certs/starring.pem&sslmode=verify-full",
            "/",
            "/",
            database_password()
        ));
        let verified = RuntimeDatabaseUrlSecretV1::parse(verified).unwrap();
        let verified = verified.connection_secret();
        assert_eq!(
            verified.endpoint(),
            &RuntimeDatabaseEndpointV1::Network("db.example".to_string())
        );
        assert_eq!(
            verified.ssl_root_cert(),
            Some("/etc/ssl/certs/starring.pem")
        );
        assert_eq!(verified.ssl_mode(), RuntimeDatabaseSslModeV1::VerifyFull);
    }

    #[test]
    fn discord_token_parser_is_ascii_non_whitespace_and_bounded() {
        assert_eq!(
            RuntimeDiscordBotTokenV1::parse(resolved_secret("short-token".to_string()))
                .unwrap_err(),
            RuntimeSecretResolutionErrorV1::InvalidSecret
        );
        assert_eq!(
            RuntimeDiscordBotTokenV1::parse(resolved_secret("token with space".to_string()))
                .unwrap_err(),
            RuntimeSecretResolutionErrorV1::InvalidSecret
        );
        assert_eq!(
            RuntimeDiscordBotTokenV1::parse(resolved_secret("토큰".to_string())).unwrap_err(),
            RuntimeSecretResolutionErrorV1::InvalidSecret
        );
        let oversized = "x".repeat(MAX_DISCORD_TOKEN_BYTES + 1);
        assert_eq!(
            RuntimeDiscordBotTokenV1::parse(resolved_secret(oversized)).unwrap_err(),
            RuntimeSecretResolutionErrorV1::InvalidSecret
        );
    }

    fn resolved_secret(value: String) -> ResolvedSecretV1 {
        ResolvedSecretV1::from_zeroizing(Zeroizing::new(value)).unwrap()
    }

    fn database_url(capability: DatabaseCapabilityV1) -> String {
        format!(
            "postgresql:{}{}runtime_{}:{}@db.example:5432/starring?sslmode=verify-full",
            "/",
            "/",
            capability.code(),
            database_password()
        )
    }

    fn database_password() -> &'static str {
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef0123456789_-"
    }
}
