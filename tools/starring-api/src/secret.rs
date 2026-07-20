use std::fmt::{Debug, Formatter};
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::io::Read;
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::process::{Child, Command, Stdio};
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::thread;
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::time::{Duration, Instant};

use authoring_application_discord::{DiscordOAuthClientSecretV1, DiscordOAuthSecretError};
use authoring_application_postgres::{
    ProductActionDigestKeyV1, ProductActionDigestKeyringV1, SnapshotEnvelopeKeyV1,
    SnapshotEnvelopeKeyringV1,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::config::{DatabaseRoleV1, ProductionConfigV1};

const MAX_ENVIRONMENT_NAME_BYTES: usize = 128;
const MAX_KEYCHAIN_COMPONENT_BYTES: usize = 128;
const MAX_SECRET_BYTES: usize = 32 * 1024;
const MAX_DATABASE_URL_BYTES: usize = 8 * 1024;
const MAX_DISCORD_TOKEN_BYTES: usize = 8 * 1024;
const MAX_KEYRING_BYTES: usize = 16 * 1024;
const MAX_KEYRING_KEYS: usize = 8;
#[cfg(any(target_os = "macos", all(test, unix)))]
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(any(target_os = "macos", all(test, unix)))]
const KEYCHAIN_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(any(target_os = "macos", all(test, unix)))]
const KEYCHAIN_CAPTURE_BYTES: usize = MAX_SECRET_BYTES + 2;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SecretReferenceKindV1 {
    Environment,
    Keychain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SecretReferenceParseErrorV1 {
    #[error("secret reference source is unsupported")]
    UnsupportedSource,
    #[error("secret reference is invalid")]
    InvalidReference,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SecretReferenceV1 {
    source: SecretReferenceSourceV1,
}

#[derive(Clone, PartialEq, Eq)]
enum SecretReferenceSourceV1 {
    Environment(String),
    Keychain { service: String, account: String },
}

impl SecretReferenceV1 {
    pub(crate) fn environment(name: &str) -> Result<Self, SecretReferenceParseErrorV1> {
        if !valid_environment_name(name) {
            return Err(SecretReferenceParseErrorV1::InvalidReference);
        }
        Ok(Self {
            source: SecretReferenceSourceV1::Environment(name.to_string()),
        })
    }

    pub(crate) fn keychain(
        service: &str,
        account: &str,
    ) -> Result<Self, SecretReferenceParseErrorV1> {
        if !valid_keychain_component(service) || !valid_keychain_component(account) {
            return Err(SecretReferenceParseErrorV1::InvalidReference);
        }
        Ok(Self {
            source: SecretReferenceSourceV1::Keychain {
                service: service.to_string(),
                account: account.to_string(),
            },
        })
    }

    pub(crate) fn parse(value: &str) -> Result<Self, SecretReferenceParseErrorV1> {
        if let Some(name) = value.strip_prefix("env:") {
            return Self::environment(name);
        }
        if let Some(pair) = value.strip_prefix("keychain:") {
            let mut components = pair.split(':');
            let service = components
                .next()
                .ok_or(SecretReferenceParseErrorV1::InvalidReference)?;
            let account = components
                .next()
                .ok_or(SecretReferenceParseErrorV1::InvalidReference)?;
            if components.next().is_some() {
                return Err(SecretReferenceParseErrorV1::InvalidReference);
            }
            return Self::keychain(service, account);
        }
        Err(SecretReferenceParseErrorV1::UnsupportedSource)
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> SecretReferenceKindV1 {
        match self.source {
            SecretReferenceSourceV1::Environment(_) => SecretReferenceKindV1::Environment,
            SecretReferenceSourceV1::Keychain { .. } => SecretReferenceKindV1::Keychain,
        }
    }
}

impl Debug for SecretReferenceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretReferenceV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnvironmentSecretReadErrorV1 {
    InvalidEncoding,
}

pub(crate) trait EnvironmentSecretReaderV1 {
    fn read_secret(
        &self,
        name: &str,
    ) -> Result<Option<Zeroizing<String>>, EnvironmentSecretReadErrorV1>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ProcessEnvironmentSecretReaderV1;

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
pub(crate) enum KeychainSecretReadErrorV1 {
    Unavailable,
    Timeout,
    OutputTooLarge,
}

pub(crate) trait KeychainSecretReaderV1 {
    fn read_secret(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MacOsKeychainSecretReaderV1;

impl KeychainSecretReaderV1 for MacOsKeychainSecretReaderV1 {
    fn read_secret(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
        #[cfg(target_os = "macos")]
        {
            read_macos_keychain_secret(service, account)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (service, account);
            Err(KeychainSecretReadErrorV1::Unavailable)
        }
    }
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_secret(
    service: &str,
    account: &str,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    let mut command = Command::new("/usr/bin/security");
    command
        .args(["find-generic-password", "-w", "-s", service, "-a", account])
        .env_clear();
    run_keychain_command(command, KEYCHAIN_TIMEOUT)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn run_keychain_command(
    mut command: Command,
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    configure_keychain_command(&mut command);
    let child = command
        .spawn()
        .map_err(|_| KeychainSecretReadErrorV1::Unavailable)?;
    capture_keychain_child(child, timeout)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn configure_keychain_command(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn capture_keychain_child(
    mut child: Child,
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap(&mut child);
        return Err(KeychainSecretReadErrorV1::Unavailable);
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_and_reap(&mut child);
        return Err(KeychainSecretReadErrorV1::Unavailable);
    };
    let stdout_capture = match thread::Builder::new()
        .name("starring-keychain-stdout".to_string())
        .spawn(move || capture_bounded(stdout))
    {
        Ok(capture) => capture,
        Err(_) => {
            terminate_and_reap(&mut child);
            return Err(KeychainSecretReadErrorV1::Unavailable);
        }
    };
    let stderr_capture = match thread::Builder::new()
        .name("starring-keychain-stderr".to_string())
        .spawn(move || capture_bounded(stderr))
    {
        Ok(capture) => capture,
        Err(_) => {
            terminate_and_reap(&mut child);
            let _ = stdout_capture.join();
            return Err(KeychainSecretReadErrorV1::Unavailable);
        }
    };
    let started_at = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started_at.elapsed() < timeout => {
                thread::sleep(KEYCHAIN_POLL_INTERVAL.min(timeout));
            }
            Ok(None) => {
                terminate_and_reap(&mut child);
                let _ = stdout_capture.join();
                let _ = stderr_capture.join();
                return Err(KeychainSecretReadErrorV1::Timeout);
            }
            Err(_) => {
                terminate_and_reap(&mut child);
                let _ = stdout_capture.join();
                let _ = stderr_capture.join();
                return Err(KeychainSecretReadErrorV1::Unavailable);
            }
        }
    };
    let (stdout, stderr) = join_keychain_captures(stdout_capture, stderr_capture)?;
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

#[cfg(any(target_os = "macos", all(test, unix)))]
fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
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
fn capture_bounded(mut reader: impl Read) -> Result<BoundedCaptureV1, KeychainSecretReadErrorV1> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(KEYCHAIN_CAPTURE_BYTES.min(4 * 1024)));
    let mut overflowed = false;
    let mut buffer = Zeroizing::new([0_u8; 1024]);
    loop {
        let read = reader
            .read(&mut *buffer)
            .map_err(|_| KeychainSecretReadErrorV1::Unavailable)?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretResolutionErrorV1 {
    #[error("referenced secret is missing")]
    Missing,
    #[error("referenced secret encoding is invalid")]
    InvalidEncoding,
    #[error("referenced secret is invalid")]
    InvalidSecret,
    #[error("referenced secret exceeds the supported size")]
    TooLarge,
    #[error("Keychain secret lookup is unavailable")]
    KeychainUnavailable,
    #[error("Keychain secret lookup timed out")]
    KeychainTimeout,
}

pub(crate) struct ResolvedSecretV1(Zeroizing<String>);

impl ResolvedSecretV1 {
    fn from_zeroizing(value: Zeroizing<String>) -> Result<Self, SecretResolutionErrorV1> {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(SecretResolutionErrorV1::InvalidSecret);
        }
        if value.len() > MAX_SECRET_BYTES {
            return Err(SecretResolutionErrorV1::TooLarge);
        }
        Ok(Self(value))
    }

    fn expose_secret(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn into_zeroizing(self) -> Zeroizing<String> {
        self.0
    }
}

impl Debug for ResolvedSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResolvedSecretV1(<redacted>)")
    }
}

pub(crate) struct DatabaseUrlSecretV1(DatabaseConnectionSecretV1);

pub(crate) struct DatabaseConnectionSecretV1 {
    username: String,
    password: Zeroizing<String>,
    database: String,
    endpoint: DatabaseEndpointV1,
    port: u16,
    ssl_mode: DatabaseSslModeV1,
    ssl_root_cert: Option<String>,
}

pub(crate) enum DatabaseEndpointV1 {
    Network(String),
    Socket(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatabaseSslModeV1 {
    Disable,
    Allow,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl DatabaseConnectionSecretV1 {
    pub(crate) fn into_parts(
        self,
    ) -> (
        String,
        Zeroizing<String>,
        String,
        DatabaseEndpointV1,
        u16,
        DatabaseSslModeV1,
        Option<String>,
    ) {
        let Self {
            username,
            password,
            database,
            endpoint,
            port,
            ssl_mode,
            ssl_root_cert,
        } = self;
        (
            username,
            password,
            database,
            endpoint,
            port,
            ssl_mode,
            ssl_root_cert,
        )
    }
}

impl DatabaseUrlSecretV1 {
    pub(crate) fn parse(secret: ResolvedSecretV1) -> Result<Self, SecretResolutionErrorV1> {
        let value = secret.into_zeroizing();
        if value.len() > MAX_DATABASE_URL_BYTES {
            return Err(SecretResolutionErrorV1::InvalidSecret);
        }
        parse_database_connection_secret(&value)
            .map(Self)
            .ok_or(SecretResolutionErrorV1::InvalidSecret)
    }

    pub(crate) fn into_connection_secret(self) -> DatabaseConnectionSecretV1 {
        self.0
    }
}

impl Debug for DatabaseUrlSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseUrlSecretV1(<redacted>)")
    }
}

pub(crate) struct DiscordBotTokenV1(Zeroizing<String>);

impl DiscordBotTokenV1 {
    pub(crate) fn parse(secret: ResolvedSecretV1) -> Result<Self, SecretResolutionErrorV1> {
        let value = secret.into_zeroizing();
        if value.len() > MAX_DISCORD_TOKEN_BYTES
            || value.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            return Err(SecretResolutionErrorV1::InvalidSecret);
        }
        Ok(Self(value))
    }

    pub(crate) fn into_zeroizing(self) -> Zeroizing<String> {
        self.0
    }
}

impl Debug for DiscordBotTokenV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiscordBotTokenV1(<redacted>)")
    }
}

pub(crate) struct SecretResolverV1<E, K> {
    environment: E,
    keychain: K,
}

impl<E, K> SecretResolverV1<E, K> {
    pub(crate) fn new(environment: E, keychain: K) -> Self {
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
    pub(crate) fn resolve(
        &self,
        reference: &SecretReferenceV1,
    ) -> Result<ResolvedSecretV1, SecretResolutionErrorV1> {
        let value = match &reference.source {
            SecretReferenceSourceV1::Environment(name) => self
                .environment
                .read_secret(name)
                .map_err(|_| SecretResolutionErrorV1::InvalidEncoding)?
                .ok_or(SecretResolutionErrorV1::Missing)?,
            SecretReferenceSourceV1::Keychain { service, account } => {
                let bytes = self
                    .keychain
                    .read_secret(service, account)
                    .map_err(map_keychain_error)?;
                zeroizing_utf8(bytes)?
            }
        };
        ResolvedSecretV1::from_zeroizing(value)
    }

    pub(crate) fn resolve_database_url(
        &self,
        reference: &SecretReferenceV1,
    ) -> Result<DatabaseUrlSecretV1, SecretResolutionErrorV1> {
        DatabaseUrlSecretV1::parse(self.resolve(reference)?)
    }

    pub(crate) fn resolve_discord_bot_token(
        &self,
        reference: &SecretReferenceV1,
    ) -> Result<DiscordBotTokenV1, SecretResolutionErrorV1> {
        DiscordBotTokenV1::parse(self.resolve(reference)?)
    }
}

impl<E, K> Debug for SecretResolverV1<E, K> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretResolverV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeyringPayloadErrorV1 {
    #[error("keyring payload is invalid")]
    InvalidPayload,
    #[error("keyring payload version is unsupported")]
    UnsupportedVersion,
    #[error("keyring is invalid")]
    InvalidKeyring,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedKeyringV1<'a> {
    version: u8,
    #[serde(borrow)]
    active: EncodedKeyV1<'a>,
    #[serde(borrow)]
    retired: Vec<EncodedKeyV1<'a>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedKeyV1<'a> {
    id: &'a str,
    material: &'a str,
}

pub(crate) fn parse_product_action_digest_keyring_v1(
    secret: &ResolvedSecretV1,
) -> Result<ProductActionDigestKeyringV1, KeyringPayloadErrorV1> {
    let payload = parse_keyring_payload(secret)?;
    let active = product_action_key(&payload.active)?;
    let retired = payload
        .retired
        .iter()
        .map(product_action_key)
        .collect::<Result<Vec<_>, _>>()?;
    ProductActionDigestKeyringV1::new(active, retired)
        .map_err(|_| KeyringPayloadErrorV1::InvalidKeyring)
}

pub(crate) fn parse_snapshot_envelope_keyring_v1(
    secret: &ResolvedSecretV1,
) -> Result<SnapshotEnvelopeKeyringV1, KeyringPayloadErrorV1> {
    let payload = parse_keyring_payload(secret)?;
    let active = snapshot_envelope_key(&payload.active)?;
    let retired = payload
        .retired
        .iter()
        .map(snapshot_envelope_key)
        .collect::<Result<Vec<_>, _>>()?;
    SnapshotEnvelopeKeyringV1::new(active, retired)
        .map_err(|_| KeyringPayloadErrorV1::InvalidKeyring)
}

fn parse_keyring_payload(
    secret: &ResolvedSecretV1,
) -> Result<EncodedKeyringV1<'_>, KeyringPayloadErrorV1> {
    if secret.expose_secret().len() > MAX_KEYRING_BYTES {
        return Err(KeyringPayloadErrorV1::InvalidPayload);
    }
    let payload: EncodedKeyringV1<'_> = serde_json::from_str(secret.expose_secret())
        .map_err(|_| KeyringPayloadErrorV1::InvalidPayload)?;
    if payload.version != 1 {
        return Err(KeyringPayloadErrorV1::UnsupportedVersion);
    }
    if payload.retired.len() >= MAX_KEYRING_KEYS {
        return Err(KeyringPayloadErrorV1::InvalidKeyring);
    }
    Ok(payload)
}

fn product_action_key(
    encoded: &EncodedKeyV1<'_>,
) -> Result<ProductActionDigestKeyV1, KeyringPayloadErrorV1> {
    ProductActionDigestKeyV1::from_zeroizing(encoded.id, decode_key_material(encoded.material)?)
        .map_err(|_| KeyringPayloadErrorV1::InvalidKeyring)
}

fn snapshot_envelope_key(
    encoded: &EncodedKeyV1<'_>,
) -> Result<SnapshotEnvelopeKeyV1, KeyringPayloadErrorV1> {
    SnapshotEnvelopeKeyV1::new(encoded.id, decode_key_material(encoded.material)?)
        .map_err(|_| KeyringPayloadErrorV1::InvalidKeyring)
}

fn decode_key_material(value: &str) -> Result<Zeroizing<[u8; 32]>, KeyringPayloadErrorV1> {
    if value.len() != 44
        || value.as_bytes().last() != Some(&b'=')
        || !value.as_bytes()[..43]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
    {
        return Err(KeyringPayloadErrorV1::InvalidPayload);
    }
    let mut decoded = Zeroizing::new([0_u8; 33]);
    let decoded_length = STANDARD
        .decode_slice(value, &mut *decoded)
        .map_err(|_| KeyringPayloadErrorV1::InvalidPayload)?;
    if decoded_length != 32 {
        return Err(KeyringPayloadErrorV1::InvalidPayload);
    }
    let mut material = Zeroizing::new([0_u8; 32]);
    material.copy_from_slice(&decoded[..32]);
    Ok(material)
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ENVIRONMENT_NAME_BYTES
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_keychain_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_KEYCHAIN_COMPONENT_BYTES
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@' | b'/')
        })
}

fn parse_database_connection_secret(value: &str) -> Option<DatabaseConnectionSecretV1> {
    if !value.is_ascii() || value.contains('%') {
        return None;
    }
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "postgres" | "postgresql") || url.fragment().is_some() {
        return None;
    }
    let username = url.username();
    let password = url.password()?;
    let database = url.path().strip_prefix('/')?;
    if !valid_database_identifier(username)
        || !valid_database_password(password)
        || !valid_database_identifier(database)
    {
        return None;
    }

    let mut ssl_mode = None;
    let mut socket = None;
    let mut query_port = None;
    let mut ssl_root_cert = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "sslmode" if ssl_mode.is_none() => {
                ssl_mode = Some(DatabaseSslModeV1::parse(&value)?);
            }
            "host" if socket.is_none() && value.starts_with('/') => {
                if !valid_absolute_database_path(&value) {
                    return None;
                }
                socket = Some(value.into_owned());
            }
            "port" if query_port.is_none() => {
                query_port = Some(value.parse::<u16>().ok().filter(|port| *port != 0)?);
            }
            "sslrootcert" if ssl_root_cert.is_none() => {
                if !valid_absolute_database_path(&value) {
                    return None;
                }
                ssl_root_cert = Some(value.into_owned());
            }
            _ => return None,
        }
    }
    let ssl_mode = ssl_mode?;
    let authority_port = url.port();
    if authority_port.is_some() && query_port.is_some() {
        return None;
    }
    let port = authority_port.or(query_port)?;
    let authority_host = url.host_str();
    let endpoint = if let Some(socket) = socket {
        if authority_host != Some("localhost")
            || ssl_mode != DatabaseSslModeV1::Disable
            || ssl_root_cert.is_some()
        {
            return None;
        }
        DatabaseEndpointV1::Socket(socket)
    } else {
        DatabaseEndpointV1::Network(authority_host?.to_string())
    };
    Some(DatabaseConnectionSecretV1 {
        username: username.to_string(),
        password: Zeroizing::new(password.to_string()),
        database: database.to_string(),
        endpoint,
        port,
        ssl_mode,
        ssl_root_cert,
    })
}

impl DatabaseSslModeV1 {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "disable" => Some(Self::Disable),
            "allow" => Some(Self::Allow),
            "prefer" => Some(Self::Prefer),
            "require" => Some(Self::Require),
            "verify-ca" => Some(Self::VerifyCa),
            "verify-full" => Some(Self::VerifyFull),
            _ => None,
        }
    }
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
) -> Result<Zeroizing<String>, SecretResolutionErrorV1> {
    let moved = std::mem::take(&mut *bytes);
    match String::from_utf8(moved) {
        Ok(value) => Ok(Zeroizing::new(value)),
        Err(error) => {
            let mut invalid = error.into_bytes();
            invalid.zeroize();
            Err(SecretResolutionErrorV1::InvalidEncoding)
        }
    }
}

fn map_keychain_error(error: KeychainSecretReadErrorV1) -> SecretResolutionErrorV1 {
    match error {
        KeychainSecretReadErrorV1::Unavailable => SecretResolutionErrorV1::KeychainUnavailable,
        KeychainSecretReadErrorV1::Timeout => SecretResolutionErrorV1::KeychainTimeout,
        KeychainSecretReadErrorV1::OutputTooLarge => SecretResolutionErrorV1::TooLarge,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProductionSecretResolutionErrorV1 {
    #[error("production database secret resolution failed")]
    Database {
        role: DatabaseRoleV1,
        #[source]
        source: SecretResolutionErrorV1,
    },
    #[error("Discord OAuth client secret resolution failed")]
    DiscordOAuthClientSecret(#[source] SecretResolutionErrorV1),
    #[error("Discord OAuth client secret is invalid")]
    InvalidDiscordOAuthClientSecret,
    #[error("Discord bot token resolution failed")]
    DiscordBotToken(#[source] SecretResolutionErrorV1),
    #[error("product action keyring resolution failed")]
    ProductActionKeyringSecret(#[source] SecretResolutionErrorV1),
    #[error("product action keyring payload is invalid")]
    ProductActionKeyring(#[source] KeyringPayloadErrorV1),
    #[error("snapshot envelope keyring resolution failed")]
    SnapshotEnvelopeKeyringSecret(#[source] SecretResolutionErrorV1),
    #[error("snapshot envelope keyring payload is invalid")]
    SnapshotEnvelopeKeyring(#[source] KeyringPayloadErrorV1),
    #[error("cryptographic key material must be unique across purposes")]
    CrossPurposeKeyMaterialAlias,
    #[error("production secret role cardinality is invalid")]
    InvalidDatabaseRoleCardinality,
}

pub struct ResolvedProductionSecretsV1 {
    database_urls: [DatabaseUrlSecretV1; 13],
    discord_bot_token: DiscordBotTokenV1,
    discord_oauth_client_secret: DiscordOAuthClientSecretV1,
    product_action_keyring: ProductActionDigestKeyringV1,
    snapshot_envelope_keyring: SnapshotEnvelopeKeyringV1,
}

pub fn resolve_production_secrets_v1(
    config: &ProductionConfigV1,
) -> Result<ResolvedProductionSecretsV1, ProductionSecretResolutionErrorV1> {
    ResolvedProductionSecretsV1::from_system(config)
}

pub(crate) type ResolvedProductionSecretPartsV1 = (
    [DatabaseUrlSecretV1; 13],
    DiscordBotTokenV1,
    DiscordOAuthClientSecretV1,
    ProductActionDigestKeyringV1,
    SnapshotEnvelopeKeyringV1,
);

impl ResolvedProductionSecretsV1 {
    pub(crate) fn from_system(
        config: &ProductionConfigV1,
    ) -> Result<Self, ProductionSecretResolutionErrorV1> {
        Self::resolve(config, &SecretResolverV1::default())
    }

    pub(crate) fn resolve<E: EnvironmentSecretReaderV1, K: KeychainSecretReaderV1>(
        config: &ProductionConfigV1,
        resolver: &SecretResolverV1<E, K>,
    ) -> Result<Self, ProductionSecretResolutionErrorV1> {
        let references = config.secret_references();
        let mut database_urls = Vec::with_capacity(DatabaseRoleV1::ALL.len());
        for role in DatabaseRoleV1::ALL {
            let database_url = resolver
                .resolve_database_url(references.database(role))
                .map_err(|source| ProductionSecretResolutionErrorV1::Database { role, source })?;
            database_urls.push(database_url);
        }
        let database_urls = database_urls
            .try_into()
            .map_err(|_| ProductionSecretResolutionErrorV1::InvalidDatabaseRoleCardinality)?;
        let oauth_secret = resolver
            .resolve(references.discord_oauth_client_secret())
            .map_err(ProductionSecretResolutionErrorV1::DiscordOAuthClientSecret)?;
        let mut oauth_secret = oauth_secret.into_zeroizing();
        let oauth_secret = std::mem::take(&mut *oauth_secret);
        let discord_oauth_client_secret = DiscordOAuthClientSecretV1::from_owned(oauth_secret)
            .map_err(map_discord_oauth_secret_error)?;
        let discord_bot_token = resolver
            .resolve_discord_bot_token(references.discord_bot_token())
            .map_err(ProductionSecretResolutionErrorV1::DiscordBotToken)?;
        let action_secret = resolver
            .resolve(references.product_action_keyring())
            .map_err(ProductionSecretResolutionErrorV1::ProductActionKeyringSecret)?;
        let product_action_keyring = parse_product_action_digest_keyring_v1(&action_secret)
            .map_err(ProductionSecretResolutionErrorV1::ProductActionKeyring)?;
        let snapshot_secret = resolver
            .resolve(references.snapshot_envelope_keyring())
            .map_err(ProductionSecretResolutionErrorV1::SnapshotEnvelopeKeyringSecret)?;
        let snapshot_envelope_keyring = parse_snapshot_envelope_keyring_v1(&snapshot_secret)
            .map_err(ProductionSecretResolutionErrorV1::SnapshotEnvelopeKeyring)?;
        let action_payload = parse_keyring_payload(&action_secret)
            .map_err(ProductionSecretResolutionErrorV1::ProductActionKeyring)?;
        let snapshot_payload = parse_keyring_payload(&snapshot_secret)
            .map_err(ProductionSecretResolutionErrorV1::SnapshotEnvelopeKeyring)?;
        if keyring_payloads_alias_material(&action_payload, &snapshot_payload) {
            return Err(ProductionSecretResolutionErrorV1::CrossPurposeKeyMaterialAlias);
        }
        Ok(Self {
            database_urls,
            discord_bot_token,
            discord_oauth_client_secret,
            product_action_keyring,
            snapshot_envelope_keyring,
        })
    }

    pub(crate) fn into_parts(self) -> ResolvedProductionSecretPartsV1 {
        (
            self.database_urls,
            self.discord_bot_token,
            self.discord_oauth_client_secret,
            self.product_action_keyring,
            self.snapshot_envelope_keyring,
        )
    }
}

impl Debug for ResolvedProductionSecretsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResolvedProductionSecretsV1(<redacted>)")
    }
}

fn map_discord_oauth_secret_error(
    _error: DiscordOAuthSecretError,
) -> ProductionSecretResolutionErrorV1 {
    ProductionSecretResolutionErrorV1::InvalidDiscordOAuthClientSecret
}

fn keyring_payloads_alias_material(
    left: &EncodedKeyringV1<'_>,
    right: &EncodedKeyringV1<'_>,
) -> bool {
    let mut left = std::iter::once(&left.active).chain(left.retired.iter());
    let right = std::iter::once(&right.active)
        .chain(right.retired.iter())
        .collect::<Vec<_>>();
    left.any(|left| right.iter().any(|right| left.material == right.material))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

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

    impl KeychainSecretReaderV1 for FakeKeychainV1 {
        fn read_secret(
            &self,
            service: &str,
            account: &str,
        ) -> Result<Zeroizing<Vec<u8>>, KeychainSecretReadErrorV1> {
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

    fn material(seed: u8) -> String {
        let bytes = std::array::from_fn::<_, 32, _>(|index| seed.wrapping_add(index as u8));
        STANDARD.encode(bytes)
    }

    fn keyring_json(active_id: &str, active_material: &str, retired: &[(&str, &str)]) -> String {
        let retired = retired
            .iter()
            .map(|(id, material)| {
                serde_json::json!({
                    "id": id,
                    "material": material,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "version": 1,
            "active": {
                "id": active_id,
                "material": active_material,
            },
            "retired": retired,
        })
        .to_string()
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
        assert!(matches!(
            run_keychain_command(command, Duration::from_secs(1)),
            Err(KeychainSecretReadErrorV1::Unavailable)
        ));
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
        assert!(matches!(
            capture_keychain_child(child, Duration::from_millis(30)),
            Err(KeychainSecretReadErrorV1::Timeout)
        ));
        assert!(started_at.elapsed() < Duration::from_secs(2));
        assert!(!process_is_alive(process_id));
    }

    #[cfg(unix)]
    #[test]
    fn keychain_process_runner_rejects_oversized_stdout() {
        let mut command = process_command("/usr/bin/printf");
        command.arg("x".repeat(KEYCHAIN_CAPTURE_BYTES + 1));
        assert!(matches!(
            run_keychain_command(command, Duration::from_secs(2)),
            Err(KeychainSecretReadErrorV1::OutputTooLarge)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn keychain_process_runner_rejects_oversized_stderr() {
        let mut command = process_command("/usr/bin/find");
        let paths = (0..512)
            .map(|index| format!("/__starring_missing_{index:04}_{}", "x".repeat(64)))
            .collect::<Vec<_>>();
        command.args(paths);
        assert!(matches!(
            run_keychain_command(command, Duration::from_secs(2)),
            Err(KeychainSecretReadErrorV1::OutputTooLarge)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn keychain_capture_join_waits_for_both_sides_before_returning_error() {
        let stderr_completed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stdout_capture = thread::spawn(|| Err(KeychainSecretReadErrorV1::Unavailable));
        let completed = stderr_completed.clone();
        let stderr_capture = thread::spawn(move || {
            completed.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(BoundedCaptureV1 {
                bytes: Zeroizing::new(Vec::new()),
                overflowed: false,
            })
        });
        assert!(matches!(
            join_keychain_captures(stdout_capture, stderr_capture),
            Err(KeychainSecretReadErrorV1::Unavailable)
        ));
        assert!(stderr_completed.load(std::sync::atomic::Ordering::SeqCst));
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
        assert!(matches!(
            capture_keychain_child(child, Duration::from_secs(1)),
            Err(KeychainSecretReadErrorV1::Unavailable)
        ));
        assert!(!process_is_alive(process_id));
    }

    #[test]
    fn product_action_parser_source_requires_zeroizing_constructor() {
        let source = include_str!("secret.rs");
        let (_, parser_and_after) = source.split_once("fn product_action_key(").unwrap();
        let (parser, _) = parser_and_after
            .split_once("fn snapshot_envelope_key(")
            .unwrap();
        assert!(parser.contains("ProductActionDigestKeyV1::from_zeroizing"));
        assert!(!parser.contains("ProductActionDigestKeyV1::from_bytes"));
    }

    #[test]
    fn references_accept_only_explicit_nonliteral_sources() {
        let environment = SecretReferenceV1::parse("env:STARRING_DATABASE_URL").unwrap();
        let keychain =
            SecretReferenceV1::parse("keychain:starring.production:database.oauth").unwrap();
        assert_eq!(environment.kind(), SecretReferenceKindV1::Environment);
        assert_eq!(keychain.kind(), SecretReferenceKindV1::Keychain);
        assert_eq!(
            SecretReferenceV1::parse("postgresql:opaque"),
            Err(SecretReferenceParseErrorV1::UnsupportedSource)
        );
        assert_eq!(
            SecretReferenceV1::parse("env:lowercase"),
            Err(SecretReferenceParseErrorV1::InvalidReference)
        );
        assert_eq!(
            SecretReferenceV1::parse("keychain:service:account:extra"),
            Err(SecretReferenceParseErrorV1::InvalidReference)
        );
        assert_eq!(format!("{environment:?}"), "SecretReferenceV1(<redacted>)");
    }

    #[test]
    fn fake_sources_resolve_without_process_environment_mutation() {
        let mut environment = FakeEnvironmentV1::default();
        environment
            .values
            .insert("STARRING_DATABASE_URL".into(), database_url());
        let mut keychain = FakeKeychainV1::default();
        keychain.values.insert(
            ("starring.production".into(), "discord.bot".into()),
            b"bot-token".to_vec(),
        );
        let resolver = resolver(environment, keychain);
        let database = resolver
            .resolve_database_url(&SecretReferenceV1::environment("STARRING_DATABASE_URL").unwrap())
            .unwrap();
        let token = resolver
            .resolve_discord_bot_token(
                &SecretReferenceV1::keychain("starring.production", "discord.bot").unwrap(),
            )
            .unwrap();
        assert_eq!(format!("{database:?}"), "DatabaseUrlSecretV1(<redacted>)");
        assert_eq!(format!("{token:?}"), "DiscordBotTokenV1(<redacted>)");
        let database = database.into_connection_secret();
        assert_eq!(database.username, "starring_identity_oauth");
        assert!(matches!(
            database.endpoint,
            DatabaseEndpointV1::Network(ref host) if host == "db.example"
        ));
        assert_eq!(database.port, 5432);
        assert_eq!(database.database, "starring");
        assert!(matches!(database.ssl_mode, DatabaseSslModeV1::VerifyFull));
        assert_eq!(token.into_zeroizing().as_str(), "bot-token");
    }

    #[test]
    fn resolution_errors_are_closed_and_secret_values_are_redacted() {
        let missing = resolver(FakeEnvironmentV1::default(), FakeKeychainV1::default());
        assert_eq!(
            missing
                .resolve(&SecretReferenceV1::environment("MISSING_SECRET").unwrap())
                .unwrap_err(),
            SecretResolutionErrorV1::Missing
        );
        let invalid_environment = FakeEnvironmentV1 {
            invalid: true,
            ..FakeEnvironmentV1::default()
        };
        let invalid = resolver(invalid_environment, FakeKeychainV1::default());
        assert_eq!(
            invalid
                .resolve(&SecretReferenceV1::environment("INVALID_SECRET").unwrap())
                .unwrap_err(),
            SecretResolutionErrorV1::InvalidEncoding
        );
        let value = ResolvedSecretV1::from_zeroizing(Zeroizing::new("hidden".to_string())).unwrap();
        assert_eq!(format!("{value:?}"), "ResolvedSecretV1(<redacted>)");
    }

    #[test]
    fn database_and_token_shapes_are_bounded() {
        let invalid_database =
            ResolvedSecretV1::from_zeroizing(Zeroizing::new("mysql:opaque".to_string())).unwrap();
        assert_eq!(
            DatabaseUrlSecretV1::parse(invalid_database).unwrap_err(),
            SecretResolutionErrorV1::InvalidSecret
        );
        let invalid_token =
            ResolvedSecretV1::from_zeroizing(Zeroizing::new("token with space".to_string()))
                .unwrap();
        assert_eq!(
            DiscordBotTokenV1::parse(invalid_token).unwrap_err(),
            SecretResolutionErrorV1::InvalidSecret
        );
    }

    #[test]
    fn database_urls_are_complete_and_ignore_no_credential_component() {
        let socket = ResolvedSecretV1::from_zeroizing(Zeroizing::new(format!(
            "postgresql:{}{}:{}@localhost:5432/starring?host=/private/tmp&sslmode=disable",
            "/",
            "/starring_identity_oauth",
            database_password()
        )))
        .unwrap();
        let socket = DatabaseUrlSecretV1::parse(socket)
            .unwrap()
            .into_connection_secret();
        assert!(matches!(
            socket.endpoint,
            DatabaseEndpointV1::Socket(ref path) if path == "/private/tmp"
        ));
        assert_eq!(socket.username, "starring_identity_oauth");
        assert_eq!(socket.database, "starring");
        assert!(matches!(socket.ssl_mode, DatabaseSslModeV1::Disable));

        for invalid in [
            format!("postgresql:{}{}db.example:5432/starring?sslmode=verify-full", "/", "/"),
            format!("postgresql:{}{}starring_identity_oauth@db.example:5432/starring?sslmode=verify-full", "/", "/"),
            format!("postgresql:{}{}starring_identity_oauth:short@db.example:5432/starring?sslmode=verify-full", "/", "/"),
            format!("postgresql:{}{}starring_identity_oauth:{}@db.example/starring?sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}starring_identity_oauth:{}@db.example:5432/?sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}starring_identity_oauth:{}@db.example:5432/starring", "/", "/", database_password()),
            format!("postgresql:{}{}starring_identity_oauth:{}@db.example:5432/starring?sslmode=verify-full&sslmode=verify-full", "/", "/", database_password()),
            format!("postgresql:{}{}starring_identity_oauth:{}@db.example:5432/starring?sslmode=verify-full&options=-c", "/", "/", database_password()),
            format!("postgresql:{}{}starring_identity_oauth:{}@db.example:5432/starring?sslmode=verify-full&user=other", "/", "/", database_password()),
            format!("postgresql:{}{}starring_identity_oauth:{}@db.example:5432/starring?sslmode=verify-full&sslrootcert=relative.pem", "/", "/", database_password()),
            format!("postgresql:{}{}starring_identity_oauth:{}@localhost:5432/starring?host=/private/tmp&sslmode=verify-full", "/", "/", database_password()),
        ] {
            let secret = ResolvedSecretV1::from_zeroizing(Zeroizing::new(invalid)).unwrap();
            assert_eq!(
                DatabaseUrlSecretV1::parse(secret).unwrap_err(),
                SecretResolutionErrorV1::InvalidSecret
            );
        }
    }

    #[test]
    fn strict_keyring_payloads_build_both_production_keyrings() {
        let active = material(11);
        let retired = material(73);
        let payload = keyring_json("active-v2", &active, &[("retired-v1", &retired)]);
        let secret = ResolvedSecretV1::from_zeroizing(Zeroizing::new(payload)).unwrap();
        let action = parse_product_action_digest_keyring_v1(&secret).unwrap();
        let snapshot = parse_snapshot_envelope_keyring_v1(&secret).unwrap();
        assert_eq!(
            format!("{action:?}"),
            "ProductDecisionDigestKeyringV1(<redacted>)"
        );
        assert_eq!(snapshot.active_key_id(), "active-v2");
        assert_eq!(snapshot.configured_key_count(), 2);
    }

    #[test]
    fn keyring_parser_rejects_unknown_fields_versions_and_aliases() {
        let active = material(17);
        let retired = material(17);
        let unknown = serde_json::json!({
            "version": 1,
            "active": {"id": "active", "material": active},
            "retired": [],
            "extra": true,
        })
        .to_string();
        let unknown = ResolvedSecretV1::from_zeroizing(Zeroizing::new(unknown)).unwrap();
        assert_eq!(
            parse_product_action_digest_keyring_v1(&unknown).unwrap_err(),
            KeyringPayloadErrorV1::InvalidPayload
        );
        let unsupported = keyring_json("active", &material(19), &[])
            .replace(concat!("\"version\"", ":1"), concat!("\"version\"", ":2"));
        let unsupported = ResolvedSecretV1::from_zeroizing(Zeroizing::new(unsupported)).unwrap();
        assert_eq!(
            parse_snapshot_envelope_keyring_v1(&unsupported).unwrap_err(),
            KeyringPayloadErrorV1::UnsupportedVersion
        );
        let aliased = keyring_json("active", &active, &[("retired", &retired)]);
        let aliased = ResolvedSecretV1::from_zeroizing(Zeroizing::new(aliased)).unwrap();
        assert_eq!(
            parse_product_action_digest_keyring_v1(&aliased).unwrap_err(),
            KeyringPayloadErrorV1::InvalidKeyring
        );
        assert_eq!(
            parse_snapshot_envelope_keyring_v1(&aliased).unwrap_err(),
            KeyringPayloadErrorV1::InvalidKeyring
        );
    }

    #[test]
    fn keyring_parser_rejects_noncanonical_or_weak_material() {
        let short = keyring_json("active", "AAAA", &[]);
        let short = ResolvedSecretV1::from_zeroizing(Zeroizing::new(short)).unwrap();
        assert_eq!(
            parse_product_action_digest_keyring_v1(&short).unwrap_err(),
            KeyringPayloadErrorV1::InvalidPayload
        );
        let weak = STANDARD.encode([9_u8; 32]);
        let weak = keyring_json("active", &weak, &[]);
        let weak = ResolvedSecretV1::from_zeroizing(Zeroizing::new(weak)).unwrap();
        assert_eq!(
            parse_product_action_digest_keyring_v1(&weak).unwrap_err(),
            KeyringPayloadErrorV1::InvalidKeyring
        );
        assert_eq!(
            parse_snapshot_envelope_keyring_v1(&weak).unwrap_err(),
            KeyringPayloadErrorV1::InvalidKeyring
        );
    }

    #[test]
    fn keyring_material_cannot_be_reused_across_cryptographic_purposes() {
        let shared = material(31);
        let action = keyring_json("action-active", &shared, &[]);
        let snapshot = keyring_json("snapshot-active", &shared, &[]);
        let action = ResolvedSecretV1::from_zeroizing(Zeroizing::new(action)).unwrap();
        let snapshot = ResolvedSecretV1::from_zeroizing(Zeroizing::new(snapshot)).unwrap();
        let action = parse_keyring_payload(&action).unwrap();
        let snapshot = parse_keyring_payload(&snapshot).unwrap();
        assert!(keyring_payloads_alias_material(&action, &snapshot));

        let distinct = keyring_json("snapshot-active", &material(101), &[]);
        let distinct = ResolvedSecretV1::from_zeroizing(Zeroizing::new(distinct)).unwrap();
        let distinct = parse_keyring_payload(&distinct).unwrap();
        assert!(!keyring_payloads_alias_material(&action, &distinct));
    }

    fn database_url() -> String {
        format!(
            "postgresql:{}{}starring_identity_oauth:{}@db.example:5432/starring?sslmode=verify-full",
            "/",
            "/",
            database_password()
        )
    }

    fn database_password() -> &'static str {
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef0123456789_-"
    }
}
