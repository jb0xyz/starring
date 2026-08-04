use std::ffi::OsString;
use std::fmt::{Debug, Display, Formatter};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::{NonZeroU32, NonZeroUsize};
use std::time::Duration;

use crate::runtime_controller::RuntimeServingControllerConfigV2;

const MAX_CONFIGURATION_VALUE_BYTES: usize = 8 * 1024;
const MAX_ENVIRONMENT_NAME_BYTES: usize = 128;
const MAX_KEYCHAIN_COMPONENT_BYTES: usize = 128;
const DEFAULT_HEALTH_PORT: u16 = 9091;
const MIN_BIND_PORT: u16 = 1024;
const MIN_POOL_CONNECTIONS: u32 = 1;
const MAX_POOL_CONNECTIONS: u32 = 4;
const MIN_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MIN_MAX_LIFETIME: Duration = Duration::from_secs(60);
const MAX_MAX_LIFETIME: Duration = Duration::from_secs(60 * 60);
const MIN_STATEMENT_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const MIN_LOCK_TIMEOUT: Duration = Duration::from_millis(50);
const MAX_GLOBAL_ADMISSION_CAPACITY: usize = 65_536;
const MAX_COMMAND_CAPACITY: usize = 64;
const MAX_LIFECYCLE_CAPACITY: usize = 1_024;
const MAX_REJECTION_ACKNOWLEDGEMENT_CAPACITY: usize = 1_024;
const MAX_GATEWAY_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_INSTANCE_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);
const MIN_GATEWAY_OWNER_LEASE: Duration = Duration::from_secs(1);
const MAX_GATEWAY_OWNER_LEASE: Duration = Duration::from_secs(300);
const MAX_STARTUP_OWNER_STATEMENT_TIMEOUT: Duration = Duration::from_secs(4);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DatabaseCapabilityV1 {
    Convergence,
    ExactTarget,
    Panel,
    Serving,
    Interaction,
}

impl DatabaseCapabilityV1 {
    pub const ALL: [Self; 5] = [
        Self::Convergence,
        Self::ExactTarget,
        Self::Panel,
        Self::Serving,
        Self::Interaction,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Convergence => "convergence",
            Self::ExactTarget => "exact_target",
            Self::Panel => "panel",
            Self::Serving => "serving",
            Self::Interaction => "interaction",
        }
    }

    const fn reference_environment_name(self) -> &'static str {
        match self {
            Self::Convergence => "STARRING_RUNTIME_CONVERGENCE_DATABASE_URL_SECRET_REFERENCE",
            Self::ExactTarget => "STARRING_RUNTIME_EXACT_TARGET_DATABASE_URL_SECRET_REFERENCE",
            Self::Panel => "STARRING_RUNTIME_PANEL_DATABASE_URL_SECRET_REFERENCE",
            Self::Serving => "STARRING_RUNTIME_SERVING_DATABASE_URL_SECRET_REFERENCE",
            Self::Interaction => "STARRING_RUNTIME_INTERACTION_DATABASE_URL_SECRET_REFERENCE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConfigurationFieldV1 {
    HealthBindAddress,
    DatabaseMaxConnections,
    DatabaseAcquireTimeout,
    DatabaseIdleTimeout,
    DatabaseMaxLifetime,
    DatabaseStatementTimeout,
    DatabaseLockTimeout,
    GatewayGlobalAdmissionCapacity,
    GatewayCommandCapacity,
    GatewayLifecycleCapacity,
    GatewayRejectionAcknowledgementCapacity,
    GatewayDrainTimeout,
    InstanceLookupTimeout,
    GatewayOwnerLease,
    GatewayOwnerRenewBefore,
    GatewayOwnerSafetyMargin,
    DiscordTransportMode,
    DiscordGatewayProxyUrl,
    DiscordEffectHttpProxyAuthority,
    DatabaseUrlSecretReference(DatabaseCapabilityV1),
    DiscordBotTokenSecretReference,
    InteractionTokenEnvelopeKeyringSecretReference,
}

impl RuntimeConfigurationFieldV1 {
    pub const fn environment_name(self) -> &'static str {
        match self {
            Self::HealthBindAddress => "STARRING_RUNTIME_HEALTH_BIND_ADDRESS",
            Self::DatabaseMaxConnections => "STARRING_RUNTIME_DATABASE_MAX_CONNECTIONS",
            Self::DatabaseAcquireTimeout => {
                "STARRING_RUNTIME_DATABASE_ACQUIRE_TIMEOUT_MILLISECONDS"
            }
            Self::DatabaseIdleTimeout => "STARRING_RUNTIME_DATABASE_IDLE_TIMEOUT_SECONDS",
            Self::DatabaseMaxLifetime => "STARRING_RUNTIME_DATABASE_MAX_LIFETIME_SECONDS",
            Self::DatabaseStatementTimeout => {
                "STARRING_RUNTIME_DATABASE_STATEMENT_TIMEOUT_MILLISECONDS"
            }
            Self::DatabaseLockTimeout => "STARRING_RUNTIME_DATABASE_LOCK_TIMEOUT_MILLISECONDS",
            Self::GatewayGlobalAdmissionCapacity => {
                "STARRING_RUNTIME_GATEWAY_GLOBAL_ADMISSION_CAPACITY"
            }
            Self::GatewayCommandCapacity => "STARRING_RUNTIME_GATEWAY_COMMAND_CAPACITY",
            Self::GatewayLifecycleCapacity => "STARRING_RUNTIME_GATEWAY_LIFECYCLE_CAPACITY",
            Self::GatewayRejectionAcknowledgementCapacity => {
                "STARRING_RUNTIME_GATEWAY_REJECTION_ACKNOWLEDGEMENT_CAPACITY"
            }
            Self::GatewayDrainTimeout => "STARRING_RUNTIME_GATEWAY_DRAIN_TIMEOUT_SECONDS",
            Self::InstanceLookupTimeout => "STARRING_RUNTIME_INSTANCE_LOOKUP_TIMEOUT_MILLISECONDS",
            Self::GatewayOwnerLease => "STARRING_RUNTIME_GATEWAY_OWNER_LEASE_MILLISECONDS",
            Self::GatewayOwnerRenewBefore => {
                "STARRING_RUNTIME_GATEWAY_OWNER_RENEW_BEFORE_MILLISECONDS"
            }
            Self::GatewayOwnerSafetyMargin => {
                "STARRING_RUNTIME_GATEWAY_OWNER_SAFETY_MARGIN_MILLISECONDS"
            }
            Self::DiscordTransportMode => "STARRING_RUNTIME_DISCORD_TRANSPORT_MODE",
            Self::DiscordGatewayProxyUrl => "STARRING_RUNTIME_DISCORD_GATEWAY_PROXY_URL",
            Self::DiscordEffectHttpProxyAuthority => {
                "STARRING_RUNTIME_DISCORD_EFFECT_HTTP_PROXY_AUTHORITY"
            }
            Self::DatabaseUrlSecretReference(capability) => capability.reference_environment_name(),
            Self::DiscordBotTokenSecretReference => {
                "STARRING_RUNTIME_DISCORD_BOT_TOKEN_SECRET_REFERENCE"
            }
            Self::InteractionTokenEnvelopeKeyringSecretReference => {
                "STARRING_RUNTIME_INTERACTION_TOKEN_ENVELOPE_KEYRING_SECRET_REFERENCE"
            }
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::HealthBindAddress => "health_bind_address",
            Self::DatabaseMaxConnections => "database_max_connections",
            Self::DatabaseAcquireTimeout => "database_acquire_timeout",
            Self::DatabaseIdleTimeout => "database_idle_timeout",
            Self::DatabaseMaxLifetime => "database_max_lifetime",
            Self::DatabaseStatementTimeout => "database_statement_timeout",
            Self::DatabaseLockTimeout => "database_lock_timeout",
            Self::GatewayGlobalAdmissionCapacity => "gateway_global_admission_capacity",
            Self::GatewayCommandCapacity => "gateway_command_capacity",
            Self::GatewayLifecycleCapacity => "gateway_lifecycle_capacity",
            Self::GatewayRejectionAcknowledgementCapacity => {
                "gateway_rejection_acknowledgement_capacity"
            }
            Self::GatewayDrainTimeout => "gateway_drain_timeout",
            Self::InstanceLookupTimeout => "instance_lookup_timeout",
            Self::GatewayOwnerLease => "gateway_owner_lease",
            Self::GatewayOwnerRenewBefore => "gateway_owner_renew_before",
            Self::GatewayOwnerSafetyMargin => "gateway_owner_safety_margin",
            Self::DiscordTransportMode => "discord_transport_mode",
            Self::DiscordGatewayProxyUrl => "discord_gateway_proxy_url",
            Self::DiscordEffectHttpProxyAuthority => "discord_effect_http_proxy_authority",
            Self::DatabaseUrlSecretReference(capability) => match capability {
                DatabaseCapabilityV1::Convergence => "convergence_database_url_secret_reference",
                DatabaseCapabilityV1::ExactTarget => "exact_target_database_url_secret_reference",
                DatabaseCapabilityV1::Panel => "panel_database_url_secret_reference",
                DatabaseCapabilityV1::Serving => "serving_database_url_secret_reference",
                DatabaseCapabilityV1::Interaction => "interaction_database_url_secret_reference",
            },
            Self::DiscordBotTokenSecretReference => "discord_bot_token_secret_reference",
            Self::InteractionTokenEnvelopeKeyringSecretReference => {
                "interaction_token_envelope_keyring_secret_reference"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConfigErrorV1 {
    AmbientDatabaseConfiguration,
    Missing(RuntimeConfigurationFieldV1),
    InvalidEncoding(RuntimeConfigurationFieldV1),
    InvalidValue(RuntimeConfigurationFieldV1),
    DuplicateSecretReference,
}

impl RuntimeConfigErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::AmbientDatabaseConfiguration => "runtime_config_ambient_postgres",
            Self::Missing(_) => "runtime_config_missing",
            Self::InvalidEncoding(_) => "runtime_config_invalid_encoding",
            Self::InvalidValue(_) => "runtime_config_invalid_value",
            Self::DuplicateSecretReference => "runtime_config_duplicate_secret_reference",
        }
    }

    pub const fn field(self) -> Option<RuntimeConfigurationFieldV1> {
        match self {
            Self::Missing(field) | Self::InvalidEncoding(field) | Self::InvalidValue(field) => {
                Some(field)
            }
            Self::AmbientDatabaseConfiguration | Self::DuplicateSecretReference => None,
        }
    }
}

impl Display for RuntimeConfigErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::AmbientDatabaseConfiguration => "ambient PostgreSQL configuration is forbidden",
            Self::Missing(_) => "required runtime configuration is missing",
            Self::InvalidEncoding(_) => "runtime configuration encoding is invalid",
            Self::InvalidValue(_) => "runtime configuration value is invalid",
            Self::DuplicateSecretReference => {
                "runtime secret references must be unique across capabilities"
            }
        })
    }
}

impl std::error::Error for RuntimeConfigErrorV1 {}

trait ConfigurationSourceV1 {
    fn read(&self, name: &str) -> Option<OsString>;
}

#[derive(Clone, Copy, Debug, Default)]
struct ProcessConfigurationSourceV1;

impl ConfigurationSourceV1 for ProcessConfigurationSourceV1 {
    fn read(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeSecretReferenceV1 {
    source: RuntimeSecretReferenceSourceV1,
}

#[derive(Clone, PartialEq, Eq)]
enum RuntimeSecretReferenceSourceV1 {
    Environment(String),
    Keychain { service: String, account: String },
}

impl RuntimeSecretReferenceV1 {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if let Some(environment_name) = value.strip_prefix("env:") {
            if !valid_environment_name(environment_name)
                || FORBIDDEN_POSTGRES_ENVIRONMENT.contains(&environment_name)
            {
                return None;
            }
            return Some(Self {
                source: RuntimeSecretReferenceSourceV1::Environment(environment_name.to_string()),
            });
        }
        let components = value.strip_prefix("keychain:")?;
        let mut components = components.split(':');
        let service = components.next()?;
        let account = components.next()?;
        if components.next().is_some()
            || !valid_keychain_component(service)
            || !valid_keychain_component(account)
        {
            return None;
        }
        Some(Self {
            source: RuntimeSecretReferenceSourceV1::Keychain {
                service: service.to_string(),
                account: account.to_string(),
            },
        })
    }

    pub fn environment_name(&self) -> Option<&str> {
        match &self.source {
            RuntimeSecretReferenceSourceV1::Environment(name) => Some(name),
            RuntimeSecretReferenceSourceV1::Keychain { .. } => None,
        }
    }

    pub fn keychain_identity(&self) -> Option<(&str, &str)> {
        match &self.source {
            RuntimeSecretReferenceSourceV1::Environment(_) => None,
            RuntimeSecretReferenceSourceV1::Keychain { service, account } => {
                Some((service, account))
            }
        }
    }
}

impl Debug for RuntimeSecretReferenceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSecretReferenceV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatabasePoolConfigV1 {
    max_connections_per_capability: NonZeroU32,
    acquire_timeout: Duration,
    idle_timeout: Duration,
    max_lifetime: Duration,
}

impl DatabasePoolConfigV1 {
    pub fn max_connections_per_capability(self) -> NonZeroU32 {
        self.max_connections_per_capability
    }

    pub fn total_connection_ceiling(self) -> u32 {
        self.max_connections_per_capability.get() * DatabaseCapabilityV1::ALL.len() as u32
    }

    pub fn acquire_timeout(self) -> Duration {
        self.acquire_timeout
    }

    pub fn idle_timeout(self) -> Duration {
        self.idle_timeout
    }

    pub fn max_lifetime(self) -> Duration {
        self.max_lifetime
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatabaseOperationConfigV1 {
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl DatabaseOperationConfigV1 {
    pub fn statement_timeout(self) -> Duration {
        self.statement_timeout
    }

    pub fn lock_timeout(self) -> Duration {
        self.lock_timeout
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayResourceConfigV1 {
    global_admission_capacity: NonZeroUsize,
    command_capacity: NonZeroUsize,
    lifecycle_capacity: NonZeroUsize,
    rejection_acknowledgement_capacity: NonZeroUsize,
    drain_timeout: Duration,
    instance_lookup_timeout: Duration,
    discord_transport: RuntimeDiscordTransportConfigV1,
}

impl Default for GatewayResourceConfigV1 {
    fn default() -> Self {
        Self {
            global_admission_capacity: NonZeroUsize::new(256).expect("nonzero capacity"),
            command_capacity: NonZeroUsize::new(8).expect("nonzero capacity"),
            lifecycle_capacity: NonZeroUsize::new(64).expect("nonzero capacity"),
            rejection_acknowledgement_capacity: NonZeroUsize::new(64).expect("nonzero capacity"),
            drain_timeout: Duration::from_secs(15),
            instance_lookup_timeout: Duration::from_millis(500),
            discord_transport: RuntimeDiscordTransportConfigV1::Direct,
        }
    }
}

impl GatewayResourceConfigV1 {
    pub fn global_admission_capacity(self) -> NonZeroUsize {
        self.global_admission_capacity
    }

    pub fn command_capacity(self) -> NonZeroUsize {
        self.command_capacity
    }

    pub fn lifecycle_capacity(self) -> NonZeroUsize {
        self.lifecycle_capacity
    }

    pub fn rejection_acknowledgement_capacity(self) -> NonZeroUsize {
        self.rejection_acknowledgement_capacity
    }

    pub fn drain_timeout(self) -> Duration {
        self.drain_timeout
    }

    pub fn instance_lookup_timeout(self) -> Duration {
        self.instance_lookup_timeout
    }

    pub(crate) fn discord_transport(self) -> RuntimeDiscordTransportConfigV1 {
        self.discord_transport
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordTransportConfigV1 {
    #[default]
    Direct,
    LoopbackProxy {
        gateway_address: SocketAddrV4,
        effect_http_proxy_address: SocketAddrV4,
    },
}

impl RuntimeDiscordTransportConfigV1 {
    pub(crate) fn gateway_proxy_url(self) -> Option<String> {
        match self {
            Self::Direct => None,
            Self::LoopbackProxy {
                gateway_address, ..
            } => Some(format!("ws://{gateway_address}")),
        }
    }

    pub(crate) fn effect_http_proxy_address(self) -> Option<SocketAddrV4> {
        match self {
            Self::Direct => None,
            Self::LoopbackProxy {
                effect_http_proxy_address,
                ..
            } => Some(effect_http_proxy_address),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayOwnerTimingConfigV1 {
    lease_for: Duration,
    renew_before: Duration,
    safety_margin: Duration,
}

impl GatewayOwnerTimingConfigV1 {
    pub fn lease_for(self) -> Duration {
        self.lease_for
    }

    pub fn renew_before(self) -> Duration {
        self.renew_before
    }

    pub fn safety_margin(self) -> Duration {
        self.safety_margin
    }

    #[cfg(test)]
    pub(crate) fn from_parts_for_test_v1(
        lease_for: Duration,
        renew_before: Duration,
        safety_margin: Duration,
    ) -> Self {
        Self {
            lease_for,
            renew_before,
            safety_margin,
        }
    }
}

#[derive(Clone)]
pub struct RuntimeSecretReferencesV1 {
    database_urls: [RuntimeSecretReferenceV1; 5],
    discord_bot_token: RuntimeSecretReferenceV1,
    interaction_token_envelope_keyring: RuntimeSecretReferenceV1,
}

impl RuntimeSecretReferencesV1 {
    pub(crate) fn from_parts(
        database_urls: [RuntimeSecretReferenceV1; 5],
        discord_bot_token: RuntimeSecretReferenceV1,
        interaction_token_envelope_keyring: RuntimeSecretReferenceV1,
    ) -> Self {
        Self {
            database_urls,
            discord_bot_token,
            interaction_token_envelope_keyring,
        }
    }

    pub fn database_url(&self, capability: DatabaseCapabilityV1) -> &RuntimeSecretReferenceV1 {
        &self.database_urls[capability.index()]
    }

    pub fn discord_bot_token(&self) -> &RuntimeSecretReferenceV1 {
        &self.discord_bot_token
    }

    pub fn interaction_token_envelope_keyring(&self) -> &RuntimeSecretReferenceV1 {
        &self.interaction_token_envelope_keyring
    }
}

impl Debug for RuntimeSecretReferencesV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSecretReferencesV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct RuntimeConfigV1 {
    health_bind_addr: SocketAddr,
    database_pool: DatabasePoolConfigV1,
    database_operation: DatabaseOperationConfigV1,
    gateway: GatewayResourceConfigV1,
    gateway_owner: GatewayOwnerTimingConfigV1,
    runtime_controller: RuntimeServingControllerConfigV2,
    secret_references: RuntimeSecretReferencesV1,
}

impl RuntimeConfigV1 {
    pub fn from_process_environment() -> Result<Self, RuntimeConfigErrorV1> {
        Self::from_source(&ProcessConfigurationSourceV1)
    }

    fn from_source(source: &impl ConfigurationSourceV1) -> Result<Self, RuntimeConfigErrorV1> {
        if FORBIDDEN_POSTGRES_ENVIRONMENT
            .iter()
            .any(|name| source.read(name).is_some())
        {
            return Err(RuntimeConfigErrorV1::AmbientDatabaseConfiguration);
        }
        let health_bind_addr = parse_health_bind_addr(source)?;
        let database_pool = parse_database_pool(source)?;
        let database_operation = parse_database_operation(source)?;
        let gateway = parse_gateway_resources(source)?;
        let runtime_controller = RuntimeServingControllerConfigV2::production_v2();
        let gateway_owner =
            parse_gateway_owner_timing(source, database_operation, &runtime_controller)?;
        let secret_references = parse_secret_references(source)?;
        Ok(Self {
            health_bind_addr,
            database_pool,
            database_operation,
            gateway,
            gateway_owner,
            runtime_controller,
            secret_references,
        })
    }

    pub fn health_bind_addr(&self) -> SocketAddr {
        self.health_bind_addr
    }

    pub fn database_pool(&self) -> DatabasePoolConfigV1 {
        self.database_pool
    }

    pub fn database_operation(&self) -> DatabaseOperationConfigV1 {
        self.database_operation
    }

    pub fn gateway(&self) -> GatewayResourceConfigV1 {
        self.gateway
    }

    pub fn gateway_owner(&self) -> GatewayOwnerTimingConfigV1 {
        self.gateway_owner
    }

    pub(crate) fn runtime_controller(&self) -> RuntimeServingControllerConfigV2 {
        self.runtime_controller.clone()
    }

    pub fn secret_references(&self) -> &RuntimeSecretReferencesV1 {
        &self.secret_references
    }
}

impl Debug for RuntimeConfigV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeConfigV1")
            .field("health_bind_addr", &self.health_bind_addr)
            .field("database_pool", &self.database_pool)
            .field("database_operation", &self.database_operation)
            .field("gateway", &self.gateway)
            .field("gateway_owner", &self.gateway_owner)
            .field("runtime_controller", &self.runtime_controller)
            .field("secret_references", &"<redacted>")
            .finish()
    }
}

fn parse_health_bind_addr(
    source: &impl ConfigurationSourceV1,
) -> Result<SocketAddr, RuntimeConfigErrorV1> {
    let field = RuntimeConfigurationFieldV1::HealthBindAddress;
    let address = match read_optional(source, field)? {
        Some(value) => value
            .parse::<SocketAddr>()
            .map_err(|_| RuntimeConfigErrorV1::InvalidValue(field))?,
        None => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, DEFAULT_HEALTH_PORT)),
    };
    match address {
        SocketAddr::V4(address)
            if *address.ip() == Ipv4Addr::LOCALHOST && address.port() >= MIN_BIND_PORT => {}
        _ => return Err(RuntimeConfigErrorV1::InvalidValue(field)),
    }
    Ok(address)
}

fn parse_database_pool(
    source: &impl ConfigurationSourceV1,
) -> Result<DatabasePoolConfigV1, RuntimeConfigErrorV1> {
    let max_field = RuntimeConfigurationFieldV1::DatabaseMaxConnections;
    let max_connections = parse_optional_number::<u32>(source, max_field, 2)?;
    let max_connections_per_capability = NonZeroU32::new(max_connections)
        .filter(|value| (MIN_POOL_CONNECTIONS..=MAX_POOL_CONNECTIONS).contains(&value.get()));
    let Some(max_connections_per_capability) = max_connections_per_capability else {
        return Err(RuntimeConfigErrorV1::InvalidValue(max_field));
    };
    let acquire_field = RuntimeConfigurationFieldV1::DatabaseAcquireTimeout;
    let acquire_timeout =
        Duration::from_millis(parse_optional_number::<u64>(source, acquire_field, 2_000)?);
    if !(MIN_ACQUIRE_TIMEOUT..=MAX_ACQUIRE_TIMEOUT).contains(&acquire_timeout) {
        return Err(RuntimeConfigErrorV1::InvalidValue(acquire_field));
    }
    let idle_field = RuntimeConfigurationFieldV1::DatabaseIdleTimeout;
    let idle_timeout = Duration::from_secs(parse_optional_number::<u64>(source, idle_field, 60)?);
    if !(MIN_IDLE_TIMEOUT..=MAX_IDLE_TIMEOUT).contains(&idle_timeout) {
        return Err(RuntimeConfigErrorV1::InvalidValue(idle_field));
    }
    let lifetime_field = RuntimeConfigurationFieldV1::DatabaseMaxLifetime;
    let max_lifetime =
        Duration::from_secs(parse_optional_number::<u64>(source, lifetime_field, 600)?);
    if !(MIN_MAX_LIFETIME..=MAX_MAX_LIFETIME).contains(&max_lifetime)
        || max_lifetime <= idle_timeout
    {
        return Err(RuntimeConfigErrorV1::InvalidValue(lifetime_field));
    }
    Ok(DatabasePoolConfigV1 {
        max_connections_per_capability,
        acquire_timeout,
        idle_timeout,
        max_lifetime,
    })
}

fn parse_database_operation(
    source: &impl ConfigurationSourceV1,
) -> Result<DatabaseOperationConfigV1, RuntimeConfigErrorV1> {
    let statement_field = RuntimeConfigurationFieldV1::DatabaseStatementTimeout;
    let statement_timeout = Duration::from_millis(parse_optional_number::<u64>(
        source,
        statement_field,
        2_000,
    )?);
    if !(MIN_STATEMENT_TIMEOUT..=MAX_STATEMENT_TIMEOUT).contains(&statement_timeout) {
        return Err(RuntimeConfigErrorV1::InvalidValue(statement_field));
    }
    let lock_field = RuntimeConfigurationFieldV1::DatabaseLockTimeout;
    let lock_timeout =
        Duration::from_millis(parse_optional_number::<u64>(source, lock_field, 1_000)?);
    if lock_timeout < MIN_LOCK_TIMEOUT || lock_timeout >= statement_timeout {
        return Err(RuntimeConfigErrorV1::InvalidValue(lock_field));
    }
    Ok(DatabaseOperationConfigV1 {
        statement_timeout,
        lock_timeout,
    })
}

fn parse_gateway_resources(
    source: &impl ConfigurationSourceV1,
) -> Result<GatewayResourceConfigV1, RuntimeConfigErrorV1> {
    let global_admission_capacity = parse_capacity(
        source,
        RuntimeConfigurationFieldV1::GatewayGlobalAdmissionCapacity,
        256,
        MAX_GLOBAL_ADMISSION_CAPACITY,
    )?;
    let command_capacity = parse_capacity(
        source,
        RuntimeConfigurationFieldV1::GatewayCommandCapacity,
        8,
        MAX_COMMAND_CAPACITY,
    )?;
    let lifecycle_capacity = parse_capacity(
        source,
        RuntimeConfigurationFieldV1::GatewayLifecycleCapacity,
        64,
        MAX_LIFECYCLE_CAPACITY,
    )?;
    let rejection_acknowledgement_capacity = parse_capacity(
        source,
        RuntimeConfigurationFieldV1::GatewayRejectionAcknowledgementCapacity,
        64,
        MAX_REJECTION_ACKNOWLEDGEMENT_CAPACITY,
    )?;
    let drain_field = RuntimeConfigurationFieldV1::GatewayDrainTimeout;
    let drain_timeout = Duration::from_secs(parse_optional_number::<u64>(source, drain_field, 15)?);
    if drain_timeout.is_zero() || drain_timeout > MAX_GATEWAY_DRAIN_TIMEOUT {
        return Err(RuntimeConfigErrorV1::InvalidValue(drain_field));
    }
    let lookup_field = RuntimeConfigurationFieldV1::InstanceLookupTimeout;
    let instance_lookup_timeout =
        Duration::from_millis(parse_optional_number::<u64>(source, lookup_field, 500)?);
    if instance_lookup_timeout.is_zero() || instance_lookup_timeout > MAX_INSTANCE_LOOKUP_TIMEOUT {
        return Err(RuntimeConfigErrorV1::InvalidValue(lookup_field));
    }
    let discord_transport = parse_discord_transport(source)?;
    Ok(GatewayResourceConfigV1 {
        global_admission_capacity,
        command_capacity,
        lifecycle_capacity,
        rejection_acknowledgement_capacity,
        drain_timeout,
        instance_lookup_timeout,
        discord_transport,
    })
}

fn parse_discord_transport(
    source: &impl ConfigurationSourceV1,
) -> Result<RuntimeDiscordTransportConfigV1, RuntimeConfigErrorV1> {
    let mode_field = RuntimeConfigurationFieldV1::DiscordTransportMode;
    let gateway_field = RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl;
    let effect_http_field = RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority;
    let mode = read_optional(source, mode_field)?;
    let gateway = read_optional(source, gateway_field)?;
    let effect_http = read_optional(source, effect_http_field)?;
    match (mode, gateway, effect_http) {
        (None, None, None) => Ok(RuntimeDiscordTransportConfigV1::Direct),
        (None, _, _) => Err(RuntimeConfigErrorV1::Missing(mode_field)),
        (Some(mode), None, _) if mode == "loopback_proxy_v1" => {
            Err(RuntimeConfigErrorV1::Missing(gateway_field))
        }
        (Some(mode), Some(_), None) if mode == "loopback_proxy_v1" => {
            Err(RuntimeConfigErrorV1::Missing(effect_http_field))
        }
        (Some(mode), Some(gateway), Some(effect_http)) if mode == "loopback_proxy_v1" => {
            let gateway_address = parse_canonical_loopback_gateway_proxy_v1(&gateway)
                .ok_or(RuntimeConfigErrorV1::InvalidValue(gateway_field))?;
            let effect_http_proxy_address = parse_canonical_loopback_http_proxy_v1(&effect_http)
                .ok_or(RuntimeConfigErrorV1::InvalidValue(effect_http_field))?;
            if gateway_address.port() == effect_http_proxy_address.port() {
                return Err(RuntimeConfigErrorV1::InvalidValue(effect_http_field));
            }
            Ok(RuntimeDiscordTransportConfigV1::LoopbackProxy {
                gateway_address,
                effect_http_proxy_address,
            })
        }
        (Some(_), _, _) => Err(RuntimeConfigErrorV1::InvalidValue(mode_field)),
    }
}

fn parse_canonical_loopback_gateway_proxy_v1(value: &str) -> Option<SocketAddrV4> {
    let authority = value.strip_prefix("ws://")?;
    let address = parse_canonical_loopback_proxy_authority_v1(authority)?;
    (value == format!("ws://{address}")).then_some(address)
}

fn parse_canonical_loopback_http_proxy_v1(value: &str) -> Option<SocketAddrV4> {
    let address = parse_canonical_loopback_proxy_authority_v1(value)?;
    (value == address.to_string()).then_some(address)
}

fn parse_canonical_loopback_proxy_authority_v1(value: &str) -> Option<SocketAddrV4> {
    let address = value.parse::<SocketAddrV4>().ok()?;
    (*address.ip() == Ipv4Addr::LOCALHOST && address.port() >= MIN_BIND_PORT).then_some(address)
}

fn parse_gateway_owner_timing(
    source: &impl ConfigurationSourceV1,
    database_operation: DatabaseOperationConfigV1,
    runtime_controller: &RuntimeServingControllerConfigV2,
) -> Result<GatewayOwnerTimingConfigV1, RuntimeConfigErrorV1> {
    if database_operation.statement_timeout() > MAX_STARTUP_OWNER_STATEMENT_TIMEOUT {
        return Err(RuntimeConfigErrorV1::InvalidValue(
            RuntimeConfigurationFieldV1::DatabaseStatementTimeout,
        ));
    }
    let lease_field = RuntimeConfigurationFieldV1::GatewayOwnerLease;
    let lease_for =
        Duration::from_millis(parse_optional_number::<u64>(source, lease_field, 60_000)?);
    if !(MIN_GATEWAY_OWNER_LEASE..=MAX_GATEWAY_OWNER_LEASE).contains(&lease_for) {
        return Err(RuntimeConfigErrorV1::InvalidValue(lease_field));
    }
    let renew_field = RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore;
    let renew_before =
        Duration::from_millis(parse_optional_number::<u64>(source, renew_field, 40_000)?);
    if renew_before.is_zero() || renew_before >= lease_for {
        return Err(RuntimeConfigErrorV1::InvalidValue(renew_field));
    }
    let safety_field = RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin;
    let safety_margin =
        Duration::from_millis(parse_optional_number::<u64>(source, safety_field, 5_000)?);
    if safety_margin <= database_operation.statement_timeout() || safety_margin >= renew_before {
        return Err(RuntimeConfigErrorV1::InvalidValue(safety_field));
    }
    let renewal_window = renew_before
        .checked_sub(safety_margin)
        .ok_or(RuntimeConfigErrorV1::InvalidValue(renew_field))?;
    if renewal_window <= database_operation.statement_timeout() {
        return Err(RuntimeConfigErrorV1::InvalidValue(renew_field));
    }
    let certification_runway = runtime_controller
        .gateway_ready_timeout_v2()
        .checked_add(database_operation.statement_timeout())
        .ok_or(RuntimeConfigErrorV1::InvalidValue(renew_field))?;
    if renewal_window <= certification_runway {
        return Err(RuntimeConfigErrorV1::InvalidValue(renew_field));
    }
    Ok(GatewayOwnerTimingConfigV1 {
        lease_for,
        renew_before,
        safety_margin,
    })
}

fn parse_capacity(
    source: &impl ConfigurationSourceV1,
    field: RuntimeConfigurationFieldV1,
    default: usize,
    maximum: usize,
) -> Result<NonZeroUsize, RuntimeConfigErrorV1> {
    let value = parse_optional_number::<usize>(source, field, default)?;
    NonZeroUsize::new(value)
        .filter(|value| value.get() <= maximum)
        .ok_or(RuntimeConfigErrorV1::InvalidValue(field))
}

fn parse_secret_references(
    source: &impl ConfigurationSourceV1,
) -> Result<RuntimeSecretReferencesV1, RuntimeConfigErrorV1> {
    let mut database_urls = Vec::with_capacity(DatabaseCapabilityV1::ALL.len());
    for capability in DatabaseCapabilityV1::ALL {
        let field = RuntimeConfigurationFieldV1::DatabaseUrlSecretReference(capability);
        database_urls.push(parse_secret_reference(source, field)?);
    }
    let discord_bot_token = parse_secret_reference(
        source,
        RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference,
    )?;
    let interaction_token_envelope_keyring = parse_secret_reference(
        source,
        RuntimeConfigurationFieldV1::InteractionTokenEnvelopeKeyringSecretReference,
    )?;
    let duplicate_database = database_urls.iter().enumerate().any(|(index, candidate)| {
        database_urls
            .iter()
            .skip(index + 1)
            .any(|other| candidate == other)
    });
    if duplicate_database
        || database_urls
            .iter()
            .any(|reference| reference == &discord_bot_token)
        || database_urls
            .iter()
            .any(|reference| reference == &interaction_token_envelope_keyring)
        || discord_bot_token == interaction_token_envelope_keyring
    {
        return Err(RuntimeConfigErrorV1::DuplicateSecretReference);
    }
    let database_urls: [RuntimeSecretReferenceV1; 5] = database_urls
        .try_into()
        .map_err(|_| RuntimeConfigErrorV1::DuplicateSecretReference)?;
    Ok(RuntimeSecretReferencesV1::from_parts(
        database_urls,
        discord_bot_token,
        interaction_token_envelope_keyring,
    ))
}

fn parse_secret_reference(
    source: &impl ConfigurationSourceV1,
    field: RuntimeConfigurationFieldV1,
) -> Result<RuntimeSecretReferenceV1, RuntimeConfigErrorV1> {
    let value = read_required(source, field)?;
    RuntimeSecretReferenceV1::parse(&value).ok_or(RuntimeConfigErrorV1::InvalidValue(field))
}

fn parse_optional_number<T>(
    source: &impl ConfigurationSourceV1,
    field: RuntimeConfigurationFieldV1,
    default: T,
) -> Result<T, RuntimeConfigErrorV1>
where
    T: std::str::FromStr,
{
    let Some(value) = read_optional(source, field)? else {
        return Ok(default);
    };
    if !valid_unsigned_decimal(&value) {
        return Err(RuntimeConfigErrorV1::InvalidValue(field));
    }
    value
        .parse::<T>()
        .map_err(|_| RuntimeConfigErrorV1::InvalidValue(field))
}

fn read_required(
    source: &impl ConfigurationSourceV1,
    field: RuntimeConfigurationFieldV1,
) -> Result<String, RuntimeConfigErrorV1> {
    read_optional(source, field)?.ok_or(RuntimeConfigErrorV1::Missing(field))
}

fn read_optional(
    source: &impl ConfigurationSourceV1,
    field: RuntimeConfigurationFieldV1,
) -> Result<Option<String>, RuntimeConfigErrorV1> {
    let Some(value) = source.read(field.environment_name()) else {
        return Ok(None);
    };
    let value = value
        .into_string()
        .map_err(|_| RuntimeConfigErrorV1::InvalidEncoding(field))?;
    if value.is_empty()
        || value.len() > MAX_CONFIGURATION_VALUE_BYTES
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(RuntimeConfigErrorV1::InvalidValue(field));
    }
    Ok(Some(value))
}

fn valid_unsigned_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    struct FakeConfigurationSourceV1 {
        values: BTreeMap<String, OsString>,
    }

    impl ConfigurationSourceV1 for FakeConfigurationSourceV1 {
        fn read(&self, name: &str) -> Option<OsString> {
            self.values.get(name).cloned()
        }
    }

    impl FakeConfigurationSourceV1 {
        fn insert(&mut self, field: RuntimeConfigurationFieldV1, value: impl Into<OsString>) {
            self.values
                .insert(field.environment_name().to_string(), value.into());
        }
    }

    fn valid_source() -> FakeConfigurationSourceV1 {
        let mut source = FakeConfigurationSourceV1::default();
        for capability in DatabaseCapabilityV1::ALL {
            source.insert(
                RuntimeConfigurationFieldV1::DatabaseUrlSecretReference(capability),
                format!(
                    "env:STARRING_RUNTIME_SECRET_DATABASE_{}",
                    capability.index()
                ),
            );
        }
        source.insert(
            RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference,
            "env:STARRING_RUNTIME_SECRET_DISCORD_BOT_TOKEN",
        );
        source.insert(
            RuntimeConfigurationFieldV1::InteractionTokenEnvelopeKeyringSecretReference,
            "env:STARRING_RUNTIME_SECRET_INTERACTION_TOKEN_ENVELOPE_KEYRING",
        );
        source
    }

    #[test]
    fn defaults_are_loopback_only_and_match_runtime_resource_limits() {
        let config = RuntimeConfigV1::from_source(&valid_source()).unwrap();
        assert_eq!(
            config.health_bind_addr(),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9091))
        );
        assert_eq!(
            config
                .database_pool()
                .max_connections_per_capability()
                .get(),
            2
        );
        assert_eq!(config.database_pool().total_connection_ceiling(), 10);
        assert_eq!(
            config.database_pool().acquire_timeout(),
            Duration::from_secs(2)
        );
        assert_eq!(
            config.database_pool().idle_timeout(),
            Duration::from_secs(60)
        );
        assert_eq!(
            config.database_pool().max_lifetime(),
            Duration::from_secs(600)
        );
        assert_eq!(
            config.database_operation().statement_timeout(),
            Duration::from_secs(2)
        );
        assert_eq!(
            config.database_operation().lock_timeout(),
            Duration::from_secs(1)
        );
        assert_eq!(config.gateway().global_admission_capacity().get(), 256);
        assert_eq!(config.gateway().command_capacity().get(), 8);
        assert_eq!(config.gateway().lifecycle_capacity().get(), 64);
        assert_eq!(
            config.gateway().rejection_acknowledgement_capacity().get(),
            64
        );
        assert_eq!(config.gateway().drain_timeout(), Duration::from_secs(15));
        assert_eq!(
            config.gateway().instance_lookup_timeout(),
            Duration::from_millis(500)
        );
        assert_eq!(
            config.gateway().discord_transport(),
            RuntimeDiscordTransportConfigV1::Direct
        );
        assert_eq!(config.gateway_owner().lease_for(), Duration::from_secs(60));
        assert_eq!(
            config.gateway_owner().renew_before(),
            Duration::from_secs(40)
        );
        assert_eq!(
            config.gateway_owner().safety_margin(),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn all_secret_references_are_required_indirect_and_unique() {
        let config = RuntimeConfigV1::from_source(&valid_source()).unwrap();
        for capability in DatabaseCapabilityV1::ALL {
            let expected = format!("STARRING_RUNTIME_SECRET_DATABASE_{}", capability.index());
            assert_eq!(
                config
                    .secret_references()
                    .database_url(capability)
                    .environment_name(),
                Some(expected.as_str())
            );
        }
        assert_eq!(
            config
                .secret_references()
                .discord_bot_token()
                .environment_name(),
            Some("STARRING_RUNTIME_SECRET_DISCORD_BOT_TOKEN")
        );
        assert_eq!(
            config
                .secret_references()
                .interaction_token_envelope_keyring()
                .environment_name(),
            Some("STARRING_RUNTIME_SECRET_INTERACTION_TOKEN_ENVELOPE_KEYRING")
        );
        let mut missing = valid_source();
        missing.values.remove(
            RuntimeConfigurationFieldV1::DatabaseUrlSecretReference(
                DatabaseCapabilityV1::ExactTarget,
            )
            .environment_name(),
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&missing).unwrap_err(),
            RuntimeConfigErrorV1::Missing(RuntimeConfigurationFieldV1::DatabaseUrlSecretReference(
                DatabaseCapabilityV1::ExactTarget
            ))
        );
        let mut keychain = valid_source();
        keychain.insert(
            RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference,
            "keychain:starring.runtime:discord.bot",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&keychain)
                .unwrap()
                .secret_references()
                .discord_bot_token()
                .keychain_identity(),
            Some(("starring.runtime", "discord.bot"))
        );
        let mut literal = valid_source();
        literal.insert(
            RuntimeConfigurationFieldV1::DatabaseUrlSecretReference(DatabaseCapabilityV1::Serving),
            format!("postgresql:{}{}runtime", '/', '/'),
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&literal).unwrap_err(),
            RuntimeConfigErrorV1::InvalidValue(
                RuntimeConfigurationFieldV1::DatabaseUrlSecretReference(
                    DatabaseCapabilityV1::Serving
                )
            )
        );
        let mut duplicate = valid_source();
        duplicate.insert(
            RuntimeConfigurationFieldV1::DatabaseUrlSecretReference(
                DatabaseCapabilityV1::Interaction,
            ),
            "env:STARRING_RUNTIME_SECRET_DATABASE_0",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&duplicate).unwrap_err(),
            RuntimeConfigErrorV1::DuplicateSecretReference
        );
        let mut token_alias = valid_source();
        token_alias.insert(
            RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference,
            "env:STARRING_RUNTIME_SECRET_DATABASE_0",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&token_alias).unwrap_err(),
            RuntimeConfigErrorV1::DuplicateSecretReference
        );
        let mut keyring_alias = valid_source();
        keyring_alias.insert(
            RuntimeConfigurationFieldV1::InteractionTokenEnvelopeKeyringSecretReference,
            "env:STARRING_RUNTIME_SECRET_DISCORD_BOT_TOKEN",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&keyring_alias).unwrap_err(),
            RuntimeConfigErrorV1::DuplicateSecretReference
        );
        let mut keyring_database_alias = valid_source();
        keyring_database_alias.insert(
            RuntimeConfigurationFieldV1::InteractionTokenEnvelopeKeyringSecretReference,
            "env:STARRING_RUNTIME_SECRET_DATABASE_4",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&keyring_database_alias).unwrap_err(),
            RuntimeConfigErrorV1::DuplicateSecretReference
        );
        let mut missing_keyring = valid_source();
        missing_keyring.values.remove(
            RuntimeConfigurationFieldV1::InteractionTokenEnvelopeKeyringSecretReference
                .environment_name(),
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&missing_keyring).unwrap_err(),
            RuntimeConfigErrorV1::Missing(
                RuntimeConfigurationFieldV1::InteractionTokenEnvelopeKeyringSecretReference
            )
        );
        for rejected in [
            "env:",
            "env:_PRIVATE",
            "env:lowercase",
            "env:PGPASSWORD",
            "env:NAME-WITH-DASH",
            "keychain:",
            "keychain:service",
            "keychain:service:account:extra",
            "keychain:-service:account",
        ] {
            let mut malformed = valid_source();
            malformed.insert(
                RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference,
                rejected,
            );
            assert_eq!(
                RuntimeConfigV1::from_source(&malformed).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(
                    RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference
                )
            );
        }
    }

    #[test]
    fn health_listener_accepts_only_unprivileged_loopback_addresses() {
        let mut accepted = valid_source();
        accepted.insert(
            RuntimeConfigurationFieldV1::HealthBindAddress,
            "127.0.0.1:9188",
        );
        assert!(RuntimeConfigV1::from_source(&accepted).is_ok());
        for rejected in [
            "0.0.0.0:9188",
            "127.0.0.2:9188",
            "192.0.2.1:9188",
            "[::1]:9188",
            "127.0.0.1:0",
            "127.0.0.1:80",
        ] {
            let mut source = valid_source();
            source.insert(RuntimeConfigurationFieldV1::HealthBindAddress, rejected);
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(RuntimeConfigurationFieldV1::HealthBindAddress)
            );
        }
    }

    #[test]
    fn pool_and_database_timeouts_are_nonzero_bounded_and_ordered() {
        for (field, rejected) in [
            (RuntimeConfigurationFieldV1::DatabaseMaxConnections, "0"),
            (RuntimeConfigurationFieldV1::DatabaseMaxConnections, "5"),
            (RuntimeConfigurationFieldV1::DatabaseAcquireTimeout, "99"),
            (RuntimeConfigurationFieldV1::DatabaseAcquireTimeout, "5001"),
            (RuntimeConfigurationFieldV1::DatabaseIdleTimeout, "29"),
            (RuntimeConfigurationFieldV1::DatabaseIdleTimeout, "601"),
            (RuntimeConfigurationFieldV1::DatabaseStatementTimeout, "99"),
            (
                RuntimeConfigurationFieldV1::DatabaseStatementTimeout,
                "30001",
            ),
        ] {
            let mut source = valid_source();
            source.insert(field, rejected);
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(field)
            );
        }
        let mut equal_lifetime = valid_source();
        equal_lifetime.insert(RuntimeConfigurationFieldV1::DatabaseIdleTimeout, "60");
        equal_lifetime.insert(RuntimeConfigurationFieldV1::DatabaseMaxLifetime, "60");
        assert_eq!(
            RuntimeConfigV1::from_source(&equal_lifetime).unwrap_err(),
            RuntimeConfigErrorV1::InvalidValue(RuntimeConfigurationFieldV1::DatabaseMaxLifetime)
        );
        let mut excessive_lock = valid_source();
        excessive_lock.insert(RuntimeConfigurationFieldV1::DatabaseStatementTimeout, "500");
        for rejected in ["500", "501"] {
            excessive_lock.insert(RuntimeConfigurationFieldV1::DatabaseLockTimeout, rejected);
            assert_eq!(
                RuntimeConfigV1::from_source(&excessive_lock).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(
                    RuntimeConfigurationFieldV1::DatabaseLockTimeout
                )
            );
        }
    }

    #[test]
    fn gateway_capacities_and_timeouts_are_nonzero_and_bounded() {
        for (field, rejected) in [
            (
                RuntimeConfigurationFieldV1::GatewayGlobalAdmissionCapacity,
                "0",
            ),
            (
                RuntimeConfigurationFieldV1::GatewayGlobalAdmissionCapacity,
                "65537",
            ),
            (RuntimeConfigurationFieldV1::GatewayCommandCapacity, "65"),
            (
                RuntimeConfigurationFieldV1::GatewayLifecycleCapacity,
                "1025",
            ),
            (
                RuntimeConfigurationFieldV1::GatewayRejectionAcknowledgementCapacity,
                "1025",
            ),
            (RuntimeConfigurationFieldV1::GatewayDrainTimeout, "0"),
            (RuntimeConfigurationFieldV1::GatewayDrainTimeout, "61"),
            (RuntimeConfigurationFieldV1::InstanceLookupTimeout, "0"),
            (RuntimeConfigurationFieldV1::InstanceLookupTimeout, "2001"),
        ] {
            let mut source = valid_source();
            source.insert(field, rejected);
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(field)
            );
        }
    }

    #[test]
    fn loopback_discord_transport_is_explicit_canonical_and_split_by_protocol() {
        let mut source = valid_source();
        source.insert(
            RuntimeConfigurationFieldV1::DiscordTransportMode,
            "loopback_proxy_v1",
        );
        source.insert(
            RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl,
            "ws://127.0.0.1:21001",
        );
        source.insert(
            RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority,
            "127.0.0.1:21002",
        );
        let transport = RuntimeConfigV1::from_source(&source)
            .unwrap()
            .gateway()
            .discord_transport();
        assert_eq!(
            transport.gateway_proxy_url().as_deref(),
            Some("ws://127.0.0.1:21001")
        );
        assert_eq!(
            transport.effect_http_proxy_address(),
            Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 21002))
        );
    }

    #[test]
    fn discord_gateway_and_effect_http_proxy_require_distinct_ports() {
        let mut source = valid_source();
        source.insert(
            RuntimeConfigurationFieldV1::DiscordTransportMode,
            "loopback_proxy_v1",
        );
        source.insert(
            RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl,
            "ws://127.0.0.1:21001",
        );
        source.insert(
            RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority,
            "127.0.0.1:21001",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&source).unwrap_err(),
            RuntimeConfigErrorV1::InvalidValue(
                RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority
            )
        );
    }

    #[test]
    fn discord_transport_variables_are_all_or_none() {
        let fields = [
            (
                RuntimeConfigurationFieldV1::DiscordTransportMode,
                "loopback_proxy_v1",
            ),
            (
                RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl,
                "ws://127.0.0.1:21001",
            ),
            (
                RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority,
                "127.0.0.1:21002",
            ),
        ];
        for mask in 1_u8..7 {
            let mut source = valid_source();
            for (index, (field, value)) in fields.iter().enumerate() {
                if mask & (1 << index) != 0 {
                    source.insert(*field, *value);
                }
            }
            assert!(RuntimeConfigV1::from_source(&source).is_err(), "{mask}");
        }
    }

    #[test]
    fn discord_transport_rejects_non_opt_in_modes_and_unsafe_gateway_urls() {
        for mode in ["direct", "loopback_proxy_v2", "LOOPBACK_PROXY_V1"] {
            let mut source = valid_source();
            source.insert(RuntimeConfigurationFieldV1::DiscordTransportMode, mode);
            source.insert(
                RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl,
                "ws://127.0.0.1:21001",
            );
            source.insert(
                RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority,
                "127.0.0.1:21002",
            );
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(
                    RuntimeConfigurationFieldV1::DiscordTransportMode
                )
            );
        }
        for gateway in [
            "wss://127.0.0.1:21001",
            "ws://127.0.0.2:21001",
            "ws://0.0.0.0:21001",
            "ws://127.0.0.1:80",
            "ws://127.0.0.1:21001/",
            "ws://[::1]:21001",
        ] {
            let mut source = valid_source();
            source.insert(
                RuntimeConfigurationFieldV1::DiscordTransportMode,
                "loopback_proxy_v1",
            );
            source.insert(RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl, gateway);
            source.insert(
                RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority,
                "127.0.0.1:21002",
            );
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(
                    RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl
                )
            );
        }
    }

    #[test]
    fn discord_transport_rejects_unsafe_effect_http_authorities() {
        for authority in [
            "http://127.0.0.1:21002",
            "127.0.0.2:21002",
            "0.0.0.0:21002",
            "127.0.0.1:80",
            "127.0.0.1:21002/",
            "[::1]:21002",
        ] {
            let mut source = valid_source();
            source.insert(
                RuntimeConfigurationFieldV1::DiscordTransportMode,
                "loopback_proxy_v1",
            );
            source.insert(
                RuntimeConfigurationFieldV1::DiscordGatewayProxyUrl,
                "ws://127.0.0.1:21001",
            );
            source.insert(
                RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority,
                authority,
            );
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(
                    RuntimeConfigurationFieldV1::DiscordEffectHttpProxyAuthority
                )
            );
        }
    }

    #[test]
    fn gateway_owner_timing_is_bounded_ordered_and_covers_certification_runway() {
        for (field, rejected) in [
            (RuntimeConfigurationFieldV1::GatewayOwnerLease, "999"),
            (RuntimeConfigurationFieldV1::GatewayOwnerLease, "300001"),
            (RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore, "0"),
            (
                RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore,
                "30000",
            ),
            (RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin, "0"),
            (
                RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin,
                "2000",
            ),
            (
                RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin,
                "40000",
            ),
        ] {
            let mut source = valid_source();
            source.insert(field, rejected);
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(field)
            );
        }

        let mut short_window = valid_source();
        short_window.insert(
            RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore,
            "35000",
        );
        short_window.insert(
            RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin,
            "3000",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&short_window).unwrap_err(),
            RuntimeConfigErrorV1::InvalidValue(
                RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore
            )
        );

        let mut legacy_timing = valid_source();
        legacy_timing.insert(RuntimeConfigurationFieldV1::GatewayOwnerLease, "30000");
        legacy_timing.insert(
            RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore,
            "10000",
        );
        legacy_timing.insert(
            RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin,
            "3000",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&legacy_timing).unwrap_err(),
            RuntimeConfigErrorV1::InvalidValue(
                RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore
            )
        );

        let mut cleanup_overrun = valid_source();
        cleanup_overrun.insert(
            RuntimeConfigurationFieldV1::DatabaseStatementTimeout,
            "4001",
        );
        cleanup_overrun.insert(
            RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin,
            "5000",
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&cleanup_overrun).unwrap_err(),
            RuntimeConfigErrorV1::InvalidValue(
                RuntimeConfigurationFieldV1::DatabaseStatementTimeout
            )
        );

        let mut custom = valid_source();
        custom.insert(RuntimeConfigurationFieldV1::GatewayOwnerLease, "90000");
        custom.insert(
            RuntimeConfigurationFieldV1::GatewayOwnerRenewBefore,
            "50000",
        );
        custom.insert(
            RuntimeConfigurationFieldV1::GatewayOwnerSafetyMargin,
            "5000",
        );
        let timing = RuntimeConfigV1::from_source(&custom)
            .unwrap()
            .gateway_owner();
        assert_eq!(timing.lease_for(), Duration::from_secs(90));
        assert_eq!(timing.renew_before(), Duration::from_secs(50));
        assert_eq!(timing.safety_margin(), Duration::from_secs(5));
    }

    #[test]
    fn ambient_postgres_configuration_is_rejected_before_reference_parsing() {
        for name in FORBIDDEN_POSTGRES_ENVIRONMENT {
            let mut source = valid_source();
            source.values.insert(name.to_string(), "ambient".into());
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::AmbientDatabaseConfiguration
            );
        }
    }

    #[test]
    fn numeric_and_text_values_require_canonical_bounded_encoding() {
        for value in ["01", "+1", " 1", "1 ", "1\n"] {
            let mut source = valid_source();
            source.insert(RuntimeConfigurationFieldV1::GatewayCommandCapacity, value);
            assert_eq!(
                RuntimeConfigV1::from_source(&source).unwrap_err(),
                RuntimeConfigErrorV1::InvalidValue(
                    RuntimeConfigurationFieldV1::GatewayCommandCapacity
                )
            );
        }
        let mut oversized = valid_source();
        oversized.insert(
            RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference,
            "A".repeat(MAX_CONFIGURATION_VALUE_BYTES + 1),
        );
        assert_eq!(
            RuntimeConfigV1::from_source(&oversized).unwrap_err(),
            RuntimeConfigErrorV1::InvalidValue(
                RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_configuration_is_rejected_without_echoing_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let mut source = valid_source();
        source.values.insert(
            RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference
                .environment_name()
                .to_string(),
            OsString::from_vec(vec![0xff]),
        );
        let error = RuntimeConfigV1::from_source(&source).unwrap_err();
        assert_eq!(
            error,
            RuntimeConfigErrorV1::InvalidEncoding(
                RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference
            )
        );
        assert_eq!(
            error.to_string(),
            "runtime configuration encoding is invalid"
        );
    }

    #[test]
    fn debug_display_and_stable_codes_do_not_expose_reference_names() {
        let config = RuntimeConfigV1::from_source(&valid_source()).unwrap();
        let keychain = RuntimeSecretReferenceV1::parse(
            "keychain:starring.runtime.private:discord.bot.private",
        )
        .unwrap();
        let config_debug = format!("{config:?}");
        let references_debug = format!("{:?}", config.secret_references());
        let keychain_debug = format!("{keychain:?}");
        assert!(config_debug.contains("<redacted>"));
        assert!(references_debug.contains("<redacted>"));
        assert!(!config_debug.contains("STARRING_RUNTIME_SECRET"));
        assert!(!references_debug.contains("STARRING_RUNTIME_SECRET"));
        assert!(!keychain_debug.contains("starring.runtime.private"));
        assert!(!keychain_debug.contains("discord.bot.private"));
        let error = RuntimeConfigErrorV1::InvalidValue(
            RuntimeConfigurationFieldV1::DiscordBotTokenSecretReference,
        );
        assert_eq!(error.code(), "runtime_config_invalid_value");
        assert_eq!(
            error.field().map(RuntimeConfigurationFieldV1::code),
            Some("discord_bot_token_secret_reference")
        );
        assert_eq!(error.to_string(), "runtime configuration value is invalid");
        assert!(!error.to_string().contains("DISCORD"));
    }
}
