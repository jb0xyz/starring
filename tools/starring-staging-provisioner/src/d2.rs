use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, Row};
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::crypto::GeneratedSecretsV1;
use crate::identity::{
    APPLICATION_DATABASE_IDENTITIES, CLUSTER_ADMIN_ROLE, DATABASE_NAME, RUNTIME_KEYCHAIN_SERVICE,
};
use crate::keychain::{DynamicSecretItemRefV1, KeychainClientV1};
use crate::keyring::validate_keyring_set;
use crate::postgres::alter_role_password_sql;

const API_ROLE_BOOTSTRAP: &str =
    include_str!("../../../ops/postgres/staging-api-role-bootstrap.sql");
const API_ROLE_ENABLE: &str = include_str!("../../../ops/postgres/staging-api-role-enable.sql");
const RUNTIME_ROLE_BOOTSTRAP: &str =
    include_str!("../../../ops/postgres/staging-runtime-role-bootstrap.sql");
const INSTALLATION_ONBOARDING: &str =
    include_str!("../../../ops/postgres/staging-product-installation-onboard.sql");
const DATABASE_HOST: &str = "127.0.0.1";
const RUNTIME_TEMP_TABLE_CLEANUP: &str = "DROP TABLE pg_temp.starring_runtime_capability_functions; DROP TABLE pg_temp.starring_runtime_capability_roles";
const ADMIN_DATABASE: &str = "postgres";
const OWNER_ACCOUNT: &str = "lifecycle-owner";
const PRODUCT_ACTION_ACCOUNT: &str = "keyring.product-action";
const SNAPSHOT_ENVELOPE_ACCOUNT: &str = "keyring.snapshot-envelope";
const INTERACTION_TOKEN_ACCOUNT: &str = "interaction.token-envelope-keyring";
const ADMIN_ACCOUNT: &str = "database.cluster-admin";
const WORKER_TOKEN_ACCOUNT: &str = "authoring.bearer-token";
const EMPTY_BINDINGS_FINGERPRINT: &str =
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";
const EMPTY_RESOURCE_BINDINGS_JSON: &str = "{\"channel_bindings\":{},\"role_bindings\":{}}";
const RESOURCE_CONTEXT_DOMAIN_V2: &str = "starring.intent.resource_context.v2\0";
const AUTHORITY_PAYLOAD_DOMAIN_V1: &[u8] = b"starring.installation-authority.payload.v1\0";
const D2_HUB_BINDING_KEY: &str = "community_hub";
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const PROTECTED_KEYCHAIN_SERVICES: [&str; 5] = [
    "starring-api.staging",
    "starring.runtime.staging",
    "starring.postgres.staging",
    "com.starring.llm-api-key",
    "com.cloudflare.tunnel.macmini-llm-prod",
];

unsafe extern "C" {
    fn geteuid() -> u32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum D2ProvisionerErrorV1 {
    #[error("d2_platform_unsupported")]
    UnsupportedPlatform,
    #[error("d2_arguments_invalid")]
    Arguments,
    #[error("d2_manifest_invalid")]
    Manifest,
    #[error("d2_manifest_digest_invalid")]
    ManifestDigest,
    #[error("d2_target_invalid")]
    Target,
    #[error("d2_external_credentials_unavailable")]
    ExternalCredentials,
    #[error("d2_keychain_owner_invalid")]
    KeychainOwner,
    #[error("d2_provisioning_busy")]
    Busy,
    #[error("d2_database_contract_failed")]
    DatabaseContract,
    #[error("d2_role_bootstrap_failed")]
    RoleBootstrap,
    #[error("d2_credential_sealing_failed")]
    CredentialSealing,
    #[error("d2_role_activation_failed")]
    RoleActivation,
    #[error("d2_replay_verification_failed")]
    ReplayVerification,
    #[error("d2_partial_state_quarantined")]
    PartialStateQuarantined,
    #[error("d2_quarantine_failed")]
    Quarantine,
    #[error("d2_cleanup_failed")]
    Cleanup,
    #[error("d2_onboarding_input_invalid")]
    OnboardingInput,
    #[error("d2_onboarding_failed")]
    Onboarding,
    #[error("d2_inspection_failed")]
    Inspection,
}

impl D2ProvisionerErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "d2_platform_unsupported",
            Self::Arguments => "d2_arguments_invalid",
            Self::Manifest => "d2_manifest_invalid",
            Self::ManifestDigest => "d2_manifest_digest_invalid",
            Self::Target => "d2_target_invalid",
            Self::ExternalCredentials => "d2_external_credentials_unavailable",
            Self::KeychainOwner => "d2_keychain_owner_invalid",
            Self::Busy => "d2_provisioning_busy",
            Self::DatabaseContract => "d2_database_contract_failed",
            Self::RoleBootstrap => "d2_role_bootstrap_failed",
            Self::CredentialSealing => "d2_credential_sealing_failed",
            Self::RoleActivation => "d2_role_activation_failed",
            Self::ReplayVerification => "d2_replay_verification_failed",
            Self::PartialStateQuarantined => "d2_partial_state_quarantined",
            Self::Quarantine => "d2_quarantine_failed",
            Self::Cleanup => "d2_cleanup_failed",
            Self::OnboardingInput => "d2_onboarding_input_invalid",
            Self::Onboarding => "d2_onboarding_failed",
            Self::Inspection => "d2_inspection_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2ProvisioningOutcomeV1 {
    Fresh,
    ExactReplay,
}

impl D2ProvisioningOutcomeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::ExactReplay => "exact_replay",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct D2ProvisioningReportV1 {
    outcome: D2ProvisioningOutcomeV1,
    application_credentials: usize,
    keyrings: usize,
    external_credentials_checked: usize,
    activated_roles: usize,
    worker_credentials: usize,
}

impl D2ProvisioningReportV1 {
    pub const fn outcome(self) -> D2ProvisioningOutcomeV1 {
        self.outcome
    }

    pub const fn application_credentials(self) -> usize {
        self.application_credentials
    }

    pub const fn keyrings(self) -> usize {
        self.keyrings
    }

    pub const fn external_credentials_checked(self) -> usize {
        self.external_credentials_checked
    }

    pub const fn activated_roles(self) -> usize {
        self.activated_roles
    }

    pub const fn worker_credentials(self) -> usize {
        self.worker_credentials
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct D2QuarantineReportV1 {
    quarantined_roles: usize,
    removed_credentials: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2OnboardingOutcomeV1 {
    Fresh,
    ExactReplay,
}

impl D2OnboardingOutcomeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::ExactReplay => "exact_replay",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2OnboardingReportV1 {
    outcome: D2OnboardingOutcomeV1,
    installation_id: String,
    principal_id: String,
    binding_key: &'static str,
    hub_channel_id: String,
}

impl D2OnboardingReportV1 {
    pub const fn outcome(&self) -> D2OnboardingOutcomeV1 {
        self.outcome
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }

    pub const fn binding_key(&self) -> &'static str {
        self.binding_key
    }

    pub fn hub_channel_id(&self) -> &str {
        &self.hub_channel_id
    }
}

impl D2QuarantineReportV1 {
    pub const fn quarantined_roles(self) -> usize {
        self.quarantined_roles
    }

    pub const fn removed_credentials(self) -> usize {
        self.removed_credentials
    }
}

#[derive(Deserialize)]
struct ManifestV1 {
    schema_version: u32,
    run_id: String,
    database: DatabaseManifestV1,
    discord: DiscordManifestV1,
    keychain_services: KeychainServicesV1,
    external_keychain: ExternalKeychainV1,
    protected_staging: ProtectedStagingV1,
}

#[derive(Deserialize)]
struct DiscordManifestV1 {
    guild_id: String,
    application_id: String,
    bot_user_id: String,
    actor_id: String,
    hub_channel_id: String,
    resource_prefix: String,
}

#[derive(Deserialize)]
struct DatabaseManifestV1 {
    name: String,
    cluster_root: PathBuf,
    socket_directory: PathBuf,
    port: u16,
}

#[derive(Deserialize)]
struct KeychainServicesV1 {
    api: String,
    runtime: String,
    postgres: String,
    worker: String,
}

#[derive(Clone, Deserialize)]
struct KeychainIdentityV1 {
    service: String,
    account: String,
}

#[derive(Deserialize)]
struct ExternalKeychainV1 {
    discord_oauth_client_secret: KeychainIdentityV1,
    discord_bot_token: KeychainIdentityV1,
    tunnel_token: KeychainIdentityV1,
}

#[derive(Deserialize)]
struct ProtectedStagingV1 {
    mutation_allowed: bool,
}

pub(crate) struct D2ConfigV1 {
    pub(crate) run_id: String,
    cluster_root: PathBuf,
    socket_directory: PathBuf,
    port: u16,
    api_service: String,
    runtime_service: String,
    postgres_service: String,
    worker_service: String,
    pub(crate) discord_guild_id: String,
    pub(crate) discord_application_id: String,
    discord_hub_channel_id: String,
    pub(crate) resource_prefix: String,
    external: [KeychainIdentityV1; 3],
}

struct D2OwnedSecretItemV1 {
    service: String,
    account: &'static str,
    value: Zeroizing<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagedCredentialStateV1 {
    Absent,
    Complete,
    Partial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagedRoleStateV1 {
    Absent,
    Quarantined,
    Enabled,
    Partial,
}

pub async fn provision_d2_from_manifest(
    manifest_path: &Path,
) -> Result<D2ProvisioningReportV1, D2ProvisionerErrorV1> {
    let config = load_config(manifest_path)?;
    let keychain =
        KeychainClientV1::new().map_err(|_| D2ProvisionerErrorV1::UnsupportedPlatform)?;
    verify_keychain_owner(&keychain, &config)?;
    preflight_external_credentials(&keychain, &config)?;
    let credential_state = inspect_credential_state(&keychain, &config)?;
    let mut database = if credential_state == ManagedCredentialStateV1::Complete {
        connect_network_and_lock(&config, &keychain).await?
    } else {
        connect_socket_and_lock(&config).await?
    };
    let role_state = inspect_role_state(&mut database).await?;
    match (credential_state, role_state) {
        (ManagedCredentialStateV1::Absent, ManagedRoleStateV1::Absent)
        | (ManagedCredentialStateV1::Absent, ManagedRoleStateV1::Quarantined) => {
            provision_fresh(&config, &keychain, &mut database).await
        }
        (ManagedCredentialStateV1::Complete, ManagedRoleStateV1::Enabled) => {
            match verify_exact_replay(&config, &keychain, &mut database).await {
                Ok(()) => Ok(report(D2ProvisioningOutcomeV1::ExactReplay)),
                Err(_) => {
                    quarantine_and_cleanup(&config, &keychain, &mut database).await?;
                    Err(D2ProvisionerErrorV1::PartialStateQuarantined)
                }
            }
        }
        _ => {
            quarantine_and_cleanup(&config, &keychain, &mut database).await?;
            Err(D2ProvisionerErrorV1::PartialStateQuarantined)
        }
    }
}

pub async fn quarantine_d2_from_manifest(
    manifest_path: &Path,
) -> Result<D2QuarantineReportV1, D2ProvisionerErrorV1> {
    let config = load_config(manifest_path)?;
    let keychain =
        KeychainClientV1::new().map_err(|_| D2ProvisionerErrorV1::UnsupportedPlatform)?;
    verify_keychain_owner(&keychain, &config)?;
    let mut database = if keychain
        .read_optional_dynamic(&config.postgres_service, ADMIN_ACCOUNT)
        .map_err(|_| D2ProvisionerErrorV1::Cleanup)?
        .is_some()
    {
        connect_network_and_lock(&config, &keychain).await?
    } else {
        connect_socket_and_lock(&config).await?
    };
    let quarantined_roles = quarantine_database(&mut database).await?;
    let removed_credentials = cleanup_credentials(&keychain, &config)?;
    Ok(D2QuarantineReportV1 {
        quarantined_roles,
        removed_credentials,
    })
}

pub async fn onboard_d2_from_manifest(
    manifest_path: &Path,
    principal_id: &str,
    display_name: &str,
    installation_id: &str,
) -> Result<D2OnboardingReportV1, D2ProvisionerErrorV1> {
    let config = load_config(manifest_path)?;
    let discord_user_id = principal_id
        .strip_prefix("discord:")
        .filter(|value| valid_snowflake(value))
        .ok_or(D2ProvisionerErrorV1::OnboardingInput)?;
    if display_name.is_empty()
        || display_name.chars().count() > 128
        || display_name.trim() != display_name
        || display_name.chars().any(char::is_control)
    {
        return Err(D2ProvisionerErrorV1::OnboardingInput);
    }
    let expected_installation_id = format!("installation:{}", config.resource_prefix);
    if installation_id != expected_installation_id
        || !valid_product_identifier(installation_id, 128)
    {
        return Err(D2ProvisionerErrorV1::OnboardingInput);
    }
    let keychain =
        KeychainClientV1::new().map_err(|_| D2ProvisionerErrorV1::UnsupportedPlatform)?;
    verify_keychain_owner(&keychain, &config)?;
    let mut database = connect_network_and_lock(&config, &keychain).await?;
    verify_enabled_roles(&mut database)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Onboarding)?;
    let tenant_id = format!("tenant:{}", config.resource_prefix);
    let ruleset_key = "studyroom";
    let resource_bindings = d2_resource_bindings_json(&config.discord_hub_channel_id)?;
    let binding_fingerprint = d2_resource_binding_fingerprint_v2(&config.discord_hub_channel_id)?;
    let authority_payload_digest =
        d2_authority_payload_digest_v1(&tenant_id, installation_id, &binding_fingerprint)?;
    let created_by_request_digest = d2_onboarding_request_digest(
        &config.run_id,
        &tenant_id,
        installation_id,
        principal_id,
        display_name,
        &authority_payload_digest,
    );
    let existed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM public.automation_installations WHERE installation_id = $1)",
    )
    .bind(installation_id)
    .fetch_one(&mut database)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Onboarding)?;
    let system_identifier: String = sqlx::query_scalar(
        "SELECT control.system_identifier::TEXT FROM pg_catalog.pg_control_system() AS control",
    )
    .fetch_one(&mut database)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Onboarding)?;
    let inputs = OnboardingInputsV1 {
        system_identifier: &system_identifier,
        tenant_id: &tenant_id,
        tenant_display_name: display_name,
        installation_id,
        discord_application_id: &config.discord_application_id,
        discord_guild_id: &config.discord_guild_id,
        discord_hub_channel_id: &config.discord_hub_channel_id,
        ruleset_key,
        principal_id,
        discord_user_id,
        resource_bindings: &resource_bindings,
        binding_fingerprint: &binding_fingerprint,
        authority_payload_digest: &authority_payload_digest,
        created_by_request_digest: &created_by_request_digest,
        port: config.port,
    };
    let script = render_onboarding_script(&inputs)?;
    sqlx::raw_sql(&script)
        .execute(&mut database)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Onboarding)?;
    verify_onboarding(&mut database, &inputs).await?;
    Ok(D2OnboardingReportV1 {
        outcome: if existed {
            D2OnboardingOutcomeV1::ExactReplay
        } else {
            D2OnboardingOutcomeV1::Fresh
        },
        installation_id: installation_id.to_owned(),
        principal_id: principal_id.to_owned(),
        binding_key: D2_HUB_BINDING_KEY,
        hub_channel_id: config.discord_hub_channel_id,
    })
}

fn report(outcome: D2ProvisioningOutcomeV1) -> D2ProvisioningReportV1 {
    D2ProvisioningReportV1 {
        outcome,
        application_credentials: APPLICATION_DATABASE_IDENTITIES.len(),
        keyrings: 3,
        external_credentials_checked: 3,
        activated_roles: APPLICATION_DATABASE_IDENTITIES.len(),
        worker_credentials: 1,
    }
}

async fn provision_fresh(
    config: &D2ConfigV1,
    keychain: &KeychainClientV1,
    database: &mut PgConnection,
) -> Result<D2ProvisioningReportV1, D2ProvisionerErrorV1> {
    if let Err(error) = bootstrap_roles(database).await {
        return cleanup_after_failure(config, keychain, database, error).await;
    }
    if inspect_role_state(database).await? != ManagedRoleStateV1::Quarantined {
        return cleanup_after_failure(
            config,
            keychain,
            database,
            D2ProvisionerErrorV1::RoleBootstrap,
        )
        .await;
    }
    let secrets =
        GeneratedSecretsV1::generate().map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    validate_keyring_set(
        secrets.product_action_keyring_payload(),
        secrets.snapshot_envelope_keyring_payload(),
        secrets.interaction_token_envelope_keyring_payload(),
    )
    .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    let owned_items = build_owned_secret_items(config, &secrets)?;
    let item_refs = owned_items
        .iter()
        .map(|item| DynamicSecretItemRefV1 {
            service: &item.service,
            account: item.account,
            value: item.value.as_slice(),
        })
        .collect::<Vec<_>>();
    let keychain_update = match keychain.begin_create_dynamic(&item_refs) {
        Ok(update) => update,
        Err(_) => {
            return cleanup_after_failure(
                config,
                keychain,
                database,
                D2ProvisionerErrorV1::CredentialSealing,
            )
            .await;
        }
    };
    let activation = async {
        apply_verifiers(database, &secrets).await?;
        activate_roles(database).await?;
        verify_enabled_roles(database).await?;
        verify_owned_secret_shapes(config, &owned_items)?;
        Ok::<(), D2ProvisionerErrorV1>(())
    }
    .await;
    match activation {
        Ok(()) => {
            keychain_update.commit();
            Ok(report(D2ProvisioningOutcomeV1::Fresh))
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *database).await;
            let quarantine = quarantine_database(database).await;
            let rollback = keychain_update.rollback();
            if quarantine.is_err() {
                Err(D2ProvisionerErrorV1::Quarantine)
            } else if rollback.is_err() {
                Err(D2ProvisionerErrorV1::Cleanup)
            } else {
                Err(error)
            }
        }
    }
}

async fn cleanup_after_failure<T>(
    config: &D2ConfigV1,
    keychain: &KeychainClientV1,
    database: &mut PgConnection,
    error: D2ProvisionerErrorV1,
) -> Result<T, D2ProvisionerErrorV1> {
    quarantine_and_cleanup(config, keychain, database).await?;
    Err(error)
}

async fn quarantine_and_cleanup(
    config: &D2ConfigV1,
    keychain: &KeychainClientV1,
    database: &mut PgConnection,
) -> Result<(), D2ProvisionerErrorV1> {
    sqlx::query("ROLLBACK")
        .execute(&mut *database)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
    quarantine_database(database).await?;
    cleanup_credentials(keychain, config)?;
    Ok(())
}

pub(crate) fn load_config(manifest_path: &Path) -> Result<D2ConfigV1, D2ProvisionerErrorV1> {
    if !cfg!(target_os = "macos") {
        return Err(D2ProvisionerErrorV1::UnsupportedPlatform);
    }
    if !manifest_path.is_absolute()
        || manifest_path
            .components()
            .any(|part| part.as_os_str() == "..")
    {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    let metadata =
        fs::symlink_metadata(manifest_path).map_err(|_| D2ProvisionerErrorV1::Manifest)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_user_id()
    {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    let payload = fs::read(manifest_path).map_err(|_| D2ProvisionerErrorV1::Manifest)?;
    if payload.is_empty() || payload.len() > MAX_MANIFEST_BYTES || payload.last() != Some(&b'\n') {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    let canonical = &payload[..payload.len() - 1];
    if canonical.contains(&b'\n') || canonical.contains(&b'\r') {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    verify_manifest_digest(manifest_path, canonical)?;
    let manifest: ManifestV1 =
        serde_json::from_slice(canonical).map_err(|_| D2ProvisionerErrorV1::Manifest)?;
    validate_manifest(manifest)
}

fn verify_manifest_digest(
    manifest_path: &Path,
    canonical: &[u8],
) -> Result<(), D2ProvisionerErrorV1> {
    let digest_path = manifest_path.with_file_name("manifest.sha256");
    let metadata =
        fs::symlink_metadata(&digest_path).map_err(|_| D2ProvisionerErrorV1::ManifestDigest)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_user_id()
    {
        return Err(D2ProvisionerErrorV1::ManifestDigest);
    }
    let expected =
        fs::read_to_string(digest_path).map_err(|_| D2ProvisionerErrorV1::ManifestDigest)?;
    let expected = expected.strip_suffix('\n').unwrap_or(&expected);
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(D2ProvisionerErrorV1::ManifestDigest);
    }
    let observed = format!("{:x}", Sha256::digest(canonical));
    if !bool::from(observed.as_bytes().ct_eq(expected.as_bytes())) {
        return Err(D2ProvisionerErrorV1::ManifestDigest);
    }
    Ok(())
}

fn validate_manifest(manifest: ManifestV1) -> Result<D2ConfigV1, D2ProvisionerErrorV1> {
    if manifest.schema_version != 1 || !valid_run_id(&manifest.run_id) {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    let root = PathBuf::from(format!("/private/tmp/starring-d2-{}", manifest.run_id));
    if manifest.database.name != DATABASE_NAME
        || manifest.database.cluster_root != root.join("postgres")
        || manifest.database.socket_directory != root.join("socket")
        || manifest.database.port < 1024
        || manifest.database.port == 5432
        || manifest.protected_staging.mutation_allowed
    {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    if !valid_discord_manifest(&manifest.discord) {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    validate_target_directory(&root, 0o700)?;
    validate_target_directory(&manifest.database.cluster_root, 0o700)?;
    validate_target_directory(&manifest.database.socket_directory, 0o700)?;
    if !manifest.database.cluster_root.join("PG_VERSION").is_file() {
        return Err(D2ProvisionerErrorV1::Target);
    }
    let suffix = manifest
        .run_id
        .rsplit_once('-')
        .map(|(_, suffix)| suffix)
        .ok_or(D2ProvisionerErrorV1::Manifest)?;
    let expected = KeychainServicesV1 {
        api: format!("starring.d2.{suffix}.api"),
        runtime: format!("starring.d2.{suffix}.runtime"),
        postgres: format!("starring.d2.{suffix}.postgres"),
        worker: format!("starring.d2.{suffix}.worker"),
    };
    if manifest.keychain_services.api != expected.api
        || manifest.keychain_services.runtime != expected.runtime
        || manifest.keychain_services.postgres != expected.postgres
        || manifest.keychain_services.worker != expected.worker
    {
        return Err(D2ProvisionerErrorV1::Manifest);
    }
    let external = [
        manifest.external_keychain.discord_oauth_client_secret,
        manifest.external_keychain.discord_bot_token,
        manifest.external_keychain.tunnel_token,
    ];
    let managed_services = [
        expected.api.as_str(),
        expected.runtime.as_str(),
        expected.postgres.as_str(),
        expected.worker.as_str(),
    ];
    let mut identities = BTreeSet::new();
    for identity in &external {
        if !valid_keychain_component(&identity.service)
            || !valid_keychain_component(&identity.account)
            || managed_services.contains(&identity.service.as_str())
            || PROTECTED_KEYCHAIN_SERVICES.contains(&identity.service.as_str())
            || !identities.insert((identity.service.as_str(), identity.account.as_str()))
        {
            return Err(D2ProvisionerErrorV1::Manifest);
        }
    }
    Ok(D2ConfigV1 {
        run_id: manifest.run_id,
        cluster_root: manifest.database.cluster_root,
        socket_directory: manifest.database.socket_directory,
        port: manifest.database.port,
        api_service: expected.api,
        runtime_service: expected.runtime,
        postgres_service: expected.postgres,
        worker_service: expected.worker,
        discord_guild_id: manifest.discord.guild_id,
        discord_application_id: manifest.discord.application_id,
        discord_hub_channel_id: manifest.discord.hub_channel_id,
        resource_prefix: manifest.discord.resource_prefix,
        external,
    })
}

fn valid_discord_manifest(discord: &DiscordManifestV1) -> bool {
    valid_snowflake(&discord.guild_id)
        && valid_snowflake(&discord.application_id)
        && valid_snowflake(&discord.bot_user_id)
        && valid_snowflake(&discord.actor_id)
        && valid_snowflake(&discord.hub_channel_id)
        && discord.hub_channel_id != discord.guild_id
        && discord.hub_channel_id != discord.application_id
        && discord.hub_channel_id != discord.bot_user_id
        && discord.hub_channel_id != discord.actor_id
        && valid_product_identifier(&discord.resource_prefix, 128)
}

fn validate_target_directory(path: &Path, mode: u32) -> Result<(), D2ProvisionerErrorV1> {
    let metadata = fs::symlink_metadata(path).map_err(|_| D2ProvisionerErrorV1::Target)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_user_id()
        || metadata.mode() & 0o777 != mode
        || path
            .canonicalize()
            .map_err(|_| D2ProvisionerErrorV1::Target)?
            != path
    {
        return Err(D2ProvisionerErrorV1::Target);
    }
    Ok(())
}

fn valid_run_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 32
        && value.starts_with("d2-")
        && bytes[3..11].iter().all(u8::is_ascii_digit)
        && bytes[11] == b't'
        && bytes[12..18].iter().all(u8::is_ascii_digit)
        && bytes[18] == b'z'
        && bytes[19] == b'-'
        && bytes[20..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn effective_user_id() -> u32 {
    unsafe { geteuid() }
}

fn valid_keychain_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 192
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn valid_product_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn valid_snowflake(value: &str) -> bool {
    !value.starts_with('0')
        && !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

fn managed_identities(config: &D2ConfigV1) -> Vec<(&str, &'static str)> {
    let mut identities = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| {
            let service = if identity.service == RUNTIME_KEYCHAIN_SERVICE {
                config.runtime_service.as_str()
            } else {
                config.api_service.as_str()
            };
            (service, identity.account)
        })
        .collect::<Vec<_>>();
    identities.extend([
        (config.api_service.as_str(), PRODUCT_ACTION_ACCOUNT),
        (config.api_service.as_str(), SNAPSHOT_ENVELOPE_ACCOUNT),
        (config.runtime_service.as_str(), INTERACTION_TOKEN_ACCOUNT),
        (config.postgres_service.as_str(), ADMIN_ACCOUNT),
        (config.worker_service.as_str(), WORKER_TOKEN_ACCOUNT),
    ]);
    identities
}

fn verify_keychain_owner(
    keychain: &KeychainClientV1,
    config: &D2ConfigV1,
) -> Result<(), D2ProvisionerErrorV1> {
    for service in [
        config.api_service.as_str(),
        config.runtime_service.as_str(),
        config.postgres_service.as_str(),
        config.worker_service.as_str(),
    ] {
        let value = keychain
            .read_required_dynamic(service, OWNER_ACCOUNT)
            .map_err(|_| D2ProvisionerErrorV1::KeychainOwner)?;
        if value.len() != config.run_id.len()
            || !bool::from(value.as_slice().ct_eq(config.run_id.as_bytes()))
        {
            return Err(D2ProvisionerErrorV1::KeychainOwner);
        }
    }
    Ok(())
}

fn preflight_external_credentials(
    keychain: &KeychainClientV1,
    config: &D2ConfigV1,
) -> Result<(), D2ProvisionerErrorV1> {
    for identity in &config.external {
        keychain
            .read_required_dynamic(&identity.service, &identity.account)
            .map_err(|_| D2ProvisionerErrorV1::ExternalCredentials)?;
    }
    Ok(())
}

fn inspect_credential_state(
    keychain: &KeychainClientV1,
    config: &D2ConfigV1,
) -> Result<ManagedCredentialStateV1, D2ProvisionerErrorV1> {
    let identities = managed_identities(config);
    let mut present = 0;
    for (service, account) in &identities {
        if keychain
            .read_optional_dynamic(service, account)
            .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?
            .is_some()
        {
            present += 1;
        }
    }
    Ok(if present == 0 {
        ManagedCredentialStateV1::Absent
    } else if present == identities.len() {
        ManagedCredentialStateV1::Complete
    } else {
        ManagedCredentialStateV1::Partial
    })
}

async fn connect_socket_and_lock(
    config: &D2ConfigV1,
) -> Result<PgConnection, D2ProvisionerErrorV1> {
    let options = PgConnectOptions::new()
        .host(
            config
                .socket_directory
                .to_str()
                .ok_or(D2ProvisionerErrorV1::Target)?,
        )
        .port(config.port)
        .username(CLUSTER_ADMIN_ROLE)
        .database(DATABASE_NAME)
        .ssl_mode(PgSslMode::Disable)
        .application_name("starring-d2-sealed-provisioner");
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    verify_database_contract(&mut connection, config).await?;
    let acquired: bool = sqlx::query_scalar(
        "SELECT pg_catalog.pg_try_advisory_lock(pg_catalog.hashtextextended('starring-d2-sealed-provisioner:' || $1, 0))",
    )
    .bind(&config.run_id)
    .fetch_one(&mut connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Busy)?;
    if !acquired {
        return Err(D2ProvisionerErrorV1::Busy);
    }
    verify_database_contract(&mut connection, config).await?;
    Ok(connection)
}

async fn connect_network_and_lock(
    config: &D2ConfigV1,
    keychain: &KeychainClientV1,
) -> Result<PgConnection, D2ProvisionerErrorV1> {
    let value = keychain
        .read_required_dynamic(&config.postgres_service, ADMIN_ACCOUNT)
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let options =
        exact_dynamic_connect_options(&value, CLUSTER_ADMIN_ROLE, config.port, ADMIN_DATABASE)?
            .database(DATABASE_NAME)
            .application_name("starring-d2-sealed-provisioner-network");
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    verify_network_database_contract(&mut connection, config).await?;
    let acquired: bool = sqlx::query_scalar(
        "SELECT pg_catalog.pg_try_advisory_lock(pg_catalog.hashtextextended('starring-d2-sealed-provisioner:' || $1, 0))",
    )
    .bind(&config.run_id)
    .fetch_one(&mut connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Busy)?;
    if !acquired {
        return Err(D2ProvisionerErrorV1::Busy);
    }
    verify_network_database_contract(&mut connection, config).await?;
    Ok(connection)
}

pub(crate) async fn connect_inspection_database(
    config: &D2ConfigV1,
) -> Result<PgConnection, D2ProvisionerErrorV1> {
    let keychain =
        KeychainClientV1::new().map_err(|_| D2ProvisionerErrorV1::UnsupportedPlatform)?;
    verify_keychain_owner(&keychain, config)?;
    let value = keychain
        .read_required_dynamic(&config.postgres_service, ADMIN_ACCOUNT)
        .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let options =
        exact_dynamic_connect_options(&value, CLUSTER_ADMIN_ROLE, config.port, ADMIN_DATABASE)
            .map_err(|_| D2ProvisionerErrorV1::Inspection)?
            .database(DATABASE_NAME)
            .application_name("starring-d2-sealed-inspector");
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    verify_network_database_contract(&mut connection, config)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    Ok(connection)
}

pub(crate) async fn connect_inspection_admin_database(
    config: &D2ConfigV1,
) -> Result<PgConnection, D2ProvisionerErrorV1> {
    let keychain =
        KeychainClientV1::new().map_err(|_| D2ProvisionerErrorV1::UnsupportedPlatform)?;
    verify_keychain_owner(&keychain, config)?;
    let value = keychain
        .read_required_dynamic(&config.postgres_service, ADMIN_ACCOUNT)
        .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let options =
        exact_dynamic_connect_options(&value, CLUSTER_ADMIN_ROLE, config.port, ADMIN_DATABASE)
            .map_err(|_| D2ProvisionerErrorV1::Inspection)?
            .application_name("starring-d2-sealed-inspector-absence");
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let exact: bool = sqlx::query_scalar(
        "SELECT current_setting('data_directory') = $1 AND current_setting('port')::INTEGER = $2 AND current_setting('server_version_num')::INTEGER BETWEEN 160000 AND 169999 AND current_setting('data_checksums') = 'on' AND current_user = session_user AND current_user = 'starring_cluster_admin' AND admin.rolsuper AND admin.rolcanlogin AND NOT admin.rolcreatedb AND NOT admin.rolcreaterole AND NOT admin.rolinherit AND NOT admin.rolreplication AND NOT admin.rolbypassrls AND admin.rolconnlimit = 2 AND admin.rolpassword LIKE 'SCRAM-SHA-256$4096:%' AND pg_catalog.current_database() = 'postgres' AND pg_catalog.inet_client_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_port() = $2 AND control.system_identifier::TEXT ~ '^[0-9]+$' AND NOT COALESCE((SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()), TRUE) FROM pg_catalog.pg_control_system() AS control INNER JOIN pg_catalog.pg_authid AS admin ON admin.rolname = 'starring_cluster_admin'",
    )
    .bind(config.cluster_root.to_string_lossy().as_ref())
    .bind(i32::from(config.port))
    .fetch_one(&mut connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    if !exact {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(connection)
}

async fn verify_network_database_contract(
    connection: &mut PgConnection,
    config: &D2ConfigV1,
) -> Result<(), D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT current_setting('data_directory'), current_setting('server_version_num')::INTEGER, current_setting('data_checksums'), pg_catalog.inet_client_addr() = '127.0.0.1'::INET, pg_catalog.inet_server_addr() = '127.0.0.1'::INET, pg_catalog.inet_server_port() = $1, current_user = session_user AND current_user = 'starring_cluster_admin', admin.rolsuper AND admin.rolcanlogin AND NOT admin.rolcreatedb AND NOT admin.rolcreaterole AND NOT admin.rolinherit AND NOT admin.rolreplication AND NOT admin.rolbypassrls AND admin.rolconnlimit = 2 AND admin.rolpassword LIKE 'SCRAM-SHA-256$4096:%', pg_catalog.current_database() = 'starring_runtime_staging', control.system_identifier::TEXT, database_owner.rolname = 'starring_owner', schema_owner.rolname = 'starring_owner', NOT COALESCE((SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()), TRUE) FROM pg_catalog.pg_control_system() AS control INNER JOIN pg_catalog.pg_authid AS admin ON admin.rolname = 'starring_cluster_admin' INNER JOIN pg_catalog.pg_database AS database_row ON database_row.datname = 'starring_runtime_staging' INNER JOIN pg_catalog.pg_roles AS database_owner ON database_owner.oid = database_row.datdba INNER JOIN pg_catalog.pg_namespace AS public_schema ON public_schema.nspname = 'public' INNER JOIN pg_catalog.pg_roles AS schema_owner ON schema_owner.oid = public_schema.nspowner",
    )
    .bind(i32::from(config.port))
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?
    .ok_or(D2ProvisionerErrorV1::ReplayVerification)?;
    let data_directory: String = row
        .try_get(0)
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let version: i32 = row
        .try_get(1)
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let checksums: String = row
        .try_get(2)
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let system_identifier: String = row
        .try_get(9)
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let checks = (3..=8)
        .chain(10..=12)
        .all(|index| row.try_get::<bool, _>(index).unwrap_or(false));
    if data_directory != config.cluster_root.to_string_lossy()
        || !(160000..170000).contains(&version)
        || checksums != "on"
        || system_identifier.is_empty()
        || !system_identifier.bytes().all(|byte| byte.is_ascii_digit())
        || !checks
    {
        return Err(D2ProvisionerErrorV1::ReplayVerification);
    }
    Ok(())
}

async fn verify_database_contract(
    connection: &mut PgConnection,
    config: &D2ConfigV1,
) -> Result<(), D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT current_setting('data_directory'), current_setting('port')::INTEGER, current_setting('server_version_num')::INTEGER, current_setting('data_checksums'), pg_catalog.inet_client_addr() IS NULL, current_user = session_user AND current_user = 'starring_cluster_admin', admin.rolsuper AND admin.rolcanlogin AND NOT admin.rolcreatedb AND NOT admin.rolcreaterole AND NOT admin.rolinherit AND NOT admin.rolreplication AND NOT admin.rolbypassrls AND admin.rolconnlimit = 2, pg_catalog.current_database() = 'starring_runtime_staging', control.system_identifier::TEXT, database_owner.rolname = 'starring_owner', schema_owner.rolname = 'starring_owner', NOT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity AS activity WHERE activity.pid <> pg_catalog.pg_backend_pid() AND activity.backend_type = 'client backend'), NOT EXISTS (SELECT 1 FROM pg_catalog.pg_prepared_xacts) FROM pg_catalog.pg_control_system() AS control INNER JOIN pg_catalog.pg_authid AS admin ON admin.rolname = 'starring_cluster_admin' INNER JOIN pg_catalog.pg_database AS database_row ON database_row.datname = 'starring_runtime_staging' INNER JOIN pg_catalog.pg_roles AS database_owner ON database_owner.oid = database_row.datdba INNER JOIN pg_catalog.pg_namespace AS public_schema ON public_schema.nspname = 'public' INNER JOIN pg_catalog.pg_roles AS schema_owner ON schema_owner.oid = public_schema.nspowner",
    )
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?
    .ok_or(D2ProvisionerErrorV1::DatabaseContract)?;
    let data_directory: String = row
        .try_get(0)
        .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    let port: i32 = row
        .try_get(1)
        .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    let version: i32 = row
        .try_get(2)
        .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    let checksums: String = row
        .try_get(3)
        .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    let system_identifier: String = row
        .try_get(8)
        .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    let checks = (4..=7)
        .chain(9..=12)
        .all(|index| row.try_get::<bool, _>(index).unwrap_or(false));
    if data_directory != config.cluster_root.to_string_lossy()
        || port != i32::from(config.port)
        || !(160000..170000).contains(&version)
        || checksums != "on"
        || system_identifier.is_empty()
        || !system_identifier.bytes().all(|byte| byte.is_ascii_digit())
        || !checks
    {
        return Err(D2ProvisionerErrorV1::DatabaseContract);
    }
    Ok(())
}

async fn inspect_role_state(
    connection: &mut PgConnection,
) -> Result<ManagedRoleStateV1, D2ProvisionerErrorV1> {
    let roles = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    let rows = sqlx::query(
        "SELECT role.rolname, role.rolcanlogin, NOT role.rolsuper AND NOT role.rolcreatedb AND NOT role.rolcreaterole AND NOT role.rolinherit AND NOT role.rolreplication AND NOT role.rolbypassrls AND role.rolconnlimit = 4 AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting WHERE setting.setrole = role.oid) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid = role.oid OR membership.member = role.oid), role.rolpassword FROM pg_catalog.pg_authid AS role WHERE role.rolname = ANY($1::TEXT[]) ORDER BY role.rolname",
    )
    .bind(&roles)
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    if rows.is_empty() {
        return Ok(ManagedRoleStateV1::Absent);
    }
    if rows.len() != roles.len() {
        return Ok(ManagedRoleStateV1::Partial);
    }
    let mut all_quarantined = true;
    let mut all_enabled = true;
    for row in rows {
        let login: bool = row
            .try_get(1)
            .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
        let exact: bool = row
            .try_get(2)
            .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
        let password: Option<String> = row
            .try_get(3)
            .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
        all_quarantined &= exact && !login && password.is_none();
        all_enabled &= exact
            && login
            && password
                .as_deref()
                .is_some_and(crate::crypto::valid_scram_verifier);
    }
    Ok(if all_quarantined {
        ManagedRoleStateV1::Quarantined
    } else if all_enabled {
        ManagedRoleStateV1::Enabled
    } else {
        ManagedRoleStateV1::Partial
    })
}

async fn bootstrap_roles(connection: &mut PgConnection) -> Result<(), D2ProvisionerErrorV1> {
    set_api_script_context(connection).await?;
    sqlx::raw_sql(API_ROLE_BOOTSTRAP)
        .execute(&mut *connection)
        .await
        .map_err(|_| D2ProvisionerErrorV1::RoleBootstrap)?;
    run_runtime_script(connection, false).await
}

async fn activate_roles(connection: &mut PgConnection) -> Result<(), D2ProvisionerErrorV1> {
    set_api_script_context(connection).await?;
    sqlx::raw_sql(API_ROLE_ENABLE)
        .execute(&mut *connection)
        .await
        .map_err(|_| D2ProvisionerErrorV1::RoleActivation)?;
    run_runtime_script(connection, true)
        .await
        .map_err(|_| D2ProvisionerErrorV1::RoleActivation)
}

async fn set_api_script_context(connection: &mut PgConnection) -> Result<(), D2ProvisionerErrorV1> {
    let system_identifier: String = sqlx::query_scalar(
        "SELECT control.system_identifier::TEXT FROM pg_catalog.pg_control_system() AS control",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    sqlx::query(
        "SELECT set_config('starring.expected_staging_database', $1, FALSE), set_config('starring.expected_staging_system_identifier', $2, FALSE)",
    )
    .bind(DATABASE_NAME)
    .bind(system_identifier)
    .execute(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::DatabaseContract)?;
    Ok(())
}

async fn run_runtime_script(
    connection: &mut PgConnection,
    enable: bool,
) -> Result<(), D2ProvisionerErrorV1> {
    let system_identifier: String = sqlx::query_scalar(
        "SELECT control.system_identifier::TEXT FROM pg_catalog.pg_control_system() AS control",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::RoleBootstrap)?;
    let acknowledgement = format!(
        "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:{DATABASE_NAME}:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
    );
    let script = render_runtime_script(enable, &system_identifier, &acknowledgement)?;
    sqlx::raw_sql(&script)
        .execute(&mut *connection)
        .await
        .map_err(|_| D2ProvisionerErrorV1::RoleBootstrap)?;
    sqlx::raw_sql(RUNTIME_TEMP_TABLE_CLEANUP)
        .execute(connection)
        .await
        .map(|_| ())
        .map_err(|_| D2ProvisionerErrorV1::RoleBootstrap)
}

fn render_runtime_script(
    enable: bool,
    system_identifier: &str,
    acknowledgement: &str,
) -> Result<String, D2ProvisionerErrorV1> {
    if system_identifier.is_empty()
        || !system_identifier.bytes().all(|byte| byte.is_ascii_digit())
        || acknowledgement
            != format!(
                "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:{DATABASE_NAME}:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
            )
    {
        return Err(D2ProvisionerErrorV1::RoleBootstrap);
    }
    let start = RUNTIME_ROLE_BOOTSTRAP
        .find("SET lock_timeout = '5s';")
        .ok_or(D2ProvisionerErrorV1::RoleBootstrap)?;
    let mut rendered = RUNTIME_ROLE_BOOTSTRAP[start..].to_owned();
    for (placeholder, value) in [
        ("'runtime_enable'", if enable { "on" } else { "off" }),
        ("'expected_database'", DATABASE_NAME),
        ("'expected_system_identifier'", system_identifier),
        (
            "'runtime_dedicated_cluster_acknowledgement'",
            acknowledgement,
        ),
        ("'runtime_execution_role'", "starring_runtime_execution"),
        (
            "'runtime_exact_target_role'",
            "starring_runtime_exact_target",
        ),
        ("'runtime_panel_role'", "starring_runtime_panel"),
        ("'runtime_serving_role'", "starring_runtime_serving"),
        ("'runtime_interaction_role'", "starring_runtime_interaction"),
    ] {
        rendered = rendered.replace(&format!(":{placeholder}"), &format!("'{value}'"));
    }
    if rendered.contains(":'") || rendered.lines().any(|line| line.starts_with('\\')) {
        return Err(D2ProvisionerErrorV1::RoleBootstrap);
    }
    Ok(rendered)
}

struct OnboardingInputsV1<'a> {
    system_identifier: &'a str,
    tenant_id: &'a str,
    tenant_display_name: &'a str,
    installation_id: &'a str,
    discord_application_id: &'a str,
    discord_guild_id: &'a str,
    discord_hub_channel_id: &'a str,
    ruleset_key: &'a str,
    principal_id: &'a str,
    discord_user_id: &'a str,
    resource_bindings: &'a str,
    binding_fingerprint: &'a str,
    authority_payload_digest: &'a str,
    created_by_request_digest: &'a str,
    port: u16,
}

fn render_onboarding_script(
    inputs: &OnboardingInputsV1<'_>,
) -> Result<String, D2ProvisionerErrorV1> {
    if !valid_product_identifier(inputs.tenant_id, 128)
        || !valid_product_identifier(inputs.installation_id, 128)
        || !valid_product_identifier(inputs.principal_id, 128)
        || !valid_snowflake(inputs.discord_application_id)
        || !valid_snowflake(inputs.discord_guild_id)
        || !valid_snowflake(inputs.discord_hub_channel_id)
        || !valid_snowflake(inputs.discord_user_id)
        || d2_resource_bindings_json(inputs.discord_hub_channel_id).as_deref()
            != Ok(inputs.resource_bindings)
        || d2_resource_binding_fingerprint_v2(inputs.discord_hub_channel_id).as_deref()
            != Ok(inputs.binding_fingerprint)
        || !valid_digest(inputs.authority_payload_digest)
        || !valid_digest(inputs.created_by_request_digest)
        || inputs.system_identifier.is_empty()
        || !inputs
            .system_identifier
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        || inputs.port < 1024
        || inputs.port == 5432
    {
        return Err(D2ProvisionerErrorV1::OnboardingInput);
    }
    let start = INSTALLATION_ONBOARDING
        .find("SET lock_timeout = '5s';")
        .ok_or(D2ProvisionerErrorV1::Onboarding)?;
    let mut rendered = INSTALLATION_ONBOARDING[start..].to_owned();
    let final_transaction = "\\if :commit_onboarding\nCOMMIT;\n\\else\nROLLBACK;\n\\endif";
    if rendered.matches(final_transaction).count() != 1 {
        return Err(D2ProvisionerErrorV1::Onboarding);
    }
    rendered = rendered.replace(final_transaction, "COMMIT;");
    let fixed_port = "pg_catalog.inet_server_port() IS DISTINCT FROM 5432";
    if rendered.matches(fixed_port).count() != 1 {
        return Err(D2ProvisionerErrorV1::Onboarding);
    }
    rendered = rendered.replace(
        fixed_port,
        &format!(
            "pg_catalog.inet_server_port() IS DISTINCT FROM {}",
            inputs.port
        ),
    );
    if rendered.matches(EMPTY_RESOURCE_BINDINGS_JSON).count() != 1
        || rendered.matches(EMPTY_BINDINGS_FINGERPRINT).count() != 1
    {
        return Err(D2ProvisionerErrorV1::Onboarding);
    }
    rendered = rendered.replace(EMPTY_RESOURCE_BINDINGS_JSON, inputs.resource_bindings);
    rendered = rendered.replace(EMPTY_BINDINGS_FINGERPRINT, inputs.binding_fingerprint);
    for (name, value) in [
        ("expected_database", DATABASE_NAME),
        ("expected_system_identifier", inputs.system_identifier),
        ("tenant_id", inputs.tenant_id),
        ("tenant_display_name", inputs.tenant_display_name),
        ("installation_id", inputs.installation_id),
        ("discord_application_id", inputs.discord_application_id),
        ("discord_guild_id", inputs.discord_guild_id),
        ("ruleset_key", inputs.ruleset_key),
        ("created_by_principal_id", inputs.principal_id),
        ("created_by_discord_user_id", inputs.discord_user_id),
        ("binding_fingerprint", inputs.binding_fingerprint),
        ("authority_payload_digest", inputs.authority_payload_digest),
        (
            "created_by_request_digest",
            inputs.created_by_request_digest,
        ),
        ("commit_onboarding", "true"),
    ] {
        let placeholder = format!(":'{name}'");
        if !rendered.contains(&placeholder) {
            return Err(D2ProvisionerErrorV1::Onboarding);
        }
        rendered = rendered.replace(&placeholder, &sql_literal(value));
    }
    if rendered.contains(":'") || rendered.lines().any(|line| line.starts_with('\\')) {
        return Err(D2ProvisionerErrorV1::Onboarding);
    }
    Ok(rendered)
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn d2_resource_bindings_json(hub_channel_id: &str) -> Result<String, D2ProvisionerErrorV1> {
    if !valid_snowflake(hub_channel_id) {
        return Err(D2ProvisionerErrorV1::OnboardingInput);
    }
    Ok(format!(
        "{{\"role_bindings\":{{}},\"channel_bindings\":{{\"{D2_HUB_BINDING_KEY}\":\"{hub_channel_id}\"}}}}"
    ))
}

fn d2_resource_binding_fingerprint_v2(
    hub_channel_id: &str,
) -> Result<String, D2ProvisionerErrorV1> {
    if !valid_snowflake(hub_channel_id) {
        return Err(D2ProvisionerErrorV1::OnboardingInput);
    }
    Ok(digest_fields(
        RESOURCE_CONTEXT_DOMAIN_V2,
        &["channel", D2_HUB_BINDING_KEY, hub_channel_id],
    ))
}

fn d2_onboarding_request_digest(
    run_id: &str,
    tenant_id: &str,
    installation_id: &str,
    principal_id: &str,
    display_name: &str,
    authority_payload_digest: &str,
) -> String {
    digest_fields(
        "starring-d2-onboarding-request-v1",
        &[
            run_id,
            tenant_id,
            installation_id,
            principal_id,
            display_name,
            authority_payload_digest,
        ],
    )
}

fn d2_authority_payload_digest_v1(
    tenant_id: &str,
    installation_id: &str,
    binding_fingerprint: &str,
) -> Result<String, D2ProvisionerErrorV1> {
    if !valid_product_identifier(tenant_id, 128)
        || !valid_product_identifier(installation_id, 128)
        || !valid_digest(binding_fingerprint)
    {
        return Err(D2ProvisionerErrorV1::OnboardingInput);
    }
    let revision = 1_u64.to_be_bytes();
    let binding_revision = 1_u64.to_be_bytes();
    let policy_revision = 1_u64.to_be_bytes();
    let required_approvals = 1_u32.to_be_bytes();
    let activation_ttl_seconds = 86_400_u64.to_be_bytes();
    Ok(digest_byte_fields(
        AUTHORITY_PAYLOAD_DOMAIN_V1,
        &[
            tenant_id.as_bytes(),
            installation_id.as_bytes(),
            &revision,
            &binding_revision,
            binding_fingerprint.as_bytes(),
            &policy_revision,
            &required_approvals,
            &activation_ttl_seconds,
        ],
    ))
}

fn digest_byte_fields(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    digest.update((domain.len() as u64).to_be_bytes());
    digest.update(domain);
    for field in fields {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field);
    }
    format!("{:x}", digest.finalize())
}

fn digest_fields(domain: &str, fields: &[&str]) -> String {
    let fields = fields
        .iter()
        .map(|field| field.as_bytes())
        .collect::<Vec<_>>();
    digest_byte_fields(domain.as_bytes(), &fields)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

async fn verify_onboarding(
    connection: &mut PgConnection,
    inputs: &OnboardingInputsV1<'_>,
) -> Result<(), D2ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) = 1 FROM public.product_tenants AS tenant WHERE tenant.tenant_id = $1 AND tenant.lifecycle_state = 'active' AND tenant.display_name = $2 AND tenant.display_metadata = '{\"environment\":\"staging\",\"onboarding\":\"operator_v1\"}'::JSONB) AND (SELECT COUNT(*) = 1 FROM public.automation_installations AS installation WHERE installation.installation_id = $3 AND installation.tenant_id = $1 AND installation.discord_application_id = $4 AND installation.discord_guild_id = $5 AND installation.ruleset_key = $6 AND installation.lifecycle_state = 'active' AND installation.current_authority_revision = 1) AND (SELECT COUNT(*) = 1 FROM public.automation_installation_authority_versions AS authority WHERE authority.installation_id = $3 AND authority.revision = 1 AND authority.tenant_id = $1 AND authority.binding_revision = 1 AND authority.resource_bindings = $7::JSONB AND authority.binding_fingerprint = $8 AND authority.policy_revision = 1 AND authority.required_approvals = 1 AND authority.activation_ttl_seconds = 86400 AND authority.authority_payload_digest = $9 AND authority.created_by_principal_id = $10 AND authority.created_by_request_digest = $11) AND (SELECT COUNT(*) = 1 FROM public.runtime_slot_writer_fences_v2 AS fence WHERE fence.slot_guild_id = $5 AND fence.slot_ruleset_key = $6 AND fence.writer_epoch BETWEEN 1 AND 9223372036854775807 AND pg_catalog.isfinite(fence.updated_at))",
    )
    .bind(inputs.tenant_id)
    .bind(inputs.tenant_display_name)
    .bind(inputs.installation_id)
    .bind(inputs.discord_application_id)
    .bind(inputs.discord_guild_id)
    .bind(inputs.ruleset_key)
    .bind(inputs.resource_bindings)
    .bind(inputs.binding_fingerprint)
    .bind(inputs.authority_payload_digest)
    .bind(inputs.principal_id)
    .bind(inputs.created_by_request_digest)
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Onboarding)?;
    if exact {
        Ok(())
    } else {
        Err(D2ProvisionerErrorV1::Onboarding)
    }
}

fn build_owned_secret_items(
    config: &D2ConfigV1,
    secrets: &GeneratedSecretsV1,
) -> Result<Vec<D2OwnedSecretItemV1>, D2ProvisionerErrorV1> {
    let mut items = Vec::with_capacity(APPLICATION_DATABASE_IDENTITIES.len() + 5);
    for secret in secrets.database() {
        let service = if secret.identity().service == RUNTIME_KEYCHAIN_SERVICE {
            config.runtime_service.clone()
        } else {
            config.api_service.clone()
        };
        let value = dynamic_database_url(
            secret.identity().role,
            secret.password(),
            config.port,
            DATABASE_NAME,
        );
        items.push(D2OwnedSecretItemV1 {
            service,
            account: secret.identity().account,
            value: Zeroizing::new(value.as_bytes().to_vec()),
        });
    }
    let admin = dynamic_database_url(
        CLUSTER_ADMIN_ROLE,
        secrets.admin().password(),
        config.port,
        ADMIN_DATABASE,
    );
    items.push(D2OwnedSecretItemV1 {
        service: config.postgres_service.clone(),
        account: ADMIN_ACCOUNT,
        value: Zeroizing::new(admin.as_bytes().to_vec()),
    });
    items.push(D2OwnedSecretItemV1 {
        service: config.api_service.clone(),
        account: PRODUCT_ACTION_ACCOUNT,
        value: Zeroizing::new(secrets.product_action_keyring_payload().to_vec()),
    });
    items.push(D2OwnedSecretItemV1 {
        service: config.api_service.clone(),
        account: SNAPSHOT_ENVELOPE_ACCOUNT,
        value: Zeroizing::new(secrets.snapshot_envelope_keyring_payload().to_vec()),
    });
    items.push(D2OwnedSecretItemV1 {
        service: config.runtime_service.clone(),
        account: INTERACTION_TOKEN_ACCOUNT,
        value: Zeroizing::new(
            secrets
                .interaction_token_envelope_keyring_payload()
                .to_vec(),
        ),
    });
    let mut worker_random = Zeroizing::new([0_u8; 32]);
    getrandom::fill(&mut *worker_random).map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    let worker_token = Zeroizing::new(URL_SAFE_NO_PAD.encode(worker_random.as_slice()));
    items.push(D2OwnedSecretItemV1 {
        service: config.worker_service.clone(),
        account: WORKER_TOKEN_ACCOUNT,
        value: Zeroizing::new(worker_token.as_bytes().to_vec()),
    });
    if items.len() != APPLICATION_DATABASE_IDENTITIES.len() + 5 {
        return Err(D2ProvisionerErrorV1::CredentialSealing);
    }
    Ok(items)
}

fn dynamic_database_url(
    role: &str,
    password: &str,
    port: u16,
    database: &str,
) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "postgresql://{role}:{password}@{DATABASE_HOST}:{port}/{database}?sslmode=disable"
    ))
}

async fn apply_verifiers(
    connection: &mut PgConnection,
    secrets: &GeneratedSecretsV1,
) -> Result<(), D2ProvisionerErrorV1> {
    if inspect_role_state(connection).await? != ManagedRoleStateV1::Quarantined {
        return Err(D2ProvisionerErrorV1::CredentialSealing);
    }
    let mut transaction = connection
        .begin()
        .await
        .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    for secret in secrets.database() {
        let mutation = alter_role_password_sql(secret.identity().role, secret.verifier())
            .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
        sqlx::query(&mutation)
            .execute(&mut *transaction)
            .await
            .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    }
    let admin_mutation = alter_role_password_sql(CLUSTER_ADMIN_ROLE, secrets.admin().verifier())
        .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    sqlx::query(&admin_mutation)
        .execute(&mut *transaction)
        .await
        .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)?;
    transaction
        .commit()
        .await
        .map_err(|_| D2ProvisionerErrorV1::CredentialSealing)
}

async fn verify_enabled_roles(connection: &mut PgConnection) -> Result<(), D2ProvisionerErrorV1> {
    if inspect_role_state(connection).await? != ManagedRoleStateV1::Enabled {
        return Err(D2ProvisionerErrorV1::RoleActivation);
    }
    let admin: bool = sqlx::query_scalar(
        "SELECT rolcanlogin AND rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolinherit AND NOT rolreplication AND NOT rolbypassrls AND rolconnlimit = 2 AND rolpassword LIKE 'SCRAM-SHA-256$4096:%' FROM pg_catalog.pg_authid WHERE rolname = 'starring_cluster_admin'",
    )
    .fetch_optional(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::RoleActivation)?
    .ok_or(D2ProvisionerErrorV1::RoleActivation)?;
    if admin {
        Ok(())
    } else {
        Err(D2ProvisionerErrorV1::RoleActivation)
    }
}

fn verify_owned_secret_shapes(
    config: &D2ConfigV1,
    items: &[D2OwnedSecretItemV1],
) -> Result<(), D2ProvisionerErrorV1> {
    for identity in APPLICATION_DATABASE_IDENTITIES {
        let service = if identity.service == RUNTIME_KEYCHAIN_SERVICE {
            config.runtime_service.as_str()
        } else {
            config.api_service.as_str()
        };
        let item = items
            .iter()
            .find(|item| item.service == service && item.account == identity.account)
            .ok_or(D2ProvisionerErrorV1::ReplayVerification)?;
        exact_dynamic_connect_options(
            item.value.as_slice(),
            identity.role,
            config.port,
            DATABASE_NAME,
        )?;
    }
    let admin = find_item(items, &config.postgres_service, ADMIN_ACCOUNT)?;
    exact_dynamic_connect_options(admin, CLUSTER_ADMIN_ROLE, config.port, ADMIN_DATABASE)?;
    let product = find_item(items, &config.api_service, PRODUCT_ACTION_ACCOUNT)?;
    let snapshot = find_item(items, &config.api_service, SNAPSHOT_ENVELOPE_ACCOUNT)?;
    let interaction = find_item(items, &config.runtime_service, INTERACTION_TOKEN_ACCOUNT)?;
    validate_keyring_set(product, snapshot, interaction)
        .map(|_| ())
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let worker = find_item(items, &config.worker_service, WORKER_TOKEN_ACCOUNT)?;
    if worker.len() != 43
        || !worker
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(D2ProvisionerErrorV1::ReplayVerification);
    }
    Ok(())
}

async fn verify_dynamic_connections(
    config: &D2ConfigV1,
    items: &[D2OwnedSecretItemV1],
) -> Result<(), D2ProvisionerErrorV1> {
    for identity in APPLICATION_DATABASE_IDENTITIES {
        let service = if identity.service == RUNTIME_KEYCHAIN_SERVICE {
            config.runtime_service.as_str()
        } else {
            config.api_service.as_str()
        };
        let item = items
            .iter()
            .find(|item| item.service == service && item.account == identity.account)
            .ok_or(D2ProvisionerErrorV1::ReplayVerification)?;
        let options = exact_dynamic_connect_options(
            item.value.as_slice(),
            identity.role,
            config.port,
            DATABASE_NAME,
        )?;
        let mut connection = PgConnection::connect_with(&options)
            .await
            .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
        verify_application_connection(&mut connection, identity.role, config.port).await?;
    }
    let admin = items
        .iter()
        .find(|item| item.service == config.postgres_service && item.account == ADMIN_ACCOUNT)
        .ok_or(D2ProvisionerErrorV1::ReplayVerification)?;
    let options = exact_dynamic_connect_options(
        admin.value.as_slice(),
        CLUSTER_ADMIN_ROLE,
        config.port,
        ADMIN_DATABASE,
    )?;
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    verify_admin_tcp_connection(&mut connection, config.port).await
}

async fn verify_exact_replay(
    config: &D2ConfigV1,
    keychain: &KeychainClientV1,
    database: &mut PgConnection,
) -> Result<(), D2ProvisionerErrorV1> {
    verify_enabled_roles(database).await?;
    let mut items = Vec::with_capacity(APPLICATION_DATABASE_IDENTITIES.len() + 4);
    for (service, account) in managed_identities(config) {
        let value = keychain
            .read_required_dynamic(service, account)
            .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
        items.push(D2OwnedSecretItemV1 {
            service: service.to_owned(),
            account,
            value,
        });
    }
    let product = find_item(&items, &config.api_service, PRODUCT_ACTION_ACCOUNT)?;
    let snapshot = find_item(&items, &config.api_service, SNAPSHOT_ENVELOPE_ACCOUNT)?;
    let interaction = find_item(&items, &config.runtime_service, INTERACTION_TOKEN_ACCOUNT)?;
    validate_keyring_set(product, snapshot, interaction)
        .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let worker = find_item(&items, &config.worker_service, WORKER_TOKEN_ACCOUNT)?;
    if worker.len() != 43
        || !worker
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(D2ProvisionerErrorV1::ReplayVerification);
    }
    verify_dynamic_connections(config, &items).await
}

fn find_item<'a>(
    items: &'a [D2OwnedSecretItemV1],
    service: &str,
    account: &str,
) -> Result<&'a [u8], D2ProvisionerErrorV1> {
    items
        .iter()
        .find(|item| item.service == service && item.account == account)
        .map(|item| item.value.as_slice())
        .ok_or(D2ProvisionerErrorV1::ReplayVerification)
}

fn exact_dynamic_connect_options(
    value: &[u8],
    role: &str,
    port: u16,
    database: &str,
) -> Result<PgConnectOptions, D2ProvisionerErrorV1> {
    let value = std::str::from_utf8(value).map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    let prefix = format!("postgresql://{role}:");
    let suffix = format!("@{DATABASE_HOST}:{port}/{database}?sslmode=disable");
    let password = value
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
        .ok_or(D2ProvisionerErrorV1::ReplayVerification)?;
    if password.len() != 43
        || !password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(D2ProvisionerErrorV1::ReplayVerification);
    }
    let options =
        PgConnectOptions::from_str(value).map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    Ok(options
        .host(DATABASE_HOST)
        .port(port)
        .username(role)
        .database(database)
        .ssl_mode(PgSslMode::Disable)
        .application_name("starring-d2-sealed-provisioner-verifier"))
}

async fn verify_application_connection(
    connection: &mut PgConnection,
    role: &str,
    port: u16,
) -> Result<(), D2ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(
        "SELECT current_user = session_user AND current_user = $1 AND pg_catalog.current_database() = 'starring_runtime_staging' AND pg_catalog.inet_client_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_port() = $2 AND NOT COALESCE((SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()), TRUE)",
    )
    .bind(role)
    .bind(i32::from(port))
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    if exact {
        Ok(())
    } else {
        Err(D2ProvisionerErrorV1::ReplayVerification)
    }
}

async fn verify_admin_tcp_connection(
    connection: &mut PgConnection,
    port: u16,
) -> Result<(), D2ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(
        "SELECT current_user = session_user AND current_user = 'starring_cluster_admin' AND pg_catalog.current_database() = 'postgres' AND pg_catalog.inet_client_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_port() = $1 AND NOT COALESCE((SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()), TRUE)",
    )
    .bind(i32::from(port))
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::ReplayVerification)?;
    if exact {
        Ok(())
    } else {
        Err(D2ProvisionerErrorV1::ReplayVerification)
    }
}

async fn quarantine_database(connection: &mut PgConnection) -> Result<usize, D2ProvisionerErrorV1> {
    let existing = existing_managed_roles(connection).await?;
    let mut transaction = connection
        .begin()
        .await
        .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
    for role in &existing {
        let role = APPLICATION_DATABASE_IDENTITIES
            .iter()
            .find(|identity| identity.role == role)
            .map(|identity| identity.role)
            .ok_or(D2ProvisionerErrorV1::Quarantine)?;
        let sql = format!(
            "ALTER ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL 'infinity' PASSWORD NULL"
        );
        sqlx::query(&sql)
            .execute(&mut *transaction)
            .await
            .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
        sqlx::query(&format!("ALTER ROLE {role} RESET ALL"))
            .execute(&mut *transaction)
            .await
            .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
        sqlx::query(&format!(
            "ALTER ROLE {role} IN DATABASE {DATABASE_NAME} RESET ALL"
        ))
        .execute(&mut *transaction)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
    }
    sqlx::query("ALTER ROLE starring_cluster_admin PASSWORD NULL")
        .execute(&mut *transaction)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
    transaction
        .commit()
        .await
        .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
    let roles = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    let access_quarantined: bool = sqlx::query_scalar(
        "SELECT COUNT(*) = $2 AND COALESCE(BOOL_AND(NOT rolcanlogin AND rolpassword IS NULL), TRUE) FROM pg_catalog.pg_authid WHERE rolname = ANY($1::TEXT[])",
    )
    .bind(&roles)
    .bind(existing.len() as i64)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Quarantine)?;
    let admin_password_absent: bool = sqlx::query_scalar(
        "SELECT rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname = 'starring_cluster_admin'",
    )
    .fetch_optional(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Quarantine)?
    .ok_or(D2ProvisionerErrorV1::Quarantine)?;
    if !access_quarantined || !admin_password_absent {
        return Err(D2ProvisionerErrorV1::Quarantine);
    }
    Ok(existing.len())
}

async fn existing_managed_roles(
    connection: &mut PgConnection,
) -> Result<Vec<String>, D2ProvisionerErrorV1> {
    let roles = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    sqlx::query_scalar(
        "SELECT rolname FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[]) ORDER BY rolname",
    )
    .bind(roles)
    .fetch_all(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Quarantine)
}

fn cleanup_credentials(
    keychain: &KeychainClientV1,
    config: &D2ConfigV1,
) -> Result<usize, D2ProvisionerErrorV1> {
    let identities = managed_identities(config);
    let mut removed = 0;
    for (service, account) in &identities {
        if keychain
            .read_optional_dynamic(service, account)
            .map_err(|_| D2ProvisionerErrorV1::Cleanup)?
            .is_some()
        {
            keychain
                .delete_dynamic(service, account)
                .map_err(|_| D2ProvisionerErrorV1::Cleanup)?;
            removed += 1;
        }
    }
    for (service, account) in identities {
        if keychain
            .read_optional_dynamic(service, account)
            .map_err(|_| D2ProvisionerErrorV1::Cleanup)?
            .is_some()
        {
            return Err(D2ProvisionerErrorV1::Cleanup);
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_and_keychain_components_are_strict() {
        assert!(valid_run_id("d2-20260801t120000z-0123456789ab"));
        assert!(!valid_run_id("d2-20260801t120000z-0123456789AB"));
        assert!(valid_keychain_component("starring.d2.0123456789ab.api"));
        assert!(!valid_keychain_component("starring d2"));
        assert!(!valid_keychain_component("-starring"));
    }

    #[test]
    fn inventory_is_exact_and_excludes_lifecycle_owner() {
        let config = D2ConfigV1 {
            run_id: "d2-20260801t120000z-0123456789ab".to_owned(),
            cluster_root: PathBuf::from(
                "/private/tmp/starring-d2-d2-20260801t120000z-0123456789ab/postgres",
            ),
            socket_directory: PathBuf::from(
                "/private/tmp/starring-d2-d2-20260801t120000z-0123456789ab/socket",
            ),
            port: 25432,
            api_service: "starring.d2.0123456789ab.api".to_owned(),
            runtime_service: "starring.d2.0123456789ab.runtime".to_owned(),
            postgres_service: "starring.d2.0123456789ab.postgres".to_owned(),
            worker_service: "starring.d2.0123456789ab.worker".to_owned(),
            discord_guild_id: "123456789012345678".to_owned(),
            discord_application_id: "223456789012345678".to_owned(),
            discord_hub_channel_id: "323456789012345678".to_owned(),
            resource_prefix: "starring-d2-20260801-0123456789ab".to_owned(),
            external: [
                KeychainIdentityV1 {
                    service: "d2.external.oauth".to_owned(),
                    account: "credential".to_owned(),
                },
                KeychainIdentityV1 {
                    service: "d2.external.bot".to_owned(),
                    account: "credential".to_owned(),
                },
                KeychainIdentityV1 {
                    service: "d2.external.tunnel".to_owned(),
                    account: "credential".to_owned(),
                },
            ],
        };
        let inventory = managed_identities(&config);
        assert_eq!(inventory.len(), 25);
        assert_eq!(inventory.iter().collect::<BTreeSet<_>>().len(), 25);
        assert!(!inventory
            .iter()
            .any(|(_, account)| *account == OWNER_ACCOUNT));
        assert_eq!(
            inventory
                .iter()
                .filter(|(service, _)| *service == config.api_service)
                .count(),
            17
        );
        assert_eq!(
            inventory
                .iter()
                .filter(|(service, _)| *service == config.runtime_service)
                .count(),
            6
        );
        assert_eq!(
            inventory
                .iter()
                .filter(|(service, _)| *service == config.postgres_service)
                .count(),
            1
        );
        assert_eq!(
            inventory
                .iter()
                .filter(|(service, _)| *service == config.worker_service)
                .count(),
            1
        );
    }

    #[test]
    fn dynamic_database_url_shape_is_exact_and_secret_free_errors_are_stable() {
        let password = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let value =
            dynamic_database_url("starring_runtime_execution", password, 25432, DATABASE_NAME);
        assert!(exact_dynamic_connect_options(
            value.as_bytes(),
            "starring_runtime_execution",
            25432,
            DATABASE_NAME
        )
        .is_ok());
        assert!(exact_dynamic_connect_options(
            value.as_bytes(),
            "starring_runtime_execution",
            25433,
            DATABASE_NAME
        )
        .is_err());
        for error in [
            D2ProvisionerErrorV1::CredentialSealing,
            D2ProvisionerErrorV1::ReplayVerification,
            D2ProvisionerErrorV1::PartialStateQuarantined,
            D2ProvisionerErrorV1::Cleanup,
        ] {
            assert_eq!(error.to_string(), error.code());
            assert!(!error.to_string().contains("postgresql://"));
            assert!(!error.to_string().contains("SCRAM-SHA-256"));
        }
    }

    #[test]
    fn exact_role_manifests_are_compiled_into_the_candidate() {
        assert!(API_ROLE_BOOTSTRAP.starts_with("BEGIN;"));
        assert!(API_ROLE_ENABLE.starts_with("BEGIN;"));
        assert!(RUNTIME_ROLE_BOOTSTRAP.starts_with("\\set ON_ERROR_STOP on"));
        assert!(API_ROLE_BOOTSTRAP.contains("starring_authoring_session_writer"));
        assert!(RUNTIME_ROLE_BOOTSTRAP.contains(") <> 84 THEN"));
        assert!(RUNTIME_ROLE_BOOTSTRAP
            .contains("starring_runtime_interaction_effect_response_tail_finalize_v1"));
        assert!(
            INSTALLATION_ONBOARDING.contains("staging installation onboarding replay conflicts")
        );
    }

    #[test]
    fn runtime_renderer_preserves_all_exact_session_guards() {
        let system_identifier = "7663763942264209752";
        let acknowledgement = format!(
            "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:{DATABASE_NAME}:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
        );
        let quarantined =
            render_runtime_script(false, system_identifier, &acknowledgement).unwrap();
        let enabled = render_runtime_script(true, system_identifier, &acknowledgement).unwrap();
        for required in [
            "SET lock_timeout = '5s';",
            "SET starring.runtime_enable = 'off';",
            "SET starring.expected_staging_database = 'starring_runtime_staging';",
            "SET starring.expected_staging_system_identifier = '7663763942264209752';",
            "SET starring.runtime_dedicated_cluster_acknowledgement =",
            "'starring_runtime_execution'",
            "'starring_runtime_exact_target'",
            "'starring_runtime_panel'",
            "'starring_runtime_serving'",
            "'starring_runtime_interaction'",
        ] {
            assert!(quarantined.contains(required), "{required}");
        }
        assert!(enabled.contains("SET starring.runtime_enable = 'on';"));
        assert!(!quarantined.contains(":'"));
        assert!(!enabled.lines().any(|line| line.starts_with('\\')));
    }

    #[test]
    fn runtime_script_cleanup_drops_both_preserved_temp_tables_in_dependency_order() {
        assert_eq!(
            RUNTIME_TEMP_TABLE_CLEANUP.split("; ").collect::<Vec<_>>(),
            [
                "DROP TABLE pg_temp.starring_runtime_capability_functions",
                "DROP TABLE pg_temp.starring_runtime_capability_roles",
            ]
        );
    }

    #[test]
    fn onboarding_renderer_generalizes_only_the_bound_target_and_inputs() {
        let hub_channel_id = "423456789012345678";
        let resource_bindings = d2_resource_bindings_json(hub_channel_id).unwrap();
        let binding_fingerprint = d2_resource_binding_fingerprint_v2(hub_channel_id).unwrap();
        let authority = d2_authority_payload_digest_v1(
            "tenant:starring-d2-test",
            "installation:starring-d2-test",
            &binding_fingerprint,
        )
        .unwrap();
        let request = d2_onboarding_request_digest(
            "d2-20260801t120000z-0123456789ab",
            "tenant:starring-d2-test",
            "installation:starring-d2-test",
            "discord:323456789012345678",
            "보건 O'Hara",
            &authority,
        );
        let inputs = OnboardingInputsV1 {
            system_identifier: "7663763942264209752",
            tenant_id: "tenant:starring-d2-test",
            tenant_display_name: "보건 O'Hara",
            installation_id: "installation:starring-d2-test",
            discord_application_id: "123456789012345678",
            discord_guild_id: "223456789012345678",
            discord_hub_channel_id: hub_channel_id,
            ruleset_key: "studyroom",
            principal_id: "discord:323456789012345678",
            discord_user_id: "323456789012345678",
            resource_bindings: &resource_bindings,
            binding_fingerprint: &binding_fingerprint,
            authority_payload_digest: &authority,
            created_by_request_digest: &request,
            port: 25432,
        };
        let rendered = render_onboarding_script(&inputs).unwrap();
        assert!(rendered.contains("pg_catalog.inet_server_port() IS DISTINCT FROM 25432"));
        assert!(!rendered.contains("pg_catalog.inet_server_port() IS DISTINCT FROM 5432"));
        assert!(rendered.contains("SET starring.onboarding_tenant_display_name = '보건 O''Hara';"));
        assert!(rendered.contains(&format!(
            "resource_bindings PG_CATALOG.JSONB :=\n        '{resource_bindings}'::PG_CATALOG.JSONB;"
        )));
        assert!(rendered.contains(&format!(
            "SET starring.onboarding_binding_fingerprint = '{binding_fingerprint}';"
        )));
        assert!(!rendered.contains(EMPTY_RESOURCE_BINDINGS_JSON));
        assert!(!rendered.contains(EMPTY_BINDINGS_FINGERPRINT));
        assert!(rendered.trim_end().ends_with("COMMIT;"));
        assert!(!rendered.contains(":'"));
        assert!(!rendered.lines().any(|line| line.starts_with('\\')));
        assert_eq!(authority.len(), 64);
        assert_eq!(request.len(), 64);
    }

    #[test]
    fn d2_hub_binding_matches_the_resource_context_v2_golden_vector() {
        let fingerprint = d2_resource_binding_fingerprint_v2("700").unwrap();
        assert_eq!(
            d2_resource_bindings_json("700").unwrap(),
            "{\"role_bindings\":{},\"channel_bindings\":{\"community_hub\":\"700\"}}"
        );
        assert_eq!(
            fingerprint,
            "27c51a7b90c32b1fd4095deefe2f48cfdda9f41416f5208cc683e8adf42418d4"
        );
        let authority_payload_digest = d2_authority_payload_digest_v1(
            "tenant:starring-d2-test",
            "installation:starring-d2-test",
            &fingerprint,
        )
        .unwrap();
        assert_eq!(
            authority_payload_digest,
            "c16a709b84262f3854b18ceba1a34b94f927de7693dfe411d3a4154c7a9a2dad"
        );
        let changed_fingerprint = d2_resource_binding_fingerprint_v2("701").unwrap();
        let changed_authority_payload_digest = d2_authority_payload_digest_v1(
            "tenant:starring-d2-test",
            "installation:starring-d2-test",
            &changed_fingerprint,
        )
        .unwrap();
        assert_ne!(
            d2_onboarding_request_digest(
                "d2-20260801t120000z-0123456789ab",
                "tenant:starring-d2-test",
                "installation:starring-d2-test",
                "discord:323456789012345678",
                "Operator",
                &authority_payload_digest,
            ),
            d2_onboarding_request_digest(
                "d2-20260801t120000z-0123456789ab",
                "tenant:starring-d2-test",
                "installation:starring-d2-test",
                "discord:323456789012345678",
                "Operator",
                &changed_authority_payload_digest,
            )
        );
        assert_ne!(authority_payload_digest, changed_authority_payload_digest);
        assert!(d2_resource_bindings_json("0").is_err());
        assert!(d2_resource_binding_fingerprint_v2("18446744073709551616").is_err());
        assert!(INSTALLATION_ONBOARDING.contains(EMPTY_RESOURCE_BINDINGS_JSON));
        assert!(INSTALLATION_ONBOARDING.contains(EMPTY_BINDINGS_FINGERPRINT));
    }

    #[test]
    fn d2_hub_channel_identity_is_distinct_from_guild_and_application() {
        let mut discord = DiscordManifestV1 {
            guild_id: "123456789012345678".to_owned(),
            application_id: "223456789012345678".to_owned(),
            bot_user_id: "223456789012345678".to_owned(),
            actor_id: "423456789012345678".to_owned(),
            hub_channel_id: "323456789012345678".to_owned(),
            resource_prefix: "starring-d2-20260801-0123456789ab".to_owned(),
        };
        assert!(valid_discord_manifest(&discord));
        discord.hub_channel_id.clone_from(&discord.guild_id);
        assert!(!valid_discord_manifest(&discord));
        discord.hub_channel_id.clone_from(&discord.application_id);
        assert!(!valid_discord_manifest(&discord));
        discord.hub_channel_id.clone_from(&discord.bot_user_id);
        assert!(!valid_discord_manifest(&discord));
        discord.hub_channel_id.clone_from(&discord.actor_id);
        assert!(!valid_discord_manifest(&discord));
    }
}
