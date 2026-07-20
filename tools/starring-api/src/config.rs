use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use authoring_application_discord::{DiscordApplicationIdV1, DiscordBotUserIdV1};
use product_control_http::HttpBoundaryConfig;
use url::{Host, Url};

use crate::secret::SecretReferenceV1;

const MIN_BIND_PORT: u16 = 1024;
const MAX_CONFIGURATION_VALUE_BYTES: usize = 8 * 1024;
const MAX_RETURN_PATHS: usize = 64;
const MIN_POOL_CONNECTIONS: u32 = 1;
const MAX_POOL_CONNECTIONS: u32 = 4;
const MIN_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MIN_MAX_LIFETIME: Duration = Duration::from_secs(60);
const MAX_MAX_LIFETIME: Duration = Duration::from_secs(60 * 60);
const MAX_DISCORD_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_WRITE_AUTHORITY_LIFETIME: Duration = Duration::from_secs(5);
const MAX_READ_AUTHORITY_LIFETIME: Duration = Duration::from_secs(30);
const FORBIDDEN_POSTGRES_ENVIRONMENT: [&str; 13] = [
    "PGAPPNAME",
    "PGDATABASE",
    "PGHOST",
    "PGHOSTADDR",
    "PGOPTIONS",
    "PGPASSFILE",
    "PGPASSWORD",
    "PGPORT",
    "PGSSLCERT",
    "PGSSLKEY",
    "PGSSLMODE",
    "PGSSLROOTCERT",
    "PGUSER",
];
#[cfg(test)]
const DISCORD_API_ORIGIN: &str = concat!("https:", "/", "/discord.com");

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DatabaseRoleV1 {
    OAuthFlowWriter,
    SessionIssuer,
    SessionApi,
    SecurityRevoker,
    InstallationAuthorityReader,
    AuthorizedSnapshotReader,
    PromotionExecutor,
    DecisionReader,
    ApprovalExecutor,
    RejectionExecutor,
    ApplyExecutor,
    DeploymentStatusReader,
    OperationalDeploymentStatusReader,
}

impl DatabaseRoleV1 {
    pub const ALL: [Self; 13] = [
        Self::OAuthFlowWriter,
        Self::SessionIssuer,
        Self::SessionApi,
        Self::SecurityRevoker,
        Self::InstallationAuthorityReader,
        Self::AuthorizedSnapshotReader,
        Self::PromotionExecutor,
        Self::DecisionReader,
        Self::ApprovalExecutor,
        Self::RejectionExecutor,
        Self::ApplyExecutor,
        Self::DeploymentStatusReader,
        Self::OperationalDeploymentStatusReader,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    const fn reference_environment_name(self) -> &'static str {
        match self {
            Self::OAuthFlowWriter => "STARRING_API_OAUTH_FLOW_WRITER_DATABASE_SECRET_REFERENCE",
            Self::SessionIssuer => "STARRING_API_SESSION_ISSUER_DATABASE_SECRET_REFERENCE",
            Self::SessionApi => "STARRING_API_SESSION_API_DATABASE_SECRET_REFERENCE",
            Self::SecurityRevoker => "STARRING_API_SECURITY_REVOKER_DATABASE_SECRET_REFERENCE",
            Self::InstallationAuthorityReader => {
                "STARRING_API_INSTALLATION_AUTHORITY_DATABASE_SECRET_REFERENCE"
            }
            Self::AuthorizedSnapshotReader => {
                "STARRING_API_AUTHORIZED_SNAPSHOT_DATABASE_SECRET_REFERENCE"
            }
            Self::PromotionExecutor => "STARRING_API_PROMOTION_EXECUTOR_DATABASE_SECRET_REFERENCE",
            Self::DecisionReader => "STARRING_API_DECISION_READER_DATABASE_SECRET_REFERENCE",
            Self::ApprovalExecutor => "STARRING_API_APPROVAL_EXECUTOR_DATABASE_SECRET_REFERENCE",
            Self::RejectionExecutor => "STARRING_API_REJECTION_EXECUTOR_DATABASE_SECRET_REFERENCE",
            Self::ApplyExecutor => "STARRING_API_APPLY_EXECUTOR_DATABASE_SECRET_REFERENCE",
            Self::DeploymentStatusReader => {
                "STARRING_API_DEPLOYMENT_STATUS_DATABASE_SECRET_REFERENCE"
            }
            Self::OperationalDeploymentStatusReader => {
                "STARRING_API_OPERATIONAL_STATUS_DATABASE_SECRET_REFERENCE"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductionConfigurationFieldV1 {
    BindPort,
    PublicOrigin,
    OAuthReturnPaths,
    DefaultReturnPath,
    PoolMaxConnections,
    PoolAcquireTimeout,
    PoolIdleTimeout,
    PoolMaxLifetime,
    DiscordApplicationId,
    DiscordBotUserId,
    DiscordRequestTimeout,
    DiscordWriteAuthorityLifetime,
    DiscordReadAuthorityLifetime,
    DatabaseSecretReference(DatabaseRoleV1),
    DiscordOAuthClientSecretReference,
    DiscordBotTokenReference,
    ProductActionKeyringReference,
    SnapshotEnvelopeKeyringReference,
}

impl ProductionConfigurationFieldV1 {
    const fn environment_name(self) -> &'static str {
        match self {
            Self::BindPort => "STARRING_API_BIND_PORT",
            Self::PublicOrigin => "STARRING_API_PUBLIC_ORIGIN",
            Self::OAuthReturnPaths => "STARRING_API_OAUTH_RETURN_PATHS_JSON",
            Self::DefaultReturnPath => "STARRING_API_OAUTH_DEFAULT_RETURN_PATH",
            Self::PoolMaxConnections => "STARRING_API_DATABASE_MAX_CONNECTIONS",
            Self::PoolAcquireTimeout => "STARRING_API_DATABASE_ACQUIRE_TIMEOUT_MILLISECONDS",
            Self::PoolIdleTimeout => "STARRING_API_DATABASE_IDLE_TIMEOUT_SECONDS",
            Self::PoolMaxLifetime => "STARRING_API_DATABASE_MAX_LIFETIME_SECONDS",
            Self::DiscordApplicationId => "STARRING_API_DISCORD_APPLICATION_ID",
            Self::DiscordBotUserId => "STARRING_API_DISCORD_BOT_USER_ID",
            Self::DiscordRequestTimeout => "STARRING_API_DISCORD_REQUEST_TIMEOUT_MILLISECONDS",
            Self::DiscordWriteAuthorityLifetime => {
                "STARRING_API_DISCORD_WRITE_AUTHORITY_LIFETIME_MILLISECONDS"
            }
            Self::DiscordReadAuthorityLifetime => {
                "STARRING_API_DISCORD_READ_AUTHORITY_LIFETIME_MILLISECONDS"
            }
            Self::DatabaseSecretReference(role) => role.reference_environment_name(),
            Self::DiscordOAuthClientSecretReference => {
                "STARRING_API_DISCORD_OAUTH_CLIENT_SECRET_REFERENCE"
            }
            Self::DiscordBotTokenReference => "STARRING_API_DISCORD_BOT_TOKEN_REFERENCE",
            Self::ProductActionKeyringReference => {
                "STARRING_API_PRODUCT_ACTION_KEYRING_SECRET_REFERENCE"
            }
            Self::SnapshotEnvelopeKeyringReference => {
                "STARRING_API_SNAPSHOT_ENVELOPE_KEYRING_SECRET_REFERENCE"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductionConfigErrorV1 {
    #[error("ambient PostgreSQL configuration is forbidden")]
    AmbientDatabaseConfiguration,
    #[error("required production configuration is missing")]
    Missing(ProductionConfigurationFieldV1),
    #[error("production configuration encoding is invalid")]
    InvalidEncoding(ProductionConfigurationFieldV1),
    #[error("production configuration value is invalid")]
    InvalidValue(ProductionConfigurationFieldV1),
    #[error("production secret references must be unique across capabilities")]
    DuplicateSecretReference,
}

pub(crate) trait NonSecretConfigurationSourceV1 {
    fn read(&self, name: &str) -> Option<std::ffi::OsString>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ProcessConfigurationSourceV1;

impl NonSecretConfigurationSourceV1 for ProcessConfigurationSourceV1 {
    fn read(&self, name: &str) -> Option<std::ffi::OsString> {
        std::env::var_os(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PoolConfigV1 {
    max_connections: NonZeroU32,
    acquire_timeout: Duration,
    idle_timeout: Duration,
    max_lifetime: Duration,
}

impl PoolConfigV1 {
    pub(crate) fn new(
        max_connections: u32,
        acquire_timeout: Duration,
        idle_timeout: Duration,
        max_lifetime: Duration,
    ) -> Result<Self, ProductionConfigErrorV1> {
        let max_connections = NonZeroU32::new(max_connections)
            .filter(|value| (MIN_POOL_CONNECTIONS..=MAX_POOL_CONNECTIONS).contains(&value.get()));
        let Some(max_connections) = max_connections else {
            return Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::PoolMaxConnections,
            ));
        };
        if !(MIN_ACQUIRE_TIMEOUT..=MAX_ACQUIRE_TIMEOUT).contains(&acquire_timeout) {
            return Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::PoolAcquireTimeout,
            ));
        }
        if !(MIN_IDLE_TIMEOUT..=MAX_IDLE_TIMEOUT).contains(&idle_timeout) {
            return Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::PoolIdleTimeout,
            ));
        }
        if !(MIN_MAX_LIFETIME..=MAX_MAX_LIFETIME).contains(&max_lifetime)
            || max_lifetime <= idle_timeout
        {
            return Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::PoolMaxLifetime,
            ));
        }
        Ok(Self {
            max_connections,
            acquire_timeout,
            idle_timeout,
            max_lifetime,
        })
    }

    pub(crate) fn max_connections(self) -> u32 {
        self.max_connections.get()
    }

    pub(crate) fn acquire_timeout(self) -> Duration {
        self.acquire_timeout
    }

    pub(crate) fn idle_timeout(self) -> Duration {
        self.idle_timeout
    }

    pub(crate) fn max_lifetime(self) -> Duration {
        self.max_lifetime
    }

    #[cfg(test)]
    pub(crate) fn total_connection_ceiling(self) -> u32 {
        self.max_connections.get() * DatabaseRoleV1::ALL.len() as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiscordTimingConfigV1 {
    request_timeout: Duration,
    write_authority_lifetime: Duration,
    read_authority_lifetime: Duration,
}

impl DiscordTimingConfigV1 {
    pub(crate) fn request_timeout(self) -> Duration {
        self.request_timeout
    }

    pub(crate) fn write_authority_lifetime(self) -> Duration {
        self.write_authority_lifetime
    }

    pub(crate) fn read_authority_lifetime(self) -> Duration {
        self.read_authority_lifetime
    }
}

#[derive(Clone)]
pub(crate) struct DiscordPublicConfigV1 {
    application_id: DiscordApplicationIdV1,
    bot_user_id: DiscordBotUserIdV1,
    oauth_redirect_uri: String,
    timing: DiscordTimingConfigV1,
}

impl DiscordPublicConfigV1 {
    pub(crate) fn application_id(&self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    pub(crate) fn bot_user_id(&self) -> DiscordBotUserIdV1 {
        self.bot_user_id
    }

    pub(crate) fn oauth_redirect_uri(&self) -> &str {
        &self.oauth_redirect_uri
    }

    #[cfg(test)]
    pub(crate) fn api_origin(&self) -> &'static str {
        DISCORD_API_ORIGIN
    }

    pub(crate) fn timing(&self) -> DiscordTimingConfigV1 {
        self.timing
    }
}

impl Debug for DiscordPublicConfigV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiscordPublicConfigV1")
            .field("application_id", &self.application_id)
            .field("oauth_redirect_uri", &self.oauth_redirect_uri)
            .field("timing", &self.timing)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) struct ProductionSecretReferencesV1 {
    database: [SecretReferenceV1; 13],
    discord_oauth_client_secret: SecretReferenceV1,
    discord_bot_token: SecretReferenceV1,
    product_action_keyring: SecretReferenceV1,
    snapshot_envelope_keyring: SecretReferenceV1,
}

impl ProductionSecretReferencesV1 {
    pub(crate) fn database(&self, role: DatabaseRoleV1) -> &SecretReferenceV1 {
        &self.database[role.index()]
    }

    pub(crate) fn discord_oauth_client_secret(&self) -> &SecretReferenceV1 {
        &self.discord_oauth_client_secret
    }

    pub(crate) fn discord_bot_token(&self) -> &SecretReferenceV1 {
        &self.discord_bot_token
    }

    pub(crate) fn product_action_keyring(&self) -> &SecretReferenceV1 {
        &self.product_action_keyring
    }

    pub(crate) fn snapshot_envelope_keyring(&self) -> &SecretReferenceV1 {
        &self.snapshot_envelope_keyring
    }
}

impl Debug for ProductionSecretReferencesV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductionSecretReferencesV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct ProductionConfigV1 {
    bind_addr: SocketAddr,
    public_origin: String,
    public_host: String,
    oauth_redirect_uri: String,
    return_paths: Arc<[String]>,
    default_return_path: String,
    http_boundary: HttpBoundaryConfig,
    pool: PoolConfigV1,
    discord: DiscordPublicConfigV1,
    secret_references: ProductionSecretReferencesV1,
}

impl ProductionConfigV1 {
    pub fn from_process_environment() -> Result<Self, ProductionConfigErrorV1> {
        Self::from_source(&ProcessConfigurationSourceV1)
    }

    pub(crate) fn from_source(
        source: &impl NonSecretConfigurationSourceV1,
    ) -> Result<Self, ProductionConfigErrorV1> {
        if FORBIDDEN_POSTGRES_ENVIRONMENT
            .iter()
            .any(|name| source.read(name).is_some())
        {
            return Err(ProductionConfigErrorV1::AmbientDatabaseConfiguration);
        }
        let bind_port = parse_number::<u16>(source, ProductionConfigurationFieldV1::BindPort)?;
        if bind_port < MIN_BIND_PORT {
            return Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::BindPort,
            ));
        }
        let public_origin = read_required(source, ProductionConfigurationFieldV1::PublicOrigin)?;
        let public_host = validate_public_origin(&public_origin)?;
        let return_paths = parse_return_paths(source)?;
        let default_return_path =
            read_required(source, ProductionConfigurationFieldV1::DefaultReturnPath)?;
        if !return_paths.iter().any(|path| path == &default_return_path) {
            return Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::DefaultReturnPath,
            ));
        }
        let http_boundary = HttpBoundaryConfig::production(
            &public_origin,
            return_paths.iter().cloned(),
        )
        .map_err(|_| {
            ProductionConfigErrorV1::InvalidValue(ProductionConfigurationFieldV1::OAuthReturnPaths)
        })?;
        let pool = parse_pool(source)?;
        let application_id =
            parse_number::<u64>(source, ProductionConfigurationFieldV1::DiscordApplicationId)?;
        let application_id = DiscordApplicationIdV1::new(application_id).map_err(|_| {
            ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::DiscordApplicationId,
            )
        })?;
        let bot_user_id =
            parse_number::<u64>(source, ProductionConfigurationFieldV1::DiscordBotUserId)?;
        let bot_user_id = DiscordBotUserIdV1::new(bot_user_id).map_err(|_| {
            ProductionConfigErrorV1::InvalidValue(ProductionConfigurationFieldV1::DiscordBotUserId)
        })?;
        let timing = parse_discord_timing(source)?;
        let oauth_redirect_uri = format!("{public_origin}/oauth/discord/callback");
        let discord = DiscordPublicConfigV1 {
            application_id,
            bot_user_id,
            oauth_redirect_uri: oauth_redirect_uri.clone(),
            timing,
        };
        let secret_references = parse_secret_references(source)?;
        Ok(Self {
            bind_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, bind_port)),
            public_origin,
            public_host,
            oauth_redirect_uri,
            return_paths: return_paths.into(),
            default_return_path,
            http_boundary,
            pool,
            discord,
            secret_references,
        })
    }

    pub(crate) fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    #[cfg(test)]
    pub(crate) fn public_origin(&self) -> &str {
        &self.public_origin
    }

    #[cfg(test)]
    pub(crate) fn public_host(&self) -> &str {
        &self.public_host
    }

    pub(crate) fn oauth_redirect_uri(&self) -> &str {
        &self.oauth_redirect_uri
    }

    pub(crate) fn return_paths(&self) -> &[String] {
        &self.return_paths
    }

    pub(crate) fn default_return_path(&self) -> &str {
        &self.default_return_path
    }

    pub(crate) fn http_boundary(&self) -> HttpBoundaryConfig {
        self.http_boundary.clone()
    }

    pub(crate) fn pool_config(&self) -> PoolConfigV1 {
        self.pool
    }

    pub(crate) fn discord(&self) -> &DiscordPublicConfigV1 {
        &self.discord
    }

    pub(crate) fn secret_references(&self) -> &ProductionSecretReferencesV1 {
        &self.secret_references
    }
}

impl Debug for ProductionConfigV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductionConfigV1")
            .field("bind_addr", &self.bind_addr)
            .field("public_origin", &self.public_origin)
            .field("public_host", &self.public_host)
            .field("return_paths", &self.return_paths)
            .field("default_return_path", &self.default_return_path)
            .field("pool", &self.pool)
            .field("discord", &self.discord)
            .field("secret_references", &"<redacted>")
            .finish()
    }
}

fn parse_pool(
    source: &impl NonSecretConfigurationSourceV1,
) -> Result<PoolConfigV1, ProductionConfigErrorV1> {
    let max_connections =
        parse_number::<u32>(source, ProductionConfigurationFieldV1::PoolMaxConnections)?;
    let acquire_timeout = Duration::from_millis(parse_number::<u64>(
        source,
        ProductionConfigurationFieldV1::PoolAcquireTimeout,
    )?);
    let idle_timeout = Duration::from_secs(parse_number::<u64>(
        source,
        ProductionConfigurationFieldV1::PoolIdleTimeout,
    )?);
    let max_lifetime = Duration::from_secs(parse_number::<u64>(
        source,
        ProductionConfigurationFieldV1::PoolMaxLifetime,
    )?);
    PoolConfigV1::new(max_connections, acquire_timeout, idle_timeout, max_lifetime)
}

fn parse_discord_timing(
    source: &impl NonSecretConfigurationSourceV1,
) -> Result<DiscordTimingConfigV1, ProductionConfigErrorV1> {
    let request_timeout = Duration::from_millis(parse_number::<u64>(
        source,
        ProductionConfigurationFieldV1::DiscordRequestTimeout,
    )?);
    let write_authority_lifetime = Duration::from_millis(parse_number::<u64>(
        source,
        ProductionConfigurationFieldV1::DiscordWriteAuthorityLifetime,
    )?);
    let read_authority_lifetime = Duration::from_millis(parse_number::<u64>(
        source,
        ProductionConfigurationFieldV1::DiscordReadAuthorityLifetime,
    )?);
    if request_timeout.is_zero() || request_timeout > MAX_DISCORD_REQUEST_TIMEOUT {
        return Err(ProductionConfigErrorV1::InvalidValue(
            ProductionConfigurationFieldV1::DiscordRequestTimeout,
        ));
    }
    if write_authority_lifetime.is_zero() || write_authority_lifetime > MAX_WRITE_AUTHORITY_LIFETIME
    {
        return Err(ProductionConfigErrorV1::InvalidValue(
            ProductionConfigurationFieldV1::DiscordWriteAuthorityLifetime,
        ));
    }
    if read_authority_lifetime.is_zero() || read_authority_lifetime > MAX_READ_AUTHORITY_LIFETIME {
        return Err(ProductionConfigErrorV1::InvalidValue(
            ProductionConfigurationFieldV1::DiscordReadAuthorityLifetime,
        ));
    }
    Ok(DiscordTimingConfigV1 {
        request_timeout,
        write_authority_lifetime,
        read_authority_lifetime,
    })
}

fn parse_secret_references(
    source: &impl NonSecretConfigurationSourceV1,
) -> Result<ProductionSecretReferencesV1, ProductionConfigErrorV1> {
    let mut database = Vec::with_capacity(DatabaseRoleV1::ALL.len());
    for role in DatabaseRoleV1::ALL {
        let field = ProductionConfigurationFieldV1::DatabaseSecretReference(role);
        database.push(parse_secret_reference(source, field)?);
    }
    let duplicate = database.iter().enumerate().any(|(index, candidate)| {
        database
            .iter()
            .skip(index + 1)
            .any(|other| candidate == other)
    });
    if duplicate {
        return Err(ProductionConfigErrorV1::DuplicateSecretReference);
    }
    let database: [SecretReferenceV1; 13] = database
        .try_into()
        .map_err(|_| ProductionConfigErrorV1::DuplicateSecretReference)?;
    let discord_oauth_client_secret = parse_secret_reference(
        source,
        ProductionConfigurationFieldV1::DiscordOAuthClientSecretReference,
    )?;
    let discord_bot_token = parse_secret_reference(
        source,
        ProductionConfigurationFieldV1::DiscordBotTokenReference,
    )?;
    let product_action_keyring = parse_secret_reference(
        source,
        ProductionConfigurationFieldV1::ProductActionKeyringReference,
    )?;
    let snapshot_envelope_keyring = parse_secret_reference(
        source,
        ProductionConfigurationFieldV1::SnapshotEnvelopeKeyringReference,
    )?;
    let purpose_references = [
        &discord_oauth_client_secret,
        &discord_bot_token,
        &product_action_keyring,
        &snapshot_envelope_keyring,
    ];
    let duplicate_purpose = purpose_references
        .iter()
        .enumerate()
        .any(|(index, candidate)| {
            purpose_references
                .iter()
                .skip(index + 1)
                .any(|other| candidate == other)
        });
    let aliases_database = purpose_references
        .iter()
        .any(|candidate| database.iter().any(|database| candidate == &database));
    if duplicate_purpose || aliases_database {
        return Err(ProductionConfigErrorV1::DuplicateSecretReference);
    }
    Ok(ProductionSecretReferencesV1 {
        database,
        discord_oauth_client_secret,
        discord_bot_token,
        product_action_keyring,
        snapshot_envelope_keyring,
    })
}

fn parse_secret_reference(
    source: &impl NonSecretConfigurationSourceV1,
    field: ProductionConfigurationFieldV1,
) -> Result<SecretReferenceV1, ProductionConfigErrorV1> {
    let value = read_required(source, field)?;
    SecretReferenceV1::parse(&value).map_err(|_| ProductionConfigErrorV1::InvalidValue(field))
}

fn parse_return_paths(
    source: &impl NonSecretConfigurationSourceV1,
) -> Result<Vec<String>, ProductionConfigErrorV1> {
    let field = ProductionConfigurationFieldV1::OAuthReturnPaths;
    let value = read_required(source, field)?;
    let paths = serde_json::from_str::<Vec<String>>(&value)
        .map_err(|_| ProductionConfigErrorV1::InvalidValue(field))?;
    let unique = paths.iter().collect::<BTreeSet<_>>();
    if paths.is_empty()
        || paths.len() > MAX_RETURN_PATHS
        || paths.len() != unique.len()
        || paths.iter().any(|path| !valid_return_path(path))
    {
        return Err(ProductionConfigErrorV1::InvalidValue(field));
    }
    Ok(paths)
}

fn validate_public_origin(value: &str) -> Result<String, ProductionConfigErrorV1> {
    let field = ProductionConfigurationFieldV1::PublicOrigin;
    let url = Url::parse(value).map_err(|_| ProductionConfigErrorV1::InvalidValue(field))?;
    let Host::Domain(domain) = url
        .host()
        .ok_or(ProductionConfigErrorV1::InvalidValue(field))?
    else {
        return Err(ProductionConfigErrorV1::InvalidValue(field));
    };
    let origin = url.origin().ascii_serialization();
    if url.scheme() != "https"
        || value != origin
        || url.username() != ""
        || url.password().is_some()
        || url.path() != "/"
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !valid_public_domain(domain)
    {
        return Err(ProductionConfigErrorV1::InvalidValue(field));
    }
    Ok(domain.to_string())
}

fn valid_public_domain(value: &str) -> bool {
    value.len() <= 253
        && value.contains('.')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
        })
}

fn valid_return_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (value == "/" || !value.ends_with('/'))
        && !value.is_empty()
        && value.len() <= 256
        && value.is_ascii()
        && value.starts_with('/')
        && !bytes
            .windows(2)
            .any(|window| window[0] == b'/' && window[1] == b'/')
        && value
            .bytes()
            .all(|byte| byte == b'/' || byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && (value == "/"
            || value
                .split('/')
                .skip(1)
                .all(|segment| !matches!(segment, "." | "..") && !segment.is_empty()))
}

fn parse_number<T: std::str::FromStr>(
    source: &impl NonSecretConfigurationSourceV1,
    field: ProductionConfigurationFieldV1,
) -> Result<T, ProductionConfigErrorV1> {
    read_required(source, field)?
        .parse::<T>()
        .map_err(|_| ProductionConfigErrorV1::InvalidValue(field))
}

fn read_required(
    source: &impl NonSecretConfigurationSourceV1,
    field: ProductionConfigurationFieldV1,
) -> Result<String, ProductionConfigErrorV1> {
    let value = source
        .read(field.environment_name())
        .ok_or(ProductionConfigErrorV1::Missing(field))?
        .into_string()
        .map_err(|_| ProductionConfigErrorV1::InvalidEncoding(field))?;
    if value.is_empty()
        || value.len() > MAX_CONFIGURATION_VALUE_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(ProductionConfigErrorV1::InvalidValue(field));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use super::*;

    #[derive(Default)]
    struct FakeConfigSourceV1 {
        values: BTreeMap<String, OsString>,
    }

    impl NonSecretConfigurationSourceV1 for FakeConfigSourceV1 {
        fn read(&self, name: &str) -> Option<OsString> {
            self.values.get(name).cloned()
        }
    }

    impl FakeConfigSourceV1 {
        fn insert(&mut self, field: ProductionConfigurationFieldV1, value: impl Into<OsString>) {
            self.values
                .insert(field.environment_name().to_string(), value.into());
        }
    }

    fn valid_source() -> FakeConfigSourceV1 {
        let mut source = FakeConfigSourceV1::default();
        source.insert(ProductionConfigurationFieldV1::BindPort, "8080");
        source.insert(
            ProductionConfigurationFieldV1::PublicOrigin,
            format!("https:{}{}starring.example", "/", "/"),
        );
        source.insert(
            ProductionConfigurationFieldV1::OAuthReturnPaths,
            r#"["/","/app","/settings"]"#,
        );
        source.insert(ProductionConfigurationFieldV1::DefaultReturnPath, "/app");
        source.insert(ProductionConfigurationFieldV1::PoolMaxConnections, "2");
        source.insert(ProductionConfigurationFieldV1::PoolAcquireTimeout, "2000");
        source.insert(ProductionConfigurationFieldV1::PoolIdleTimeout, "60");
        source.insert(ProductionConfigurationFieldV1::PoolMaxLifetime, "600");
        source.insert(ProductionConfigurationFieldV1::DiscordApplicationId, "1234");
        source.insert(ProductionConfigurationFieldV1::DiscordBotUserId, "5678");
        source.insert(
            ProductionConfigurationFieldV1::DiscordRequestTimeout,
            "5000",
        );
        source.insert(
            ProductionConfigurationFieldV1::DiscordWriteAuthorityLifetime,
            "5000",
        );
        source.insert(
            ProductionConfigurationFieldV1::DiscordReadAuthorityLifetime,
            "30000",
        );
        for role in DatabaseRoleV1::ALL {
            source.insert(
                ProductionConfigurationFieldV1::DatabaseSecretReference(role),
                format!("env:STARRING_DATABASE_SECRET_{}", role.index()),
            );
        }
        source.insert(
            ProductionConfigurationFieldV1::DiscordOAuthClientSecretReference,
            "keychain:starring.production:discord.oauth",
        );
        source.insert(
            ProductionConfigurationFieldV1::DiscordBotTokenReference,
            "keychain:starring.production:discord.bot",
        );
        source.insert(
            ProductionConfigurationFieldV1::ProductActionKeyringReference,
            "env:STARRING_PRODUCT_ACTION_KEYRING",
        );
        source.insert(
            ProductionConfigurationFieldV1::SnapshotEnvelopeKeyringReference,
            "env:STARRING_SNAPSHOT_ENVELOPE_KEYRING",
        );
        source
    }

    #[test]
    fn production_config_is_loopback_only_and_canonical() {
        let config = ProductionConfigV1::from_source(&valid_source()).unwrap();
        assert_eq!(config.bind_addr(), "127.0.0.1:8080".parse().unwrap());
        assert_eq!(
            config.public_origin(),
            format!("https:{}{}starring.example", "/", "/")
        );
        assert_eq!(config.public_host(), "starring.example");
        assert_eq!(
            config.oauth_redirect_uri(),
            format!(
                "https:{}{}starring.example/oauth/discord/callback",
                "/", "/"
            )
        );
        assert_eq!(config.default_return_path(), "/app");
        assert_eq!(config.return_paths(), ["/", "/app", "/settings"]);
        assert_eq!(config.pool_config().total_connection_ceiling(), 26);
        assert_eq!(config.discord().application_id().get(), 1234);
        assert_eq!(config.discord().bot_user_id().get(), 5678);
        assert!(config.discord().api_origin().ends_with("discord.com"));
        assert_eq!(DatabaseRoleV1::ALL.len(), 13);
    }

    #[test]
    fn ambient_postgres_configuration_is_rejected_before_secret_resolution() {
        for name in FORBIDDEN_POSTGRES_ENVIRONMENT {
            let mut source = valid_source();
            source.values.insert(name.to_string(), "ambient".into());
            assert_eq!(
                ProductionConfigV1::from_source(&source).unwrap_err(),
                ProductionConfigErrorV1::AmbientDatabaseConfiguration
            );
        }
    }

    #[test]
    fn public_origin_rejects_noncanonical_and_non_domain_values() {
        for origin in [
            format!("http:{}{}starring.example", "/", "/"),
            format!("https:{}{}STARRING.example", "/", "/"),
            format!("https:{}{}127.0.0.1", "/", "/"),
            format!("https:{}{}starring.example/", "/", "/"),
            format!("https:{}{}starring.example:8443", "/", "/"),
            format!("https:{}{}localhost", "/", "/"),
        ] {
            let mut source = valid_source();
            source.insert(ProductionConfigurationFieldV1::PublicOrigin, origin);
            assert_eq!(
                ProductionConfigV1::from_source(&source).unwrap_err(),
                ProductionConfigErrorV1::InvalidValue(ProductionConfigurationFieldV1::PublicOrigin)
            );
        }
    }

    #[test]
    fn return_paths_are_strict_unique_and_contain_default() {
        let mut duplicate = valid_source();
        duplicate.insert(
            ProductionConfigurationFieldV1::OAuthReturnPaths,
            r#"["/app","/app"]"#,
        );
        assert_eq!(
            ProductionConfigV1::from_source(&duplicate).unwrap_err(),
            ProductionConfigErrorV1::InvalidValue(ProductionConfigurationFieldV1::OAuthReturnPaths)
        );
        let mut missing_default = valid_source();
        missing_default.insert(
            ProductionConfigurationFieldV1::DefaultReturnPath,
            "/missing",
        );
        assert_eq!(
            ProductionConfigV1::from_source(&missing_default).unwrap_err(),
            ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::DefaultReturnPath
            )
        );
    }

    #[test]
    fn pool_settings_are_small_and_bounded() {
        let pool = PoolConfigV1::new(
            4,
            Duration::from_secs(2),
            Duration::from_secs(60),
            Duration::from_secs(600),
        )
        .unwrap();
        assert_eq!(pool.max_connections(), 4);
        assert_eq!(pool.total_connection_ceiling(), 52);
        assert_eq!(
            PoolConfigV1::new(
                5,
                Duration::from_secs(2),
                Duration::from_secs(60),
                Duration::from_secs(600)
            ),
            Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::PoolMaxConnections
            ))
        );
        assert_eq!(
            PoolConfigV1::new(
                2,
                Duration::from_secs(2),
                Duration::from_secs(60),
                Duration::from_secs(60)
            ),
            Err(ProductionConfigErrorV1::InvalidValue(
                ProductionConfigurationFieldV1::PoolMaxLifetime
            ))
        );
    }

    #[test]
    fn all_thirteen_database_references_are_required_and_unique() {
        let mut missing = valid_source();
        let missing_role = DatabaseRoleV1::ApprovalExecutor;
        missing
            .values
            .remove(missing_role.reference_environment_name());
        assert_eq!(
            ProductionConfigV1::from_source(&missing).unwrap_err(),
            ProductionConfigErrorV1::Missing(
                ProductionConfigurationFieldV1::DatabaseSecretReference(missing_role)
            )
        );
        let mut duplicate = valid_source();
        duplicate.insert(
            ProductionConfigurationFieldV1::DatabaseSecretReference(
                DatabaseRoleV1::ApprovalExecutor,
            ),
            "env:STARRING_DATABASE_SECRET_0",
        );
        assert_eq!(
            ProductionConfigV1::from_source(&duplicate).unwrap_err(),
            ProductionConfigErrorV1::DuplicateSecretReference
        );
    }

    #[test]
    fn cross_purpose_secret_references_are_distinct_from_every_database_role() {
        let mut same_keyrings = valid_source();
        same_keyrings.insert(
            ProductionConfigurationFieldV1::SnapshotEnvelopeKeyringReference,
            "env:STARRING_PRODUCT_ACTION_KEYRING",
        );
        assert_eq!(
            ProductionConfigV1::from_source(&same_keyrings).unwrap_err(),
            ProductionConfigErrorV1::DuplicateSecretReference
        );
        let mut database_alias = valid_source();
        database_alias.insert(
            ProductionConfigurationFieldV1::DiscordBotTokenReference,
            "env:STARRING_DATABASE_SECRET_0",
        );
        assert_eq!(
            ProductionConfigV1::from_source(&database_alias).unwrap_err(),
            ProductionConfigErrorV1::DuplicateSecretReference
        );
    }

    #[test]
    fn configuration_debug_redacts_all_secret_reference_identifiers() {
        let config = ProductionConfigV1::from_source(&valid_source()).unwrap();
        let debug = format!("{config:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("STARRING_DATABASE_SECRET"));
        assert!(!debug.contains("starring.production"));
    }
}
