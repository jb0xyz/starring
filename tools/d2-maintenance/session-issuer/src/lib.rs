use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use thiserror::Error;
use zeroize::Zeroizing;

pub const DATABASE_NAME: &str = "starring_runtime_staging";
pub const D2_PUBLIC_ORIGIN: &str = "https://d2-api.starring.co.kr";
pub const MAX_CHILD_OUTPUT_BYTES: usize = 1_048_576;
pub const MAX_SCENARIO_BYTES: u64 = 49_152;
pub const SESSION_LIFETIME_SECONDS: f64 = 600.0;

const OAUTH_ACCOUNT: &str = "database.oauth-flow-writer";
const ISSUER_ACCOUNT: &str = "database.session-issuer";
const SECURITY_ACCOUNT: &str = "database.security-revoker";
const OWNER_ACCOUNT: &str = "lifecycle-owner";
const OAUTH_ROLE: &str = "starring_identity_oauth";
const ISSUER_ROLE: &str = "starring_identity_issuer";
const SECURITY_ROLE: &str = "starring_identity_security";
const MAX_MANIFEST_BYTES: u64 = 131_072;
const MAX_KEYCHAIN_VALUE_BYTES: usize = 4_096;
const TRUSTED_RUNNER_BYTES: &[u8] = include_bytes!("../../headless_product_runner.mjs");
const TRUSTED_PRODUCT_DRIVER_BYTES: &[u8] =
    include_bytes!("../../../d2-certification/product_driver.js");
const TRUSTED_SCENARIO_NAME: &str = "study-room.v1.json";
const TRUSTED_SCENARIO_BYTES: &[u8] = include_bytes!("../../scenarios/study-room.v1.json");
const ISSUER_CARGO_TOML_BYTES: &[u8] = include_bytes!("../Cargo.toml");
const ISSUER_CARGO_LOCK_BYTES: &[u8] = include_bytes!("../Cargo.lock");
const ISSUER_LIB_RS_BYTES: &[u8] = include_bytes!("lib.rs");
const ISSUER_MAIN_RS_BYTES: &[u8] = include_bytes!("main.rs");
const ISSUER_SOURCE_DIGEST_DOMAIN: &[u8] = b"starring.d2a.session-issuer-source.v1\0";
const GLOBAL_LOCK_PATH: &str = "/private/tmp/starring-d2-certification.lock";
pub const SESSION_LIFECYCLE_NAME: &str = "d2a-session-lifecycle.json";
pub const D2A_TEARDOWN_FENCE_NAME: &str = "d2a-teardown-fence.json";
const SESSION_LIFECYCLE_KIND: &str = "starring.d2a.session-lifecycle.v1";
const D2A_TEARDOWN_FENCE_KIND: &str = "starring.d2a.teardown-fence.v1";
const MAX_SESSION_LIFECYCLE_BYTES: u64 = 16_384;
const DIRECT_ONBOARDING_EVIDENCE_NAME: &str = "d2a-onboarding-evidence.json";
const DIRECT_ONBOARDING_EVIDENCE_KIND: &str = "starring.d2a.direct-onboarding-evidence.v1";
const AUTOMATED_MAINTENANCE_CLASS: &str = "automated_maintenance_v1";
const D2_HUMAN_BOUNDARIES: [&str; 6] = [
    "create_disposable_discord_guild",
    "complete_discord_oauth",
    "confirm_product_preview",
    "execute_real_discord_interactions",
    "confirm_replacement_preview",
    "delete_disposable_discord_guild",
];
const DISCORD_OWNERSHIP_REGISTRY_PATH: &str =
    "/private/tmp/starring-d2-discord-ownership-registry.json";
const D2_CLOUDFLARE_TUNNEL_ID: &str = "57c22e8a-0ec2-4f67-a882-2c355b0348df";
const PROTECTED_KEYCHAIN_SERVICES: [&str; 5] = [
    "starring-api.staging",
    "starring.runtime.staging",
    "starring.postgres.staging",
    "com.starring.llm-api-key",
    "com.cloudflare.tunnel.macmini-llm-prod",
];
const CODEX_WORKER_SOURCE_FILES: [&str; 7] = [
    "admission-registry.mjs",
    "codex-runner.mjs",
    "metrics-log.mjs",
    "protocol.mjs",
    "request-timeline.mjs",
    "scheduler.mjs",
    "worker.mjs",
];
const D2_TOOLCHAIN_SOURCE_FILES: [&str; 15] = [
    "d2_certification.py",
    "d2_evidence.py",
    "d2_finalization.py",
    "d2_legacy_substrate_recovery.py",
    "d2_orchestrator_composition.py",
    "d2_orchestrator_contract.py",
    "d2_orchestrator_platform.py",
    "d2_preflight_evidence.py",
    "d2_drained_runtime_restart.py",
    "d2_live_runtime_restart.py",
    "d2_run.py",
    "d2_source_contract.py",
    "d2_worker_evidence.py",
    "isolated_orchestrator.py",
    "product_driver.js",
];
const CERTIFICATION_TRANSPORT_SOURCE_FILES: [&str; 10] = [
    ".gitignore",
    "Cargo.lock",
    "Cargo.toml",
    "src/config.rs",
    "src/control.rs",
    "src/gateway.rs",
    "src/http_proxy.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/state.rs",
];

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum IssuerError {
    #[error("arguments_invalid")]
    ArgumentsInvalid,
    #[error("ambient_postgres_environment_rejected")]
    AmbientPostgresEnvironment,
    #[error("manifest_path_invalid")]
    ManifestPath,
    #[error("manifest_permissions_invalid")]
    ManifestPermissions,
    #[error("manifest_invalid")]
    Manifest,
    #[error("manifest_digest_invalid")]
    ManifestDigest,
    #[error("manifest_contract_invalid")]
    ManifestContract,
    #[error("orchestrator_state_invalid")]
    OrchestratorState,
    #[error("orchestrator_not_active")]
    OrchestratorNotActive,
    #[error("isolated_runtime_invalid")]
    IsolatedRuntime,
    #[error("candidate_service_inactive")]
    CandidateServiceInactive,
    #[error("api_loopback_origin_invalid")]
    ApiLoopbackOrigin,
    #[error("api_loopback_connect_failed")]
    ApiLoopbackConnect,
    #[error("api_loopback_write_failed")]
    ApiLoopbackWrite,
    #[error("api_loopback_read_failed")]
    ApiLoopbackRead,
    #[error("api_loopback_response_empty")]
    ApiLoopbackResponseEmpty,
    #[error("api_loopback_status_invalid")]
    ApiLoopbackStatus,
    #[error("keychain_identity_invalid")]
    KeychainIdentity,
    #[error("keychain_secret_unavailable")]
    KeychainSecret,
    #[error("database_credential_invalid")]
    DatabaseCredential,
    #[error("candidate_node_invalid")]
    CandidateNode,
    #[error("child_runner_invalid")]
    ChildRunner,
    #[error("scenario_invalid")]
    Scenario,
    #[error("evidence_invalid")]
    Evidence,
    #[error("evidence_too_large")]
    EvidenceTooLarge,
    #[error("entropy_unavailable")]
    Entropy,
    #[error("database_operation_failed")]
    Database,
    #[error("child_failed")]
    Child,
    #[error("child_timed_out")]
    ChildTimeout,
    #[error("child_interrupted")]
    ChildInterrupted,
    #[error("d2_operation_busy")]
    D2OperationBusy,
    #[error("core_dump_policy_failed")]
    CoreDumpPolicy,
    #[error("d2a_taint_invalid")]
    D2aTaint,
    #[error("discord_ownership_invalid")]
    DiscordOwnership,
    #[error("discord_hub_preflight_failed")]
    DiscordHubPreflight,
    #[error("direct_onboarding_failed")]
    DirectOnboarding,
    #[error("direct_onboarding_output_invalid")]
    DirectOnboardingOutput,
    #[error("direct_onboarding_evidence_invalid")]
    DirectOnboardingEvidence,
    #[error("commercial_onboarding_artifact_rejected")]
    CommercialOnboardingArtifact,
    #[error("issuer_process_isolation_required")]
    ProcessIsolation,
    #[error("session_lifecycle_invalid")]
    SessionLifecycle,
    #[error("session_lifecycle_binary_invalid")]
    SessionLifecycleBinary,
    #[error("session_lifecycle_source_invalid")]
    SessionLifecycleSource,
    #[error("session_lifecycle_boot_identity_invalid")]
    SessionLifecycleBootIdentity,
    #[error("session_lifecycle_existing_marker_invalid")]
    SessionLifecycleExistingMarker,
    #[error("session_lifecycle_handoff_invalid")]
    SessionLifecycleHandoff,
    #[error("session_lifecycle_reentry_invalid")]
    SessionLifecycleReentry,
    #[error("session_lifecycle_cas_failed")]
    SessionLifecycleCas,
    #[error("manual_recovery_required")]
    ManualRecoveryRequired,
    #[error("d2a_teardown_fence_invalid")]
    D2aTeardownFence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    AuthSmoke,
    DirectOnboard,
    OneShot,
}

impl Operation {
    pub fn parse(value: &str) -> Result<Self, IssuerError> {
        match value {
            "auth-smoke" => Ok(Self::AuthSmoke),
            "direct-onboard" => Ok(Self::DirectOnboard),
            "one-shot" => Ok(Self::OneShot),
            _ => Err(IssuerError::ArgumentsInvalid),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthSmoke => "auth-smoke",
            Self::DirectOnboard => "direct-onboard",
            Self::OneShot => "one-shot",
        }
    }
}

#[derive(Debug)]
pub struct Arguments {
    pub manifest_path: PathBuf,
    pub operation: Operation,
    pub display_name: Option<String>,
    pub scenario_path: Option<PathBuf>,
    pub child: Vec<String>,
}

pub fn parse_arguments<I>(arguments: I) -> Result<Arguments, IssuerError>
where
    I: IntoIterator<Item = String>,
{
    let values = arguments.into_iter().collect::<Vec<_>>();
    let separator = values.iter().position(|value| value == "--");
    let option_end = separator.unwrap_or(values.len());
    let child = separator
        .map(|index| values[index + 1..].to_vec())
        .unwrap_or_default();
    let mut manifest_path = None;
    let mut operation = None;
    let mut display_name = None;
    let mut scenario_path = None;
    let mut index = 0;
    while index < option_end {
        let option = values[index].as_str();
        index += 1;
        if index >= option_end {
            return Err(IssuerError::ArgumentsInvalid);
        }
        let value = values[index].clone();
        index += 1;
        match option {
            "--manifest" if manifest_path.is_none() => {
                manifest_path = Some(PathBuf::from(value));
            }
            "--operation" if operation.is_none() => {
                operation = Some(Operation::parse(&value)?);
            }
            "--display-name" if display_name.is_none() => {
                display_name = Some(value);
            }
            "--scenario" if scenario_path.is_none() => {
                scenario_path = Some(PathBuf::from(value));
            }
            _ => return Err(IssuerError::ArgumentsInvalid),
        }
    }
    let arguments = Arguments {
        manifest_path: manifest_path.ok_or(IssuerError::ArgumentsInvalid)?,
        operation: operation.ok_or(IssuerError::ArgumentsInvalid)?,
        display_name,
        scenario_path,
        child,
    };
    let shape_is_valid = match arguments.operation {
        Operation::DirectOnboard => {
            separator.is_none()
                && arguments.child.is_empty()
                && arguments.scenario_path.is_none()
                && arguments
                    .display_name
                    .as_deref()
                    .is_some_and(valid_display_name)
        }
        Operation::AuthSmoke => {
            separator.is_some()
                && !arguments.child.is_empty()
                && arguments.scenario_path.is_none()
                && arguments.display_name.is_none()
        }
        Operation::OneShot => {
            separator.is_some()
                && !arguments.child.is_empty()
                && arguments.scenario_path.is_some()
                && arguments.display_name.is_none()
        }
    };
    if !shape_is_valid {
        return Err(IssuerError::ArgumentsInvalid);
    }
    Ok(arguments)
}

fn valid_display_name(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && value.chars().count() <= 128
        && value.len() <= 512
        && !value.chars().any(char::is_control)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    authoring: Value,
    certification_class: String,
    cloudflare: CloudflareManifest,
    commit_sha: String,
    created_at: String,
    schema_version: u64,
    run_id: String,
    public_origin: String,
    database: DatabaseManifest,
    discord: DiscordManifest,
    keychain_services: KeychainServices,
    protected_staging: ProtectedStaging,
    services: BTreeMap<String, ServiceManifest>,
    candidates: CandidateManifest,
    expected_steps: Vec<Value>,
    external_keychain: ExternalKeychainManifest,
    human_boundaries: Vec<String>,
    source_trees: SourceTreesManifest,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CloudflareManifest {
    tunnel_id: String,
    public_origin: String,
    origin_service: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalKeychainManifest {
    discord_oauth_client_secret: KeychainIdentityManifest,
    discord_bot_token: KeychainIdentityManifest,
    tunnel_token: KeychainIdentityManifest,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KeychainIdentityManifest {
    service: String,
    account: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceTreesManifest {
    codex_worker: SourceTreeManifest,
    d2_toolchain: SourceTreeManifest,
    certification_transport: SourceTreeManifest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceTreeManifest {
    root: PathBuf,
    files: Vec<String>,
    sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DatabaseManifest {
    cluster_root: PathBuf,
    name: String,
    port: u16,
    socket_directory: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DiscordManifest {
    actor_id: String,
    application_id: String,
    bot_user_id: String,
    guild_id: String,
    hub_channel_id: String,
    resource_prefix: String,
    disposable_guild_required: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KeychainServices {
    api: String,
    postgres: String,
    runtime: String,
    worker: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProtectedStaging {
    database: String,
    launchd_labels: Vec<String>,
    mutation_allowed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ServiceManifest {
    label: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    gateway_port: Option<u16>,
    #[serde(default)]
    http_port: Option<u16>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateManifest {
    api: CandidateFile,
    runtime: CandidateFile,
    codex_worker: CandidateFile,
    codex: CandidateFile,
    db_bootstrap: CandidateFile,
    sealed_provisioner: CandidateFile,
    certification_transport: CandidateFile,
    node: CandidateFile,
    cloudflared: CandidateFile,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateFile {
    path: PathBuf,
    sha256: String,
}

#[derive(Debug)]
pub struct ValidatedRun {
    pub manifest_path: PathBuf,
    pub run_directory: PathBuf,
    pub run_id: String,
    pub commit_sha: String,
    pub manifest_sha256: String,
    pub isolated_root: PathBuf,
    pub socket_directory: PathBuf,
    pub cluster_root: PathBuf,
    pub database_port: u16,
    pub api_port: u16,
    pub api_keychain_service: String,
    pub actor_id: String,
    pub principal_id: String,
    pub guild_id: String,
    pub installation_id: String,
    pub public_origin: String,
    pub discord_application_id: String,
    pub discord_hub_channel_id: String,
    pub sealed_provisioner_path: PathBuf,
    pub sealed_provisioner_sha256: String,
    pub candidate_node_path: PathBuf,
    pub candidate_node_sha256: String,
    pub service_labels: Vec<String>,
    pub uid: u32,
    discord_bot_user_id: String,
    discord_bot_token_service: String,
    discord_bot_token_account: String,
    cloudflare_tunnel_id: String,
    source_trees: SourceTreesManifest,
    all_candidates: Vec<CandidateBinding>,
    candidate_services: Vec<CandidateServiceBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CandidateBinding {
    name: &'static str,
    path: PathBuf,
    sha256: String,
    expected_mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CandidateServiceBinding {
    name: &'static str,
    label: String,
    configured_program: String,
    arguments: Vec<String>,
    process_arguments: Vec<String>,
    plist_path: PathBuf,
    process_candidate: CandidateBinding,
    supporting_candidates: Vec<CandidateBinding>,
    environment: BTreeMap<String, String>,
    working_directory: String,
    log_path: String,
    expected_plist: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchdJob {
    pid: i32,
    program: String,
    plist_path: String,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    working_directory: String,
    stdout_path: String,
    stderr_path: String,
    umask: String,
    minimum_runtime: u64,
    exit_timeout: u64,
    soft_maxfiles: u64,
    hard_maxfiles: u64,
    runs: u64,
    state: String,
}

pub struct GlobalOperationLock {
    file: File,
    _directory_anchor: Option<File>,
    _run_directory_anchor: Option<File>,
}

impl Drop for GlobalOperationLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub fn acquire_global_operation_lock() -> Result<GlobalOperationLock, IssuerError> {
    let uid = current_uid()?;
    let parent = Path::new("/private/tmp");
    let parent_metadata = parent
        .symlink_metadata()
        .map_err(|_| IssuerError::D2OperationBusy)?;
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != 0
        || parent_metadata.permissions().mode() & 0o7777 != 0o1777
    {
        return Err(IssuerError::D2OperationBusy);
    }
    acquire_owned_exclusive_lock(Path::new(GLOBAL_LOCK_PATH), uid).map(|(lock, _created)| lock)
}

pub fn acquire_run_coordinator_lock(
    run: &ValidatedRun,
) -> Result<GlobalOperationLock, IssuerError> {
    // The commercial coordinator creates this directory lazily on its first
    // operation. Direct onboarding is deliberately earlier than that first
    // commercial step, so the issuer must perform the same create-once
    // transition while it still holds the global D2 operation lock.
    acquire_run_coordinator_lock_with_hook(&run.run_directory, run.uid, || {})
}

fn acquire_run_coordinator_lock_with_hook<F>(
    run_directory: &Path,
    uid: u32,
    after_directory_open: F,
) -> Result<GlobalOperationLock, IssuerError>
where
    F: FnOnce(),
{
    let error = IssuerError::D2OperationBusy;
    let run_anchor = open_owned_directory(run_directory, uid, 0o700, error)?;
    let created_directory =
        unsafe { libc::mkdirat(run_anchor.as_raw_fd(), c"coordinator".as_ptr(), 0o700) };
    let created_directory = if created_directory == 0 {
        true
    } else if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
        false
    } else {
        return Err(error);
    };
    let directory_anchor = open_owned_directory_at(&run_anchor, c"coordinator", uid, 0o700, error)?;
    if created_directory {
        run_anchor.sync_all().map_err(|_| error)?;
    }
    after_directory_open();

    let (file, created_lock) = open_owned_lock_at(&directory_anchor, uid, error)?;
    if created_lock {
        directory_anchor.sync_all().map_err(|_| error)?;
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(error);
    }

    // The lock is useful only while the two named directory entries still
    // resolve to the exact inodes opened above. Reopen through the anchored
    // parent descriptors, never through a path that can follow an exchanged
    // intermediate symlink.
    let named_run_directory = open_owned_directory(run_directory, uid, 0o700, error)?;
    if directory_identity(&named_run_directory.metadata().map_err(|_| error)?)
        != directory_identity(&run_anchor.metadata().map_err(|_| error)?)
    {
        return Err(error);
    }
    let named_directory = open_owned_directory_at(&run_anchor, c"coordinator", uid, 0o700, error)?;
    if directory_identity(&named_directory.metadata().map_err(|_| error)?)
        != directory_identity(&directory_anchor.metadata().map_err(|_| error)?)
    {
        return Err(error);
    }
    let named_lock = open_existing_lock_at(&directory_anchor, uid, error)?;
    if file_identity_tuple(&named_lock.metadata().map_err(|_| error)?)
        != file_identity_tuple(&file.metadata().map_err(|_| error)?)
    {
        return Err(error);
    }
    Ok(GlobalOperationLock {
        file,
        _directory_anchor: Some(directory_anchor),
        _run_directory_anchor: Some(run_anchor),
    })
}

fn open_owned_directory(
    path: &Path,
    uid: u32,
    mode: u32,
    error: IssuerError,
) -> Result<File, IssuerError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| error)?;
    let opened = file.metadata().map_err(|_| error)?;
    let named = path.symlink_metadata().map_err(|_| error)?;
    if !valid_owned_directory_metadata(&opened, uid, mode)
        || directory_identity(&opened) != directory_identity(&named)
    {
        return Err(error);
    }
    Ok(file)
}

fn open_owned_directory_at(
    parent: &File,
    name: &CStr,
    uid: u32,
    mode: u32,
    error: IssuerError,
) -> Result<File, IssuerError> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(error);
    }
    let file = unsafe { File::from_raw_fd(descriptor) };
    if !valid_owned_directory_metadata(&file.metadata().map_err(|_| error)?, uid, mode) {
        return Err(error);
    }
    Ok(file)
}

fn valid_owned_directory_metadata(metadata: &fs::Metadata, uid: u32, mode: u32) -> bool {
    metadata.file_type().is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == uid
        && metadata.permissions().mode() & 0o777 == mode
}

fn directory_identity(metadata: &fs::Metadata) -> (u64, u64, u32, u32, u64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.mode(),
        metadata.uid(),
        metadata.nlink(),
    )
}

fn open_owned_lock_at(
    directory: &File,
    uid: u32,
    error: IssuerError,
) -> Result<(File, bool), IssuerError> {
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c"coordinator.lock".as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    let (file, created) = if descriptor >= 0 {
        (unsafe { File::from_raw_fd(descriptor) }, true)
    } else if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
        (open_existing_lock_at(directory, uid, error)?, false)
    } else {
        return Err(error);
    };
    if created {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| error)?;
        file.sync_all().map_err(|_| error)?;
    }
    validate_lock_metadata(&file.metadata().map_err(|_| error)?, uid, error)?;
    Ok((file, created))
}

fn open_existing_lock_at(
    directory: &File,
    uid: u32,
    error: IssuerError,
) -> Result<File, IssuerError> {
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c"coordinator.lock".as_ptr(),
            libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(error);
    }
    let file = unsafe { File::from_raw_fd(descriptor) };
    validate_lock_metadata(&file.metadata().map_err(|_| error)?, uid, error)?;
    Ok(file)
}

fn validate_lock_metadata(
    metadata: &fs::Metadata,
    uid: u32,
    error: IssuerError,
) -> Result<(), IssuerError> {
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(error);
    }
    Ok(())
}

fn acquire_owned_exclusive_lock(
    path: &Path,
    uid: u32,
) -> Result<(GlobalOperationLock, bool), IssuerError> {
    let create = || {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    };
    let existing = || {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    };
    let (file, created) = match create() {
        Ok(file) => (file, true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            (existing().map_err(|_| IssuerError::D2OperationBusy)?, false)
        }
        Err(_) => return Err(IssuerError::D2OperationBusy),
    };
    if created {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| IssuerError::D2OperationBusy)?;
        file.sync_all().map_err(|_| IssuerError::D2OperationBusy)?;
    }
    let metadata = file.metadata().map_err(|_| IssuerError::D2OperationBusy)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(IssuerError::D2OperationBusy);
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(IssuerError::D2OperationBusy);
    }
    Ok((
        GlobalOperationLock {
            file,
            _directory_anchor: None,
            _run_directory_anchor: None,
        },
        created,
    ))
}

pub fn disable_core_dumps() -> Result<(), IssuerError> {
    let disabled = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &disabled) } != 0 {
        return Err(IssuerError::CoreDumpPolicy);
    }
    let mut observed = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut observed) } != 0
        || observed.rlim_cur != 0
        || observed.rlim_max != 0
    {
        return Err(IssuerError::CoreDumpPolicy);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SessionLifecycleStatus {
    Active,
    NotIssued,
    Revoked,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum SessionLifecycleOrigin {
    Bootstrap,
    Issuer,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct SessionLifecycleMarker {
    schema_version: u8,
    kind: String,
    run_id: String,
    manifest_sha256: String,
    operation: String,
    origin: SessionLifecycleOrigin,
    issuer_sha256: String,
    issuer_source_sha256: String,
    uid: u32,
    boot_identity: String,
    process_group_id: Option<i32>,
    started_at: String,
    status: SessionLifecycleStatus,
    session_revoked: bool,
    revoked_at: Option<String>,
    quarantined_at: Option<String>,
}

#[derive(Debug)]
struct SessionLifecycleSnapshot {
    marker: SessionLifecycleMarker,
    payload: Vec<u8>,
    metadata: fs::Metadata,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum D2aTeardownFenceStatus {
    Open,
    Closing,
    Closed,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct D2aTeardownFence {
    schema_version: u8,
    kind: String,
    run_id: String,
    manifest_sha256: String,
    status: D2aTeardownFenceStatus,
    updated_at: String,
}

/// An active marker is deliberately not recoverable by this binary: it contains no
/// session material.  A killed issuer therefore leaves a durable quarantine boundary
/// for a human to resolve after the bounded server-side lifetime has elapsed.
pub struct SessionLifecycle {
    path: PathBuf,
    uid: u32,
    marker: SessionLifecycleMarker,
    issuance_attempted: bool,
}

impl SessionLifecycle {
    pub fn begin(run: &ValidatedRun, operation: Operation) -> Result<Self, IssuerError> {
        let process_group_id = require_dedicated_process_session()?;
        let boot_identity =
            current_boot_identity().map_err(|_| IssuerError::SessionLifecycleBootIdentity)?;
        let issuer_sha256 = current_issuer_sha256(IssuerError::SessionLifecycleBinary)?;
        let issuer_source_sha256 = issuer_source_digest();
        if !valid_digest(&issuer_source_sha256) {
            return Err(IssuerError::SessionLifecycleSource);
        }
        let path = run.run_directory.join(SESSION_LIFECYCLE_NAME);
        let expected = match path.symlink_metadata() {
            Ok(_) => {
                let snapshot = read_session_lifecycle_snapshot(&path, run.uid)
                    .map_err(|_| IssuerError::SessionLifecycleExistingMarker)?;
                validate_session_lifecycle_marker(&snapshot.marker, run)
                    .map_err(|_| IssuerError::SessionLifecycleExistingMarker)?;
                if snapshot.marker.issuer_sha256 != issuer_sha256 {
                    return Err(IssuerError::SessionLifecycleBinary);
                }
                if snapshot.marker.issuer_source_sha256 != issuer_source_sha256 {
                    return Err(IssuerError::SessionLifecycleSource);
                }
                require_lifecycle_operation_handoff(Some(&snapshot.marker), operation)
                    .map_err(|_| IssuerError::SessionLifecycleHandoff)?;
                require_safe_lifecycle_reentry(
                    &snapshot.marker,
                    &boot_identity,
                    process_group_absent,
                )
                .map_err(|_| IssuerError::SessionLifecycleReentry)?;
                Some(snapshot)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                require_lifecycle_operation_handoff(None, operation)
                    .map_err(|_| IssuerError::SessionLifecycleHandoff)?;
                None
            }
            Err(_) => return Err(IssuerError::SessionLifecycleExistingMarker),
        };
        let marker = SessionLifecycleMarker {
            schema_version: 1,
            kind: SESSION_LIFECYCLE_KIND.to_string(),
            run_id: run.run_id.clone(),
            manifest_sha256: run.manifest_sha256.clone(),
            operation: operation.as_str().to_string(),
            origin: SessionLifecycleOrigin::Issuer,
            issuer_sha256,
            issuer_source_sha256,
            uid: run.uid,
            boot_identity,
            process_group_id: Some(process_group_id),
            started_at: lifecycle_timestamp(),
            status: SessionLifecycleStatus::Active,
            session_revoked: false,
            revoked_at: None,
            quarantined_at: None,
        };
        validate_session_lifecycle_marker(&marker, run)?;
        write_session_lifecycle_marker_cas(
            &path,
            &run.run_directory,
            run.uid,
            &marker,
            expected.as_ref(),
        )
        .map_err(|_| IssuerError::SessionLifecycleCas)?;
        Ok(Self {
            path,
            uid: run.uid,
            marker,
            issuance_attempted: false,
        })
    }

    pub fn mark_issuance_attempted(&mut self) -> Result<(), IssuerError> {
        if self.marker.status != SessionLifecycleStatus::Active || self.issuance_attempted {
            return Err(IssuerError::SessionLifecycle);
        }
        let observed = read_session_lifecycle_marker(&self.path, self.uid)?;
        let process_group_id = require_dedicated_process_session()?;
        if observed != self.marker
            || Some(process_group_id) != self.marker.process_group_id
            || current_boot_identity()? != self.marker.boot_identity
        {
            return Err(IssuerError::SessionLifecycle);
        }
        self.issuance_attempted = true;
        Ok(())
    }

    pub fn issuance_attempted(&self) -> bool {
        self.issuance_attempted
    }

    pub fn mark_revoked(&mut self) -> Result<(), IssuerError> {
        if !self.issuance_attempted {
            return Err(IssuerError::SessionLifecycle);
        }
        self.transition(SessionLifecycleStatus::Revoked)
    }

    pub fn finish_error(&mut self) -> Result<(), IssuerError> {
        if self.marker.status == SessionLifecycleStatus::Active {
            let status = if self.issuance_attempted {
                SessionLifecycleStatus::Quarantined
            } else {
                SessionLifecycleStatus::NotIssued
            };
            self.transition(status)?;
        }
        Ok(())
    }

    fn transition(&mut self, status: SessionLifecycleStatus) -> Result<(), IssuerError> {
        if self.marker.status != SessionLifecycleStatus::Active
            || !matches!(
                status,
                SessionLifecycleStatus::NotIssued
                    | SessionLifecycleStatus::Revoked
                    | SessionLifecycleStatus::Quarantined
            )
        {
            return Err(IssuerError::SessionLifecycle);
        }
        let observed = read_session_lifecycle_snapshot(&self.path, self.uid)?;
        if observed.marker != self.marker {
            return Err(IssuerError::SessionLifecycle);
        }
        let transitioned_at = lifecycle_timestamp();
        let mut next = self.marker.clone();
        next.status = status;
        next.session_revoked = status == SessionLifecycleStatus::Revoked;
        next.revoked_at =
            (status == SessionLifecycleStatus::Revoked).then_some(transitioned_at.clone());
        next.quarantined_at =
            (status == SessionLifecycleStatus::Quarantined).then_some(transitioned_at);
        write_session_lifecycle_marker_cas(
            &self.path,
            self.path.parent().ok_or(IssuerError::SessionLifecycle)?,
            self.uid,
            &next,
            Some(&observed),
        )?;
        self.marker = next;
        Ok(())
    }
}

fn require_lifecycle_operation_handoff(
    existing: Option<&SessionLifecycleMarker>,
    operation: Operation,
) -> Result<(), IssuerError> {
    match (operation, existing) {
        (Operation::DirectOnboard, Some(marker))
            if marker.origin == SessionLifecycleOrigin::Bootstrap
                && marker.operation == "direct-onboard" =>
        {
            Ok(())
        }
        (Operation::DirectOnboard, _) => Err(IssuerError::SessionLifecycle),
        (_, Some(marker)) if marker.origin == SessionLifecycleOrigin::Issuer => Ok(()),
        _ => Err(IssuerError::SessionLifecycle),
    }
}

impl Drop for SessionLifecycle {
    fn drop(&mut self) {
        // SIGKILL/abort cannot run destructors and intentionally leaves `active`.  Ordinary
        // unwinding or an overlooked early return still has enough process context to make
        // the safe `not_issued` or fail-closed quarantine terminal state durable.
        let _ = self.finish_error();
    }
}

pub fn require_dedicated_process_session() -> Result<i32, IssuerError> {
    let pid = unsafe { libc::getpid() };
    let process_group_id = unsafe { libc::getpgrp() };
    let session_id = unsafe { libc::getsid(0) };
    validate_dedicated_process_session(pid, process_group_id, session_id)?;
    Ok(process_group_id)
}

fn validate_dedicated_process_session(
    pid: i32,
    process_group_id: i32,
    session_id: i32,
) -> Result<(), IssuerError> {
    if pid <= 1 || process_group_id != pid || session_id != pid {
        return Err(IssuerError::ProcessIsolation);
    }
    Ok(())
}

/// This check must be called while the global D2 operation lock is held.  Absence is
/// treated as `open` for runs created before the fence existed; once teardown writes a
/// closing/closed fence the run can never issue another D2A session.
pub fn require_open_d2a_teardown_fence(run: &ValidatedRun) -> Result<(), IssuerError> {
    let path = run.run_directory.join(D2A_TEARDOWN_FENCE_NAME);
    let fence = match path.symlink_metadata() {
        Ok(_) => read_d2a_teardown_fence(&path, run.uid)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(IssuerError::D2aTeardownFence),
    };
    if fence.schema_version != 1
        || fence.kind != D2A_TEARDOWN_FENCE_KIND
        || fence.run_id != run.run_id
        || fence.manifest_sha256 != run.manifest_sha256
        || !canonical_lifecycle_timestamp(&fence.updated_at)
    {
        return Err(IssuerError::D2aTeardownFence);
    }
    if fence.status != D2aTeardownFenceStatus::Open {
        return Err(IssuerError::ManualRecoveryRequired);
    }
    Ok(())
}

fn validate_session_lifecycle_marker(
    marker: &SessionLifecycleMarker,
    run: &ValidatedRun,
) -> Result<(), IssuerError> {
    let operation_valid = matches!(
        marker.operation.as_str(),
        "auth-smoke" | "direct-onboard" | "one-shot"
    );
    let state_valid = valid_session_lifecycle_state(marker);
    if marker.schema_version != 1
        || marker.kind != SESSION_LIFECYCLE_KIND
        || marker.run_id != run.run_id
        || marker.manifest_sha256 != run.manifest_sha256
        || !operation_valid
        || !valid_digest(&marker.issuer_sha256)
        || !valid_digest(&marker.issuer_source_sha256)
        || marker.uid != run.uid
        || !valid_boot_identity(&marker.boot_identity)
        || !canonical_lifecycle_timestamp(&marker.started_at)
        || !state_valid
    {
        return Err(IssuerError::SessionLifecycle);
    }
    Ok(())
}

fn require_safe_lifecycle_reentry<F>(
    marker: &SessionLifecycleMarker,
    current_boot_identity: &str,
    process_group_absent: F,
) -> Result<(), IssuerError>
where
    F: FnOnce(i32) -> bool,
{
    if marker.origin == SessionLifecycleOrigin::Bootstrap {
        if marker.operation == "direct-onboard"
            && marker.status == SessionLifecycleStatus::NotIssued
            && marker.process_group_id.is_none()
        {
            return Ok(());
        }
        return Err(IssuerError::SessionLifecycle);
    }
    if !matches!(
        marker.status,
        SessionLifecycleStatus::NotIssued | SessionLifecycleStatus::Revoked
    ) {
        return Err(IssuerError::ManualRecoveryRequired);
    }
    let Some(process_group_id) = marker.process_group_id else {
        return Err(IssuerError::SessionLifecycle);
    };
    // A reboot proves every process group from the old boot is gone.  Never probe an old
    // numeric pgid after reboot because it may now identify an unrelated process group.
    if marker.boot_identity != current_boot_identity {
        return Ok(());
    }
    if !process_group_absent(process_group_id) {
        return Err(IssuerError::ManualRecoveryRequired);
    }
    Ok(())
}

fn process_group_absent(process_group_id: i32) -> bool {
    if process_group_id <= 1 {
        return false;
    }
    let result = unsafe { libc::kill(-process_group_id, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn valid_session_lifecycle_state(marker: &SessionLifecycleMarker) -> bool {
    let positive_process_group = marker.process_group_id.is_some_and(|value| value > 1);
    if marker.origin == SessionLifecycleOrigin::Bootstrap {
        return marker.operation == "direct-onboard"
            && marker.status == SessionLifecycleStatus::NotIssued
            && marker.process_group_id.is_none()
            && !marker.session_revoked
            && marker.revoked_at.is_none()
            && marker.quarantined_at.is_none();
    }
    match marker.status {
        SessionLifecycleStatus::Active => {
            positive_process_group
                && !marker.session_revoked
                && marker.revoked_at.is_none()
                && marker.quarantined_at.is_none()
        }
        SessionLifecycleStatus::NotIssued => {
            positive_process_group
                && !marker.session_revoked
                && marker.revoked_at.is_none()
                && marker.quarantined_at.is_none()
        }
        SessionLifecycleStatus::Revoked => {
            positive_process_group
                && marker.session_revoked
                && marker
                    .revoked_at
                    .as_deref()
                    .is_some_and(canonical_lifecycle_timestamp)
                && marker.quarantined_at.is_none()
        }
        SessionLifecycleStatus::Quarantined => {
            positive_process_group
                && !marker.session_revoked
                && marker.revoked_at.is_none()
                && marker
                    .quarantined_at
                    .as_deref()
                    .is_some_and(canonical_lifecycle_timestamp)
        }
    }
}

fn lifecycle_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn canonical_lifecycle_timestamp(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value).is_ok_and(|parsed| {
        parsed.offset().local_minus_utc() == 0
            && parsed
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Nanos, true)
                == value
    })
}

#[cfg(target_os = "macos")]
fn current_boot_identity() -> Result<String, IssuerError> {
    let mut boot_time = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.boottime".as_ptr(),
            (&mut boot_time as *mut libc::timeval).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0
        || size != std::mem::size_of::<libc::timeval>()
        || boot_time.tv_sec <= 0
        || !(0..1_000_000).contains(&boot_time.tv_usec)
    {
        return Err(IssuerError::SessionLifecycle);
    }
    Ok(format!(
        "darwin-boottime:{}:{}",
        boot_time.tv_sec, boot_time.tv_usec
    ))
}

#[cfg(target_os = "linux")]
fn current_boot_identity() -> Result<String, IssuerError> {
    let value = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| IssuerError::SessionLifecycle)?;
    let value = value.trim_end_matches('\n');
    let identity = format!("linux-boot-id:{value}");
    if !valid_boot_identity(&identity) {
        return Err(IssuerError::SessionLifecycle);
    }
    Ok(identity)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_boot_identity() -> Result<String, IssuerError> {
    Err(IssuerError::SessionLifecycle)
}

fn valid_boot_identity(value: &str) -> bool {
    if let Some(value) = value.strip_prefix("darwin-boottime:") {
        let Some((seconds, microseconds)) = value.split_once(':') else {
            return false;
        };
        return !seconds.starts_with('0')
            && seconds.bytes().all(|byte| byte.is_ascii_digit())
            && seconds.parse::<u64>().is_ok_and(|seconds| seconds > 0)
            && microseconds.bytes().all(|byte| byte.is_ascii_digit())
            && microseconds
                .parse::<u32>()
                .is_ok_and(|microseconds| microseconds < 1_000_000);
    }
    let Some(value) = value.strip_prefix("linux-boot-id:") else {
        return false;
    };
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase(),
        })
}

fn read_session_lifecycle_marker(
    path: &Path,
    uid: u32,
) -> Result<SessionLifecycleMarker, IssuerError> {
    Ok(read_session_lifecycle_snapshot(path, uid)?.marker)
}

fn read_d2a_teardown_fence(path: &Path, uid: u32) -> Result<D2aTeardownFence, IssuerError> {
    read_owned_sorted_json(path, uid, IssuerError::D2aTeardownFence)
}

fn read_session_lifecycle_snapshot(
    path: &Path,
    uid: u32,
) -> Result<SessionLifecycleSnapshot, IssuerError> {
    let error = IssuerError::SessionLifecycle;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| error)?;
    let metadata = file.metadata().map_err(|_| error)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > MAX_SESSION_LIFECYCLE_BYTES
    {
        return Err(error);
    }
    let mut observed = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut observed).map_err(|_| error)?;
    let after = file.metadata().map_err(|_| error)?;
    let named = path.symlink_metadata().map_err(|_| error)?;
    if observed.len() as u64 != metadata.len()
        || file_identity_tuple(&metadata) != file_identity_tuple(&after)
        || file_identity_tuple(&after) != file_identity_tuple(&named)
    {
        return Err(error);
    }
    let payload = observed.strip_suffix(b"\n").ok_or(error)?;
    if payload.ends_with(b"\n") || payload.ends_with(b"\r") {
        return Err(error);
    }
    let marker: SessionLifecycleMarker = serde_json::from_slice(payload).map_err(|_| error)?;
    let mut expected = serde_json::to_vec(&marker).map_err(|_| error)?;
    expected.push(b'\n');
    if observed != expected {
        return Err(error);
    }
    Ok(SessionLifecycleSnapshot {
        marker,
        payload: observed,
        metadata,
    })
}

fn read_owned_sorted_json<T>(path: &Path, uid: u32, error: IssuerError) -> Result<T, IssuerError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| error)?;
    let metadata = file.metadata().map_err(|_| error)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > MAX_SESSION_LIFECYCLE_BYTES
    {
        return Err(error);
    }
    let mut observed = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut observed).map_err(|_| error)?;
    if file.metadata().map_err(|_| error)?.len() != metadata.len() {
        return Err(error);
    }
    let payload = observed.strip_suffix(b"\n").ok_or(error)?;
    if payload.ends_with(b"\n") || payload.ends_with(b"\r") {
        return Err(error);
    }
    let value: Value = serde_json::from_slice(payload).map_err(|_| error)?;
    if serde_json::to_vec(&value).map_err(|_| error)? != payload {
        return Err(error);
    }
    serde_json::from_value(value).map_err(|_| error)
}

fn write_session_lifecycle_marker_cas(
    path: &Path,
    run_directory: &Path,
    uid: u32,
    marker: &SessionLifecycleMarker,
    expected: Option<&SessionLifecycleSnapshot>,
) -> Result<(), IssuerError> {
    let mut payload = serde_json::to_vec(marker).map_err(|_| IssuerError::SessionLifecycle)?;
    payload.push(b'\n');
    if payload.len() as u64 > MAX_SESSION_LIFECYCLE_BYTES {
        return Err(IssuerError::SessionLifecycle);
    }
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| IssuerError::SessionLifecycle)?;
    let temporary = run_directory.join(format!(
        ".d2a-session-lifecycle.{}-{}.tmp",
        std::process::id(),
        hex_bytes(&nonce)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)
            .map_err(|_| IssuerError::SessionLifecycle)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| IssuerError::SessionLifecycle)?;
        file.write_all(&payload)
            .map_err(|_| IssuerError::SessionLifecycle)?;
        file.sync_all().map_err(|_| IssuerError::SessionLifecycle)?;
        let metadata = file.metadata().map_err(|_| IssuerError::SessionLifecycle)?;
        if !metadata.file_type().is_file()
            || metadata.uid() != uid
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o777 != 0o600
            || metadata.len() != payload.len() as u64
        {
            return Err(IssuerError::SessionLifecycle);
        }
        drop(file);
        match expected {
            Some(expected) => {
                let observed = read_session_lifecycle_snapshot(path, uid)?;
                if observed.payload != expected.payload
                    || file_identity_tuple(&observed.metadata)
                        != file_identity_tuple(&expected.metadata)
                {
                    return Err(IssuerError::SessionLifecycle);
                }
                fs::rename(&temporary, path).map_err(|_| IssuerError::SessionLifecycle)?;
            }
            None => {
                // A hard-link publication is the portable no-replace primitive here: it
                // atomically fails if another writer created the marker after our absence
                // check. The temporary link is then removed, restoring the required nlink=1.
                fs::hard_link(&temporary, path).map_err(|_| IssuerError::SessionLifecycle)?;
                fs::remove_file(&temporary).map_err(|_| IssuerError::SessionLifecycle)?;
            }
        }
        File::open(run_directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| IssuerError::SessionLifecycle)?;
        let observed = read_session_lifecycle_marker(path, uid)?;
        if observed != *marker {
            return Err(IssuerError::SessionLifecycle);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
fn write_session_lifecycle_marker(
    path: &Path,
    run_directory: &Path,
    uid: u32,
    marker: &SessionLifecycleMarker,
) -> Result<(), IssuerError> {
    let expected = match path.symlink_metadata() {
        Ok(_) => Some(read_session_lifecycle_snapshot(path, uid)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(IssuerError::SessionLifecycle),
    };
    write_session_lifecycle_marker_cas(path, run_directory, uid, marker, expected.as_ref())
}

#[derive(Serialize)]
struct D2aTaintMarker<'a> {
    schema_version: u8,
    kind: &'static str,
    run_id: &'a str,
    manifest_sha256: &'a str,
    certification_class: &'static str,
    direct_auth_used: bool,
    release_eligible: bool,
    issuer_sha256: String,
    issuer_source_sha256: String,
    runner_sha256: String,
    product_driver_sha256: String,
    scenario_sha256: String,
}

pub fn persist_d2a_taint(run: &ValidatedRun) -> Result<(), IssuerError> {
    let payload = expected_d2a_taint_payload(run)?;
    let path = run.run_directory.join("d2a-taint.json");
    match path.symlink_metadata() {
        Ok(_) => validate_existing_taint(&path, run.uid, &payload),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)
                .map_err(|_| IssuerError::D2aTaint)?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| IssuerError::D2aTaint)?;
            file.write_all(&payload)
                .map_err(|_| IssuerError::D2aTaint)?;
            file.sync_all().map_err(|_| IssuerError::D2aTaint)?;
            let metadata = file.metadata().map_err(|_| IssuerError::D2aTaint)?;
            if !metadata.file_type().is_file()
                || metadata.uid() != run.uid
                || metadata.permissions().mode() & 0o777 != 0o600
            {
                return Err(IssuerError::D2aTaint);
            }
            File::open(&run.run_directory)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| IssuerError::D2aTaint)?;
            validate_existing_taint(&path, run.uid, &payload)
        }
        Err(_) => Err(IssuerError::D2aTaint),
    }
}

pub fn require_d2a_taint(run: &ValidatedRun) -> Result<(), IssuerError> {
    let payload = expected_d2a_taint_payload(run)?;
    validate_existing_taint(&run.run_directory.join("d2a-taint.json"), run.uid, &payload)
}

fn expected_d2a_taint_payload(run: &ValidatedRun) -> Result<Vec<u8>, IssuerError> {
    let executable = std::env::current_exe().map_err(|_| IssuerError::D2aTaint)?;
    if !executable.is_absolute()
        || fs::canonicalize(&executable).map_err(|_| IssuerError::D2aTaint)? != executable
    {
        return Err(IssuerError::D2aTaint);
    }
    require_safe_executable(&executable, run.uid, IssuerError::D2aTaint)?;
    let marker = D2aTaintMarker {
        schema_version: 1,
        kind: "starring.d2a.run-taint.v1",
        run_id: &run.run_id,
        manifest_sha256: &run.manifest_sha256,
        certification_class: AUTOMATED_MAINTENANCE_CLASS,
        direct_auth_used: true,
        release_eligible: false,
        issuer_sha256: digest_file(&executable, 512 * 1024 * 1024, IssuerError::D2aTaint)?,
        issuer_source_sha256: issuer_source_digest(),
        runner_sha256: hex_digest(TRUSTED_RUNNER_BYTES),
        product_driver_sha256: hex_digest(TRUSTED_PRODUCT_DRIVER_BYTES),
        scenario_sha256: hex_digest(TRUSTED_SCENARIO_BYTES),
    };
    // Serialize the struct directly: field order is part of the byte-exact bootstrap
    // replay contract shared with d2a.py's insertion-ordered marker builder.
    let mut payload = serde_json::to_vec(&marker).map_err(|_| IssuerError::D2aTaint)?;
    payload.push(b'\n');
    Ok(payload)
}

pub fn issuer_source_digest() -> String {
    let mut digest = Sha256::new();
    digest.update(ISSUER_SOURCE_DIGEST_DOMAIN);
    for (name, bytes) in [
        ("Cargo.toml", ISSUER_CARGO_TOML_BYTES),
        ("Cargo.lock", ISSUER_CARGO_LOCK_BYTES),
        ("src/lib.rs", ISSUER_LIB_RS_BYTES),
        ("src/main.rs", ISSUER_MAIN_RS_BYTES),
    ] {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    hex_bytes(&digest.finalize())
}

fn validate_existing_taint(path: &Path, uid: u32, expected: &[u8]) -> Result<(), IssuerError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| IssuerError::D2aTaint)?;
    let metadata = file.metadata().map_err(|_| IssuerError::D2aTaint)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() != expected.len() as u64
    {
        return Err(IssuerError::D2aTaint);
    }
    let mut observed = Vec::with_capacity(expected.len());
    file.read_to_end(&mut observed)
        .map_err(|_| IssuerError::D2aTaint)?;
    if observed != expected {
        return Err(IssuerError::D2aTaint);
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectOnboardingEvidence {
    schema_version: u8,
    kind: String,
    certification_class: String,
    operation: String,
    observed_at: String,
    run_id: String,
    manifest_sha256: String,
    principal_id: String,
    guild_id: String,
    discord_application_id: String,
    hub_channel_id: String,
    binding_key: String,
    installation_id: String,
    outcome: String,
    provisioner_sha256: String,
    issuer_sha256: String,
    issuer_source_sha256: String,
    discord_hub_preflight: bool,
    direct_auth_used: bool,
    session_revoked: bool,
    release_eligible: bool,
}

pub fn reject_commercial_onboarding_artifacts(run: &ValidatedRun) -> Result<(), IssuerError> {
    for path in [
        run.run_directory
            .join("orchestrator/onboarding-evidence.json"),
        run.run_directory
            .join("orchestrator/coordinator-sources/step-04-onboarding.json"),
    ] {
        match path.symlink_metadata() {
            Ok(_) => return Err(IssuerError::CommercialOnboardingArtifact),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(IssuerError::CommercialOnboardingArtifact),
        }
    }
    Ok(())
}

pub fn persist_direct_onboarding_evidence(
    run: &ValidatedRun,
    outcome: &str,
    observed_at: &str,
) -> Result<Value, IssuerError> {
    reject_commercial_onboarding_artifacts(run)?;
    let issuer_sha256 = current_issuer_sha256(IssuerError::DirectOnboardingEvidence)?;
    let evidence = DirectOnboardingEvidence {
        schema_version: 1,
        kind: DIRECT_ONBOARDING_EVIDENCE_KIND.to_string(),
        certification_class: AUTOMATED_MAINTENANCE_CLASS.to_string(),
        operation: Operation::DirectOnboard.as_str().to_string(),
        observed_at: observed_at.to_string(),
        run_id: run.run_id.clone(),
        manifest_sha256: run.manifest_sha256.clone(),
        principal_id: run.principal_id.clone(),
        guild_id: run.guild_id.clone(),
        discord_application_id: run.discord_application_id.clone(),
        hub_channel_id: run.discord_hub_channel_id.clone(),
        binding_key: "community_hub".to_string(),
        installation_id: run.installation_id.clone(),
        outcome: outcome.to_string(),
        provisioner_sha256: run.sealed_provisioner_sha256.clone(),
        issuer_sha256,
        issuer_source_sha256: issuer_source_digest(),
        discord_hub_preflight: true,
        direct_auth_used: true,
        session_revoked: true,
        release_eligible: false,
    };
    validate_direct_onboarding_evidence(run, &evidence)?;
    let value =
        serde_json::to_value(&evidence).map_err(|_| IssuerError::DirectOnboardingEvidence)?;
    let mut payload =
        serde_json::to_vec(&value).map_err(|_| IssuerError::DirectOnboardingEvidence)?;
    payload.push(b'\n');
    let path = run.run_directory.join(DIRECT_ONBOARDING_EVIDENCE_NAME);
    match path.symlink_metadata() {
        Ok(_) => require_direct_onboarding_evidence(run),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)
                .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
            file.write_all(&payload)
                .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
            file.sync_all()
                .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
            let metadata = file
                .metadata()
                .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
            if !metadata.file_type().is_file()
                || metadata.uid() != run.uid
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o777 != 0o600
                || metadata.len() != payload.len() as u64
            {
                return Err(IssuerError::DirectOnboardingEvidence);
            }
            File::open(&run.run_directory)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
            require_direct_onboarding_evidence(run)
        }
        Err(_) => Err(IssuerError::DirectOnboardingEvidence),
    }
}

pub fn require_direct_onboarding_evidence(run: &ValidatedRun) -> Result<Value, IssuerError> {
    reject_commercial_onboarding_artifacts(run)?;
    let path = run.run_directory.join(DIRECT_ONBOARDING_EVIDENCE_NAME);
    require_owned_regular(&path, run.uid, 0o600, IssuerError::DirectOnboardingEvidence)?;
    let metadata = path
        .symlink_metadata()
        .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
    if metadata.nlink() != 1 {
        return Err(IssuerError::DirectOnboardingEvidence);
    }
    let (value, _canonical) =
        read_canonical_json(&path, 64 * 1024, IssuerError::DirectOnboardingEvidence)?;
    let evidence: DirectOnboardingEvidence =
        serde_json::from_value(value.clone()).map_err(|_| IssuerError::DirectOnboardingEvidence)?;
    validate_direct_onboarding_evidence(run, &evidence)?;
    Ok(value)
}

fn validate_direct_onboarding_evidence(
    run: &ValidatedRun,
    evidence: &DirectOnboardingEvidence,
) -> Result<(), IssuerError> {
    let issuer_sha256 = current_issuer_sha256(IssuerError::DirectOnboardingEvidence)?;
    let timestamp = DateTime::parse_from_rfc3339(&evidence.observed_at)
        .map_err(|_| IssuerError::DirectOnboardingEvidence)?;
    if evidence.schema_version != 1
        || evidence.kind != DIRECT_ONBOARDING_EVIDENCE_KIND
        || evidence.certification_class != AUTOMATED_MAINTENANCE_CLASS
        || evidence.operation != Operation::DirectOnboard.as_str()
        || !evidence.observed_at.ends_with('Z')
        || timestamp.offset().local_minus_utc() != 0
        || evidence.run_id != run.run_id
        || evidence.manifest_sha256 != run.manifest_sha256
        || evidence.principal_id != run.principal_id
        || evidence.guild_id != run.guild_id
        || evidence.discord_application_id != run.discord_application_id
        || evidence.hub_channel_id != run.discord_hub_channel_id
        || evidence.binding_key != "community_hub"
        || evidence.installation_id != run.installation_id
        || !matches!(evidence.outcome.as_str(), "fresh" | "exact_replay")
        || evidence.provisioner_sha256 != run.sealed_provisioner_sha256
        || evidence.issuer_sha256 != issuer_sha256
        || evidence.issuer_source_sha256 != issuer_source_digest()
        || !evidence.discord_hub_preflight
        || !evidence.direct_auth_used
        || !evidence.session_revoked
        || evidence.release_eligible
    {
        return Err(IssuerError::DirectOnboardingEvidence);
    }
    Ok(())
}

fn current_issuer_sha256(error: IssuerError) -> Result<String, IssuerError> {
    let executable = std::env::current_exe().map_err(|_| error)?;
    if !executable.is_absolute() || fs::canonicalize(&executable).map_err(|_| error)? != executable
    {
        return Err(error);
    }
    let uid = current_uid().map_err(|_| error)?;
    require_safe_executable(&executable, uid, error)?;
    digest_file(&executable, 512 * 1024 * 1024, error)
}

#[derive(Debug, Deserialize)]
struct OrchestratorState {
    schema_version: u64,
    manifest_sha256: String,
    run_id: String,
    phase: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateStartEvidence {
    api_sha256: String,
    runtime_sha256: String,
    codex_worker_sha256: String,
    d2_toolchain_sha256: String,
    certification_transport_sha256: String,
    certification_transport_source_sha256: String,
    api_build_revision: String,
    runtime_build_revision: String,
    api_ready_status: u16,
    runtime_ready_status: u16,
    worker_ready_status: u16,
    cloudflare_tunnel_id: String,
    public_origin: String,
    origin_service: String,
    transport_instance_id: String,
    transport_ready: bool,
    tunnel_ready: bool,
    process_identities: CandidateProcessIdentities,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateProcessIdentities {
    schema_version: u8,
    api: CandidateProcessIdentity,
    runtime: CandidateProcessIdentity,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateProcessIdentity {
    launchd: HistoricalLaunchdIdentity,
    process: HistoricalProcessIdentity,
    plist: HistoricalPlistIdentity,
    #[serde(default)]
    runtime_health: Option<HistoricalRuntimeHealth>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalLaunchdIdentity {
    pid: i32,
    program: String,
    plist_path: String,
    arguments: Vec<String>,
    runs: u64,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalProcessIdentity {
    pid: i32,
    start_time_seconds: u64,
    start_time_microseconds: u32,
    uid: u32,
    path: String,
    sha256: String,
    size: u64,
    mode: u32,
    device: u64,
    inode: u64,
    links: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalPlistIdentity {
    path: String,
    sha256: String,
    size: u64,
    mode: u32,
    uid: u32,
    device: u64,
    inode: u64,
    links: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalRuntimeHealth {
    schema_version: u8,
    os_pid: i32,
    process_instance_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscordOwnershipRegistry {
    kind: String,
    owners: Vec<DiscordOwnershipRecord>,
    schema_version: u8,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DiscordOwnershipRecord {
    application_id: String,
    bot_user_id: String,
    guild_id: String,
    manifest_path: String,
    manifest_sha256: String,
    run_id: String,
}

pub fn reject_ambient_postgres_environment() -> Result<(), IssuerError> {
    for (name, _value) in std::env::vars_os() {
        let name = name.as_bytes();
        if name == b"DATABASE_URL" || name.starts_with(b"PG") {
            return Err(IssuerError::AmbientPostgresEnvironment);
        }
    }
    Ok(())
}

pub fn load_validated_run(manifest_path: &Path) -> Result<ValidatedRun, IssuerError> {
    if !manifest_path.is_absolute()
        || manifest_path.file_name().and_then(|v| v.to_str()) != Some("manifest.json")
    {
        return Err(IssuerError::ManifestPath);
    }
    let uid = current_uid()?;
    let home = current_home(uid)?;
    let run_directory = manifest_path
        .parent()
        .ok_or(IssuerError::ManifestPath)?
        .to_path_buf();
    let run_id_from_path = run_directory
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(IssuerError::ManifestPath)?;
    if !valid_run_id(run_id_from_path) {
        return Err(IssuerError::ManifestPath);
    }
    let expected = home
        .join("Library/Application Support/Starring/release-certifications")
        .join(run_id_from_path)
        .join("manifest.json");
    if manifest_path != expected
        || fs::canonicalize(manifest_path).map_err(|_| IssuerError::ManifestPath)? != expected
    {
        return Err(IssuerError::ManifestPath);
    }
    require_owned_directory(
        expected.parent().ok_or(IssuerError::ManifestPath)?,
        uid,
        0o700,
        IssuerError::ManifestPermissions,
    )?;
    require_owned_regular(manifest_path, uid, 0o600, IssuerError::ManifestPermissions)?;
    let (value, canonical) =
        read_canonical_json(manifest_path, MAX_MANIFEST_BYTES, IssuerError::Manifest)?;
    let digest = hex_digest(&canonical);
    let digest_path = run_directory.join("manifest.sha256");
    require_owned_regular(&digest_path, uid, 0o600, IssuerError::ManifestPermissions)?;
    let persisted_digest = fs::read(&digest_path).map_err(|_| IssuerError::ManifestDigest)?;
    if persisted_digest.len() != 65
        || persisted_digest[64] != b'\n'
        || persisted_digest[..64] != digest.as_bytes()[..]
    {
        return Err(IssuerError::ManifestDigest);
    }
    let manifest: Manifest = serde_json::from_value(value).map_err(|_| IssuerError::Manifest)?;
    validate_manifest_contract(&manifest, &run_directory)?;

    let suffix = &manifest.run_id[manifest.run_id.len() - 12..];
    let isolated_root = PathBuf::from(format!("/private/tmp/starring-d2-{}", manifest.run_id));
    let expected_labels = expected_service_labels(suffix);
    let labels = expected_labels.values().cloned().collect::<Vec<_>>();
    let candidate_services = build_candidate_service_bindings(
        &manifest,
        &run_directory,
        &isolated_root,
        &expected_labels,
        &home,
    )?;
    let all_candidates = manifest
        .candidates
        .named_all()
        .into_iter()
        .map(|(name, file)| CandidateBinding {
            name,
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            expected_mode: if name == "codex_worker" { 0o444 } else { 0o555 },
        })
        .collect();
    let run = ValidatedRun {
        manifest_path: manifest_path.to_path_buf(),
        run_directory: run_directory.clone(),
        run_id: manifest.run_id.clone(),
        commit_sha: manifest.commit_sha.clone(),
        manifest_sha256: digest,
        isolated_root,
        socket_directory: manifest.database.socket_directory.clone(),
        cluster_root: manifest.database.cluster_root.clone(),
        database_port: manifest.database.port,
        api_port: manifest.services["api"]
            .port
            .ok_or(IssuerError::ManifestContract)?,
        api_keychain_service: manifest.keychain_services.api.clone(),
        actor_id: manifest.discord.actor_id.clone(),
        principal_id: format!("discord:{}", manifest.discord.actor_id),
        guild_id: manifest.discord.guild_id.clone(),
        installation_id: format!("installation:{}", manifest.discord.resource_prefix),
        public_origin: manifest.public_origin.clone(),
        discord_application_id: manifest.discord.application_id.clone(),
        discord_hub_channel_id: manifest.discord.hub_channel_id.clone(),
        sealed_provisioner_path: manifest.candidates.sealed_provisioner.path.clone(),
        sealed_provisioner_sha256: manifest.candidates.sealed_provisioner.sha256.clone(),
        candidate_node_path: manifest.candidates.node.path.clone(),
        candidate_node_sha256: manifest.candidates.node.sha256.clone(),
        service_labels: labels,
        uid,
        discord_bot_user_id: manifest.discord.bot_user_id.clone(),
        discord_bot_token_service: manifest.external_keychain.discord_bot_token.service.clone(),
        discord_bot_token_account: manifest.external_keychain.discord_bot_token.account.clone(),
        cloudflare_tunnel_id: manifest.cloudflare.tunnel_id.clone(),
        source_trees: manifest.source_trees.clone(),
        all_candidates,
        candidate_services,
    };
    Ok(run)
}

pub fn validate_locked_run(run: &ValidatedRun) -> Result<(), IssuerError> {
    validate_active_state(run)?;
    validate_discord_ownership(run)?;
    validate_isolated_runtime(run)?;
    validate_candidate_services(run)
}

pub fn validate_manifest_contract(
    manifest: &Manifest,
    run_directory: &Path,
) -> Result<(), IssuerError> {
    if manifest.schema_version != 1
        || !valid_run_id(&manifest.run_id)
        || run_directory.file_name().and_then(|value| value.to_str()) != Some(&manifest.run_id)
        || manifest.public_origin != D2_PUBLIC_ORIGIN
        || manifest.certification_class != "commercial_human_v1"
        || !manifest
            .human_boundaries
            .iter()
            .map(String::as_str)
            .eq(D2_HUMAN_BOUNDARIES)
    {
        return Err(IssuerError::ManifestContract);
    }
    let suffix = &manifest.run_id[manifest.run_id.len() - 12..];
    let date = &manifest.run_id[3..11];
    let isolated_root = PathBuf::from(format!("/private/tmp/starring-d2-{}", manifest.run_id));
    if manifest.database.name != DATABASE_NAME
        || manifest.database.cluster_root != isolated_root.join("postgres")
        || manifest.database.socket_directory != isolated_root.join("socket")
    {
        return Err(IssuerError::ManifestContract);
    }
    let expected_keychain = BTreeMap::from([
        ("api", format!("starring.d2.{suffix}.api")),
        ("postgres", format!("starring.d2.{suffix}.postgres")),
        ("runtime", format!("starring.d2.{suffix}.runtime")),
        ("worker", format!("starring.d2.{suffix}.worker")),
    ]);
    let actual_keychain = BTreeMap::from([
        ("api", manifest.keychain_services.api.clone()),
        ("postgres", manifest.keychain_services.postgres.clone()),
        ("runtime", manifest.keychain_services.runtime.clone()),
        ("worker", manifest.keychain_services.worker.clone()),
    ]);
    if actual_keychain != expected_keychain
        || actual_keychain
            .values()
            .any(|service| PROTECTED_KEYCHAIN_SERVICES.contains(&service.as_str()))
    {
        return Err(IssuerError::ManifestContract);
    }
    let expected_protected_labels = [
        "local.starring.api.staging",
        "local.starring.codex-worker",
        "local.starring.runtime.staging",
        "local.cloudflared.starring",
    ];
    if manifest.protected_staging.mutation_allowed
        || manifest.protected_staging.database != "starring_runtime_staging@127.0.0.1:5432"
        || !manifest
            .protected_staging
            .launchd_labels
            .iter()
            .map(String::as_str)
            .eq(expected_protected_labels)
    {
        return Err(IssuerError::ManifestContract);
    }
    let expected_labels = expected_service_labels(suffix);
    if manifest.services.len() != expected_labels.len()
        || manifest.services.iter().any(|(name, service)| {
            expected_labels.get(name) != Some(&service.label)
                || manifest
                    .protected_staging
                    .launchd_labels
                    .contains(&service.label)
        })
    {
        return Err(IssuerError::ManifestContract);
    }
    let api_port = manifest
        .services
        .get("api")
        .and_then(|service| service.port);
    let runtime_port = manifest
        .services
        .get("runtime")
        .and_then(|service| service.port);
    let worker_port = manifest
        .services
        .get("worker")
        .and_then(|service| service.port);
    let gateway_port = manifest
        .services
        .get("transport")
        .and_then(|service| service.gateway_port);
    let transport_http_port = manifest
        .services
        .get("transport")
        .and_then(|service| service.http_port);
    let ports = [
        Some(manifest.database.port),
        api_port,
        runtime_port,
        worker_port,
        gateway_port,
        transport_http_port,
    ];
    let protected_ports = [5_432_u16, 18_080, 18_181, 19_091];
    let unique_ports = ports.iter().copied().flatten().collect::<BTreeSet<_>>();
    let ports_valid = manifest.services.get("api").is_some_and(|service| {
        service.port == Some(28_080)
            && service.gateway_port.is_none()
            && service.http_port.is_none()
    }) && manifest.services.get("runtime").is_some_and(|service| {
        service.port.is_some() && service.gateway_port.is_none() && service.http_port.is_none()
    }) && manifest.services.get("worker").is_some_and(|service| {
        service.port.is_some() && service.gateway_port.is_none() && service.http_port.is_none()
    }) && manifest.services.get("transport").is_some_and(|service| {
        service.port.is_none() && service.gateway_port.is_some() && service.http_port.is_some()
    }) && manifest.services.get("tunnel").is_some_and(|service| {
        service.port.is_none() && service.gateway_port.is_none() && service.http_port.is_none()
    }) && ports.iter().all(Option::is_some)
        && unique_ports.len() == ports.len()
        && unique_ports
            .iter()
            .all(|port| *port >= 1_024 && !protected_ports.contains(port));
    if !ports_valid
        || !valid_snowflake(&manifest.discord.actor_id)
        || !valid_snowflake(&manifest.discord.application_id)
        || !valid_snowflake(&manifest.discord.bot_user_id)
        || !valid_snowflake(&manifest.discord.guild_id)
        || !valid_snowflake(&manifest.discord.hub_channel_id)
        || manifest.discord.application_id != manifest.discord.bot_user_id
        || !manifest.discord.disposable_guild_required
        || manifest.discord.resource_prefix != format!("starring-d2-{date}-{suffix}")
        || manifest
            .candidates
            .all()
            .iter()
            .any(|candidate| !valid_digest(&candidate.sha256) || !candidate.path.is_absolute())
        || !manifest.authoring.is_object()
        || manifest.cloudflare.public_origin != manifest.public_origin
        || manifest.cloudflare.origin_service
            != format!("http://127.0.0.1:{}", api_port.unwrap_or_default())
        || manifest.cloudflare.tunnel_id != D2_CLOUDFLARE_TUNNEL_ID
        || manifest.commit_sha.len() != 40
        || !manifest
            .commit_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || manifest.created_at.is_empty()
        || manifest.expected_steps.is_empty()
        || !valid_external_keychain(&manifest.external_keychain, &manifest.keychain_services)
        || !valid_source_trees(&manifest.source_trees, &manifest.candidates)
    {
        return Err(IssuerError::ManifestContract);
    }
    Ok(())
}

fn valid_external_keychain(value: &ExternalKeychainManifest, run_owned: &KeychainServices) -> bool {
    let identities = [
        &value.discord_oauth_client_secret,
        &value.discord_bot_token,
        &value.tunnel_token,
    ];
    let run_owned_services = BTreeSet::from([
        run_owned.api.as_str(),
        run_owned.postgres.as_str(),
        run_owned.runtime.as_str(),
        run_owned.worker.as_str(),
    ]);
    let standing_discord_identities = BTreeSet::from([
        ("starring-api.staging", "discord.oauth-client-secret"),
        ("starring-api.staging", "discord.bot-token"),
        ("starring.runtime.staging", "discord.bot-token"),
    ]);
    identities.iter().all(|identity| {
        valid_identity_component(&identity.service)
            && valid_identity_component(&identity.account)
            && !PROTECTED_KEYCHAIN_SERVICES.contains(&identity.service.as_str())
            && !run_owned_services.contains(identity.service.as_str())
            && !standing_discord_identities
                .contains(&(identity.service.as_str(), identity.account.as_str()))
    }) && identities
        .iter()
        .map(|identity| (&identity.service, &identity.account))
        .collect::<BTreeSet<_>>()
        .len()
        == identities.len()
}

fn valid_source_trees(value: &SourceTreesManifest, candidates: &CandidateManifest) -> bool {
    fn valid_tree(tree: &SourceTreeManifest, expected_files: &[&str]) -> bool {
        tree.root.is_absolute()
            && valid_digest(&tree.sha256)
            && tree
                .files
                .iter()
                .map(String::as_str)
                .eq(expected_files.iter().copied())
            && tree.files.iter().all(|file| {
                !file.is_empty()
                    && file.len() <= 256
                    && !file.starts_with('/')
                    && !file
                        .split('/')
                        .any(|component| component.is_empty() || component == "..")
            })
            && tree.files.iter().collect::<BTreeSet<_>>().len() == tree.files.len()
    }
    valid_tree(&value.codex_worker, &CODEX_WORKER_SOURCE_FILES)
        && valid_tree(&value.d2_toolchain, &D2_TOOLCHAIN_SOURCE_FILES)
        && valid_tree(
            &value.certification_transport,
            &CERTIFICATION_TRANSPORT_SOURCE_FILES,
        )
        && candidates.codex_worker.path.parent() == Some(value.codex_worker.root.as_path())
        && value
            .d2_toolchain
            .root
            .file_name()
            .and_then(|name| name.to_str())
            == Some("d2-certification")
        && value
            .certification_transport
            .root
            .file_name()
            .and_then(|name| name.to_str())
            == Some("d2-certification-transport")
        && value.d2_toolchain.root.parent() == value.certification_transport.root.parent()
}

fn valid_identity_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 192
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl CandidateManifest {
    fn all(&self) -> [&CandidateFile; 9] {
        [
            &self.api,
            &self.runtime,
            &self.codex_worker,
            &self.codex,
            &self.db_bootstrap,
            &self.sealed_provisioner,
            &self.certification_transport,
            &self.node,
            &self.cloudflared,
        ]
    }

    fn named_all(&self) -> [(&'static str, &CandidateFile); 9] {
        [
            ("api", &self.api),
            ("runtime", &self.runtime),
            ("codex_worker", &self.codex_worker),
            ("codex", &self.codex),
            ("db_bootstrap", &self.db_bootstrap),
            ("sealed_provisioner", &self.sealed_provisioner),
            ("certification_transport", &self.certification_transport),
            ("node", &self.node),
            ("cloudflared", &self.cloudflared),
        ]
    }
}

fn environment_base(home: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".to_string(), home.to_string()),
        (
            "PATH".to_string(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        ),
    ])
}

fn keychain_reference(service: &str, account: &str) -> String {
    format!("keychain:{service}:{account}")
}

fn manifest_service_port(manifest: &Manifest, name: &str) -> Result<u16, IssuerError> {
    manifest
        .services
        .get(name)
        .and_then(|service| service.port)
        .ok_or(IssuerError::ManifestContract)
}

fn api_environment(
    manifest: &Manifest,
    home: &str,
) -> Result<BTreeMap<String, String>, IssuerError> {
    let mut environment = environment_base(home);
    let oauth = &manifest.external_keychain.discord_oauth_client_secret;
    let bot = &manifest.external_keychain.discord_bot_token;
    environment.extend([
        (
            "STARRING_API_BIND_PORT".to_string(),
            manifest_service_port(manifest, "api")?.to_string(),
        ),
        (
            "STARRING_API_PUBLIC_ORIGIN".to_string(),
            manifest.public_origin.clone(),
        ),
        (
            "STARRING_API_OAUTH_RETURN_PATHS_JSON".to_string(),
            "[\"/v1/me\"]".to_string(),
        ),
        (
            "STARRING_API_OAUTH_DEFAULT_RETURN_PATH".to_string(),
            "/v1/me".to_string(),
        ),
        (
            "STARRING_API_DATABASE_MAX_CONNECTIONS".to_string(),
            "2".to_string(),
        ),
        (
            "STARRING_API_DATABASE_ACQUIRE_TIMEOUT_MILLISECONDS".to_string(),
            "2000".to_string(),
        ),
        (
            "STARRING_API_DATABASE_IDLE_TIMEOUT_SECONDS".to_string(),
            "120".to_string(),
        ),
        (
            "STARRING_API_DATABASE_MAX_LIFETIME_SECONDS".to_string(),
            "900".to_string(),
        ),
        (
            "STARRING_API_DISCORD_APPLICATION_ID".to_string(),
            manifest.discord.application_id.clone(),
        ),
        (
            "STARRING_API_DISCORD_BOT_USER_ID".to_string(),
            manifest.discord.bot_user_id.clone(),
        ),
        (
            "STARRING_API_DISCORD_REQUEST_TIMEOUT_MILLISECONDS".to_string(),
            "3000".to_string(),
        ),
        (
            "STARRING_API_DISCORD_WRITE_AUTHORITY_LIFETIME_MILLISECONDS".to_string(),
            "3000".to_string(),
        ),
        (
            "STARRING_API_DISCORD_READ_AUTHORITY_LIFETIME_MILLISECONDS".to_string(),
            "15000".to_string(),
        ),
        (
            "STARRING_API_AUTHORING_WORKER_URL".to_string(),
            format!(
                "http://127.0.0.1:{}",
                manifest_service_port(manifest, "worker")?
            ),
        ),
        (
            "STARRING_API_AUTHORING_WORKER_TOKEN_SECRET_REFERENCE".to_string(),
            keychain_reference(&manifest.keychain_services.worker, "authoring.bearer-token"),
        ),
        (
            "STARRING_API_DISCORD_OAUTH_CLIENT_SECRET_REFERENCE".to_string(),
            keychain_reference(&oauth.service, &oauth.account),
        ),
        (
            "STARRING_API_DISCORD_BOT_TOKEN_REFERENCE".to_string(),
            keychain_reference(&bot.service, &bot.account),
        ),
        (
            "STARRING_API_PRODUCT_ACTION_KEYRING_SECRET_REFERENCE".to_string(),
            keychain_reference(&manifest.keychain_services.api, "keyring.product-action"),
        ),
        (
            "STARRING_API_SNAPSHOT_ENVELOPE_KEYRING_SECRET_REFERENCE".to_string(),
            keychain_reference(&manifest.keychain_services.api, "keyring.snapshot-envelope"),
        ),
    ]);
    for (account, variable) in [
        (
            "database.oauth-flow-writer",
            "STARRING_API_OAUTH_FLOW_WRITER_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.session-issuer",
            "STARRING_API_SESSION_ISSUER_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.session-api",
            "STARRING_API_SESSION_API_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.security-revoker",
            "STARRING_API_SECURITY_REVOKER_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.installation-authority-reader",
            "STARRING_API_INSTALLATION_AUTHORITY_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.authorized-snapshot-reader",
            "STARRING_API_AUTHORIZED_SNAPSHOT_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.promotion-executor",
            "STARRING_API_PROMOTION_EXECUTOR_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.decision-reader",
            "STARRING_API_DECISION_READER_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.approval-executor",
            "STARRING_API_APPROVAL_EXECUTOR_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.rejection-executor",
            "STARRING_API_REJECTION_EXECUTOR_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.apply-executor",
            "STARRING_API_APPLY_EXECUTOR_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.cancellation-executor",
            "STARRING_API_CANCELLATION_EXECUTOR_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.deployment-status-reader",
            "STARRING_API_DEPLOYMENT_STATUS_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.operational-deployment-status-reader",
            "STARRING_API_OPERATIONAL_STATUS_DATABASE_SECRET_REFERENCE",
        ),
        (
            "database.authoring-session-writer",
            "STARRING_API_AUTHORING_SESSION_WRITER_DATABASE_SECRET_REFERENCE",
        ),
    ] {
        environment.insert(
            variable.to_string(),
            keychain_reference(&manifest.keychain_services.api, account),
        );
    }
    Ok(environment)
}

fn runtime_environment(
    manifest: &Manifest,
    home: &str,
) -> Result<BTreeMap<String, String>, IssuerError> {
    let mut environment = environment_base(home);
    let transport = manifest
        .services
        .get("transport")
        .ok_or(IssuerError::ManifestContract)?;
    let bot = &manifest.external_keychain.discord_bot_token;
    environment.extend([
        (
            "STARRING_RUNTIME_HEALTH_BIND_ADDRESS".to_string(),
            format!("127.0.0.1:{}", manifest_service_port(manifest, "runtime")?),
        ),
        (
            "STARRING_RUNTIME_DATABASE_MAX_CONNECTIONS".to_string(),
            "2".to_string(),
        ),
        (
            "STARRING_RUNTIME_DATABASE_ACQUIRE_TIMEOUT_MILLISECONDS".to_string(),
            "2000".to_string(),
        ),
        (
            "STARRING_RUNTIME_DATABASE_IDLE_TIMEOUT_SECONDS".to_string(),
            "60".to_string(),
        ),
        (
            "STARRING_RUNTIME_DATABASE_MAX_LIFETIME_SECONDS".to_string(),
            "600".to_string(),
        ),
        (
            "STARRING_RUNTIME_DATABASE_LOCK_TIMEOUT_MILLISECONDS".to_string(),
            "1000".to_string(),
        ),
        (
            "STARRING_RUNTIME_DATABASE_STATEMENT_TIMEOUT_MILLISECONDS".to_string(),
            "2000".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_COMMAND_CAPACITY".to_string(),
            "8".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_LIFECYCLE_CAPACITY".to_string(),
            "64".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_REJECTION_ACKNOWLEDGEMENT_CAPACITY".to_string(),
            "64".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_GLOBAL_ADMISSION_CAPACITY".to_string(),
            "256".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_OWNER_LEASE_MILLISECONDS".to_string(),
            "60000".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_OWNER_RENEW_BEFORE_MILLISECONDS".to_string(),
            "40000".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_OWNER_SAFETY_MARGIN_MILLISECONDS".to_string(),
            "5000".to_string(),
        ),
        (
            "STARRING_RUNTIME_GATEWAY_DRAIN_TIMEOUT_SECONDS".to_string(),
            "15".to_string(),
        ),
        (
            "STARRING_RUNTIME_INSTANCE_LOOKUP_TIMEOUT_MILLISECONDS".to_string(),
            "500".to_string(),
        ),
        (
            "STARRING_RUNTIME_DISCORD_TRANSPORT_MODE".to_string(),
            "loopback_proxy_v1".to_string(),
        ),
        (
            "STARRING_RUNTIME_DISCORD_GATEWAY_PROXY_URL".to_string(),
            format!(
                "ws://127.0.0.1:{}",
                transport
                    .gateway_port
                    .ok_or(IssuerError::ManifestContract)?
            ),
        ),
        (
            "STARRING_RUNTIME_DISCORD_EFFECT_HTTP_PROXY_AUTHORITY".to_string(),
            format!(
                "127.0.0.1:{}",
                transport.http_port.ok_or(IssuerError::ManifestContract)?
            ),
        ),
        (
            "STARRING_RUNTIME_DISCORD_BOT_TOKEN_SECRET_REFERENCE".to_string(),
            keychain_reference(&bot.service, &bot.account),
        ),
        (
            "STARRING_RUNTIME_INTERACTION_TOKEN_ENVELOPE_KEYRING_SECRET_REFERENCE".to_string(),
            keychain_reference(
                &manifest.keychain_services.runtime,
                "interaction.token-envelope-keyring",
            ),
        ),
    ]);
    for (account, variable) in [
        (
            "database.execution",
            "STARRING_RUNTIME_CONVERGENCE_DATABASE_URL_SECRET_REFERENCE",
        ),
        (
            "database.exact-target",
            "STARRING_RUNTIME_EXACT_TARGET_DATABASE_URL_SECRET_REFERENCE",
        ),
        (
            "database.panel",
            "STARRING_RUNTIME_PANEL_DATABASE_URL_SECRET_REFERENCE",
        ),
        (
            "database.serving",
            "STARRING_RUNTIME_SERVING_DATABASE_URL_SECRET_REFERENCE",
        ),
        (
            "database.interaction",
            "STARRING_RUNTIME_INTERACTION_DATABASE_URL_SECRET_REFERENCE",
        ),
    ] {
        environment.insert(
            variable.to_string(),
            keychain_reference(&manifest.keychain_services.runtime, account),
        );
    }
    Ok(environment)
}

fn worker_environment(
    manifest: &Manifest,
    home: &str,
    log_root: &Path,
) -> Result<BTreeMap<String, String>, IssuerError> {
    let mut environment = environment_base(home);
    environment.extend([
        ("NODE_ENV".to_string(), "production".to_string()),
        (
            "PATH".to_string(),
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        ),
        (
            "STARRING_CODEX_PATH".to_string(),
            manifest
                .candidates
                .codex
                .path
                .to_str()
                .ok_or(IssuerError::ManifestContract)?
                .to_string(),
        ),
        (
            "STARRING_CODEX_WORKER_CONCURRENCY".to_string(),
            "1".to_string(),
        ),
        (
            "STARRING_CODEX_WORKER_KEYCHAIN_SERVICE".to_string(),
            manifest.keychain_services.worker.clone(),
        ),
        (
            "STARRING_CODEX_WORKER_KEYCHAIN_ACCOUNT".to_string(),
            "authoring.bearer-token".to_string(),
        ),
        (
            "STARRING_CODEX_WORKER_MAX_QUEUE".to_string(),
            "4".to_string(),
        ),
        (
            "STARRING_CODEX_WORKER_METRICS_LOG".to_string(),
            log_root
                .join("worker-metrics.jsonl")
                .to_str()
                .ok_or(IssuerError::ManifestContract)?
                .to_string(),
        ),
        (
            "STARRING_CODEX_WORKER_PORT".to_string(),
            manifest_service_port(manifest, "worker")?.to_string(),
        ),
        (
            "STARRING_CODEX_WORKER_TIMEOUT_MS".to_string(),
            "55000".to_string(),
        ),
    ]);
    Ok(environment)
}

fn tunnel_environment(
    manifest: &Manifest,
    home: &str,
) -> Result<BTreeMap<String, String>, IssuerError> {
    let mut environment = environment_base(home);
    let identity = &manifest.external_keychain.tunnel_token;
    environment.extend([
        (
            "STARRING_D2_CLOUDFLARED_PATH".to_string(),
            manifest
                .candidates
                .cloudflared
                .path
                .to_str()
                .ok_or(IssuerError::ManifestContract)?
                .to_string(),
        ),
        (
            "STARRING_D2_CLOUDFLARE_TUNNEL_ID".to_string(),
            manifest.cloudflare.tunnel_id.clone(),
        ),
        (
            "STARRING_D2_CLOUDFLARE_ORIGIN_SERVICE".to_string(),
            manifest.cloudflare.origin_service.clone(),
        ),
        (
            "STARRING_D2_TUNNEL_KEYCHAIN_SERVICE".to_string(),
            identity.service.clone(),
        ),
        (
            "STARRING_D2_TUNNEL_KEYCHAIN_ACCOUNT".to_string(),
            identity.account.clone(),
        ),
    ]);
    Ok(environment)
}

fn launchd_plist_value(
    label: &str,
    program_arguments: &[String],
    environment: BTreeMap<String, String>,
    working_directory: &str,
    log_path: &str,
) -> Value {
    serde_json::json!({
        "Label": label,
        "ProgramArguments": program_arguments,
        "EnvironmentVariables": environment,
        "WorkingDirectory": working_directory,
        "RunAtLoad": false,
        "KeepAlive": {"SuccessfulExit": false},
        "ProcessType": "Standard",
        "ThrottleInterval": 30,
        "ExitTimeOut": 90,
        "Umask": 63,
        "StandardOutPath": log_path,
        "StandardErrorPath": log_path,
        "SoftResourceLimits": {"NumberOfFiles": 2048},
        "HardResourceLimits": {"NumberOfFiles": 4096}
    })
}

fn build_candidate_service_bindings(
    manifest: &Manifest,
    run_directory: &Path,
    isolated_root: &Path,
    labels: &BTreeMap<String, String>,
    home: &Path,
) -> Result<Vec<CandidateServiceBinding>, IssuerError> {
    fn text(path: &Path) -> Result<String, IssuerError> {
        path.to_str()
            .map(str::to_owned)
            .ok_or(IssuerError::ManifestContract)
    }
    fn candidate(name: &'static str, file: &CandidateFile, expected_mode: u32) -> CandidateBinding {
        CandidateBinding {
            name,
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            expected_mode,
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn binding(
        name: &'static str,
        labels: &BTreeMap<String, String>,
        run_directory: &Path,
        configured_program: String,
        arguments: Vec<String>,
        process_arguments: Option<Vec<String>>,
        process_candidate: CandidateBinding,
        supporting_candidates: Vec<CandidateBinding>,
        environment: BTreeMap<String, String>,
        working_directory: String,
        log_path: String,
    ) -> Result<CandidateServiceBinding, IssuerError> {
        let label = labels
            .get(name)
            .cloned()
            .ok_or(IssuerError::ManifestContract)?;
        let expected_plist = launchd_plist_value(
            &label,
            &arguments,
            environment.clone(),
            &working_directory,
            &log_path,
        );
        Ok(CandidateServiceBinding {
            name,
            plist_path: run_directory
                .join("orchestrator/launchd")
                .join(format!("{label}.plist")),
            label,
            configured_program,
            process_arguments: process_arguments.unwrap_or_else(|| arguments.clone()),
            arguments,
            process_candidate,
            supporting_candidates,
            environment,
            working_directory,
            log_path,
            expected_plist,
        })
    }

    let candidates = &manifest.candidates;
    let api = text(&candidates.api.path)?;
    let runtime = text(&candidates.runtime.path)?;
    let node = text(&candidates.node.path)?;
    let worker = text(&candidates.codex_worker.path)?;
    let transport = text(&candidates.certification_transport.path)?;
    let cloudflared = text(&candidates.cloudflared.path)?;
    let root = text(isolated_root)?;
    let home = text(home)?;
    let repository_root = candidates
        .codex_worker
        .path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or(IssuerError::ManifestContract)
        .and_then(text)?;
    let log_root = isolated_root.join("logs");
    let tunnel_runner = text(&run_directory.join("orchestrator/run-tunnel.zsh"))?;
    let transport_service = manifest
        .services
        .get("transport")
        .ok_or(IssuerError::ManifestContract)?;
    Ok(vec![
        binding(
            "api",
            labels,
            run_directory,
            api.clone(),
            vec![api],
            None,
            candidate("api", &candidates.api, 0o555),
            Vec::new(),
            api_environment(manifest, &home)?,
            home.clone(),
            text(&log_root.join("api.log"))?,
        )?,
        binding(
            "runtime",
            labels,
            run_directory,
            runtime.clone(),
            vec![runtime],
            None,
            candidate("runtime", &candidates.runtime, 0o555),
            Vec::new(),
            runtime_environment(manifest, &home)?,
            repository_root.clone(),
            text(&log_root.join("runtime.log"))?,
        )?,
        binding(
            "worker",
            labels,
            run_directory,
            node.clone(),
            vec![node, worker],
            None,
            candidate("node", &candidates.node, 0o555),
            vec![candidate("codex_worker", &candidates.codex_worker, 0o444)],
            worker_environment(manifest, &home, &log_root)?,
            repository_root,
            text(&log_root.join("worker.log"))?,
        )?,
        binding(
            "transport",
            labels,
            run_directory,
            transport.clone(),
            vec![
                transport,
                "--root".to_string(),
                root,
                "--run-id".to_string(),
                manifest.run_id.clone(),
                "--guild-id".to_string(),
                manifest.discord.guild_id.clone(),
                "--hub-channel-id".to_string(),
                manifest.discord.hub_channel_id.clone(),
                "--actor-id".to_string(),
                manifest.discord.actor_id.clone(),
                "--bot-user-id".to_string(),
                manifest.discord.bot_user_id.clone(),
                "--gateway-listen".to_string(),
                format!(
                    "127.0.0.1:{}",
                    transport_service
                        .gateway_port
                        .ok_or(IssuerError::ManifestContract)?
                ),
                "--http-listen".to_string(),
                format!(
                    "127.0.0.1:{}",
                    transport_service
                        .http_port
                        .ok_or(IssuerError::ManifestContract)?
                ),
            ],
            None,
            candidate(
                "certification_transport",
                &candidates.certification_transport,
                0o555,
            ),
            Vec::new(),
            environment_base(&home),
            text(isolated_root)?,
            text(&log_root.join("transport.log"))?,
        )?,
        binding(
            "tunnel",
            labels,
            run_directory,
            "/bin/zsh".to_string(),
            vec!["/bin/zsh".to_string(), tunnel_runner],
            Some(vec![
                cloudflared,
                "tunnel".to_string(),
                "--no-autoupdate".to_string(),
                "--loglevel".to_string(),
                "warn".to_string(),
                "--transport-loglevel".to_string(),
                "warn".to_string(),
                "run".to_string(),
                "--url".to_string(),
                manifest.cloudflare.origin_service.clone(),
                manifest.cloudflare.tunnel_id.clone(),
            ]),
            candidate("cloudflared", &candidates.cloudflared, 0o555),
            Vec::new(),
            tunnel_environment(manifest, &home)?,
            home,
            text(&log_root.join("tunnel.log"))?,
        )?,
    ])
}

fn expected_service_labels(suffix: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("api".to_string(), format!("local.starring.d2.{suffix}.api")),
        (
            "runtime".to_string(),
            format!("local.starring.d2.{suffix}.runtime"),
        ),
        (
            "transport".to_string(),
            format!("local.starring.d2.{suffix}.transport"),
        ),
        (
            "tunnel".to_string(),
            format!("local.starring.d2.{suffix}.tunnel"),
        ),
        (
            "worker".to_string(),
            format!("local.starring.d2.{suffix}.worker"),
        ),
    ])
}

fn validate_active_state(run: &ValidatedRun) -> Result<(), IssuerError> {
    let artifact_directory = run.run_directory.join("orchestrator");
    require_owned_directory(
        &artifact_directory,
        run.uid,
        0o700,
        IssuerError::OrchestratorState,
    )?;
    let state_path = artifact_directory.join("state.json");
    require_owned_regular(&state_path, run.uid, 0o600, IssuerError::OrchestratorState)?;
    let (value, _canonical) =
        read_canonical_json(&state_path, 65_536, IssuerError::OrchestratorState)?;
    let state: OrchestratorState =
        serde_json::from_value(value).map_err(|_| IssuerError::OrchestratorState)?;
    if state.schema_version != 1
        || state.manifest_sha256 != run.manifest_sha256
        || state.run_id != run.run_id
        || state.phase != "candidate_started"
    {
        return Err(IssuerError::OrchestratorNotActive);
    }
    let retirement = artifact_directory.join("candidate-start-retirement.json");
    if retirement.exists() || retirement.symlink_metadata().is_ok() {
        return Err(IssuerError::OrchestratorNotActive);
    }
    Ok(())
}

fn validate_discord_ownership(run: &ValidatedRun) -> Result<(), IssuerError> {
    let path = Path::new(DISCORD_OWNERSHIP_REGISTRY_PATH);
    require_owned_regular(path, run.uid, 0o600, IssuerError::DiscordOwnership)?;
    let bytes = stable_source_file_bytes(path, run.uid, 1024 * 1024)
        .map_err(|_| IssuerError::DiscordOwnership)?;
    let payload = bytes
        .strip_suffix(b"\n")
        .filter(|payload| !payload.ends_with(b"\n") && !payload.ends_with(b"\r"))
        .ok_or(IssuerError::DiscordOwnership)?;
    let value: Value =
        serde_json::from_slice(payload).map_err(|_| IssuerError::DiscordOwnership)?;
    if serde_json::to_vec(&value).map_err(|_| IssuerError::DiscordOwnership)? != payload {
        return Err(IssuerError::DiscordOwnership);
    }
    let registry: DiscordOwnershipRegistry =
        serde_json::from_value(value).map_err(|_| IssuerError::DiscordOwnership)?;
    if registry.schema_version != 1
        || registry.kind != "starring.d2.discord-ownership-registry.v1"
        || registry
            .owners
            .windows(2)
            .any(|owners| owners[0].run_id >= owners[1].run_id)
    {
        return Err(IssuerError::DiscordOwnership);
    }
    let mut run_ids = BTreeSet::new();
    let mut manifest_digests = BTreeSet::new();
    let mut manifest_paths = BTreeSet::new();
    let mut guild_ids = BTreeSet::new();
    let mut application_ids = BTreeSet::new();
    for owner in &registry.owners {
        if !valid_run_id(&owner.run_id)
            || !valid_digest(&owner.manifest_sha256)
            || !Path::new(&owner.manifest_path).is_absolute()
            || !valid_snowflake(&owner.guild_id)
            || !valid_snowflake(&owner.application_id)
            || !valid_snowflake(&owner.bot_user_id)
            || !run_ids.insert(owner.run_id.as_str())
            || !manifest_digests.insert(owner.manifest_sha256.as_str())
            || !manifest_paths.insert(owner.manifest_path.as_str())
            || !guild_ids.insert(owner.guild_id.as_str())
            || !application_ids.insert(owner.application_id.as_str())
        {
            return Err(IssuerError::DiscordOwnership);
        }
    }
    for entry in fs::read_dir("/private/tmp").map_err(|_| IssuerError::DiscordOwnership)? {
        let entry = entry.map_err(|_| IssuerError::DiscordOwnership)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| IssuerError::DiscordOwnership)?;
        if let Some(run_id) = name.strip_prefix("starring-d2-") {
            if valid_run_id(run_id) && !run_ids.contains(run_id) {
                return Err(IssuerError::DiscordOwnership);
            }
        }
    }
    let expected_path = run
        .manifest_path
        .to_str()
        .ok_or(IssuerError::DiscordOwnership)?;
    let expected = DiscordOwnershipRecord {
        application_id: run.discord_application_id.clone(),
        bot_user_id: run.discord_bot_user_id.clone(),
        guild_id: run.guild_id.clone(),
        manifest_path: expected_path.to_string(),
        manifest_sha256: run.manifest_sha256.clone(),
        run_id: run.run_id.clone(),
    };
    if !registry_has_exact_discord_owner(&registry, &expected) {
        return Err(IssuerError::DiscordOwnership);
    }
    Ok(())
}

fn registry_has_exact_discord_owner(
    registry: &DiscordOwnershipRegistry,
    expected: &DiscordOwnershipRecord,
) -> bool {
    registry.owners.iter().any(|owner| owner == expected)
}

fn validate_isolated_runtime(run: &ValidatedRun) -> Result<(), IssuerError> {
    require_owned_directory(
        &run.isolated_root,
        run.uid,
        0o700,
        IssuerError::IsolatedRuntime,
    )?;
    require_owned_directory(
        &run.socket_directory,
        run.uid,
        0o700,
        IssuerError::IsolatedRuntime,
    )?;
    require_owned_directory(
        &run.cluster_root,
        run.uid,
        0o700,
        IssuerError::IsolatedRuntime,
    )?;
    let pg_version = run.cluster_root.join("PG_VERSION");
    let version_metadata = pg_version
        .symlink_metadata()
        .map_err(|_| IssuerError::IsolatedRuntime)?;
    if !version_metadata.file_type().is_file()
        || version_metadata.file_type().is_symlink()
        || version_metadata.uid() != run.uid
    {
        return Err(IssuerError::IsolatedRuntime);
    }
    let socket = run
        .socket_directory
        .join(format!(".s.PGSQL.{}", run.database_port));
    let socket_metadata = socket
        .symlink_metadata()
        .map_err(|_| IssuerError::IsolatedRuntime)?;
    if !socket_metadata.file_type().is_socket() || socket_metadata.uid() != run.uid {
        return Err(IssuerError::IsolatedRuntime);
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct StableFileIdentity {
    sha256: String,
    size: u64,
    mode: u32,
    device: u64,
    inode: u64,
    links: u64,
}

fn validate_current_source_trees(run: &ValidatedRun) -> Result<(), IssuerError> {
    validate_worker_source_inventory(&run.source_trees.codex_worker, run.uid)?;
    validate_transport_source_inventory(&run.source_trees.certification_transport, run.uid)?;
    for tree in [
        &run.source_trees.codex_worker,
        &run.source_trees.d2_toolchain,
        &run.source_trees.certification_transport,
    ] {
        validate_source_tree(tree, run.uid)?;
    }
    Ok(())
}

fn validate_source_tree(tree: &SourceTreeManifest, uid: u32) -> Result<(), IssuerError> {
    validate_source_directory(&tree.root, uid)?;
    if fs::canonicalize(&tree.root).map_err(|_| IssuerError::CandidateServiceInactive)? != tree.root
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    for name in &tree.files {
        let path = tree.root.join(name);
        if fs::canonicalize(&path).map_err(|_| IssuerError::CandidateServiceInactive)? != path {
            return Err(IssuerError::CandidateServiceInactive);
        }
        let mut parent = path.parent().ok_or(IssuerError::CandidateServiceInactive)?;
        loop {
            validate_source_directory(parent, uid)?;
            if parent == tree.root {
                break;
            }
            parent = parent
                .parent()
                .ok_or(IssuerError::CandidateServiceInactive)?;
        }
        let content = stable_source_file_bytes(&path, uid, 16 * 1024 * 1024)?;
        total = total
            .checked_add(content.len() as u64)
            .ok_or(IssuerError::CandidateServiceInactive)?;
        if total > 64 * 1024 * 1024 {
            return Err(IssuerError::CandidateServiceInactive);
        }
        let encoded_name = name.as_bytes();
        digest.update(encoded_name.len().to_string().as_bytes());
        digest.update(b":");
        digest.update(encoded_name);
        digest.update(b":");
        digest.update(content.len().to_string().as_bytes());
        digest.update(b":");
        digest.update(&content);
    }
    if hex_bytes(&digest.finalize()) != tree.sha256 {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

fn validate_source_directory(path: &Path, uid: u32) -> Result<(), IssuerError> {
    let metadata = path
        .symlink_metadata()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

fn stable_source_file_bytes(path: &Path, uid: u32, maximum: u64) -> Result<Vec<u8>, IssuerError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let before = file
        .metadata()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    if !before.file_type().is_file()
        || before.uid() != uid
        || before.nlink() != 1
        || before.permissions().mode() & 0o022 != 0
        || before.len() == 0
        || before.len() > maximum
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let capacity =
        usize::try_from(before.len()).map_err(|_| IssuerError::CandidateServiceInactive)?;
    let mut content = Vec::with_capacity(capacity);
    file.read_to_end(&mut content)
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let after = file
        .metadata()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let named = path
        .symlink_metadata()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    if content.len() as u64 != before.len()
        || file_identity_tuple(&before) != file_identity_tuple(&after)
        || file_identity_tuple(&after) != file_identity_tuple(&named)
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(content)
}

fn validate_worker_source_inventory(
    tree: &SourceTreeManifest,
    uid: u32,
) -> Result<(), IssuerError> {
    validate_source_directory(&tree.root, uid)?;
    let mut observed = BTreeSet::new();
    for entry in fs::read_dir(&tree.root).map_err(|_| IssuerError::CandidateServiceInactive)? {
        let entry = entry.map_err(|_| IssuerError::CandidateServiceInactive)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| IssuerError::CandidateServiceInactive)?;
        if name.ends_with(".mjs") && !name.ends_with(".test.mjs") {
            observed.insert(name);
        }
    }
    if observed
        != CODEX_WORKER_SOURCE_FILES
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

fn validate_transport_source_inventory(
    tree: &SourceTreeManifest,
    uid: u32,
) -> Result<(), IssuerError> {
    validate_source_directory(&tree.root, uid)?;
    let mut observed = BTreeSet::new();
    for entry in fs::read_dir(&tree.root).map_err(|_| IssuerError::CandidateServiceInactive)? {
        let entry = entry.map_err(|_| IssuerError::CandidateServiceInactive)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| IssuerError::CandidateServiceInactive)?;
        if name == "target" {
            continue;
        }
        if name == "src" {
            let source = entry.path();
            validate_source_directory(&source, uid)?;
            for child in fs::read_dir(&source).map_err(|_| IssuerError::CandidateServiceInactive)? {
                let child = child.map_err(|_| IssuerError::CandidateServiceInactive)?;
                let child_name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| IssuerError::CandidateServiceInactive)?;
                observed.insert(format!("src/{child_name}"));
            }
        } else {
            observed.insert(name);
        }
    }
    if observed
        != CERTIFICATION_TRANSPORT_SOURCE_FILES
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

fn validate_candidate_services(run: &ValidatedRun) -> Result<(), IssuerError> {
    validate_current_source_trees(run)?;
    let mut candidate_identities = BTreeMap::new();
    for candidate in &run.all_candidates {
        let identity = validate_immutable_candidate(candidate, run.uid)?;
        candidate_identities.insert(candidate.name, identity);
    }
    let evidence_plists = validate_candidate_start_evidence(run, &candidate_identities)?;
    let mut observed_pids = BTreeSet::new();
    for service in &run.candidate_services {
        let expected_plist = evidence_plists.get(service.name);
        validate_service_plist(service, run.uid, expected_plist)?;
        if service.name == "tunnel" {
            validate_tunnel_runner(run)?;
        }
        let first = observe_launchd_job(run.uid, &service.label)?;
        validate_launchd_binding(service, &first)?;
        if !observed_pids.insert(first.pid) {
            return Err(IssuerError::CandidateServiceInactive);
        }
        validate_process_binding(
            first.pid,
            &service.process_candidate,
            &service.process_arguments,
            run.uid,
        )?;
        for candidate in &service.supporting_candidates {
            if !candidate_identities.contains_key(candidate.name) {
                return Err(IssuerError::CandidateServiceInactive);
            }
        }
        let second = observe_launchd_job(run.uid, &service.label)?;
        if first != second {
            return Err(IssuerError::CandidateServiceInactive);
        }
        validate_process_binding(
            second.pid,
            &service.process_candidate,
            &service.process_arguments,
            run.uid,
        )?;
    }
    validate_api_loopback_ready(run)
}

fn validate_candidate_start_evidence(
    run: &ValidatedRun,
    current_files: &BTreeMap<&'static str, StableFileIdentity>,
) -> Result<BTreeMap<&'static str, HistoricalPlistIdentity>, IssuerError> {
    let path = run.run_directory.join("orchestrator/step-03-evidence.json");
    require_owned_regular(&path, run.uid, 0o600, IssuerError::CandidateServiceInactive)?;
    let (value, _canonical) =
        read_canonical_json(&path, 1024 * 1024, IssuerError::CandidateServiceInactive)?;
    let evidence: CandidateStartEvidence =
        serde_json::from_value(value).map_err(|_| IssuerError::CandidateServiceInactive)?;
    let api = candidate_binding(run, "api")?;
    let runtime = candidate_binding(run, "runtime")?;
    let transport = candidate_binding(run, "certification_transport")?;
    if evidence.api_sha256 != api.sha256
        || evidence.runtime_sha256 != runtime.sha256
        || evidence.certification_transport_sha256 != transport.sha256
        || evidence.codex_worker_sha256 != run.source_trees.codex_worker.sha256
        || evidence.d2_toolchain_sha256 != run.source_trees.d2_toolchain.sha256
        || evidence.certification_transport_source_sha256
            != run.source_trees.certification_transport.sha256
        || evidence.api_build_revision != run.commit_sha
        || evidence.runtime_build_revision != run.commit_sha
        || evidence.api_ready_status != 200
        || evidence.runtime_ready_status != 200
        || evidence.worker_ready_status != 200
        || evidence.public_origin != run.public_origin
        || evidence.origin_service != format!("http://127.0.0.1:{}", run.api_port)
        || !evidence.transport_ready
        || !evidence.tunnel_ready
        || evidence.cloudflare_tunnel_id != run.cloudflare_tunnel_id
        || !valid_transport_instance_id(&evidence.transport_instance_id)
        || evidence.process_identities.schema_version != 1
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    validate_historical_process_identity(
        run,
        "api",
        &evidence.process_identities.api,
        api,
        current_files
            .get("api")
            .ok_or(IssuerError::CandidateServiceInactive)?,
        false,
    )?;
    validate_historical_process_identity(
        run,
        "runtime",
        &evidence.process_identities.runtime,
        runtime,
        current_files
            .get("runtime")
            .ok_or(IssuerError::CandidateServiceInactive)?,
        true,
    )?;
    if evidence.process_identities.api.process.pid
        == evidence.process_identities.runtime.process.pid
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(BTreeMap::from([
        ("api", evidence.process_identities.api.plist.clone()),
        ("runtime", evidence.process_identities.runtime.plist.clone()),
    ]))
}

fn candidate_binding<'a>(
    run: &'a ValidatedRun,
    name: &str,
) -> Result<&'a CandidateBinding, IssuerError> {
    run.all_candidates
        .iter()
        .find(|candidate| candidate.name == name)
        .ok_or(IssuerError::CandidateServiceInactive)
}

fn service_binding<'a>(
    run: &'a ValidatedRun,
    name: &str,
) -> Result<&'a CandidateServiceBinding, IssuerError> {
    run.candidate_services
        .iter()
        .find(|service| service.name == name)
        .ok_or(IssuerError::CandidateServiceInactive)
}

fn validate_historical_process_identity(
    run: &ValidatedRun,
    name: &str,
    identity: &CandidateProcessIdentity,
    candidate: &CandidateBinding,
    current_file: &StableFileIdentity,
    runtime: bool,
) -> Result<(), IssuerError> {
    let service = service_binding(run, name)?;
    let path = candidate
        .path
        .to_str()
        .ok_or(IssuerError::CandidateServiceInactive)?;
    let plist_path = service
        .plist_path
        .to_str()
        .ok_or(IssuerError::CandidateServiceInactive)?;
    let launchd = &identity.launchd;
    let process = &identity.process;
    let plist = &identity.plist;
    if launchd.pid <= 0
        || launchd.program != path
        || launchd.plist_path != plist_path
        || launchd.arguments != [path]
        || launchd.runs == 0
        || launchd.state != "running"
        || process.pid != launchd.pid
        || process.start_time_seconds == 0
        || process.start_time_microseconds >= 1_000_000
        || process.uid != run.uid
        || process.path != path
        || process.sha256 != candidate.sha256
        || process.sha256 != current_file.sha256
        || process.size != current_file.size
        || process.mode != current_file.mode
        || process.device != current_file.device
        || process.inode != current_file.inode
        || process.links != current_file.links
        || plist.path != plist_path
        || !valid_digest(&plist.sha256)
        || plist.size == 0
        || plist.size > 256 * 1024
        || plist.mode != 0o600
        || plist.uid != run.uid
        || plist.inode == 0
        || plist.links != 1
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    match (runtime, identity.runtime_health.as_ref()) {
        (false, None) => {}
        (true, Some(health))
            if health.schema_version == 1
                && health.os_pid == launchd.pid
                && valid_lower_hex(&health.process_instance_id, 32) => {}
        _ => return Err(IssuerError::CandidateServiceInactive),
    }
    Ok(())
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_transport_instance_id(value: &str) -> bool {
    value
        .strip_prefix("d2ti-")
        .is_some_and(|suffix| valid_lower_hex(suffix, 32))
}

fn validate_immutable_candidate(
    candidate: &CandidateBinding,
    uid: u32,
) -> Result<StableFileIdentity, IssuerError> {
    if !candidate.path.is_absolute()
        || fs::canonicalize(&candidate.path).map_err(|_| IssuerError::CandidateServiceInactive)?
            != candidate.path
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let parent = candidate
        .path
        .parent()
        .ok_or(IssuerError::CandidateServiceInactive)?;
    let parent_metadata = parent
        .symlink_metadata()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != uid
        || parent_metadata.permissions().mode() & 0o222 != 0
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    stable_owned_file_digest(
        &candidate.path,
        uid,
        candidate.expected_mode,
        512 * 1024 * 1024,
        Some(&candidate.sha256),
        IssuerError::CandidateServiceInactive,
    )
}

fn stable_owned_file_digest(
    path: &Path,
    uid: u32,
    expected_mode: u32,
    maximum: u64,
    expected_digest: Option<&str>,
    error: IssuerError,
) -> Result<StableFileIdentity, IssuerError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| error)?;
    let before = file.metadata().map_err(|_| error)?;
    if !before.file_type().is_file()
        || before.uid() != uid
        || before.nlink() != 1
        || before.permissions().mode() & 0o777 != expected_mode
        || before.len() == 0
        || before.len() > maximum
    {
        return Err(error);
    }
    let mut digest = Sha256::new();
    let mut observed_size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| error)?;
        if read == 0 {
            break;
        }
        observed_size = observed_size.checked_add(read as u64).ok_or(error)?;
        if observed_size > maximum {
            return Err(error);
        }
        digest.update(&buffer[..read]);
    }
    let after = file.metadata().map_err(|_| error)?;
    let named = path.symlink_metadata().map_err(|_| error)?;
    if file_identity_tuple(&before) != file_identity_tuple(&after)
        || file_identity_tuple(&after) != file_identity_tuple(&named)
        || observed_size != before.len()
    {
        return Err(error);
    }
    let sha256 = hex_bytes(&digest.finalize());
    if expected_digest.is_some_and(|expected| expected != sha256) {
        return Err(error);
    }
    Ok(StableFileIdentity {
        sha256,
        size: before.len(),
        mode: before.permissions().mode() & 0o777,
        device: before.dev(),
        inode: before.ino(),
        links: before.nlink(),
    })
}

fn file_identity_tuple(
    metadata: &fs::Metadata,
) -> (u64, u64, u32, u32, u64, u64, i64, i64, i64, i64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.mode(),
        metadata.uid(),
        metadata.nlink(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

fn observe_launchd_job(uid: u32, label: &str) -> Result<LaunchdJob, IssuerError> {
    let output = Command::new("/bin/launchctl")
        .args(["print", &format!("gui/{uid}/{label}")])
        .env_clear()
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    if !output.status.success() || output.stdout.len() > 256 * 1024 {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let text =
        std::str::from_utf8(&output.stdout).map_err(|_| IssuerError::CandidateServiceInactive)?;
    parse_launchd_job(text)
}

fn parse_launchd_job(output: &str) -> Result<LaunchdJob, IssuerError> {
    if output.contains('\0') || output.contains('\r') {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let lines = output.lines().collect::<Vec<_>>();
    let one = |prefix: &str| -> Result<&str, IssuerError> {
        let values = lines
            .iter()
            .filter_map(|line| line.strip_prefix(prefix))
            .collect::<Vec<_>>();
        if values.len() != 1 || values[0].is_empty() || values[0].trim() != values[0] {
            return Err(IssuerError::CandidateServiceInactive);
        }
        Ok(values[0])
    };
    let map_block = |opening: &str| -> Result<BTreeMap<String, String>, IssuerError> {
        let starts = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| (*line == opening).then_some(index))
            .collect::<Vec<_>>();
        if starts.len() != 1 {
            return Err(IssuerError::CandidateServiceInactive);
        }
        let mut values = BTreeMap::new();
        let mut closed = false;
        for nested in &lines[starts[0] + 1..] {
            if *nested == "\t}" {
                closed = true;
                break;
            }
            let entry = nested
                .strip_prefix("\t\t")
                .ok_or(IssuerError::CandidateServiceInactive)?;
            let (key, value) = entry
                .split_once(" => ")
                .ok_or(IssuerError::CandidateServiceInactive)?;
            if key.is_empty()
                || value.is_empty()
                || key.trim() != key
                || value.trim() != value
                || values.insert(key.to_string(), value.to_string()).is_some()
            {
                return Err(IssuerError::CandidateServiceInactive);
            }
        }
        if !closed {
            return Err(IssuerError::CandidateServiceInactive);
        }
        Ok(values)
    };
    let pid = one("\tpid = ")?
        .parse::<i32>()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let runs = one("\truns = ")?
        .parse::<u64>()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let minimum_runtime = one("\tminimum runtime = ")?
        .parse::<u64>()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let exit_timeout = one("\texit timeout = ")?
        .parse::<u64>()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let soft_maxfiles = one("\t\tmaxfiles (soft) => ")?
        .parse::<u64>()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let hard_maxfiles = one("\t\tmaxfiles (hard) => ")?
        .parse::<u64>()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    if pid <= 0 || runs == 0 || one("\tstate = ")? != "running" {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let exits = lines
        .iter()
        .filter_map(|line| line.strip_prefix("\tlast exit code = "))
        .collect::<Vec<_>>();
    if exits.len() > 1
        || exits
            .first()
            .is_some_and(|value| *value != "(never exited)")
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let mut blocks = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if *line != "\targuments = {" {
            continue;
        }
        let mut arguments = Vec::new();
        let mut closed = false;
        for nested in &lines[index + 1..] {
            if *nested == "\t}" {
                closed = true;
                break;
            }
            let argument = nested
                .strip_prefix("\t\t")
                .ok_or(IssuerError::CandidateServiceInactive)?;
            if argument.is_empty() {
                return Err(IssuerError::CandidateServiceInactive);
            }
            arguments.push(argument.to_string());
        }
        if !closed {
            return Err(IssuerError::CandidateServiceInactive);
        }
        blocks.push(arguments);
    }
    if blocks.len() != 1 || blocks[0].is_empty() {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(LaunchdJob {
        pid,
        program: one("\tprogram = ")?.to_string(),
        plist_path: one("\tpath = ")?.to_string(),
        arguments: blocks.pop().ok_or(IssuerError::CandidateServiceInactive)?,
        environment: map_block("\tenvironment = {")?,
        working_directory: one("\tworking directory = ")?.to_string(),
        stdout_path: one("\tstdout path = ")?.to_string(),
        stderr_path: one("\tstderr path = ")?.to_string(),
        umask: one("\tumask = ")?.to_string(),
        minimum_runtime,
        exit_timeout,
        soft_maxfiles,
        hard_maxfiles,
        runs,
        state: "running".to_string(),
    })
}

fn validate_launchd_binding(
    service: &CandidateServiceBinding,
    job: &LaunchdJob,
) -> Result<(), IssuerError> {
    // A GUI-domain launchd job caps the live value reported by `launchctl print`
    // at 60 seconds even though the sealed plist retains ExitTimeOut=90.  Bind
    // both representations: validate_service_plist checks the sealed 90-second
    // source value and this check pins the effective live value.
    const GUI_LAUNCHD_EFFECTIVE_EXIT_TIMEOUT_SECONDS: u64 = 60;
    let mut expected_environment = service.environment.clone();
    expected_environment.insert("XPC_SERVICE_NAME".to_string(), service.label.clone());
    let mut observed_environment = job.environment.clone();
    if let Some(value) = observed_environment.remove("OSLogRateLimit") {
        if value != "64" {
            return Err(IssuerError::CandidateServiceInactive);
        }
    }
    if job.program != service.configured_program
        || job.plist_path != service.plist_path.to_string_lossy()
        || job.arguments != service.arguments
        || observed_environment != expected_environment
        || job.working_directory != service.working_directory
        || job.stdout_path != service.log_path
        || job.stderr_path != service.log_path
        || job.umask != "77"
        || job.minimum_runtime != 30
        || job.exit_timeout != GUI_LAUNCHD_EFFECTIVE_EXIT_TIMEOUT_SECONDS
        || job.soft_maxfiles != 2048
        || job.hard_maxfiles != 4096
        || job.state != "running"
        || job.pid <= 0
        || job.runs == 0
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

fn validate_process_binding(
    pid: i32,
    candidate: &CandidateBinding,
    expected_arguments: &[String],
    uid: u32,
) -> Result<(), IssuerError> {
    let first = process_path(pid)?;
    if first != candidate.path {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "uid="])
        .env_clear()
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let observed_uid = std::str::from_utf8(&output.stdout)
        .ok()
        .map(str::trim)
        .and_then(|value| value.parse::<u32>().ok());
    if !output.status.success() || observed_uid != Some(uid) {
        return Err(IssuerError::CandidateServiceInactive);
    }
    if process_arguments(pid)? != expected_arguments {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let _ = validate_immutable_candidate(candidate, uid)?;
    if process_path(pid)? != first || process_arguments(pid)? != expected_arguments {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn process_path(pid: i32) -> Result<PathBuf, IssuerError> {
    #[link(name = "proc")]
    extern "C" {
        fn proc_pidpath(
            pid: libc::c_int,
            buffer: *mut libc::c_void,
            buffer_size: u32,
        ) -> libc::c_int;
    }
    let mut buffer = [0_u8; 4_096];
    let length = unsafe {
        proc_pidpath(
            pid,
            buffer.as_mut_ptr().cast::<libc::c_void>(),
            buffer.len() as u32,
        )
    };
    if length <= 0 || length as usize >= buffer.len() || buffer[length as usize] != 0 {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let value = std::str::from_utf8(&buffer[..length as usize])
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(path)
}

#[cfg(not(target_os = "macos"))]
fn process_path(_pid: i32) -> Result<PathBuf, IssuerError> {
    Err(IssuerError::CandidateServiceInactive)
}

#[cfg(target_os = "macos")]
fn process_arguments(pid: i32) -> Result<Vec<String>, IssuerError> {
    const KERN_PROCARGS2: libc::c_int = 49;
    let mut argmax: libc::c_int = 0;
    let mut argmax_size = std::mem::size_of::<libc::c_int>();
    let mut argmax_mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    if unsafe {
        libc::sysctl(
            argmax_mib.as_mut_ptr(),
            argmax_mib.len() as libc::c_uint,
            (&mut argmax as *mut libc::c_int).cast(),
            &mut argmax_size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || argmax <= 0
        || argmax as usize > 16 * 1024 * 1024
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let mut buffer = vec![0_u8; argmax as usize];
    let mut size = buffer.len();
    let mut arguments_mib = [libc::CTL_KERN, KERN_PROCARGS2, pid];
    if unsafe {
        libc::sysctl(
            arguments_mib.as_mut_ptr(),
            arguments_mib.len() as libc::c_uint,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size < std::mem::size_of::<libc::c_int>()
        || size > buffer.len()
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    buffer.truncate(size);
    let argument_count = libc::c_int::from_ne_bytes(
        buffer[..std::mem::size_of::<libc::c_int>()]
            .try_into()
            .map_err(|_| IssuerError::CandidateServiceInactive)?,
    );
    if argument_count <= 0 || argument_count > 1024 {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let mut cursor = std::mem::size_of::<libc::c_int>();
    while cursor < buffer.len() && buffer[cursor] != 0 {
        cursor += 1;
    }
    while cursor < buffer.len() && buffer[cursor] == 0 {
        cursor += 1;
    }
    let mut arguments = Vec::with_capacity(argument_count as usize);
    for _ in 0..argument_count {
        let end = buffer[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| cursor + offset)
            .ok_or(IssuerError::CandidateServiceInactive)?;
        if end == cursor {
            return Err(IssuerError::CandidateServiceInactive);
        }
        arguments.push(
            std::str::from_utf8(&buffer[cursor..end])
                .map_err(|_| IssuerError::CandidateServiceInactive)?
                .to_string(),
        );
        cursor = end + 1;
    }
    Ok(arguments)
}

#[cfg(not(target_os = "macos"))]
fn process_arguments(_pid: i32) -> Result<Vec<String>, IssuerError> {
    Err(IssuerError::CandidateServiceInactive)
}

fn validate_service_plist(
    service: &CandidateServiceBinding,
    uid: u32,
    expected: Option<&HistoricalPlistIdentity>,
) -> Result<(), IssuerError> {
    if fs::canonicalize(&service.plist_path).map_err(|_| IssuerError::CandidateServiceInactive)?
        != service.plist_path
    {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let first = stable_owned_file_digest(
        &service.plist_path,
        uid,
        0o600,
        256 * 1024,
        expected.map(|identity| identity.sha256.as_str()),
        IssuerError::CandidateServiceInactive,
    )?;
    if expected.is_some_and(|identity| {
        identity.size != first.size
            || identity.mode != first.mode
            || identity.device != first.device
            || identity.inode != first.inode
            || identity.links != first.links
    }) {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let bytes = stable_source_file_bytes(&service.plist_path, uid, 256 * 1024)?;
    if hex_digest(&bytes) != first.sha256 || plist_json_value(&bytes)? != service.expected_plist {
        return Err(IssuerError::CandidateServiceInactive);
    }
    let second = stable_owned_file_digest(
        &service.plist_path,
        uid,
        0o600,
        256 * 1024,
        Some(&first.sha256),
        IssuerError::CandidateServiceInactive,
    )?;
    if first.sha256 != second.sha256 {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

fn plist_json_value(bytes: &[u8]) -> Result<Value, IssuerError> {
    let mut child = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", "-"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or(IssuerError::CandidateServiceInactive)?;
    stdin
        .write_all(bytes)
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|_| IssuerError::CandidateServiceInactive)?;
    if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > 1024 * 1024 {
        return Err(IssuerError::CandidateServiceInactive);
    }
    serde_json::from_slice(&output.stdout).map_err(|_| IssuerError::CandidateServiceInactive)
}

const TUNNEL_RUNNER_BYTES: &[u8] = concat!(
    "#!/bin/zsh\n",
    "set -euo pipefail\n",
    "token=\"$(/usr/bin/security find-generic-password -a \"$STARRING_D2_TUNNEL_KEYCHAIN_ACCOUNT\" -s \"$STARRING_D2_TUNNEL_KEYCHAIN_SERVICE\" -w)\"\n",
    "export TUNNEL_TOKEN=\"$token\"\n",
    "unset token\n",
    "exec \"$STARRING_D2_CLOUDFLARED_PATH\" tunnel --no-autoupdate --loglevel warn --transport-loglevel warn run --url \"$STARRING_D2_CLOUDFLARE_ORIGIN_SERVICE\" \"$STARRING_D2_CLOUDFLARE_TUNNEL_ID\"\n",
).as_bytes();

fn validate_tunnel_runner(run: &ValidatedRun) -> Result<(), IssuerError> {
    let path = run.run_directory.join("orchestrator/run-tunnel.zsh");
    let identity = stable_owned_file_digest(
        &path,
        run.uid,
        0o700,
        64 * 1024,
        Some(&hex_digest(TUNNEL_RUNNER_BYTES)),
        IssuerError::CandidateServiceInactive,
    )?;
    if identity.size != TUNNEL_RUNNER_BYTES.len() as u64 {
        return Err(IssuerError::CandidateServiceInactive);
    }
    Ok(())
}

fn validate_api_loopback_ready(run: &ValidatedRun) -> Result<(), IssuerError> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], run.api_port));
    let host = run
        .public_origin
        .strip_prefix("https://")
        .ok_or(IssuerError::ApiLoopbackOrigin)?;
    validate_api_loopback_ready_at(address, host)
}

fn validate_api_loopback_ready_at(
    address: std::net::SocketAddr,
    host: &str,
) -> Result<(), IssuerError> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|_| IssuerError::ApiLoopbackConnect)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| IssuerError::ApiLoopbackRead)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| IssuerError::ApiLoopbackWrite)?;
    let request =
        format!("GET /health/ready HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|_| IssuerError::ApiLoopbackWrite)?;
    let mut response = Vec::new();
    let mut buffer = [0_u8; 2_048];
    while response.len() <= 16 * 1024 {
        let read = stream
            .read(&mut buffer)
            .map_err(|_| IssuerError::ApiLoopbackRead)?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
        if response.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    if response.len() > 16 * 1024 {
        return Err(IssuerError::ApiLoopbackRead);
    }
    if response.is_empty() {
        return Err(IssuerError::ApiLoopbackResponseEmpty);
    }
    let first_line = response
        .split(|byte| *byte == b'\n')
        .next()
        .and_then(|line| line.strip_suffix(b"\r"));
    if !matches!(first_line, Some(b"HTTP/1.1 200 OK" | b"HTTP/1.0 200 OK")) {
        return Err(IssuerError::ApiLoopbackStatus);
    }
    Ok(())
}

pub fn validate_child_command(run: &ValidatedRun, child: &[String]) -> Result<(), IssuerError> {
    if child.len() != 2 {
        return Err(IssuerError::ChildRunner);
    }
    let node = Path::new(&child[0]);
    if node != run.candidate_node_path || !node.is_absolute() {
        return Err(IssuerError::CandidateNode);
    }
    require_safe_executable(node, run.uid, IssuerError::CandidateNode)?;
    if fs::canonicalize(node).map_err(|_| IssuerError::CandidateNode)? != node {
        return Err(IssuerError::CandidateNode);
    }
    if digest_file(node, 512 * 1024 * 1024, IssuerError::CandidateNode)?
        != run.candidate_node_sha256
    {
        return Err(IssuerError::CandidateNode);
    }
    let runner = Path::new(&child[1]);
    if !runner.is_absolute()
        || fs::canonicalize(runner).map_err(|_| IssuerError::ChildRunner)? != runner
    {
        return Err(IssuerError::ChildRunner);
    }
    let maintenance_root = runner.parent().ok_or(IssuerError::ChildRunner)?;
    let tools_root = maintenance_root.parent().ok_or(IssuerError::ChildRunner)?;
    if maintenance_root
        .file_name()
        .and_then(|value| value.to_str())
        != Some("d2-maintenance")
        || tools_root.file_name().and_then(|value| value.to_str()) != Some("tools")
        || runner != maintenance_root.join("headless_product_runner.mjs")
        || stable_owned_file_digest(
            runner,
            run.uid,
            0o644,
            4 * 1024 * 1024,
            Some(&hex_digest(TRUSTED_RUNNER_BYTES)),
            IssuerError::ChildRunner,
        )?
        .size
            != TRUSTED_RUNNER_BYTES.len() as u64
    {
        return Err(IssuerError::ChildRunner);
    }
    let product_driver = tools_root.join("d2-certification/product_driver.js");
    if fs::canonicalize(&product_driver).map_err(|_| IssuerError::ChildRunner)? != product_driver {
        return Err(IssuerError::ChildRunner);
    }
    let product_driver_identity = stable_owned_file_digest(
        &product_driver,
        run.uid,
        0o644,
        4 * 1024 * 1024,
        Some(&hex_digest(TRUSTED_PRODUCT_DRIVER_BYTES)),
        IssuerError::ChildRunner,
    )?;
    if product_driver_identity.size != TRUSTED_PRODUCT_DRIVER_BYTES.len() as u64 {
        return Err(IssuerError::ChildRunner);
    }
    Ok(())
}

pub fn rehash_sealed_provisioner(run: &ValidatedRun) -> Result<String, IssuerError> {
    let candidate =
        candidate_binding(run, "sealed_provisioner").map_err(|_| IssuerError::DirectOnboarding)?;
    if candidate.path != run.sealed_provisioner_path
        || candidate.sha256 != run.sealed_provisioner_sha256
    {
        return Err(IssuerError::DirectOnboarding);
    }
    let identity = validate_immutable_candidate(candidate, run.uid)
        .map_err(|_| IssuerError::DirectOnboarding)?;
    if identity.sha256 != run.sealed_provisioner_sha256 {
        return Err(IssuerError::DirectOnboarding);
    }
    Ok(identity.sha256)
}

#[derive(Debug)]
pub struct ValidatedScenario {
    pub value: Value,
    pub sha256: String,
    pub session_id_prefix: String,
}

pub fn validate_scenario_path(
    path: &Path,
    uid: u32,
    runner: &Path,
) -> Result<ValidatedScenario, IssuerError> {
    let maintenance_root = runner.parent().ok_or(IssuerError::Scenario)?;
    let expected = maintenance_root
        .join("scenarios")
        .join(TRUSTED_SCENARIO_NAME);
    if !path.is_absolute()
        || fs::canonicalize(path).map_err(|_| IssuerError::Scenario)? != path
        || path != expected
    {
        return Err(IssuerError::Scenario);
    }
    let metadata = path.symlink_metadata().map_err(|_| IssuerError::Scenario)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.permissions().mode() & 0o777 != 0o644
        || metadata.len() > MAX_SCENARIO_BYTES
    {
        return Err(IssuerError::Scenario);
    }
    let bytes = stable_source_file_bytes(path, uid, MAX_SCENARIO_BYTES)
        .map_err(|_| IssuerError::Scenario)?;
    if hex_digest(&bytes) != hex_digest(TRUSTED_SCENARIO_BYTES) {
        return Err(IssuerError::Scenario);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| IssuerError::Scenario)?;
    validate_scenario_value(&value)?;
    let session_id_prefix = value
        .get("session_id_prefix")
        .and_then(Value::as_str)
        .filter(|prefix| valid_session_id_prefix(prefix))
        .ok_or(IssuerError::Scenario)?
        .to_string();
    Ok(ValidatedScenario {
        value,
        sha256: hex_digest(&bytes),
        session_id_prefix,
    })
}

fn valid_session_id_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 111
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub fn validate_scenario_value(value: &Value) -> Result<(), IssuerError> {
    if !value.is_object() {
        return Err(IssuerError::Scenario);
    }
    fn walk(value: &Value, depth: usize) -> Result<(), IssuerError> {
        if depth > 32 {
            return Err(IssuerError::Scenario);
        }
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if key.len() > 128
                        || matches!(key.as_str(), "__proto__" | "constructor" | "prototype")
                    {
                        return Err(IssuerError::Scenario);
                    }
                    walk(value, depth + 1)?;
                }
            }
            Value::Array(values) => {
                for value in values {
                    walk(value, depth + 1)?;
                }
            }
            Value::String(value) if value.len() > 16_384 => return Err(IssuerError::Scenario),
            _ => {}
        }
        Ok(())
    }
    walk(value, 0)
}

pub struct DatabaseCredential {
    role: &'static str,
    password: Zeroizing<String>,
    original: Zeroizing<String>,
}

impl DatabaseCredential {
    pub fn connect_options(&self, run: &ValidatedRun) -> PgConnectOptions {
        PgConnectOptions::new()
            .host(run.socket_directory.to_string_lossy().as_ref())
            .port(run.database_port)
            .username(self.role)
            .password(self.password.as_str())
            .database(DATABASE_NAME)
            .ssl_mode(PgSslMode::Disable)
            .application_name("starring-d2-session-issuer")
    }

    pub fn redaction_values(&self) -> [&str; 2] {
        [self.password.as_str(), self.original.as_str()]
    }
}

pub struct RunCredentials {
    pub oauth: DatabaseCredential,
    pub issuer: DatabaseCredential,
    pub security: DatabaseCredential,
}

pub fn load_discord_bot_token(run: &ValidatedRun) -> Result<Zeroizing<String>, IssuerError> {
    let value = read_keychain_value(
        &run.discord_bot_token_service,
        &run.discord_bot_token_account,
        MAX_KEYCHAIN_VALUE_BYTES,
    )?;
    let token = String::from_utf8(value.to_vec()).map_err(|_| IssuerError::KeychainSecret)?;
    if token.is_empty()
        || token.len() > MAX_KEYCHAIN_VALUE_BYTES
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-'))
    {
        return Err(IssuerError::KeychainSecret);
    }
    Ok(Zeroizing::new(token))
}

pub fn load_run_credentials(run: &ValidatedRun) -> Result<RunCredentials, IssuerError> {
    let owner = read_keychain_value(
        &run.api_keychain_service,
        OWNER_ACCOUNT,
        MAX_KEYCHAIN_VALUE_BYTES,
    )?;
    if owner.as_slice() != run.run_id.as_bytes() {
        return Err(IssuerError::KeychainIdentity);
    }
    let oauth_value = read_keychain_value(
        &run.api_keychain_service,
        OAUTH_ACCOUNT,
        MAX_KEYCHAIN_VALUE_BYTES,
    )?;
    let issuer_value = read_keychain_value(
        &run.api_keychain_service,
        ISSUER_ACCOUNT,
        MAX_KEYCHAIN_VALUE_BYTES,
    )?;
    let security_value = read_keychain_value(
        &run.api_keychain_service,
        SECURITY_ACCOUNT,
        MAX_KEYCHAIN_VALUE_BYTES,
    )?;
    Ok(RunCredentials {
        oauth: parse_database_credential(oauth_value, OAUTH_ROLE, run)?,
        issuer: parse_database_credential(issuer_value, ISSUER_ROLE, run)?,
        security: parse_database_credential(security_value, SECURITY_ROLE, run)?,
    })
}

fn read_keychain_value(
    service: &str,
    account: &str,
    maximum: usize,
) -> Result<Zeroizing<Vec<u8>>, IssuerError> {
    let output = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| IssuerError::KeychainSecret)?;
    if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > maximum + 2 {
        return Err(IssuerError::KeychainSecret);
    }
    let mut value = Zeroizing::new(output.stdout);
    if value.ends_with(b"\r\n") {
        let length = value.len() - 2;
        value.truncate(length);
    } else if value.ends_with(b"\n") {
        let length = value.len() - 1;
        value.truncate(length);
    }
    if value.is_empty() || value.len() > maximum {
        return Err(IssuerError::KeychainSecret);
    }
    Ok(value)
}

pub fn parse_database_credential(
    value: Zeroizing<Vec<u8>>,
    role: &'static str,
    run: &ValidatedRun,
) -> Result<DatabaseCredential, IssuerError> {
    let value = String::from_utf8(value.to_vec()).map_err(|_| IssuerError::DatabaseCredential)?;
    let original = Zeroizing::new(value);
    let prefix = format!("postgresql://{role}:");
    let suffix = format!(
        "@127.0.0.1:{}/{DATABASE_NAME}?sslmode=disable",
        run.database_port
    );
    let password = original
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
        .ok_or(IssuerError::DatabaseCredential)?;
    if password.len() != 43
        || !password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || URL_SAFE_NO_PAD
            .decode(password)
            .map_err(|_| IssuerError::DatabaseCredential)?
            .len()
            != 32
        || run.api_keychain_service == "starring-api.staging"
    {
        return Err(IssuerError::DatabaseCredential);
    }
    Ok(DatabaseCredential {
        role,
        password: Zeroizing::new(password.to_owned()),
        original,
    })
}

pub fn generate_four_secrets() -> Result<[Zeroizing<String>; 4], IssuerError> {
    let mut values = Vec::with_capacity(4);
    while values.len() < 4 {
        let mut bytes = Zeroizing::new([0_u8; 32]);
        getrandom::fill(&mut *bytes).map_err(|_| IssuerError::Entropy)?;
        let value = Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_slice()));
        if values
            .iter()
            .all(|existing: &Zeroizing<String>| existing.as_str() != value.as_str())
        {
            values.push(value);
        }
    }
    values.try_into().map_err(|_| IssuerError::Entropy)
}

pub fn credential_digest(secret: &str) -> [u8; 32] {
    Sha256::digest(
        URL_SAFE_NO_PAD
            .decode(secret)
            .expect("validated generated credential must decode"),
    )
    .into()
}

pub fn redact_public_evidence(raw: &[u8], exact_secrets: &[&str]) -> Result<Value, IssuerError> {
    if raw.len() > MAX_CHILD_OUTPUT_BYTES {
        return Err(IssuerError::EvidenceTooLarge);
    }
    let mut text = std::str::from_utf8(raw)
        .map_err(|_| IssuerError::Evidence)?
        .to_owned();
    for secret in exact_secrets {
        if !secret.is_empty() {
            text = text.replace(secret, "<redacted>");
        }
    }
    text = redact_postgres_urls(&text);
    let mut value: Value = serde_json::from_str(&text).map_err(|_| IssuerError::Evidence)?;
    if !value.is_object() {
        return Err(IssuerError::Evidence);
    }
    redact_sensitive_fields(&mut value, exact_secrets);
    let encoded = serde_json::to_string(&value).map_err(|_| IssuerError::Evidence)?;
    if exact_secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .any(|secret| encoded.contains(secret))
        || encoded.contains("postgresql://")
    {
        return Err(IssuerError::Evidence);
    }
    Ok(value)
}

fn redact_sensitive_fields(value: &mut Value, exact_secrets: &[&str]) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if matches!(
                    normalized.as_str(),
                    "session"
                        | "csrf"
                        | "cookie"
                        | "authorization"
                        | "password"
                        | "secret"
                        | "database_url"
                        | "state"
                        | "browser_nonce"
                        | "access_token"
                        | "refresh_token"
                ) {
                    *value = Value::String("<redacted>".to_string());
                } else {
                    redact_sensitive_fields(value, exact_secrets);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_sensitive_fields(value, exact_secrets);
            }
        }
        Value::String(text) => {
            for secret in exact_secrets {
                if !secret.is_empty() {
                    *text = text.replace(secret, "<redacted>");
                }
            }
            *text = redact_postgres_urls(text);
        }
        _ => {}
    }
}

fn redact_postgres_urls(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(index) = remaining.find("postgresql://") {
        output.push_str(&remaining[..index]);
        output.push_str("<redacted-database-url>");
        remaining = &remaining[index + "postgresql://".len()..];
        let end = remaining
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '"' | '\'' | '\\' | '<' | '>')
            })
            .unwrap_or(remaining.len());
        remaining = &remaining[end..];
    }
    output.push_str(remaining);
    output
}

fn read_canonical_json(
    path: &Path,
    maximum: u64,
    error: IssuerError,
) -> Result<(Value, Vec<u8>), IssuerError> {
    let metadata = path.symlink_metadata().map_err(|_| error)?;
    if metadata.len() == 0 || metadata.len() > maximum {
        return Err(error);
    }
    let bytes = fs::read(path).map_err(|_| error)?;
    let payload = bytes.strip_suffix(b"\n").ok_or(error)?;
    if payload.ends_with(b"\n") || payload.ends_with(b"\r") {
        return Err(error);
    }
    let value: Value = serde_json::from_slice(payload).map_err(|_| error)?;
    let canonical = serde_json::to_vec(&value).map_err(|_| error)?;
    if canonical != payload {
        return Err(error);
    }
    Ok((value, canonical))
}

fn require_owned_directory(
    path: &Path,
    uid: u32,
    expected_mode: u32,
    error: IssuerError,
) -> Result<(), IssuerError> {
    let metadata = path.symlink_metadata().map_err(|_| error)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.permissions().mode() & 0o777 != expected_mode
    {
        return Err(error);
    }
    Ok(())
}

fn require_owned_regular(
    path: &Path,
    uid: u32,
    expected_mode: u32,
    error: IssuerError,
) -> Result<(), IssuerError> {
    let metadata = path.symlink_metadata().map_err(|_| error)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.permissions().mode() & 0o777 != expected_mode
    {
        return Err(error);
    }
    Ok(())
}

fn require_safe_executable(path: &Path, uid: u32, error: IssuerError) -> Result<(), IssuerError> {
    let metadata = path.symlink_metadata().map_err(|_| error)?;
    let mode = metadata.permissions().mode() & 0o777;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || !matches!(metadata.uid(), 0) && metadata.uid() != uid
        || mode & 0o111 == 0
        || mode & 0o022 != 0
    {
        return Err(error);
    }
    Ok(())
}

fn digest_file(path: &Path, maximum: u64, error: IssuerError) -> Result<String, IssuerError> {
    let metadata = path.symlink_metadata().map_err(|_| error)?;
    if metadata.len() == 0 || metadata.len() > maximum {
        return Err(error);
    }
    let mut file = File::open(path).map_err(|_| error)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| error)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex_bytes(&digest.finalize()))
}

fn hex_digest(value: &[u8]) -> String {
    hex_bytes(&Sha256::digest(value))
}

fn hex_bytes(value: &[u8]) -> String {
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

fn valid_run_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 32
        && &bytes[..3] == b"d2-"
        && bytes[3..11].iter().all(u8::is_ascii_digit)
        && bytes[11] == b't'
        && bytes[12..18].iter().all(u8::is_ascii_digit)
        && bytes[18] == b'z'
        && bytes[19] == b'-'
        && bytes[20..].iter().all(u8::is_ascii_hexdigit)
        && !bytes[20..].iter().any(u8::is_ascii_uppercase)
}

fn valid_snowflake(value: &str) -> bool {
    !value.starts_with('0')
        && !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !value.bytes().any(|byte| byte.is_ascii_uppercase())
}

fn current_uid() -> Result<u32, IssuerError> {
    let real = unsafe { libc::getuid() };
    let effective = unsafe { libc::geteuid() };
    if real != effective {
        return Err(IssuerError::ManifestPermissions);
    }
    Ok(real)
}

fn current_home(uid: u32) -> Result<PathBuf, IssuerError> {
    let record = unsafe { libc::getpwuid(uid) };
    if record.is_null() {
        return Err(IssuerError::ManifestPath);
    }
    let bytes = unsafe { CStr::from_ptr((*record).pw_dir) }.to_bytes();
    if bytes.is_empty() || bytes[0] != b'/' || bytes.contains(&0) {
        return Err(IssuerError::ManifestPath);
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::DirBuilderExt;

    #[test]
    fn coordinator_directory_is_created_once_and_hostile_paths_are_rejected() {
        struct RemoveDirectoryOnDrop(PathBuf);
        impl Drop for RemoveDirectoryOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let run_directory = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "starring-d2-coordinator-test-{}-{}",
            std::process::id(),
            u64::from_ne_bytes(nonce)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&run_directory)
            .unwrap();
        let _cleanup = RemoveDirectoryOnDrop(run_directory.clone());
        let uid = current_uid().unwrap();

        let first_lock =
            acquire_run_coordinator_lock_with_hook(&run_directory, uid, || {}).unwrap();
        let coordinator = run_directory.join("coordinator");
        let metadata = coordinator.symlink_metadata().unwrap();
        assert!(metadata.file_type().is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!(metadata.uid(), uid);
        drop(first_lock);
        drop(acquire_run_coordinator_lock_with_hook(&run_directory, uid, || {}).unwrap());

        let relocated = run_directory.join("coordinator-relocated");
        let replacement = coordinator.clone();
        let race = acquire_run_coordinator_lock_with_hook(&run_directory, uid, || {
            fs::rename(&replacement, &relocated).unwrap();
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&replacement)
                .unwrap();
        });
        assert!(matches!(race, Err(IssuerError::D2OperationBusy)));
        fs::remove_dir(&coordinator).unwrap();
        fs::rename(&relocated, &coordinator).unwrap();

        fs::remove_file(coordinator.join("coordinator.lock")).unwrap();
        fs::remove_dir(&coordinator).unwrap();
        fs::DirBuilder::new()
            .mode(0o755)
            .create(&coordinator)
            .unwrap();
        fs::set_permissions(&coordinator, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            acquire_run_coordinator_lock_with_hook(&run_directory, uid, || {}),
            Err(IssuerError::D2OperationBusy)
        ));

        fs::remove_dir(&coordinator).unwrap();
        let redirected = run_directory.join("redirected");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&redirected)
            .unwrap();
        std::os::unix::fs::symlink(&redirected, &coordinator).unwrap();
        assert!(matches!(
            acquire_run_coordinator_lock_with_hook(&run_directory, uid, || {}),
            Err(IssuerError::D2OperationBusy)
        ));
    }

    fn valid_manifest() -> Manifest {
        serde_json::from_value(json!({
            "authoring": {"provider": "codex_chatgpt"},
            "certification_class": "commercial_human_v1",
            "cloudflare": {
                "tunnel_id": D2_CLOUDFLARE_TUNNEL_ID,
                "public_origin": D2_PUBLIC_ORIGIN,
                "origin_service": "http://127.0.0.1:28080"
            },
            "commit_sha": "a".repeat(40),
            "created_at": "2026-08-12T01:02:03Z",
            "schema_version": 1,
            "run_id": "d2-20260812t010203z-012345abcdef",
            "public_origin": D2_PUBLIC_ORIGIN,
            "database": {
                "cluster_root": "/private/tmp/starring-d2-d2-20260812t010203z-012345abcdef/postgres",
                "name": DATABASE_NAME,
                "port": 55433,
                "socket_directory": "/private/tmp/starring-d2-d2-20260812t010203z-012345abcdef/socket"
            },
            "discord": {
                "actor_id": "111111111111111111",
                "application_id": "222222222222222222",
                "bot_user_id": "222222222222222222",
                "guild_id": "333333333333333333",
                "hub_channel_id": "444444444444444444",
                "resource_prefix": "starring-d2-20260812-012345abcdef",
                "disposable_guild_required": true
            },
            "keychain_services": {
                "api": "starring.d2.012345abcdef.api",
                "postgres": "starring.d2.012345abcdef.postgres",
                "runtime": "starring.d2.012345abcdef.runtime",
                "worker": "starring.d2.012345abcdef.worker"
            },
            "protected_staging": {
                "database": "starring_runtime_staging@127.0.0.1:5432",
                "launchd_labels": [
                    "local.starring.api.staging",
                    "local.starring.codex-worker",
                    "local.starring.runtime.staging",
                    "local.cloudflared.starring"
                ],
                "mutation_allowed": false
            },
            "expected_steps": [{"step": 1, "code": "isolated_target_created"}],
            "external_keychain": {
                "discord_oauth_client_secret": {"service": "external.discord", "account": "oauth"},
                "discord_bot_token": {"service": "external.discord", "account": "bot"},
                "tunnel_token": {"service": "external.cloudflare", "account": "tunnel"}
            },
            "human_boundaries": [
                "create_disposable_discord_guild",
                "complete_discord_oauth",
                "confirm_product_preview",
                "execute_real_discord_interactions",
                "confirm_replacement_preview",
                "delete_disposable_discord_guild"
            ],
            "services": {
                "api": {"label": "local.starring.d2.012345abcdef.api", "port": 28080},
                "runtime": {"label": "local.starring.d2.012345abcdef.runtime", "port": 29091},
                "transport": {"label": "local.starring.d2.012345abcdef.transport", "gateway_port": 29101, "http_port": 29102},
                "tunnel": {"label": "local.starring.d2.012345abcdef.tunnel"},
                "worker": {"label": "local.starring.d2.012345abcdef.worker", "port": 28181}
            },
            "candidates": {
                "api": {"path": "/private/tmp/candidate-api", "sha256": "a".repeat(64)},
                "runtime": {"path": "/private/tmp/candidate-runtime", "sha256": "b".repeat(64)},
                "codex_worker": {"path": "/private/tmp/d3/candidate-bundle/codex-worker/worker.mjs", "sha256": "c".repeat(64)},
                "codex": {"path": "/private/tmp/candidate-codex", "sha256": "d".repeat(64)},
                "db_bootstrap": {"path": "/private/tmp/candidate-db", "sha256": "e".repeat(64)},
                "sealed_provisioner": {"path": "/private/tmp/candidate-provisioner", "sha256": "f".repeat(64)},
                "certification_transport": {"path": "/private/tmp/candidate-transport", "sha256": "1".repeat(64)},
                "node": {"path": "/private/tmp/candidate-node", "sha256": "2".repeat(64)},
                "cloudflared": {"path": "/private/tmp/candidate-cloudflared", "sha256": "3".repeat(64)}
            },
            "source_trees": {
                "codex_worker": {"root": "/private/tmp/d3/candidate-bundle/codex-worker", "files": CODEX_WORKER_SOURCE_FILES, "sha256": "4".repeat(64)},
                "d2_toolchain": {"root": "/private/tmp/d3/worktree/tools/d2-certification", "files": D2_TOOLCHAIN_SOURCE_FILES, "sha256": "5".repeat(64)},
                "certification_transport": {"root": "/private/tmp/d3/worktree/tools/d2-certification-transport", "files": CERTIFICATION_TRANSPORT_SOURCE_FILES, "sha256": "6".repeat(64)}
            }
        }))
        .unwrap()
    }

    #[test]
    fn pure_manifest_contract_accepts_only_exact_isolated_shape() {
        let manifest = valid_manifest();
        assert_eq!(
            validate_manifest_contract(
                &manifest,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Ok(())
        );

        let mut standing = valid_manifest();
        standing.keychain_services.api = "starring-api.staging".to_string();
        assert_eq!(
            validate_manifest_contract(
                &standing,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Err(IssuerError::ManifestContract)
        );

        let mut protected_external = valid_manifest();
        protected_external
            .external_keychain
            .discord_bot_token
            .service = "starring-api.staging".to_string();
        protected_external
            .external_keychain
            .discord_bot_token
            .account = "unrelated-account".to_string();
        assert_eq!(
            validate_manifest_contract(
                &protected_external,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Err(IssuerError::ManifestContract)
        );

        let mut run_owned_external = valid_manifest();
        run_owned_external.external_keychain.tunnel_token.service =
            run_owned_external.keychain_services.worker.clone();
        assert_eq!(
            validate_manifest_contract(
                &run_owned_external,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Err(IssuerError::ManifestContract)
        );

        let mut tcp_port = valid_manifest();
        tcp_port.database.port = 5432;
        assert_eq!(
            validate_manifest_contract(
                &tcp_port,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Err(IssuerError::ManifestContract)
        );

        let mut dynamic_ports = valid_manifest();
        dynamic_ports.database.port = 55_531;
        dynamic_ports.services.get_mut("runtime").unwrap().port = Some(55_532);
        dynamic_ports.services.get_mut("worker").unwrap().port = Some(55_533);
        dynamic_ports
            .services
            .get_mut("transport")
            .unwrap()
            .gateway_port = Some(55_534);
        dynamic_ports
            .services
            .get_mut("transport")
            .unwrap()
            .http_port = Some(55_535);
        assert_eq!(
            validate_manifest_contract(
                &dynamic_ports,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Ok(())
        );

        let mut wrong_class = valid_manifest();
        wrong_class.certification_class = "synthetic_v1".to_string();
        assert_eq!(
            validate_manifest_contract(
                &wrong_class,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Err(IssuerError::ManifestContract)
        );

        let mut reordered_boundaries = valid_manifest();
        reordered_boundaries.human_boundaries.swap(0, 1);
        assert_eq!(
            validate_manifest_contract(
                &reordered_boundaries,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Err(IssuerError::ManifestContract)
        );

        let mut missing_boundary = valid_manifest();
        missing_boundary
            .human_boundaries
            .retain(|value| value != "confirm_replacement_preview");
        assert_eq!(
            validate_manifest_contract(
                &missing_boundary,
                Path::new("/evidence/d2-20260812t010203z-012345abcdef")
            ),
            Err(IssuerError::ManifestContract)
        );
    }

    #[test]
    fn manifest_deserialization_rejects_unknown_top_level_fields() {
        let mut value = serde_json::to_value(valid_manifest()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), Value::Bool(true));
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }

    #[test]
    fn manifest_deserialization_rejects_unknown_nested_fields() {
        let mut value = serde_json::to_value(valid_manifest()).unwrap();
        value["database"]
            .as_object_mut()
            .unwrap()
            .insert("ambient_port".to_string(), json!(5432));
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }

    #[test]
    fn manifest_deserializes_the_exact_nine_candidate_inventory() {
        let manifest = valid_manifest();
        assert_eq!(manifest.candidates.all().len(), 9);
        let value = serde_json::to_value(&manifest).unwrap();
        let names = value["candidates"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "api",
                "certification_transport",
                "cloudflared",
                "codex",
                "codex_worker",
                "db_bootstrap",
                "node",
                "runtime",
                "sealed_provisioner",
            ])
        );
    }

    #[test]
    fn discord_ownership_requires_the_exact_manifest_bound_claim() {
        let owner = DiscordOwnershipRecord {
            application_id: "222222222222222222".to_string(),
            bot_user_id: "222222222222222222".to_string(),
            guild_id: "333333333333333333".to_string(),
            manifest_path: "/evidence/d2-20260812t010203z-012345abcdef/manifest.json".to_string(),
            manifest_sha256: "a".repeat(64),
            run_id: "d2-20260812t010203z-012345abcdef".to_string(),
        };
        let registry = DiscordOwnershipRegistry {
            kind: "starring.d2.discord-ownership-registry.v1".to_string(),
            owners: vec![DiscordOwnershipRecord {
                application_id: owner.application_id.clone(),
                bot_user_id: owner.bot_user_id.clone(),
                guild_id: owner.guild_id.clone(),
                manifest_path: owner.manifest_path.clone(),
                manifest_sha256: owner.manifest_sha256.clone(),
                run_id: owner.run_id.clone(),
            }],
            schema_version: 1,
        };
        assert!(registry_has_exact_discord_owner(&registry, &owner));
        let mut wrong_manifest = owner;
        wrong_manifest.manifest_sha256 = "b".repeat(64);
        assert!(!registry_has_exact_discord_owner(
            &registry,
            &wrong_manifest
        ));

        let value = json!({
            "schema_version": 1,
            "kind": "starring.d2.discord-ownership-registry.v1",
            "owners": [],
            "unexpected": true
        });
        assert!(serde_json::from_value::<DiscordOwnershipRegistry>(value).is_err());
    }

    #[test]
    fn composed_plists_bind_every_live_service_environment() {
        let manifest = valid_manifest();
        let labels = expected_service_labels("012345abcdef");
        let services = build_candidate_service_bindings(
            &manifest,
            Path::new("/evidence/d2-20260812t010203z-012345abcdef"),
            Path::new("/private/tmp/starring-d2-d2-20260812t010203z-012345abcdef"),
            &labels,
            Path::new("/Users/operator"),
        )
        .unwrap();
        let by_name = services
            .iter()
            .map(|service| (service.name, service))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(by_name["api"].environment.len(), 36);
        assert_eq!(by_name["runtime"].environment.len(), 28);
        assert_eq!(by_name["worker"].environment.len(), 11);
        assert_eq!(by_name["transport"].environment.len(), 2);
        assert_eq!(by_name["tunnel"].environment.len(), 7);
        assert_eq!(by_name["api"].expected_plist.as_object().unwrap().len(), 14);
        assert_eq!(
            by_name["worker"].environment["STARRING_CODEX_PATH"],
            "/private/tmp/candidate-codex"
        );
        assert_eq!(
            by_name["worker"].environment["STARRING_CODEX_WORKER_KEYCHAIN_SERVICE"],
            "starring.d2.012345abcdef.worker"
        );
        assert_eq!(
            by_name["tunnel"].environment["STARRING_D2_CLOUDFLARE_TUNNEL_ID"],
            D2_CLOUDFLARE_TUNNEL_ID
        );
        assert_eq!(
            by_name["tunnel"].environment["STARRING_D2_TUNNEL_KEYCHAIN_ACCOUNT"],
            "tunnel"
        );
        assert_eq!(by_name["tunnel"].expected_plist["Umask"], 63);
        assert_eq!(by_name["tunnel"].expected_plist["ExitTimeOut"], 90);
    }

    #[test]
    fn launchd_parser_and_binding_reject_live_environment_drift() {
        let manifest = valid_manifest();
        let labels = expected_service_labels("012345abcdef");
        let services = build_candidate_service_bindings(
            &manifest,
            Path::new("/evidence/d2-20260812t010203z-012345abcdef"),
            Path::new("/private/tmp/starring-d2-d2-20260812t010203z-012345abcdef"),
            &labels,
            Path::new("/Users/operator"),
        )
        .unwrap();
        let worker = services
            .iter()
            .find(|service| service.name == "worker")
            .unwrap();
        let render = |override_codex: Option<&str>| {
            let mut environment = worker.environment.clone();
            if let Some(value) = override_codex {
                environment.insert("STARRING_CODEX_PATH".to_string(), value.to_string());
            }
            environment.insert("OSLogRateLimit".to_string(), "64".to_string());
            environment.insert("XPC_SERVICE_NAME".to_string(), worker.label.clone());
            let environment = environment
                .iter()
                .map(|(key, value)| format!("\t\t{key} => {value}\n"))
                .collect::<String>();
            let arguments = worker
                .arguments
                .iter()
                .map(|argument| format!("\t\t{argument}\n"))
                .collect::<String>();
            format!(
                "gui/501/{label} = {{\n\
                 \tpath = {plist}\n\
                 \tstate = running\n\n\
                 \tprogram = {program}\n\
                 \targuments = {{\n{arguments}\t}}\n\n\
                 \tworking directory = {working}\n\n\
                 \tstdout path = {log}\n\
                 \tstderr path = {log}\n\
                 \tenvironment = {{\n{environment}\t}}\n\n\
                 \tumask = 77\n\
                 \tminimum runtime = 30\n\
                 \texit timeout = 60\n\
                 \truns = 1\n\
                 \tpid = 1234\n\
                 \tlast exit code = (never exited)\n\
                 \tresource limits = {{\n\
                 \t\tmaxfiles (soft) => 2048\n\
                 \t\tmaxfiles (hard) => 4096\n\
                 \t}}\n\
                 }}\n",
                label = worker.label,
                plist = worker.plist_path.display(),
                program = worker.configured_program,
                working = worker.working_directory,
                log = worker.log_path,
            )
        };
        let exact = parse_launchd_job(&render(None)).unwrap();
        assert_eq!(worker.expected_plist["ExitTimeOut"], 90);
        assert_eq!(exact.exit_timeout, 60);
        assert_eq!(validate_launchd_binding(worker, &exact), Ok(()));
        let drifted = parse_launchd_job(&render(Some("/tmp/untrusted-codex"))).unwrap();
        assert_eq!(
            validate_launchd_binding(worker, &drifted),
            Err(IssuerError::CandidateServiceInactive)
        );
    }

    #[test]
    fn api_loopback_probe_keeps_the_request_side_open_until_the_response() {
        use std::io::{ErrorKind, Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 512];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    return false;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let mut trailing = [0_u8; 1];
            match stream.read(&mut trailing) {
                Ok(0) => false,
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                    true
                }
                _ => false,
            }
        });

        assert_eq!(
            validate_api_loopback_ready_at(address, "d2-api.starring.co.kr"),
            Ok(())
        );
        assert!(server.join().unwrap());
    }

    #[test]
    fn api_loopback_probe_reports_empty_and_non_ready_responses_exactly() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        fn serve(response: &'static [u8]) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 512];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                if !response.is_empty() {
                    stream.write_all(response).unwrap();
                }
            });
            (address, server)
        }

        let (address, server) = serve(b"");
        assert_eq!(
            validate_api_loopback_ready_at(address, "d2-api.starring.co.kr"),
            Err(IssuerError::ApiLoopbackResponseEmpty)
        );
        server.join().unwrap();

        let (address, server) = serve(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(
            validate_api_loopback_ready_at(address, "d2-api.starring.co.kr"),
            Err(IssuerError::ApiLoopbackStatus)
        );
        server.join().unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn plist_conversion_preserves_the_complete_dictionary() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>exact</string><key>RunAtLoad</key><false/><key>Umask</key><integer>63</integer></dict></plist>"#;
        assert_eq!(
            plist_json_value(xml).unwrap(),
            json!({"Label": "exact", "RunAtLoad": false, "Umask": 63})
        );
    }

    #[test]
    fn pure_scenario_validation_rejects_non_object_and_prototype_keys() {
        assert_eq!(validate_scenario_value(&json!({"prompt": "hello"})), Ok(()));
        assert_eq!(
            validate_scenario_value(&json!(["not", "an", "object"])),
            Err(IssuerError::Scenario)
        );
        assert_eq!(
            validate_scenario_value(&json!({"nested": {"__proto__": {}}})),
            Err(IssuerError::Scenario)
        );
    }

    #[test]
    fn redaction_removes_exact_credentials_urls_and_sensitive_fields() {
        let session = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let csrf = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
        let raw = format!(
            "{{\"ok\":true,\"echo\":\"prefix-{session}-suffix\",\"session\":\"{session}\",\"csrf\":\"{csrf}\",\"debug\":\"postgresql://role:password@127.0.0.1:55433/db?sslmode=disable\"}}"
        );
        let redacted = redact_public_evidence(raw.as_bytes(), &[session, csrf]).unwrap();
        let encoded = serde_json::to_string(&redacted).unwrap();
        assert!(!encoded.contains(session));
        assert!(!encoded.contains(csrf));
        assert!(!encoded.contains("postgresql://"));
        assert_eq!(redacted["session"], "<redacted>");
        assert_eq!(redacted["csrf"], "<redacted>");
        assert_eq!(redacted["echo"], "prefix-<redacted>-suffix");
    }

    #[test]
    fn redaction_rejects_non_json_and_oversized_evidence() {
        assert_eq!(
            redact_public_evidence(b"not-json", &[]),
            Err(IssuerError::Evidence)
        );
        let oversized = vec![b'x'; MAX_CHILD_OUTPUT_BYTES + 1];
        assert_eq!(
            redact_public_evidence(&oversized, &[]),
            Err(IssuerError::EvidenceTooLarge)
        );
    }

    #[test]
    fn argument_parser_requires_explicit_operation_and_child_separator() {
        let parsed = parse_arguments([
            "--manifest".to_string(),
            "/absolute/manifest.json".to_string(),
            "--operation".to_string(),
            "auth-smoke".to_string(),
            "--".to_string(),
            "/absolute/node".to_string(),
            "/absolute/runner.mjs".to_string(),
        ])
        .unwrap();
        assert_eq!(parsed.operation, Operation::AuthSmoke);
        assert_eq!(parsed.child.len(), 2);
        assert!(parsed.display_name.is_none());
        let onboarding = parse_arguments([
            "--manifest".to_string(),
            "/absolute/manifest.json".to_string(),
            "--operation".to_string(),
            "direct-onboard".to_string(),
            "--display-name".to_string(),
            "보건".to_string(),
        ])
        .unwrap();
        assert_eq!(onboarding.operation, Operation::DirectOnboard);
        assert_eq!(onboarding.display_name.as_deref(), Some("보건"));
        assert!(onboarding.child.is_empty());
        assert_eq!(
            parse_arguments([
                "--manifest".to_string(),
                "/absolute/manifest.json".to_string(),
                "--".to_string(),
                "/absolute/node".to_string(),
            ])
            .unwrap_err(),
            IssuerError::ArgumentsInvalid
        );
        assert_eq!(
            parse_arguments([
                "--manifest".to_string(),
                "/absolute/manifest.json".to_string(),
                "--operation".to_string(),
                "direct-onboard".to_string(),
                "--display-name".to_string(),
                " leading".to_string(),
            ])
            .unwrap_err(),
            IssuerError::ArgumentsInvalid
        );
        assert_eq!(
            parse_arguments([
                "--manifest".to_string(),
                "/absolute/manifest.json".to_string(),
                "--operation".to_string(),
                "direct-onboard".to_string(),
                "--display-name".to_string(),
                "보건".to_string(),
                "--".to_string(),
                "/absolute/runner".to_string(),
            ])
            .unwrap_err(),
            IssuerError::ArgumentsInvalid
        );
    }

    #[test]
    fn direct_onboarding_evidence_has_the_exact_non_release_schema() {
        let evidence = DirectOnboardingEvidence {
            schema_version: 1,
            kind: DIRECT_ONBOARDING_EVIDENCE_KIND.to_string(),
            certification_class: AUTOMATED_MAINTENANCE_CLASS.to_string(),
            operation: Operation::DirectOnboard.as_str().to_string(),
            observed_at: "2026-08-12T01:02:03Z".to_string(),
            run_id: "d2-20260812t010203z-012345abcdef".to_string(),
            manifest_sha256: "a".repeat(64),
            principal_id: "discord:111111111111111111".to_string(),
            guild_id: "333333333333333333".to_string(),
            discord_application_id: "222222222222222222".to_string(),
            hub_channel_id: "444444444444444444".to_string(),
            binding_key: "community_hub".to_string(),
            installation_id: "installation:starring-d2-20260812-012345abcdef".to_string(),
            outcome: "fresh".to_string(),
            provisioner_sha256: "b".repeat(64),
            issuer_sha256: "c".repeat(64),
            issuer_source_sha256: "d".repeat(64),
            discord_hub_preflight: true,
            direct_auth_used: true,
            session_revoked: true,
            release_eligible: false,
        };
        let value = serde_json::to_value(evidence).unwrap();
        assert_eq!(
            value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "binding_key",
                "certification_class",
                "direct_auth_used",
                "discord_application_id",
                "discord_hub_preflight",
                "guild_id",
                "hub_channel_id",
                "installation_id",
                "issuer_sha256",
                "issuer_source_sha256",
                "kind",
                "manifest_sha256",
                "observed_at",
                "operation",
                "outcome",
                "principal_id",
                "provisioner_sha256",
                "release_eligible",
                "run_id",
                "schema_version",
                "session_revoked",
            ])
        );
        assert!(value.get("session").is_none());
        assert!(value.get("csrf").is_none());
        assert!(value.get("binding_fingerprint").is_none());
        let mut unexpected = value;
        unexpected
            .as_object_mut()
            .unwrap()
            .insert("commercial_evidence".to_string(), json!(true));
        assert!(serde_json::from_value::<DirectOnboardingEvidence>(unexpected).is_err());
    }

    #[test]
    fn generated_secrets_are_distinct_base64url_credentials() {
        let secrets = generate_four_secrets().unwrap();
        let set = secrets
            .iter()
            .map(|value| value.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(set.len(), 4);
        for secret in secrets {
            assert_eq!(secret.len(), 43);
            assert_eq!(URL_SAFE_NO_PAD.decode(secret.as_bytes()).unwrap().len(), 32);
            let _ = credential_digest(secret.as_str());
        }
    }

    #[test]
    fn scenario_digest_is_over_exact_raw_bytes() {
        let compact = br#"{"prompt":"test"}"#;
        let pretty = b"{\n  \"prompt\": \"test\"\n}\n";
        assert_ne!(hex_digest(compact), hex_digest(pretty));
        assert_eq!(hex_digest(compact), hex_digest(compact));
    }

    #[test]
    fn session_id_prefix_reserves_exact_sixteen_hex_suffix_space() {
        assert!(valid_session_id_prefix("a"));
        assert!(valid_session_id_prefix(&format!("a{}", "b".repeat(110))));
        assert!(!valid_session_id_prefix(&format!("a{}", "b".repeat(111))));
        assert!(!valid_session_id_prefix("-leading"));
        assert!(!valid_session_id_prefix("contains:colon"));
    }

    #[test]
    fn taint_replay_requires_exact_owned_mode_0600_bytes() {
        struct RemoveOnDrop(PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }

        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let path = std::env::temp_dir().join(format!(
            "starring-d2a-taint-test-{}-{}",
            std::process::id(),
            u64::from_ne_bytes(nonce)
        ));
        let _cleanup = RemoveOnDrop(path.clone());
        let expected = b"{\"release_eligible\":false}\n";
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        file.write_all(expected).unwrap();
        file.sync_all().unwrap();
        drop(file);

        let uid = current_uid().unwrap();
        assert_eq!(validate_existing_taint(&path, uid, expected), Ok(()));
        assert_eq!(
            validate_existing_taint(&path, uid, b"{\"release_eligible\":true}\n"),
            Err(IssuerError::D2aTaint)
        );
    }

    #[test]
    fn taint_marker_bytes_match_python_insertion_order_contract() {
        let manifest_sha256 = "a".repeat(64);
        let marker = D2aTaintMarker {
            schema_version: 1,
            kind: "starring.d2a.run-taint.v1",
            run_id: "d2-20260812t010203z-012345abcdef",
            manifest_sha256: &manifest_sha256,
            certification_class: "automated_maintenance_v1",
            direct_auth_used: true,
            release_eligible: false,
            issuer_sha256: "b".repeat(64),
            issuer_source_sha256: "f".repeat(64),
            runner_sha256: "c".repeat(64),
            product_driver_sha256: "d".repeat(64),
            scenario_sha256: "e".repeat(64),
        };
        let mut payload = serde_json::to_vec(&marker).unwrap();
        payload.push(b'\n');
        let expected = format!(
            "{{\"schema_version\":1,\"kind\":\"starring.d2a.run-taint.v1\",\
             \"run_id\":\"d2-20260812t010203z-012345abcdef\",\
             \"manifest_sha256\":\"{}\",\
             \"certification_class\":\"automated_maintenance_v1\",\
             \"direct_auth_used\":true,\"release_eligible\":false,\
             \"issuer_sha256\":\"{}\",\"issuer_source_sha256\":\"{}\",\
             \"runner_sha256\":\"{}\",\
             \"product_driver_sha256\":\"{}\",\"scenario_sha256\":\"{}\"}}\n",
            "a".repeat(64),
            "b".repeat(64),
            "f".repeat(64),
            "c".repeat(64),
            "d".repeat(64),
            "e".repeat(64),
        );
        assert_eq!(payload, expected.as_bytes());
    }

    fn active_session_lifecycle_marker(uid: u32) -> SessionLifecycleMarker {
        SessionLifecycleMarker {
            schema_version: 1,
            kind: SESSION_LIFECYCLE_KIND.to_string(),
            run_id: "d2-20260812t010203z-012345abcdef".to_string(),
            manifest_sha256: "a".repeat(64),
            operation: "one-shot".to_string(),
            origin: SessionLifecycleOrigin::Issuer,
            issuer_sha256: "b".repeat(64),
            issuer_source_sha256: "c".repeat(64),
            uid,
            boot_identity: "darwin-boottime:1234567890:123456".to_string(),
            process_group_id: Some(4242),
            started_at: "2026-08-12T01:02:03.123456789Z".to_string(),
            status: SessionLifecycleStatus::Active,
            session_revoked: false,
            revoked_at: None,
            quarantined_at: None,
        }
    }

    #[test]
    fn session_lifecycle_marker_has_exact_non_secret_canonical_schema() {
        let marker = active_session_lifecycle_marker(501);
        let mut payload = serde_json::to_vec(&marker).unwrap();
        payload.push(b'\n');
        let expected = format!(
            "{{\"schema_version\":1,\"kind\":\"starring.d2a.session-lifecycle.v1\",\
             \"run_id\":\"d2-20260812t010203z-012345abcdef\",\
             \"manifest_sha256\":\"{}\",\"operation\":\"one-shot\",\"origin\":\"issuer\",\
             \"issuer_sha256\":\"{}\",\"issuer_source_sha256\":\"{}\",\
             \"uid\":501,\"boot_identity\":\"darwin-boottime:1234567890:123456\",\
             \"process_group_id\":4242,\"started_at\":\"2026-08-12T01:02:03.123456789Z\",\
             \"status\":\"active\",\"session_revoked\":false,\"revoked_at\":null,\
             \"quarantined_at\":null}}\n",
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
        );
        assert_eq!(payload, expected.as_bytes());
        let value = serde_json::to_value(marker).unwrap();
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            keys,
            BTreeSet::from([
                "boot_identity",
                "issuer_sha256",
                "issuer_source_sha256",
                "kind",
                "manifest_sha256",
                "operation",
                "origin",
                "process_group_id",
                "quarantined_at",
                "revoked_at",
                "run_id",
                "schema_version",
                "session_revoked",
                "started_at",
                "status",
                "uid",
            ])
        );
        for forbidden in [
            "session",
            "session_digest",
            "credential_digest",
            "fingerprint",
            "raw_credential",
        ] {
            assert!(value.get(forbidden).is_none());
        }
    }

    #[test]
    fn session_lifecycle_states_and_reentry_are_fail_closed() {
        let mut marker = active_session_lifecycle_marker(501);
        assert!(valid_session_lifecycle_state(&marker));
        marker.process_group_id = None;
        assert!(!valid_session_lifecycle_state(&marker));
        marker.process_group_id = Some(4242);
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, &marker.boot_identity, |_| {
                panic!("active markers must not probe a process group")
            }),
            Err(IssuerError::ManualRecoveryRequired)
        );

        marker.status = SessionLifecycleStatus::Quarantined;
        marker.quarantined_at = Some("2026-08-12T01:03:03.000000000Z".to_string());
        assert!(valid_session_lifecycle_state(&marker));
        marker.process_group_id = None;
        assert!(!valid_session_lifecycle_state(&marker));
        marker.process_group_id = Some(4242);
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, &marker.boot_identity, |_| {
                panic!("quarantined markers must not probe a process group")
            }),
            Err(IssuerError::ManualRecoveryRequired)
        );

        marker.status = SessionLifecycleStatus::NotIssued;
        marker.quarantined_at = None;
        assert!(valid_session_lifecycle_state(&marker));
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, &marker.boot_identity, |_| true),
            Ok(())
        );
        marker.origin = SessionLifecycleOrigin::Bootstrap;
        marker.process_group_id = None;
        assert!(!valid_session_lifecycle_state(&marker));
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, &marker.boot_identity, |_| {
                panic!("non-direct bootstrap markers must not probe a pgid")
            }),
            Err(IssuerError::SessionLifecycle)
        );
        marker.operation = "direct-onboard".to_string();
        assert!(valid_session_lifecycle_state(&marker));
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, &marker.boot_identity, |_| {
                panic!("parent-created not_issued/null must not probe a pgid")
            }),
            Ok(())
        );
        marker.status = SessionLifecycleStatus::Active;
        assert!(!valid_session_lifecycle_state(&marker));
        marker.status = SessionLifecycleStatus::NotIssued;
        marker.origin = SessionLifecycleOrigin::Issuer;
        assert!(!valid_session_lifecycle_state(&marker));
        marker.origin = SessionLifecycleOrigin::Bootstrap;

        marker.status = SessionLifecycleStatus::Revoked;
        marker.origin = SessionLifecycleOrigin::Issuer;
        marker.session_revoked = true;
        marker.revoked_at = Some("2026-08-12T01:03:03.000000000Z".to_string());
        assert!(!valid_session_lifecycle_state(&marker));
        marker.process_group_id = Some(4242);
        assert!(valid_session_lifecycle_state(&marker));
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, &marker.boot_identity, |_| true),
            Ok(())
        );
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, &marker.boot_identity, |_| false),
            Err(IssuerError::ManualRecoveryRequired)
        );

        let probed = std::cell::Cell::new(false);
        assert_eq!(
            require_safe_lifecycle_reentry(&marker, "darwin-boottime:999999999:1", |_| {
                probed.set(true);
                false
            }),
            Ok(())
        );
        assert!(!probed.get(), "prior-boot pgids must never be probed");

        marker.session_revoked = false;
        assert!(!valid_session_lifecycle_state(&marker));
        assert_eq!(
            IssuerError::ManualRecoveryRequired.to_string(),
            "manual_recovery_required"
        );
    }

    #[test]
    fn direct_onboarding_requires_the_exact_bootstrap_handoff_marker() {
        let mut marker = active_session_lifecycle_marker(501);
        assert_eq!(
            require_lifecycle_operation_handoff(None, Operation::DirectOnboard),
            Err(IssuerError::SessionLifecycle),
            "a deleted or never-created bootstrap sentinel must fail closed"
        );
        assert_eq!(
            require_lifecycle_operation_handoff(Some(&marker), Operation::DirectOnboard),
            Err(IssuerError::SessionLifecycle)
        );
        marker.origin = SessionLifecycleOrigin::Bootstrap;
        marker.operation = "auth-smoke".to_string();
        assert_eq!(
            require_lifecycle_operation_handoff(Some(&marker), Operation::DirectOnboard),
            Err(IssuerError::SessionLifecycle)
        );
        marker.operation = "direct-onboard".to_string();
        assert_eq!(
            require_lifecycle_operation_handoff(Some(&marker), Operation::DirectOnboard),
            Ok(())
        );
        assert_eq!(
            require_lifecycle_operation_handoff(Some(&marker), Operation::AuthSmoke),
            Err(IssuerError::SessionLifecycle)
        );
        marker.origin = SessionLifecycleOrigin::Issuer;
        assert_eq!(
            require_lifecycle_operation_handoff(None, Operation::AuthSmoke),
            Err(IssuerError::SessionLifecycle),
            "deleting an issuer terminal must never create a fresh lifecycle"
        );
        assert_eq!(
            require_lifecycle_operation_handoff(Some(&marker), Operation::OneShot),
            Ok(())
        );
    }

    #[test]
    fn parent_not_issued_null_is_atomically_taken_over_by_active_issuer_identity() {
        struct RemoveDirectoryOnDrop(PathBuf);
        impl Drop for RemoveDirectoryOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "starring-d2-parent-lifecycle-test-{}-{}",
            std::process::id(),
            u64::from_ne_bytes(nonce)
        ));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let _cleanup = RemoveDirectoryOnDrop(directory.clone());
        let path = directory.join(SESSION_LIFECYCLE_NAME);
        let uid = current_uid().unwrap();

        let mut parent = active_session_lifecycle_marker(uid);
        parent.operation = "direct-onboard".to_string();
        parent.status = SessionLifecycleStatus::NotIssued;
        parent.origin = SessionLifecycleOrigin::Bootstrap;
        parent.process_group_id = None;
        assert!(valid_session_lifecycle_state(&parent));
        write_session_lifecycle_marker(&path, &directory, uid, &parent).unwrap();
        assert_eq!(
            require_safe_lifecycle_reentry(&parent, &parent.boot_identity, |_| {
                panic!("null parent marker must not probe")
            }),
            Ok(())
        );

        let stale_snapshot = read_session_lifecycle_snapshot(&path, uid).unwrap();
        let mut raced = parent.clone();
        raced.started_at = "2026-08-12T01:02:03.500000000Z".to_string();
        write_session_lifecycle_marker(&path, &directory, uid, &raced).unwrap();

        let mut active = parent.clone();
        active.status = SessionLifecycleStatus::Active;
        active.origin = SessionLifecycleOrigin::Issuer;
        active.process_group_id = Some(5252);
        active.started_at = "2026-08-12T01:02:04.000000000Z".to_string();
        assert!(valid_session_lifecycle_state(&active));
        assert_eq!(
            write_session_lifecycle_marker_cas(
                &path,
                &directory,
                uid,
                &active,
                Some(&stale_snapshot),
            ),
            Err(IssuerError::SessionLifecycle)
        );
        assert_eq!(
            read_session_lifecycle_marker(&path, uid).unwrap(),
            raced,
            "a marker mutated after validation must never be overwritten"
        );

        assert_eq!(
            write_session_lifecycle_marker_cas(&path, &directory, uid, &active, None),
            Err(IssuerError::SessionLifecycle)
        );
        assert_eq!(
            read_session_lifecycle_marker(&path, uid).unwrap(),
            raced,
            "absence publication must never replace an existing marker"
        );

        write_session_lifecycle_marker(&path, &directory, uid, &parent).unwrap();
        let current = read_session_lifecycle_snapshot(&path, uid).unwrap();
        write_session_lifecycle_marker_cas(&path, &directory, uid, &active, Some(&current))
            .unwrap();
        let observed = read_session_lifecycle_marker(&path, uid).unwrap();
        assert_eq!(observed.status, SessionLifecycleStatus::Active);
        assert_eq!(observed.process_group_id, Some(5252));
    }

    #[test]
    fn bootstrap_not_issued_status_uses_the_cross_language_snake_case_contract() {
        let uid = current_uid().unwrap();
        let mut marker = active_session_lifecycle_marker(uid);
        marker.operation = "direct-onboard".to_string();
        marker.origin = SessionLifecycleOrigin::Bootstrap;
        marker.status = SessionLifecycleStatus::NotIssued;
        marker.process_group_id = None;
        assert!(valid_session_lifecycle_state(&marker));

        let payload = serde_json::to_vec(&marker).unwrap();
        assert!(payload
            .windows(b"\"status\":\"not_issued\"".len())
            .any(|window| window == b"\"status\":\"not_issued\""));
        let parsed: SessionLifecycleMarker = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed, marker);

        let legacy = String::from_utf8(payload)
            .unwrap()
            .replace("\"status\":\"not_issued\"", "\"status\":\"notissued\"");
        assert!(serde_json::from_str::<SessionLifecycleMarker>(&legacy).is_err());
    }

    #[test]
    fn session_lifecycle_transitions_are_atomic_exact_and_terminal() {
        struct RemoveDirectoryOnDrop(PathBuf);
        impl Drop for RemoveDirectoryOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "starring-d2-lifecycle-test-{}-{}",
            std::process::id(),
            u64::from_ne_bytes(nonce)
        ));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let _cleanup = RemoveDirectoryOnDrop(directory.clone());
        let path = directory.join(SESSION_LIFECYCLE_NAME);
        let uid = current_uid().unwrap();

        let marker = active_session_lifecycle_marker(uid);
        write_session_lifecycle_marker(&path, &directory, uid, &marker).unwrap();
        let mut lifecycle = SessionLifecycle {
            path: path.clone(),
            uid,
            marker,
            issuance_attempted: true,
        };
        lifecycle.mark_revoked().unwrap();
        let revoked = read_session_lifecycle_marker(&path, uid).unwrap();
        assert_eq!(revoked.status, SessionLifecycleStatus::Revoked);
        assert!(revoked.session_revoked);
        assert!(revoked.revoked_at.is_some());
        assert!(revoked.quarantined_at.is_none());
        assert_eq!(lifecycle.finish_error(), Ok(()));
        assert_eq!(read_session_lifecycle_marker(&path, uid).unwrap(), revoked);

        let marker = active_session_lifecycle_marker(uid);
        write_session_lifecycle_marker(&path, &directory, uid, &marker).unwrap();
        let mut lifecycle = SessionLifecycle {
            path: path.clone(),
            uid,
            marker,
            issuance_attempted: false,
        };
        lifecycle.finish_error().unwrap();
        let not_issued = read_session_lifecycle_marker(&path, uid).unwrap();
        assert_eq!(not_issued.status, SessionLifecycleStatus::NotIssued);
        assert!(!not_issued.session_revoked);
        assert!(not_issued.revoked_at.is_none());
        assert!(not_issued.quarantined_at.is_none());

        let marker = active_session_lifecycle_marker(uid);
        write_session_lifecycle_marker(&path, &directory, uid, &marker).unwrap();
        let mut lifecycle = SessionLifecycle {
            path: path.clone(),
            uid,
            marker,
            issuance_attempted: true,
        };
        lifecycle.finish_error().unwrap();
        let quarantined = read_session_lifecycle_marker(&path, uid).unwrap();
        assert_eq!(quarantined.status, SessionLifecycleStatus::Quarantined);
        assert!(!quarantined.session_revoked);
        assert!(quarantined.revoked_at.is_none());
        assert!(quarantined.quarantined_at.is_some());
        assert_eq!(lifecycle.mark_revoked(), Err(IssuerError::SessionLifecycle));
    }

    #[test]
    fn sigkill_lifecycle_helper() {
        let Some(directory) = std::env::var_os("STARRING_D2_SIGKILL_TEST_DIRECTORY") else {
            return;
        };
        let directory = PathBuf::from(directory);
        let path = directory.join(SESSION_LIFECYCLE_NAME);
        let uid = current_uid().unwrap();
        let marker = active_session_lifecycle_marker(uid);
        write_session_lifecycle_marker(&path, &directory, uid, &marker).unwrap();
        let _lifecycle = SessionLifecycle {
            path,
            uid,
            marker,
            issuance_attempted: false,
        };
        let ready = directory.join("ready");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(ready)
            .unwrap();
        file.write_all(b"ready\n").unwrap();
        file.sync_all().unwrap();
        std::thread::sleep(Duration::from_secs(30));
        panic!("SIGKILL lifecycle helper was not terminated");
    }

    #[test]
    fn sigkill_leaves_the_last_fsynced_active_marker_unchanged() {
        struct RemoveDirectoryOnDrop(PathBuf);
        impl Drop for RemoveDirectoryOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "starring-d2-sigkill-marker-test-{}-{}",
            std::process::id(),
            u64::from_ne_bytes(nonce)
        ));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let _cleanup = RemoveDirectoryOnDrop(directory.clone());
        let path = directory.join(SESSION_LIFECYCLE_NAME);
        let uid = current_uid().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::sigkill_lifecycle_helper",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("STARRING_D2_SIGKILL_TEST_DIRECTORY", &directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let ready = directory.join("ready");
        for _ in 0..500 {
            if ready.is_file() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !ready.is_file() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("SIGKILL helper did not activate its marker");
        }
        assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGKILL) }, 0);
        let status = child.wait().unwrap();
        assert!(!status.success());

        // SIGKILL cannot execute Rust cleanup or a transition; the durable bytes remain
        // active and force manual recovery on the next invocation.
        let observed = read_session_lifecycle_marker(&path, uid).unwrap();
        assert_eq!(observed.status, SessionLifecycleStatus::Active);
        assert!(!observed.session_revoked);
    }

    #[test]
    fn issuer_requires_pid_process_group_and_session_identity() {
        assert_eq!(validate_dedicated_process_session(42, 42, 42), Ok(()));
        assert_eq!(
            validate_dedicated_process_session(42, 41, 42),
            Err(IssuerError::ProcessIsolation)
        );
        assert_eq!(
            validate_dedicated_process_session(42, 42, 41),
            Err(IssuerError::ProcessIsolation)
        );
        assert_eq!(
            IssuerError::ProcessIsolation.to_string(),
            "issuer_process_isolation_required"
        );
    }

    #[test]
    fn stable_diagnostics_are_allowlisted_secret_free_codes() {
        for (error, expected) in [
            (
                IssuerError::ApiLoopbackOrigin,
                "api_loopback_origin_invalid",
            ),
            (
                IssuerError::ApiLoopbackConnect,
                "api_loopback_connect_failed",
            ),
            (IssuerError::ApiLoopbackWrite, "api_loopback_write_failed"),
            (IssuerError::ApiLoopbackRead, "api_loopback_read_failed"),
            (
                IssuerError::ApiLoopbackResponseEmpty,
                "api_loopback_response_empty",
            ),
            (
                IssuerError::ApiLoopbackStatus,
                "api_loopback_status_invalid",
            ),
            (
                IssuerError::DiscordHubPreflight,
                "discord_hub_preflight_failed",
            ),
            (
                IssuerError::SessionLifecycleBinary,
                "session_lifecycle_binary_invalid",
            ),
            (
                IssuerError::SessionLifecycleSource,
                "session_lifecycle_source_invalid",
            ),
            (
                IssuerError::SessionLifecycleBootIdentity,
                "session_lifecycle_boot_identity_invalid",
            ),
            (
                IssuerError::SessionLifecycleExistingMarker,
                "session_lifecycle_existing_marker_invalid",
            ),
            (
                IssuerError::SessionLifecycleHandoff,
                "session_lifecycle_handoff_invalid",
            ),
            (
                IssuerError::SessionLifecycleReentry,
                "session_lifecycle_reentry_invalid",
            ),
            (
                IssuerError::SessionLifecycleCas,
                "session_lifecycle_cas_failed",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(expected
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_'));
        }
    }

    #[test]
    fn teardown_fence_accepts_the_orchestrator_sorted_canonical_contract() {
        struct RemoveOnDrop(PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }

        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let path = std::env::temp_dir().join(format!(
            "starring-d2-teardown-fence-test-{}-{}",
            std::process::id(),
            u64::from_ne_bytes(nonce)
        ));
        let _cleanup = RemoveOnDrop(path.clone());
        let payload = format!(
            "{{\"kind\":\"starring.d2a.teardown-fence.v1\",\"manifest_sha256\":\"{}\",\
             \"run_id\":\"d2-20260812t010203z-012345abcdef\",\"schema_version\":1,\
             \"status\":\"closing\",\"updated_at\":\"2026-08-12T01:02:03.000000000Z\"}}\n",
            "a".repeat(64)
        );
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        file.write_all(payload.as_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let fence = read_d2a_teardown_fence(&path, current_uid().unwrap()).unwrap();
        assert_eq!(fence.status, D2aTeardownFenceStatus::Closing);
    }

    #[test]
    fn issuer_source_digest_is_domain_separated_and_complete() {
        let observed = issuer_source_digest();
        assert!(valid_digest(&observed));
        let mut incomplete = Sha256::new();
        incomplete.update(ISSUER_SOURCE_DIGEST_DOMAIN);
        incomplete.update(ISSUER_CARGO_TOML_BYTES);
        assert_ne!(observed, hex_bytes(&incomplete.finalize()));
    }

    #[test]
    fn exclusive_lock_is_nonblocking_and_reports_stable_busy_error() {
        struct RemoveOnDrop(PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }

        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let path = std::env::temp_dir().join(format!(
            "starring-d2-lock-test-{}-{}",
            std::process::id(),
            u64::from_ne_bytes(nonce)
        ));
        let _cleanup = RemoveOnDrop(path.clone());
        let uid = current_uid().unwrap();
        let (first, created) = acquire_owned_exclusive_lock(&path, uid).unwrap();
        assert!(created);
        assert!(matches!(
            acquire_owned_exclusive_lock(&path, uid),
            Err(IssuerError::D2OperationBusy)
        ));
        assert_eq!(
            IssuerError::D2OperationBusy.to_string(),
            "d2_operation_busy"
        );
        drop(first);
        assert!(acquire_owned_exclusive_lock(&path, uid).is_ok());
    }

    #[test]
    fn core_dump_policy_sets_and_rechecks_both_limits_at_zero() {
        let source = include_str!("lib.rs");
        let start = source
            .find("pub fn disable_core_dumps()")
            .expect("core dump policy function");
        let end = source[start..]
            .find("\n#[derive(Serialize)]")
            .map(|offset| start + offset)
            .expect("next item after core dump policy");
        let policy = &source[start..end];
        assert!(policy.contains("libc::setrlimit(libc::RLIMIT_CORE"));
        assert!(policy.contains("libc::getrlimit(libc::RLIMIT_CORE"));
        assert!(policy.matches("rlim_cur: 0").count() >= 1);
        assert!(policy.matches("rlim_max: 0").count() >= 1);
        assert!(policy.contains("observed.rlim_cur != 0"));
        assert!(policy.contains("observed.rlim_max != 0"));
    }
}
