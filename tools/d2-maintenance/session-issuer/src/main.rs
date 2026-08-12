use std::future::Future;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use starring_d2_session_issuer::{
    acquire_global_operation_lock, acquire_run_coordinator_lock, credential_digest,
    disable_core_dumps, generate_four_secrets, load_discord_bot_token, load_run_credentials,
    load_validated_run, parse_arguments, persist_d2a_taint, persist_direct_onboarding_evidence,
    redact_public_evidence, rehash_sealed_provisioner, reject_ambient_postgres_environment,
    reject_commercial_onboarding_artifacts, require_d2a_taint, require_dedicated_process_session,
    require_direct_onboarding_evidence, require_open_d2a_teardown_fence, validate_child_command,
    validate_locked_run, validate_scenario_path, Arguments, DatabaseCredential, IssuerError,
    Operation, RunCredentials, SessionLifecycle, ValidatedRun, ValidatedScenario, DATABASE_NAME,
    MAX_CHILD_OUTPUT_BYTES, SESSION_LIFETIME_SECONDS,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use zeroize::Zeroizing;

const FLOW_LIFETIME_SECONDS: f64 = 60.0;
const CHILD_TIMEOUT: Duration = Duration::from_secs(540);
const MAX_CHILD_INPUT_BYTES: usize = 64 * 1024;
const STATEMENT_TIMEOUT: &str = "5s";
const LOCK_TIMEOUT: &str = "2s";
const REVOKE_ATTEMPTS: usize = 5;
const REVOKE_RETRY_DELAY: Duration = Duration::from_millis(100);
const DIRECT_ONBOARDING_TIMEOUT: Duration = Duration::from_secs(90);
const DIRECT_ONBOARDING_SESSION_LIFETIME_SECONDS: f64 = 120.0;
const DISCORD_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(13);
const MAX_ONBOARDING_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_PROVISIONER_STDERR_BYTES: usize = 256;
const MAX_DISCORD_OUTPUT_BYTES: usize = 20 * 1024;

type CreatedFlowRow = (
    String,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);
type ConsumedFlowRow = (
    String,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
);
type IssuedSessionRow = (
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

#[derive(Serialize)]
struct ChildInput<'a> {
    schema_version: u8,
    session: &'a str,
    csrf: &'a str,
    public_origin: &'a str,
    principal_id: &'a str,
    guild_id: &'a str,
    installation_id: &'a str,
    run_id: &'a str,
    manifest_sha256: &'a str,
    operation: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    authoring_session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario_sha256: Option<&'a str>,
}

struct Capture {
    bytes: Vec<u8>,
    exceeded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedOnboardingOutput {
    outcome: String,
    installation_id: String,
    principal_id: String,
    binding_key: String,
    hub_channel_id: String,
}

enum LifecycleDisposition<T> {
    Completed(T),
    Interrupted,
}

trait ConfirmedRevocationMarker {
    fn confirm_revoked(&mut self) -> Result<(), IssuerError>;
}

impl ConfirmedRevocationMarker for SessionLifecycle {
    fn confirm_revoked(&mut self) -> Result<(), IssuerError> {
        self.mark_revoked()
    }
}

fn finish_after_confirmed_revocation<T, M>(
    marker: &mut M,
    operation_result: Result<T, IssuerError>,
) -> Result<T, IssuerError>
where
    M: ConfirmedRevocationMarker,
{
    marker.confirm_revoked()?;
    operation_result
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), IssuerError> {
    let arguments = parse_arguments(std::env::args().skip(1))?;
    // The controller must launch this issuer with a new process group and session.  Check
    // before any Keychain value, generated credential, or product session can enter memory.
    require_dedicated_process_session()?;
    // This process-wide lock is intentionally acquired before any run state is trusted.
    // It closes the race with D2 advance/finalize and teardown.
    let _global_lock = acquire_global_operation_lock()?;
    // This irreversible per-process limit is inherited by the Node child. It must be active
    // before Keychain credentials or generated product credentials enter memory.
    disable_core_dumps()?;
    reject_ambient_postgres_environment()?;
    let run = load_validated_run(&arguments.manifest_path)?;
    // Lock order is globally fixed: process-wide D2 operation lock, then this run's
    // coordinator lock. Both guards live until mandatory session revocation completes.
    let _coordinator_lock = acquire_run_coordinator_lock(&run)?;
    validate_locked_run(&run)?;
    // The orchestrator moves this run-local fence to `closing` under the same global lock
    // before teardown.  Rechecking here makes new session issuance and teardown exclusive.
    require_open_d2a_teardown_fence(&run)?;
    // Activate the non-secret marker before any operation-specific provider, candidate,
    // Keychain, or credential path.  Errors before the issuance boundary become not_issued.
    let mut session_lifecycle = SessionLifecycle::begin(&run, arguments.operation)?;
    let operation_result = run_locked_operation(&run, arguments, &mut session_lifecycle).await;
    if operation_result.is_err() {
        session_lifecycle.finish_error()?;
    }
    operation_result
}

async fn run_locked_operation(
    run: &ValidatedRun,
    arguments: Arguments,
    session_lifecycle: &mut SessionLifecycle,
) -> Result<(), IssuerError> {
    if arguments.operation == Operation::DirectOnboard {
        require_d2a_taint(run)?;
        reject_commercial_onboarding_artifacts(run)?;
        return run_direct_onboarding(
            run,
            arguments
                .display_name
                .as_deref()
                .ok_or(IssuerError::ArgumentsInvalid)?,
            session_lifecycle,
        )
        .await;
    }
    require_direct_onboarding_evidence(run)?;
    validate_child_command(run, &arguments.child)?;
    let scenario = arguments
        .scenario_path
        .as_deref()
        .map(|path| validate_scenario_path(path, run.uid, Path::new(&arguments.child[1])))
        .transpose()?;
    persist_d2a_taint(run)?;
    let credentials = load_run_credentials(run)?;
    let [state, browser_nonce, session, csrf] = generate_four_secrets()?;
    let state_digest = credential_digest(state.as_str());
    let browser_nonce_digest = credential_digest(browser_nonce.as_str());
    let session_digest = credential_digest(session.as_str());
    let csrf_digest = credential_digest(csrf.as_str());
    let authoring_session_id = scenario
        .as_ref()
        .map(|scenario| resolved_authoring_session_id(scenario, &session_digest));
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| IssuerError::Child)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| IssuerError::Child)?;
    let lifecycle = select_lifecycle(
        issue_run_and_revoke(
            run,
            &credentials,
            &state_digest,
            &browser_nonce_digest,
            &session_digest,
            &csrf_digest,
            &arguments.child,
            session_lifecycle,
            ChildInput {
                schema_version: 1,
                session: session.as_str(),
                csrf: csrf.as_str(),
                public_origin: &run.public_origin,
                principal_id: &run.principal_id,
                guild_id: &run.guild_id,
                installation_id: &run.installation_id,
                run_id: &run.run_id,
                manifest_sha256: &run.manifest_sha256,
                operation: arguments.operation.as_str(),
                authoring_session_id: authoring_session_id.as_deref(),
                scenario: scenario.as_ref().map(|scenario| &scenario.value),
                scenario_sha256: one_shot_scenario_digest(arguments.operation, scenario.as_ref()),
            },
        ),
        interrupt.recv(),
        terminate.recv(),
    )
    .await;
    let raw_evidence = match lifecycle {
        LifecycleDisposition::Completed(result) => result?,
        LifecycleDisposition::Interrupted => {
            if !session_lifecycle.issuance_attempted() {
                return Err(IssuerError::ChildInterrupted);
            }
            // The signal can race any issuance query or commit acknowledgement. Reconcile
            // with the run-scoped revoker; a confirmed result is safe even though the
            // operation itself remains interrupted.
            revoke_session_with_retry(run, &credentials.security, &session_digest, None).await?;
            return finish_after_confirmed_revocation(
                session_lifecycle,
                Err(IssuerError::ChildInterrupted),
            );
        }
    };

    let oauth_redactions = credentials.oauth.redaction_values();
    let issuer_redactions = credentials.issuer.redaction_values();
    let security_redactions = credentials.security.redaction_values();
    let redactions = [
        state.as_str(),
        browser_nonce.as_str(),
        session.as_str(),
        csrf.as_str(),
        oauth_redactions[0],
        oauth_redactions[1],
        issuer_redactions[0],
        issuer_redactions[1],
        security_redactions[0],
        security_redactions[1],
    ];
    let evidence = redact_public_evidence(&raw_evidence, &redactions)?;
    let encoded = serde_json::to_string(&evidence).map_err(|_| IssuerError::Evidence)?;
    println!("{encoded}");
    Ok(())
}

async fn run_direct_onboarding(
    run: &ValidatedRun,
    display_name: &str,
    session_lifecycle: &mut SessionLifecycle,
) -> Result<(), IssuerError> {
    let bot_token = load_discord_bot_token(run)?;
    preflight_discord_hub(run, bot_token.as_str()).await?;
    drop(bot_token);
    rehash_sealed_provisioner(run)?;

    let credentials = load_run_credentials(run)?;
    let [state, browser_nonce, session, csrf] = generate_four_secrets()?;
    let state_digest = credential_digest(state.as_str());
    let browser_nonce_digest = credential_digest(browser_nonce.as_str());
    let session_digest = credential_digest(session.as_str());
    let csrf_digest = credential_digest(csrf.as_str());
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| IssuerError::DirectOnboarding)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| IssuerError::DirectOnboarding)?;
    let lifecycle = select_lifecycle(
        issue_onboarding_and_revoke(
            run,
            &credentials,
            &state_digest,
            &browser_nonce_digest,
            &session_digest,
            &csrf_digest,
            display_name,
            session_lifecycle,
        ),
        interrupt.recv(),
        terminate.recv(),
    )
    .await;
    let outcome = match lifecycle {
        LifecycleDisposition::Completed(result) => result?,
        LifecycleDisposition::Interrupted => {
            if !session_lifecycle.issuance_attempted() {
                return Err(IssuerError::ChildInterrupted);
            }
            let revocation =
                revoke_session_with_retry(run, &credentials.security, &session_digest, None).await;
            let rehash = rehash_sealed_provisioner(run);
            match revocation {
                Ok(()) => {
                    session_lifecycle.mark_revoked()?;
                    rehash?;
                    return Err(IssuerError::ChildInterrupted);
                }
                Err(error) => {
                    rehash?;
                    return Err(error);
                }
            }
        }
    };
    // Rebind the executing issuer and its source after the credential-consuming
    // lifecycle.  Evidence is never written if either identity changed in flight.
    require_d2a_taint(run)?;
    let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
    let evidence = persist_direct_onboarding_evidence(run, &outcome, &observed_at)?;
    let encoded =
        serde_json::to_string(&evidence).map_err(|_| IssuerError::DirectOnboardingEvidence)?;
    println!("{encoded}");
    Ok(())
}

async fn select_lifecycle<F, I, T>(
    workflow: F,
    interrupt: I,
    terminate: T,
) -> LifecycleDisposition<F::Output>
where
    F: Future,
    I: Future,
    T: Future,
{
    tokio::pin!(workflow);
    tokio::pin!(interrupt);
    tokio::pin!(terminate);
    tokio::select! {
        result = &mut workflow => LifecycleDisposition::Completed(result),
        _ = &mut interrupt => LifecycleDisposition::Interrupted,
        _ = &mut terminate => LifecycleDisposition::Interrupted,
    }
}

#[allow(clippy::too_many_arguments)]
async fn issue_run_and_revoke(
    run: &ValidatedRun,
    credentials: &RunCredentials,
    state_digest: &[u8; 32],
    browser_nonce_digest: &[u8; 32],
    session_digest: &[u8; 32],
    csrf_digest: &[u8; 32],
    child: &[String],
    session_lifecycle: &mut SessionLifecycle,
    child_input: ChildInput<'_>,
) -> Result<Vec<u8>, IssuerError> {
    // From this exact boundary onward an acknowledgement failure may hide a committed
    // session, so every error requires either confirmed revocation or quarantine.
    session_lifecycle.mark_issuance_attempted()?;
    let database_identity = match issue_session(
        run,
        credentials,
        state_digest,
        browser_nonce_digest,
        session_digest,
        csrf_digest,
        SESSION_LIFETIME_SECONDS,
    )
    .await
    {
        Ok(identity) => identity,
        Err(error) => {
            // A failed commit acknowledgement can still leave a live session. Reconcile with
            // the same digest; the command remains failed even if revocation is confirmed.
            return match revoke_session_with_retry(run, &credentials.security, session_digest, None)
                .await
            {
                Ok(()) => finish_after_confirmed_revocation(session_lifecycle, Err(error)),
                Err(revocation_error) => Err(revocation_error),
            };
        }
    };

    let child_result = run_child(child, child_input).await;
    // Revocation is mandatory even when the child exits unsuccessfully or times out.
    // `already_revoked` is accepted when the child completed normal logout.
    revoke_session_with_retry(
        run,
        &credentials.security,
        session_digest,
        Some(&database_identity),
    )
    .await?;
    finish_after_confirmed_revocation(session_lifecycle, child_result)
}

#[allow(clippy::too_many_arguments)]
async fn issue_onboarding_and_revoke(
    run: &ValidatedRun,
    credentials: &RunCredentials,
    state_digest: &[u8; 32],
    browser_nonce_digest: &[u8; 32],
    session_digest: &[u8; 32],
    csrf_digest: &[u8; 32],
    display_name: &str,
    session_lifecycle: &mut SessionLifecycle,
) -> Result<String, IssuerError> {
    session_lifecycle.mark_issuance_attempted()?;
    let database_identity = match issue_session(
        run,
        credentials,
        state_digest,
        browser_nonce_digest,
        session_digest,
        csrf_digest,
        DIRECT_ONBOARDING_SESSION_LIFETIME_SECONDS,
    )
    .await
    {
        Ok(identity) => identity,
        Err(error) => {
            let revocation =
                revoke_session_with_retry(run, &credentials.security, session_digest, None).await;
            let rehash = rehash_sealed_provisioner(run);
            let result = match revocation {
                Ok(()) => finish_after_confirmed_revocation(session_lifecycle, Err(error)),
                Err(revocation_error) => Err(revocation_error),
            };
            rehash?;
            return result;
        }
    };

    let onboarding = run_sealed_onboarding(run, display_name).await;
    let rehash = rehash_sealed_provisioner(run);
    revoke_session_with_retry(
        run,
        &credentials.security,
        session_digest,
        Some(&database_identity),
    )
    .await?;
    let result = finish_after_confirmed_revocation(session_lifecycle, onboarding);
    rehash?;
    result
}

fn resolved_authoring_session_id(
    scenario: &ValidatedScenario,
    session_digest: &[u8; 32],
) -> String {
    let mut suffix = String::with_capacity(16);
    for byte in &session_digest[..8] {
        use std::fmt::Write as _;
        write!(&mut suffix, "{byte:02x}").expect("writing to a string cannot fail");
    }
    format!("{}-{suffix}", scenario.session_id_prefix)
}

fn one_shot_scenario_digest(
    operation: Operation,
    scenario: Option<&ValidatedScenario>,
) -> Option<&str> {
    match operation {
        Operation::OneShot => scenario.map(|scenario| scenario.sha256.as_str()),
        Operation::AuthSmoke | Operation::DirectOnboard => None,
    }
}

async fn preflight_discord_hub(run: &ValidatedRun, bot_token: &str) -> Result<(), IssuerError> {
    let input =
        Zeroizing::new(format!("header = \"Authorization: Bot {bot_token}\"\n").into_bytes());
    let url = format!(
        "https://discord.com/api/v10/channels/{}",
        run.discord_hub_channel_id
    );
    let mut command = Command::new("/usr/bin/curl");
    command
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--request",
            "GET",
            "--proto",
            "=https",
            "--header",
            "Accept: application/json",
            "--max-filesize",
            "16384",
            "--write-out",
            "\n%{http_code}",
            "--connect-timeout",
            "10",
            "--max-time",
            "10",
            "--config",
            "-",
            &url,
        ])
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut process = command
        .spawn()
        .map_err(|_| IssuerError::DiscordHubPreflight)?;
    let stdout = process
        .stdout
        .take()
        .ok_or(IssuerError::DiscordHubPreflight)?;
    let stderr = process
        .stderr
        .take()
        .ok_or(IssuerError::DiscordHubPreflight)?;
    let stdout_task = tokio::spawn(read_bounded(stdout, MAX_DISCORD_OUTPUT_BYTES));
    let stderr_task = tokio::spawn(read_bounded(stderr, 4 * 1024));
    let stdin = process
        .stdin
        .take()
        .ok_or(IssuerError::DiscordHubPreflight)?;
    if write_discord_config_and_close(stdin, &input).await.is_err() {
        let _ = process.kill().await;
        let _ = process.wait().await;
        return Err(IssuerError::DiscordHubPreflight);
    }
    drop(input);
    let disposition = tokio::select! {
        status = process.wait() => status.ok().filter(|status| status.success()).map(|_| ()),
        _ = tokio::time::sleep(DISCORD_PREFLIGHT_TIMEOUT) => None,
    };
    if disposition.is_none() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    let stdout = stdout_task
        .await
        .map_err(|_| IssuerError::DiscordHubPreflight)??;
    let stderr = stderr_task
        .await
        .map_err(|_| IssuerError::DiscordHubPreflight)??;
    if disposition.is_none() || stdout.exceeded || stderr.exceeded || !stderr.bytes.is_empty() {
        return Err(IssuerError::DiscordHubPreflight);
    }
    let split = stdout
        .bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .ok_or(IssuerError::DiscordHubPreflight)?;
    let status = std::str::from_utf8(&stdout.bytes[split + 1..])
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(IssuerError::DiscordHubPreflight)?;
    let channel: Value = serde_json::from_slice(&stdout.bytes[..split])
        .map_err(|_| IssuerError::DiscordHubPreflight)?;
    if status != 200
        || channel.get("id").and_then(Value::as_str) != Some(run.discord_hub_channel_id.as_str())
        || channel.get("guild_id").and_then(Value::as_str) != Some(run.guild_id.as_str())
        || channel.get("type").and_then(Value::as_u64) != Some(0)
    {
        return Err(IssuerError::DiscordHubPreflight);
    }
    Ok(())
}

async fn write_discord_config_and_close(
    mut stdin: tokio::process::ChildStdin,
    input: &[u8],
) -> Result<(), IssuerError> {
    stdin
        .write_all(input)
        .await
        .map_err(|_| IssuerError::DiscordHubPreflight)?;
    stdin
        .shutdown()
        .await
        .map_err(|_| IssuerError::DiscordHubPreflight)?;
    drop(stdin);
    Ok(())
}

async fn run_sealed_onboarding(
    run: &ValidatedRun,
    display_name: &str,
) -> Result<String, IssuerError> {
    let mut command = Command::new(&run.sealed_provisioner_path);
    command
        .args([
            "onboard",
            "--manifest",
            run.manifest_path
                .to_str()
                .ok_or(IssuerError::DirectOnboarding)?,
            "--principal-id",
            &run.principal_id,
            "--display-name",
            display_name,
            "--installation-id",
            &run.installation_id,
        ])
        .current_dir(&run.run_directory)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut process = command.spawn().map_err(|_| IssuerError::DirectOnboarding)?;
    let stdout = process.stdout.take().ok_or(IssuerError::DirectOnboarding)?;
    let stderr = process.stderr.take().ok_or(IssuerError::DirectOnboarding)?;
    let stdout_task = tokio::spawn(read_bounded(stdout, MAX_ONBOARDING_OUTPUT_BYTES));
    let stderr_task = tokio::spawn(read_bounded(stderr, MAX_PROVISIONER_STDERR_BYTES));
    let disposition = tokio::select! {
        status = process.wait() => status.ok(),
        _ = tokio::time::sleep(DIRECT_ONBOARDING_TIMEOUT) => None,
    };
    if disposition.is_none() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    let stdout = stdout_task
        .await
        .map_err(|_| IssuerError::DirectOnboarding)??;
    let stderr = stderr_task
        .await
        .map_err(|_| IssuerError::DirectOnboarding)??;
    if stdout.exceeded || stderr.exceeded {
        return Err(IssuerError::DirectOnboardingOutput);
    }
    let Some(status) = disposition else {
        if !stderr.bytes.is_empty() {
            validate_provisioner_stderr(&stderr.bytes)?;
        }
        return Err(IssuerError::ChildTimeout);
    };
    if !status.success() {
        validate_provisioner_stderr(&stderr.bytes)?;
        return Err(IssuerError::DirectOnboarding);
    }
    if !stderr.bytes.is_empty() {
        return Err(IssuerError::DirectOnboardingOutput);
    }
    let output: SealedOnboardingOutput =
        serde_json::from_slice(&stdout.bytes).map_err(|_| IssuerError::DirectOnboardingOutput)?;
    if !matches!(output.outcome.as_str(), "fresh" | "exact_replay")
        || output.installation_id != run.installation_id
        || output.principal_id != run.principal_id
        || output.binding_key != "community_hub"
        || output.hub_channel_id != run.discord_hub_channel_id
    {
        return Err(IssuerError::DirectOnboardingOutput);
    }
    Ok(output.outcome)
}

fn validate_provisioner_stderr(stderr: &[u8]) -> Result<(), IssuerError> {
    let line = stderr
        .strip_suffix(b"\n")
        .and_then(|line| line.strip_suffix(b"\r").or(Some(line)))
        .ok_or(IssuerError::DirectOnboardingOutput)?;
    let code = std::str::from_utf8(line).map_err(|_| IssuerError::DirectOnboardingOutput)?;
    if matches!(
        code,
        "d2_platform_unsupported"
            | "d2_arguments_invalid"
            | "d2_manifest_invalid"
            | "d2_manifest_digest_invalid"
            | "d2_target_invalid"
            | "d2_external_credentials_unavailable"
            | "d2_keychain_owner_invalid"
            | "d2_provisioning_busy"
            | "d2_database_contract_failed"
            | "d2_role_bootstrap_failed"
            | "d2_credential_sealing_failed"
            | "d2_role_activation_failed"
            | "d2_replay_verification_failed"
            | "d2_partial_state_quarantined"
            | "d2_quarantine_failed"
            | "d2_cleanup_failed"
            | "d2_onboarding_input_invalid"
            | "d2_onboarding_failed"
            | "d2_inspection_failed"
            | "d2_destruction_failed"
    ) {
        Ok(())
    } else {
        Err(IssuerError::DirectOnboardingOutput)
    }
}

async fn issue_session(
    run: &ValidatedRun,
    credentials: &RunCredentials,
    state_digest: &[u8; 32],
    browser_nonce_digest: &[u8; 32],
    session_digest: &[u8; 32],
    csrf_digest: &[u8; 32],
    session_lifetime_seconds: f64,
) -> Result<String, IssuerError> {
    if !(1.0..=SESSION_LIFETIME_SECONDS).contains(&session_lifetime_seconds) {
        return Err(IssuerError::Database);
    }
    let mut oauth = connect(&credentials.oauth, run).await?;
    let oauth_identity = verify_topology(&mut oauth, Topology::OAuth).await?;
    let redirect_uri = format!("{}/oauth/discord/callback", run.public_origin);
    let mut transaction = oauth.begin().await.map_err(|_| IssuerError::Database)?;
    set_transaction_limits(&mut transaction).await?;
    let created: CreatedFlowRow = sqlx::query_as(
        "SELECT * FROM public.starring_product_oauth_flow_create_v1($1, $2, $3, $4, $5)",
    )
    .bind(state_digest.as_slice())
    .bind(browser_nonce_digest.as_slice())
    .bind(&redirect_uri)
    .bind("/")
    .bind(FLOW_LIFETIME_SECONDS)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| IssuerError::Database)?;
    validate_created_flow(&created, &redirect_uri)?;

    let consumed: ConsumedFlowRow = sqlx::query_as(
        "SELECT * FROM public.starring_product_oauth_flow_consume_v1($1, $2, $3, $4)",
    )
    .bind(state_digest.as_slice())
    .bind(browser_nonce_digest.as_slice())
    .bind(&redirect_uri)
    .bind(vec!["/"])
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| IssuerError::Database)?;
    let consumed_at = validate_consumed_flow(&consumed, &redirect_uri)?;
    transaction
        .commit()
        .await
        .map_err(|_| IssuerError::Database)?;
    oauth.close().await.map_err(|_| IssuerError::Database)?;

    let mut issuer = connect(&credentials.issuer, run).await?;
    let issuer_identity = verify_topology(&mut issuer, Topology::Issuer).await?;
    if issuer_identity != oauth_identity {
        return Err(IssuerError::Database);
    }
    let display_name = format!("Starring D2 {}", &run.run_id[run.run_id.len() - 12..]);
    let mut transaction = issuer.begin().await.map_err(|_| IssuerError::Database)?;
    set_transaction_limits(&mut transaction).await?;
    let issued: IssuedSessionRow = sqlx::query_as(
        "SELECT outcome_code, principal_id, discord_user_id, identity_revision, \
         display_profile::TEXT, idle_expires_at, absolute_expires_at, database_now \
         FROM public.starring_product_session_issue_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(state_digest.as_slice())
    .bind(&redirect_uri)
    .bind("/")
    .bind(consumed_at)
    .bind(&run.actor_id)
    .bind(&display_name)
    .bind(session_digest.as_slice())
    .bind(csrf_digest.as_slice())
    .bind(session_lifetime_seconds)
    .bind(session_lifetime_seconds)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| IssuerError::Database)?;
    validate_issued_session(&issued, run, &display_name, session_lifetime_seconds)?;
    transaction
        .commit()
        .await
        .map_err(|_| IssuerError::Database)?;
    issuer.close().await.map_err(|_| IssuerError::Database)?;
    Ok(issuer_identity)
}

async fn revoke_session(
    run: &ValidatedRun,
    credential: &DatabaseCredential,
    session_digest: &[u8; 32],
    expected_database_identity: Option<&str>,
) -> Result<(), IssuerError> {
    let mut security = connect(credential, run).await?;
    let identity = verify_topology(&mut security, Topology::Security).await?;
    if expected_database_identity.is_some_and(|expected| identity != expected) {
        return Err(IssuerError::Database);
    }
    let mut transaction = security.begin().await.map_err(|_| IssuerError::Database)?;
    set_transaction_limits(&mut transaction).await?;
    let outcome: String = sqlx::query_scalar(
        "SELECT outcome_code FROM public.starring_product_session_security_revoke_v1($1)",
    )
    .bind(session_digest.as_slice())
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| IssuerError::Database)?;
    if matches!(
        outcome.as_str(),
        "revoked" | "exact_replay" | "already_revoked"
    ) {
        transaction
            .commit()
            .await
            .map_err(|_| IssuerError::Database)?;
        security.close().await.map_err(|_| IssuerError::Database)?;
        Ok(())
    } else {
        let _ = transaction.rollback().await;
        Err(IssuerError::Database)
    }
}

async fn revoke_session_with_retry(
    run: &ValidatedRun,
    credential: &DatabaseCredential,
    session_digest: &[u8; 32],
    expected_database_identity: Option<&str>,
) -> Result<(), IssuerError> {
    for attempt in 0..REVOKE_ATTEMPTS {
        match revoke_session(run, credential, session_digest, expected_database_identity).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt + 1 == REVOKE_ATTEMPTS => return Err(error),
            Err(_) => tokio::time::sleep(REVOKE_RETRY_DELAY).await,
        }
    }
    Err(IssuerError::Database)
}

async fn connect(
    credential: &DatabaseCredential,
    run: &ValidatedRun,
) -> Result<PgConnection, IssuerError> {
    PgConnection::connect_with(&credential.connect_options(run))
        .await
        .map_err(|_| IssuerError::Database)
}

enum Topology {
    OAuth,
    Issuer,
    Security,
}

async fn verify_topology(
    connection: &mut PgConnection,
    topology: Topology,
) -> Result<String, IssuerError> {
    let (query, expected_role) = match topology {
        Topology::OAuth => (
            "SELECT public.starring_product_oauth_database_identity_v1(), \
             current_database()::TEXT, current_user::TEXT, session_user::TEXT",
            "starring_identity_oauth",
        ),
        Topology::Issuer => (
            "SELECT public.starring_product_session_issuer_database_identity_v1(), \
             current_database()::TEXT, current_user::TEXT, session_user::TEXT",
            "starring_identity_issuer",
        ),
        Topology::Security => (
            "SELECT public.starring_product_security_revoker_database_identity_v1(), \
             current_database()::TEXT, current_user::TEXT, session_user::TEXT",
            "starring_identity_security",
        ),
    };
    let (identity, database, current_user, session_user): (String, String, String, String) =
        sqlx::query_as(query)
            .fetch_one(connection)
            .await
            .map_err(|_| IssuerError::Database)?;
    if identity.len() != 36
        || database != DATABASE_NAME
        || current_user != expected_role
        || session_user != expected_role
    {
        return Err(IssuerError::Database);
    }
    Ok(identity)
}

async fn set_transaction_limits(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), IssuerError> {
    sqlx::query(
        "SELECT set_config('statement_timeout', $1, true), set_config('lock_timeout', $2, true)",
    )
    .bind(STATEMENT_TIMEOUT)
    .bind(LOCK_TIMEOUT)
    .execute(&mut **transaction)
    .await
    .map_err(|_| IssuerError::Database)?;
    Ok(())
}

fn validate_created_flow(row: &CreatedFlowRow, redirect_uri: &str) -> Result<(), IssuerError> {
    let (outcome, returned_redirect, return_path, expires_at, database_now) = row;
    let (Some(expires_at), Some(database_now)) = (expires_at, database_now) else {
        return Err(IssuerError::Database);
    };
    let remaining = expires_at
        .signed_duration_since(*database_now)
        .num_seconds();
    if outcome != "created"
        || returned_redirect.as_deref() != Some(redirect_uri)
        || return_path.as_deref() != Some("/")
        || !(1..=60).contains(&remaining)
    {
        return Err(IssuerError::Database);
    }
    Ok(())
}

fn validate_consumed_flow(
    row: &ConsumedFlowRow,
    redirect_uri: &str,
) -> Result<DateTime<Utc>, IssuerError> {
    let (outcome, returned_redirect, return_path, consumed_at) = row;
    if outcome != "claimed"
        || returned_redirect.as_deref() != Some(redirect_uri)
        || return_path.as_deref() != Some("/")
    {
        return Err(IssuerError::Database);
    }
    consumed_at.ok_or(IssuerError::Database)
}

fn validate_issued_session(
    row: &IssuedSessionRow,
    run: &ValidatedRun,
    display_name: &str,
    session_lifetime_seconds: f64,
) -> Result<(), IssuerError> {
    let (
        outcome,
        principal_id,
        discord_user_id,
        identity_revision,
        display_profile,
        idle_expires_at,
        absolute_expires_at,
        database_now,
    ) = row;
    let (Some(idle), Some(absolute), Some(now)) =
        (idle_expires_at, absolute_expires_at, database_now)
    else {
        return Err(IssuerError::Database);
    };
    let profile = display_profile
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .ok_or(IssuerError::Database)?;
    let remaining = idle.signed_duration_since(*now).num_seconds();
    if outcome != "issued"
        || principal_id.as_deref() != Some(run.principal_id.as_str())
        || discord_user_id.as_deref() != Some(run.actor_id.as_str())
        || !identity_revision.is_some_and(|revision| revision >= 1)
        || profile.get("display_name").and_then(Value::as_str) != Some(display_name)
        || idle != absolute
        || remaining < 1
        || remaining > session_lifetime_seconds.ceil() as i64
    {
        return Err(IssuerError::Database);
    }
    Ok(())
}

async fn run_child(child: &[String], input: ChildInput<'_>) -> Result<Vec<u8>, IssuerError> {
    let encoded = Zeroizing::new(serde_json::to_vec(&input).map_err(|_| IssuerError::Child)?);
    if encoded.len() > MAX_CHILD_INPUT_BYTES {
        return Err(IssuerError::Child);
    }
    let runner_parent = Path::new(&child[1])
        .parent()
        .ok_or(IssuerError::ChildRunner)?;
    let mut command = Command::new(&child[0]);
    command
        .args(&child[1..])
        .current_dir(runner_parent)
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut process = command.spawn().map_err(|_| IssuerError::Child)?;
    let stdout = process.stdout.take().ok_or(IssuerError::Child)?;
    let stderr = process.stderr.take().ok_or(IssuerError::Child)?;
    let stdout_task = tokio::spawn(read_bounded(stdout, MAX_CHILD_OUTPUT_BYTES));
    let stderr_task = tokio::spawn(read_bounded(stderr, 0));
    let mut stdin = process.stdin.take().ok_or(IssuerError::Child)?;
    if stdin.write_all(&encoded).await.is_err()
        || stdin.write_all(b"\n").await.is_err()
        || stdin.shutdown().await.is_err()
    {
        let _ = process.kill().await;
        let _ = process.wait().await;
        return Err(IssuerError::Child);
    }

    let disposition = tokio::select! {
        status = process.wait() => {
            match status {
                Ok(status) if status.success() => Ok(()),
                _ => Err(IssuerError::Child),
            }
        }
        _ = tokio::time::sleep(CHILD_TIMEOUT) => Err(IssuerError::ChildTimeout),
    };
    if disposition.is_err() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    let stdout = stdout_task.await.map_err(|_| IssuerError::Child)??;
    let _stderr = stderr_task.await.map_err(|_| IssuerError::Child)??;
    disposition?;
    if stdout.exceeded {
        return Err(IssuerError::EvidenceTooLarge);
    }
    Ok(stdout.bytes)
}

async fn read_bounded<R>(mut reader: R, maximum: usize) -> Result<Capture, IssuerError>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(maximum.min(64 * 1024));
    let mut exceeded = false;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|_| IssuerError::Child)?;
        if read == 0 {
            break;
        }
        let remaining = maximum.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&buffer[..retained]);
        if retained < read {
            exceeded = true;
        }
    }
    Ok(Capture { bytes, exceeded })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    fn fields(value: &Value) -> BTreeSet<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn auth_smoke_child_input_has_exact_flat_schema() {
        let input = ChildInput {
            schema_version: 1,
            session: "session",
            csrf: "csrf",
            public_origin: "https://d2-api.starring.co.kr",
            principal_id: "discord:1",
            guild_id: "2",
            installation_id: "installation:test",
            run_id: "run",
            manifest_sha256: "digest",
            operation: "auth-smoke",
            authoring_session_id: None,
            scenario: None,
            scenario_sha256: None,
        };
        let value = serde_json::to_value(input).unwrap();
        assert_eq!(
            fields(&value),
            BTreeSet::from([
                "csrf",
                "guild_id",
                "installation_id",
                "manifest_sha256",
                "operation",
                "principal_id",
                "public_origin",
                "run_id",
                "schema_version",
                "session",
            ])
        );
    }

    #[test]
    fn one_shot_child_input_adds_only_scenario_and_raw_digest() {
        let scenario = json!({"prompt": "test"});
        let input = ChildInput {
            schema_version: 1,
            session: "session",
            csrf: "csrf",
            public_origin: "https://d2-api.starring.co.kr",
            principal_id: "discord:1",
            guild_id: "2",
            installation_id: "installation:test",
            run_id: "run",
            manifest_sha256: "digest",
            operation: "one-shot",
            authoring_session_id: Some("d2a-study-room-v1-0123456789abcdef"),
            scenario: Some(&scenario),
            scenario_sha256: Some("raw-digest"),
        };
        let value = serde_json::to_value(input).unwrap();
        assert_eq!(value["scenario"], scenario);
        assert_eq!(value["scenario_sha256"], "raw-digest");
        assert_eq!(
            value["authoring_session_id"],
            "d2a-study-room-v1-0123456789abcdef"
        );
        assert_eq!(fields(&value).len(), 13);
    }

    #[test]
    fn resolved_authoring_id_is_prefix_plus_digest_bound_hex() {
        let scenario = ValidatedScenario {
            value: json!({}),
            sha256: "a".repeat(64),
            session_id_prefix: "d2a-study-room-v1".to_string(),
        };
        let digest = [0xab_u8; 32];
        assert_eq!(
            resolved_authoring_session_id(&scenario, &digest),
            "d2a-study-room-v1-abababababababab"
        );
    }

    #[tokio::test]
    async fn lifecycle_signal_cancels_a_pending_issue_or_child_workflow() {
        let disposition = select_lifecycle(
            std::future::pending::<()>(),
            std::future::ready(()),
            std::future::pending::<()>(),
        )
        .await;
        assert!(matches!(disposition, LifecycleDisposition::Interrupted));
    }

    #[tokio::test]
    async fn discord_preflight_closes_config_stdin_before_waiting_on_curl() {
        let mut command = Command::new("/usr/bin/curl");
        command
            .args([
                "--disable",
                "--silent",
                "--show-error",
                "--max-time",
                "2",
                "--config",
                "-",
                "file:///dev/null",
            ])
            .env_clear()
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut process = command.spawn().expect("spawn curl EOF probe");
        let stdin = process.stdin.take().expect("curl stdin");
        write_discord_config_and_close(stdin, b"header = \"Accept: application/json\"\n")
            .await
            .expect("write and close curl config");
        let status = tokio::time::timeout(Duration::from_secs(1), process.wait())
            .await
            .expect("curl must observe EOF before the wait timeout")
            .expect("wait for curl EOF probe");
        assert!(status.success());
    }

    #[test]
    fn global_lock_precedes_all_run_validation_and_signal_revokes_before_exit() {
        let source = include_str!("main.rs");
        let parsed = source
            .find("let arguments = parse_arguments")
            .expect("argument parse call");
        let isolated = source[parsed..]
            .find("require_dedicated_process_session()")
            .map(|offset| parsed + offset)
            .expect("dedicated process session check");
        let locked = source[isolated..]
            .find("let _global_lock = acquire_global_operation_lock()")
            .map(|offset| isolated + offset)
            .expect("global lock call");
        let ambient = source[locked..]
            .find("disable_core_dumps()")
            .map(|offset| locked + offset)
            .expect("core dump policy call");
        let environment = source[ambient..]
            .find("reject_ambient_postgres_environment()")
            .map(|offset| ambient + offset)
            .expect("ambient environment check");
        let validated = source[environment..]
            .find("let run = load_validated_run")
            .map(|offset| environment + offset)
            .expect("run validation call");
        let coordinator = source[validated..]
            .find("let _coordinator_lock = acquire_run_coordinator_lock")
            .map(|offset| validated + offset)
            .expect("per-run coordinator lock call");
        let fence = source[coordinator..]
            .find("require_open_d2a_teardown_fence(&run)")
            .map(|offset| coordinator + offset)
            .expect("teardown fence check");
        let lifecycle = source[fence..]
            .find("SessionLifecycle::begin(&run, arguments.operation)")
            .map(|offset| fence + offset)
            .expect("active lifecycle marker");
        let taint = source[lifecycle..]
            .find("persist_d2a_taint(run)")
            .map(|offset| lifecycle + offset)
            .expect("taint persistence call");
        let secrets = source[taint..]
            .find("generate_four_secrets()")
            .map(|offset| taint + offset)
            .expect("secret generation call");
        let issuance = source[secrets..]
            .find("issue_run_and_revoke(")
            .map(|offset| secrets + offset)
            .expect("session issuance workflow");
        assert!(
            parsed < isolated
                && isolated < locked
                && locked < ambient
                && ambient < environment
                && environment < validated
                && validated < coordinator
                && coordinator < fence
                && fence < lifecycle
                && lifecycle < taint
                && taint < secrets
                && secrets < issuance
        );

        let interrupted = source
            .find("LifecycleDisposition::Interrupted => {")
            .expect("interrupted lifecycle branch");
        let revoke = source[interrupted..]
            .find("revoke_session_with_retry")
            .map(|offset| interrupted + offset)
            .expect("mandatory signal revocation");
        let confirmed = source[revoke..]
            .find("finish_after_confirmed_revocation(")
            .map(|offset| revoke + offset)
            .expect("confirmed signal revocation transition");
        assert!(interrupted < revoke && revoke < confirmed);

        let issue_workflow = source
            .find("async fn issue_run_and_revoke")
            .expect("issue workflow definition");
        let boundary = source[issue_workflow..]
            .find("session_lifecycle.mark_issuance_attempted()")
            .map(|offset| issue_workflow + offset)
            .expect("issuance-attempt boundary");
        let issue = source[boundary..]
            .find("let database_identity = match issue_session(")
            .map(|offset| boundary + offset)
            .expect("issuance call");
        assert!(boundary < issue);
    }

    #[test]
    fn confirmed_revocation_remains_terminal_when_child_or_provisioning_fails() {
        #[derive(Default)]
        struct FakeMarker {
            revoked: bool,
        }
        impl ConfirmedRevocationMarker for FakeMarker {
            fn confirm_revoked(&mut self) -> Result<(), IssuerError> {
                self.revoked = true;
                Ok(())
            }
        }

        for operation_error in [IssuerError::Child, IssuerError::DirectOnboarding] {
            let mut marker = FakeMarker::default();
            let result: Result<(), IssuerError> =
                finish_after_confirmed_revocation(&mut marker, Err(operation_error));
            assert_eq!(result, Err(operation_error));
            assert!(marker.revoked);
        }

        let source = include_str!("main.rs");
        assert!(
            source.contains("finish_after_confirmed_revocation(session_lifecycle, child_result)")
        );
        assert!(source.contains(
            "let result = finish_after_confirmed_revocation(session_lifecycle, onboarding);"
        ));

        let marker_schema = include_str!("lib.rs");
        assert!(!marker_schema.contains("session_digest: String"));
        assert!(!marker_schema.contains("session_fingerprint"));
        assert!(!marker_schema.contains("raw_session"));
    }

    #[test]
    fn direct_onboarding_uses_a_short_session_and_persists_only_after_revocation() {
        let source = include_str!("main.rs");
        let direct = source
            .find("async fn issue_onboarding_and_revoke")
            .expect("direct onboarding lifecycle");
        let short_lifetime = source[direct..]
            .find("DIRECT_ONBOARDING_SESSION_LIFETIME_SECONDS")
            .map(|offset| direct + offset)
            .expect("short direct session lifetime");
        let invoke = source[direct..]
            .find("run_sealed_onboarding")
            .map(|offset| direct + offset)
            .expect("sealed onboarding invocation");
        let rehash = source[invoke..]
            .find("rehash_sealed_provisioner")
            .map(|offset| invoke + offset)
            .expect("post-invocation candidate rehash");
        let revoke = source[rehash..]
            .find("revoke_session_with_retry")
            .map(|offset| rehash + offset)
            .expect("mandatory direct-session revocation");
        assert!(short_lifetime < invoke && invoke < rehash && rehash < revoke);

        let entry = source
            .find("async fn run_direct_onboarding")
            .expect("direct onboarding entry");
        let lifecycle = source[entry..]
            .find("issue_onboarding_and_revoke")
            .map(|offset| entry + offset)
            .expect("direct lifecycle call");
        let evidence = source[lifecycle..]
            .find("persist_direct_onboarding_evidence")
            .map(|offset| lifecycle + offset)
            .expect("direct evidence persistence");
        assert!(lifecycle < evidence);
        assert!(source.contains("const DIRECT_ONBOARDING_SESSION_LIFETIME_SECONDS: f64 = 120.0;"));
        assert!(
            source.contains("const DIRECT_ONBOARDING_TIMEOUT: Duration = Duration::from_secs(90);")
        );
    }

    #[test]
    fn sealed_provisioner_stderr_is_a_closed_stable_allowlist() {
        assert_eq!(
            validate_provisioner_stderr(b"d2_onboarding_failed\n"),
            Ok(())
        );
        assert_eq!(
            validate_provisioner_stderr(b"database password was secret\n"),
            Err(IssuerError::DirectOnboardingOutput)
        );
        assert_eq!(
            validate_provisioner_stderr(b"d2_onboarding_failed\nextra\n"),
            Err(IssuerError::DirectOnboardingOutput)
        );
        assert_eq!(
            validate_provisioner_stderr(b""),
            Err(IssuerError::DirectOnboardingOutput)
        );
    }

    #[test]
    fn sealed_onboarding_output_rejects_extra_or_missing_fields() {
        let exact = br#"{"outcome":"fresh","installation_id":"installation:x","principal_id":"discord:1","binding_key":"community_hub","hub_channel_id":"2"}"#;
        assert!(serde_json::from_slice::<SealedOnboardingOutput>(exact).is_ok());
        let extra = br#"{"outcome":"fresh","installation_id":"installation:x","principal_id":"discord:1","binding_key":"community_hub","hub_channel_id":"2","session":"secret"}"#;
        assert!(serde_json::from_slice::<SealedOnboardingOutput>(extra).is_err());
    }
}
