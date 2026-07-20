use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use authoring_application_discord::{
    DiscordAuthorityConfigV1, DiscordOAuthClient, DiscordOAuthConfigV1,
    TwilightDiscordGuildAuthorityClient,
};
use authoring_application_postgres::{
    PostgresAuthorizedPromotionSnapshots, PostgresInstallationAuthoritySource,
    PostgresProductControl, PostgresProductDeploymentOperationalStatusesV2,
    PostgresProductDeploymentStatuses, PostgresProductIdentityConfig, PostgresProductIdentityStore,
    PostgresProductPromotions, ProductDecisionDatabasePoolsV1, ProductIdentityDatabasePoolsV1,
    XChaCha20Poly1305SnapshotEnvelopeCipherV1,
};
use product_control_http::{HttpBoundaryConfig, ProductControlFacade};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use sqlx::ConnectOptions;
use tokio::time::timeout;
use twilight_http::Client as TwilightHttpClient;

use crate::config::{DatabaseRoleV1, PoolConfigV1, ProductionConfigV1};
use crate::secret::{DatabaseUrlSecretV1, ResolvedProductionSecretsV1};
use crate::{
    ProductionAuthorityDependenciesV1, ProductionFacadeConfigurationErrorV1,
    ProductionIdentityDependenciesV1, ProductionPersistenceDependenciesV1,
    ProductionProductControlFacadeV1,
};

const APPLICATION_NAME: &str = "starring-api";
const STARTUP_READINESS_TIMEOUT: Duration = Duration::from_secs(45);
const DATABASE_POOL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductionReadinessPhaseV1 {
    Aggregate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductionCompositionErrorV1 {
    #[error("the HTTP boundary configuration is invalid")]
    HttpBoundaryConfiguration,
    #[error("the product identity configuration is invalid")]
    ProductIdentityConfiguration,
    #[error("the Discord client configuration is invalid")]
    DiscordConfiguration,
    #[error("database role {role:?} connection configuration is invalid")]
    DatabaseConfiguration { role: DatabaseRoleV1 },
    #[error("database role {role:?} connection transport is not production-safe")]
    UnsafeDatabaseTransport { role: DatabaseRoleV1 },
    #[error("database role {role:?} connection is unavailable")]
    DatabaseUnavailable { role: DatabaseRoleV1 },
    #[error("the product persistence configuration is invalid")]
    ProductPersistenceConfiguration,
    #[error("the production facade configuration is inconsistent")]
    FacadeConfiguration,
    #[error("the production dependency readiness phase {phase:?} failed")]
    ReadinessFailed { phase: ProductionReadinessPhaseV1 },
    #[error("the production dependency readiness phase {phase:?} timed out")]
    ReadinessTimedOut { phase: ProductionReadinessPhaseV1 },
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProductionDatabasePoolShutdownErrorV1 {
    #[error("database pool shutdown timed out")]
    TimedOut,
}

impl Debug for ProductionDatabasePoolShutdownErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductionDatabasePoolShutdownErrorV1(<redacted>)")
    }
}

pub struct ComposedProductionServiceV1 {
    facade: Arc<ProductionProductControlFacadeV1>,
    http_boundary: HttpBoundaryConfig,
    loopback_bind_addr: SocketAddr,
    database_shutdown: ProductionDatabasePoolShutdownV1,
}

impl ComposedProductionServiceV1 {
    pub fn facade(&self) -> Arc<ProductionProductControlFacadeV1> {
        Arc::clone(&self.facade)
    }

    pub fn http_boundary_config(&self) -> &HttpBoundaryConfig {
        &self.http_boundary
    }

    pub fn loopback_bind_addr(&self) -> SocketAddr {
        self.loopback_bind_addr
    }

    pub fn into_parts(
        self,
    ) -> (
        Arc<ProductionProductControlFacadeV1>,
        HttpBoundaryConfig,
        SocketAddr,
        ProductionDatabasePoolShutdownV1,
    ) {
        (
            self.facade,
            self.http_boundary,
            self.loopback_bind_addr,
            self.database_shutdown,
        )
    }
}

impl Debug for ComposedProductionServiceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComposedProductionServiceV1")
            .field("loopback_bind_addr", &self.loopback_bind_addr)
            .field("dependencies", &"<redacted>")
            .finish()
    }
}

#[derive(Clone)]
pub struct ProductionDatabasePoolShutdownV1 {
    pools: Arc<[PgPool; 13]>,
}

impl ProductionDatabasePoolShutdownV1 {
    pub async fn close(&self) -> Result<(), ProductionDatabasePoolShutdownErrorV1> {
        close_pool_refs_with_deadline(self.pools.each_ref().map(Some)).await
    }

    pub fn is_closed(&self) -> bool {
        self.pools.iter().all(PgPool::is_closed)
    }
}

impl Debug for ProductionDatabasePoolShutdownV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductionDatabasePoolShutdownV1(<redacted>)")
    }
}

pub async fn compose_production_service_v1(
    config: ProductionConfigV1,
    secrets: ResolvedProductionSecretsV1,
) -> Result<ComposedProductionServiceV1, ProductionCompositionErrorV1> {
    let loopback_bind_addr = config.bind_addr();
    if !loopback_bind_addr.ip().is_loopback() {
        return Err(ProductionCompositionErrorV1::HttpBoundaryConfiguration);
    }
    let return_paths = config.return_paths().to_vec();
    let http_boundary = config.http_boundary();
    let identity_config = PostgresProductIdentityConfig::production(
        config.oauth_redirect_uri(),
        return_paths.iter().cloned(),
    )
    .map_err(|_| ProductionCompositionErrorV1::ProductIdentityConfiguration)?;
    let oauth_config = DiscordOAuthConfigV1::with_deadline(
        config.discord().application_id(),
        config.discord().oauth_redirect_uri(),
        config.discord().timing().request_timeout(),
    )
    .map_err(|_| ProductionCompositionErrorV1::DiscordConfiguration)?;
    let oauth = DiscordOAuthClient::new(oauth_config)
        .map_err(|_| ProductionCompositionErrorV1::DiscordConfiguration)?;
    let authority_config = DiscordAuthorityConfigV1::new(
        config.discord().timing().request_timeout(),
        config.discord().timing().write_authority_lifetime(),
        config.discord().timing().read_authority_lifetime(),
    )
    .map_err(|_| ProductionCompositionErrorV1::DiscordConfiguration)?;
    let pool_config = config.pool_config();
    let default_return_path = config.default_return_path().to_string();
    let application_id = config.discord().application_id();
    let bot_user_id = config.discord().bot_user_id();
    let authority_deadline = config.discord().timing().request_timeout();
    let (database_urls, discord_bot_token, oauth_client_secret, action_keyring, snapshot_keyring) =
        secrets.into_parts();
    let database_pools = connect_database_pools_v1(database_urls, pool_config).await?;
    let mut discord_bot_token = discord_bot_token.into_zeroizing();
    let discord_http = Arc::new(
        TwilightHttpClient::builder()
            .token(std::mem::take(&mut *discord_bot_token))
            .timeout(authority_deadline)
            .build(),
    );
    let discord_authority =
        TwilightDiscordGuildAuthorityClient::new(discord_http, application_id, bot_user_id);
    let facade = build_facade_v1(
        &database_pools,
        FacadeConstructionDependenciesV1 {
            identity_config,
            oauth,
            oauth_client_secret,
            default_return_path,
            discord_authority,
            authority_config,
            action_keyring,
            snapshot_keyring,
        },
    );
    let facade = match facade {
        Ok(facade) => Arc::new(facade),
        Err(error) => {
            let _shutdown_result = database_pools.close().await;
            return Err(error);
        }
    };
    match timeout(STARTUP_READINESS_TIMEOUT, facade.readiness()).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            let _shutdown_result = database_pools.close().await;
            return Err(ProductionCompositionErrorV1::ReadinessFailed {
                phase: ProductionReadinessPhaseV1::Aggregate,
            });
        }
        Err(_) => {
            let _shutdown_result = database_pools.close().await;
            return Err(ProductionCompositionErrorV1::ReadinessTimedOut {
                phase: ProductionReadinessPhaseV1::Aggregate,
            });
        }
    }
    Ok(ComposedProductionServiceV1 {
        facade,
        http_boundary,
        loopback_bind_addr,
        database_shutdown: database_pools.into_shutdown(),
    })
}

struct FacadeConstructionDependenciesV1 {
    identity_config: PostgresProductIdentityConfig,
    oauth: DiscordOAuthClient,
    oauth_client_secret: authoring_application_discord::DiscordOAuthClientSecretV1,
    default_return_path: String,
    discord_authority: TwilightDiscordGuildAuthorityClient,
    authority_config: DiscordAuthorityConfigV1,
    action_keyring: authoring_application_postgres::ProductActionDigestKeyringV1,
    snapshot_keyring: authoring_application_postgres::SnapshotEnvelopeKeyringV1,
}

fn build_facade_v1(
    pools: &ConnectedDatabasePoolsV1,
    dependencies: FacadeConstructionDependenciesV1,
) -> Result<ProductionProductControlFacadeV1, ProductionCompositionErrorV1> {
    let identity = PostgresProductIdentityStore::production(
        ProductIdentityDatabasePoolsV1::new(
            pools.oauth_flow_writer.clone(),
            pools.session_issuer.clone(),
            pools.session_api.clone(),
            pools.security_revoker.clone(),
        ),
        dependencies.identity_config,
    );
    let identity = ProductionIdentityDependenciesV1::new(
        identity,
        dependencies.oauth,
        dependencies.oauth_client_secret,
        dependencies.default_return_path,
    )
    .map_err(map_facade_configuration)?;
    let authority = ProductionAuthorityDependenciesV1::new(
        PostgresInstallationAuthoritySource::new(pools.installation_authority.clone()),
        dependencies.discord_authority,
        dependencies.authority_config,
    );
    let snapshots = PostgresAuthorizedPromotionSnapshots::new(
        pools.authorized_snapshot.clone(),
        XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(dependencies.snapshot_keyring),
    );
    let promotions = PostgresProductPromotions::new(
        pools.promotion.clone(),
        dependencies.action_keyring.clone(),
    )
    .map_err(|_| ProductionCompositionErrorV1::ProductPersistenceConfiguration)?;
    let control = PostgresProductControl::new(
        ProductDecisionDatabasePoolsV1::new(
            pools.decision_reader.clone(),
            pools.approval_executor.clone(),
            pools.apply_executor.clone(),
        ),
        pools.rejection_executor.clone(),
        dependencies.action_keyring,
    )
    .map_err(|_| ProductionCompositionErrorV1::ProductPersistenceConfiguration)?;
    let persistence = ProductionPersistenceDependenciesV1::new(
        snapshots,
        promotions,
        control,
        PostgresProductDeploymentStatuses::new(pools.deployment_status.clone()),
        PostgresProductDeploymentOperationalStatusesV2::new(
            pools.operational_deployment_status.clone(),
        ),
    );
    Ok(ProductionProductControlFacadeV1::new(
        identity,
        authority,
        persistence,
    ))
}

fn map_facade_configuration(
    _error: ProductionFacadeConfigurationErrorV1,
) -> ProductionCompositionErrorV1 {
    ProductionCompositionErrorV1::FacadeConfiguration
}

struct ConnectedDatabasePoolsV1 {
    oauth_flow_writer: PgPool,
    session_issuer: PgPool,
    session_api: PgPool,
    security_revoker: PgPool,
    installation_authority: PgPool,
    authorized_snapshot: PgPool,
    promotion: PgPool,
    decision_reader: PgPool,
    approval_executor: PgPool,
    rejection_executor: PgPool,
    apply_executor: PgPool,
    deployment_status: PgPool,
    operational_deployment_status: PgPool,
}

impl ConnectedDatabasePoolsV1 {
    async fn close(&self) -> Result<(), ProductionDatabasePoolShutdownErrorV1> {
        close_pool_refs_with_deadline(self.pools().map(Some)).await
    }

    fn pools(&self) -> [&PgPool; 13] {
        [
            &self.oauth_flow_writer,
            &self.session_issuer,
            &self.session_api,
            &self.security_revoker,
            &self.installation_authority,
            &self.authorized_snapshot,
            &self.promotion,
            &self.decision_reader,
            &self.approval_executor,
            &self.rejection_executor,
            &self.apply_executor,
            &self.deployment_status,
            &self.operational_deployment_status,
        ]
    }

    fn into_shutdown(self) -> ProductionDatabasePoolShutdownV1 {
        ProductionDatabasePoolShutdownV1 {
            pools: Arc::new([
                self.oauth_flow_writer,
                self.session_issuer,
                self.session_api,
                self.security_revoker,
                self.installation_authority,
                self.authorized_snapshot,
                self.promotion,
                self.decision_reader,
                self.approval_executor,
                self.rejection_executor,
                self.apply_executor,
                self.deployment_status,
                self.operational_deployment_status,
            ]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DatabasePoolConnectErrorV1 {
    Configuration,
    UnsafeTransport,
    Unavailable,
}

async fn connect_database_pools_v1(
    database_urls: [DatabaseUrlSecretV1; 13],
    config: PoolConfigV1,
) -> Result<ConnectedDatabasePoolsV1, ProductionCompositionErrorV1> {
    let [oauth_flow_writer, session_issuer, session_api, security_revoker, installation_authority, authorized_snapshot, promotion, decision_reader, approval_executor, rejection_executor, apply_executor, deployment_status, operational_deployment_status] =
        database_urls;
    let (
        oauth_flow_writer,
        session_issuer,
        session_api,
        security_revoker,
        installation_authority,
        authorized_snapshot,
        promotion,
        decision_reader,
        approval_executor,
        rejection_executor,
        apply_executor,
        deployment_status,
        operational_deployment_status,
    ) = tokio::join!(
        connect_pool_v1(oauth_flow_writer, config),
        connect_pool_v1(session_issuer, config),
        connect_pool_v1(session_api, config),
        connect_pool_v1(security_revoker, config),
        connect_pool_v1(installation_authority, config),
        connect_pool_v1(authorized_snapshot, config),
        connect_pool_v1(promotion, config),
        connect_pool_v1(decision_reader, config),
        connect_pool_v1(approval_executor, config),
        connect_pool_v1(rejection_executor, config),
        connect_pool_v1(apply_executor, config),
        connect_pool_v1(deployment_status, config),
        connect_pool_v1(operational_deployment_status, config),
    );
    let results = [
        &oauth_flow_writer,
        &session_issuer,
        &session_api,
        &security_revoker,
        &installation_authority,
        &authorized_snapshot,
        &promotion,
        &decision_reader,
        &approval_executor,
        &rejection_executor,
        &apply_executor,
        &deployment_status,
        &operational_deployment_status,
    ];
    if results.iter().any(|result| result.is_err()) {
        let error = first_database_error(results);
        let _shutdown_result =
            close_pool_refs_with_deadline(results.map(|result| result.as_ref().ok())).await;
        return Err(error);
    }
    Ok(ConnectedDatabasePoolsV1 {
        oauth_flow_writer: oauth_flow_writer.expect("database results were checked"),
        session_issuer: session_issuer.expect("database results were checked"),
        session_api: session_api.expect("database results were checked"),
        security_revoker: security_revoker.expect("database results were checked"),
        installation_authority: installation_authority.expect("database results were checked"),
        authorized_snapshot: authorized_snapshot.expect("database results were checked"),
        promotion: promotion.expect("database results were checked"),
        decision_reader: decision_reader.expect("database results were checked"),
        approval_executor: approval_executor.expect("database results were checked"),
        rejection_executor: rejection_executor.expect("database results were checked"),
        apply_executor: apply_executor.expect("database results were checked"),
        deployment_status: deployment_status.expect("database results were checked"),
        operational_deployment_status: operational_deployment_status
            .expect("database results were checked"),
    })
}

fn first_database_error<T>(
    results: [&Result<T, DatabasePoolConnectErrorV1>; 13],
) -> ProductionCompositionErrorV1 {
    DatabaseRoleV1::ALL
        .into_iter()
        .zip(results)
        .find_map(|(role, result)| {
            result
                .as_ref()
                .err()
                .copied()
                .map(|error| map_database_connect_error(role, error))
        })
        .expect("database results contain a checked failure")
}

fn map_database_connect_error(
    role: DatabaseRoleV1,
    error: DatabasePoolConnectErrorV1,
) -> ProductionCompositionErrorV1 {
    match error {
        DatabasePoolConnectErrorV1::Configuration => {
            ProductionCompositionErrorV1::DatabaseConfiguration { role }
        }
        DatabasePoolConnectErrorV1::UnsafeTransport => {
            ProductionCompositionErrorV1::UnsafeDatabaseTransport { role }
        }
        DatabasePoolConnectErrorV1::Unavailable => {
            ProductionCompositionErrorV1::DatabaseUnavailable { role }
        }
    }
}

async fn connect_pool_v1(
    database_url: DatabaseUrlSecretV1,
    config: PoolConfigV1,
) -> Result<PgPool, DatabasePoolConnectErrorV1> {
    let database_url = database_url.into_zeroizing();
    let options = PgConnectOptions::from_str(&database_url)
        .map_err(|_| DatabasePoolConnectErrorV1::Configuration)?;
    validate_database_transport_v1(&options)?;
    let options = options
        .application_name(APPLICATION_NAME)
        .disable_statement_logging();
    let pool = PgPoolOptions::new()
        .min_connections(0)
        .max_connections(config.max_connections())
        .acquire_timeout(config.acquire_timeout())
        .idle_timeout(config.idle_timeout())
        .max_lifetime(config.max_lifetime())
        .test_before_acquire(true);
    match timeout(config.acquire_timeout(), pool.connect_with(options)).await {
        Ok(Ok(pool)) => Ok(pool),
        Ok(Err(_)) | Err(_) => Err(DatabasePoolConnectErrorV1::Unavailable),
    }
}

fn validate_database_transport_v1(
    options: &PgConnectOptions,
) -> Result<(), DatabasePoolConnectErrorV1> {
    if options.get_options().is_some() {
        return Err(DatabasePoolConnectErrorV1::Configuration);
    }
    let local = options.get_socket().is_some() || database_host_is_loopback(options.get_host());
    if !local && !matches!(options.get_ssl_mode(), PgSslMode::VerifyFull) {
        return Err(DatabasePoolConnectErrorV1::UnsafeTransport);
    }
    Ok(())
}

async fn close_pool_refs_with_deadline(
    pools: [Option<&PgPool>; 13],
) -> Result<(), ProductionDatabasePoolShutdownErrorV1> {
    let close = begin_pool_closures(pools);
    await_pool_shutdown_with_timeout(close, DATABASE_POOL_SHUTDOWN_TIMEOUT).await
}

fn begin_pool_closures<'a>(pools: [Option<&'a PgPool>; 13]) -> impl Future<Output = ()> + 'a {
    let [oauth_flow_writer, session_issuer, session_api, security_revoker, installation_authority, authorized_snapshot, promotion, decision_reader, approval_executor, rejection_executor, apply_executor, deployment_status, operational_deployment_status] =
        pools;
    let oauth_flow_writer = oauth_flow_writer.map(|pool| pool.close());
    let session_issuer = session_issuer.map(|pool| pool.close());
    let session_api = session_api.map(|pool| pool.close());
    let security_revoker = security_revoker.map(|pool| pool.close());
    let installation_authority = installation_authority.map(|pool| pool.close());
    let authorized_snapshot = authorized_snapshot.map(|pool| pool.close());
    let promotion = promotion.map(|pool| pool.close());
    let decision_reader = decision_reader.map(|pool| pool.close());
    let approval_executor = approval_executor.map(|pool| pool.close());
    let rejection_executor = rejection_executor.map(|pool| pool.close());
    let apply_executor = apply_executor.map(|pool| pool.close());
    let deployment_status = deployment_status.map(|pool| pool.close());
    let operational_deployment_status = operational_deployment_status.map(|pool| pool.close());
    async move {
        tokio::join!(
            await_optional_pool_close(oauth_flow_writer),
            await_optional_pool_close(session_issuer),
            await_optional_pool_close(session_api),
            await_optional_pool_close(security_revoker),
            await_optional_pool_close(installation_authority),
            await_optional_pool_close(authorized_snapshot),
            await_optional_pool_close(promotion),
            await_optional_pool_close(decision_reader),
            await_optional_pool_close(approval_executor),
            await_optional_pool_close(rejection_executor),
            await_optional_pool_close(apply_executor),
            await_optional_pool_close(deployment_status),
            await_optional_pool_close(operational_deployment_status),
        );
    }
}

async fn await_optional_pool_close<F>(close: Option<F>)
where
    F: Future<Output = ()>,
{
    if let Some(close) = close {
        close.await;
    }
}

async fn await_pool_shutdown_with_timeout<F>(
    close: F,
    deadline: Duration,
) -> Result<(), ProductionDatabasePoolShutdownErrorV1>
where
    F: Future<Output = ()>,
{
    timeout(deadline, close)
        .await
        .map_err(|_| ProductionDatabasePoolShutdownErrorV1::TimedOut)
}

fn database_host_is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_transport_requires_authenticated_remote_tls() {
        let insecure_remote = PgConnectOptions::new()
            .host("database.example")
            .ssl_mode(PgSslMode::Require);
        let authenticated_remote = PgConnectOptions::new()
            .host("database.example")
            .ssl_mode(PgSslMode::VerifyFull);
        assert_eq!(
            validate_database_transport_v1(&insecure_remote),
            Err(DatabasePoolConnectErrorV1::UnsafeTransport)
        );
        assert_eq!(
            validate_database_transport_v1(&authenticated_remote),
            Ok(())
        );
    }

    #[test]
    fn database_transport_allows_loopback_and_socket_connections() {
        let loopback = PgConnectOptions::new()
            .host("127.0.0.1")
            .ssl_mode(PgSslMode::Disable);
        let socket = PgConnectOptions::new()
            .socket("/private/tmp")
            .ssl_mode(PgSslMode::Disable);
        assert_eq!(validate_database_transport_v1(&loopback), Ok(()));
        assert_eq!(validate_database_transport_v1(&socket), Ok(()));
    }

    #[test]
    fn database_transport_rejects_arbitrary_startup_options() {
        let options = PgConnectOptions::new()
            .host("127.0.0.1")
            .options([("search_path", "attacker")]);
        assert_eq!(
            validate_database_transport_v1(&options),
            Err(DatabasePoolConnectErrorV1::Configuration)
        );
    }

    #[tokio::test]
    async fn shutdown_is_eagerly_closed_idempotent_and_secret_redacted() {
        assert_eq!(
            ProductionCompositionErrorV1::DatabaseUnavailable {
                role: DatabaseRoleV1::ApplyExecutor,
            }
            .to_string(),
            "database role ApplyExecutor connection is unavailable"
        );
        let database_url = format!("postgresql:{}{}opaque", "/", "/");
        let shutdown = ProductionDatabasePoolShutdownV1 {
            pools: Arc::new(std::array::from_fn(|_| {
                PgPoolOptions::new()
                    .connect_lazy(&database_url)
                    .expect("test URL is structurally valid")
            })),
        };
        assert_eq!(
            format!("{shutdown:?}"),
            "ProductionDatabasePoolShutdownV1(<redacted>)"
        );
        let close = begin_pool_closures(shutdown.pools.each_ref().map(Some));
        assert!(shutdown.is_closed());
        close.await;
        assert_eq!(shutdown.close().await, Ok(()));
        assert_eq!(shutdown.close().await, Ok(()));
    }

    #[tokio::test]
    async fn shutdown_timeout_is_typed_and_redacted() {
        let result = await_pool_shutdown_with_timeout(
            std::future::pending::<()>(),
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(result, Err(ProductionDatabasePoolShutdownErrorV1::TimedOut));
        assert_eq!(
            format!("{:?}", ProductionDatabasePoolShutdownErrorV1::TimedOut),
            "ProductionDatabasePoolShutdownErrorV1(<redacted>)"
        );
    }

    #[test]
    fn first_database_failure_preserves_exact_role_and_array_order() {
        let mut results: [Result<(), DatabasePoolConnectErrorV1>; 13] =
            std::array::from_fn(|_| Ok(()));
        results[DatabaseRoleV1::DecisionReader.index()] =
            Err(DatabasePoolConnectErrorV1::Unavailable);
        results[DatabaseRoleV1::ApplyExecutor.index()] =
            Err(DatabasePoolConnectErrorV1::Configuration);
        let references = std::array::from_fn(|index| &results[index]);
        assert_eq!(
            first_database_error(references),
            ProductionCompositionErrorV1::DatabaseUnavailable {
                role: DatabaseRoleV1::DecisionReader,
            }
        );
        assert_eq!(
            ProductionCompositionErrorV1::ReadinessFailed {
                phase: ProductionReadinessPhaseV1::Aggregate,
            }
            .to_string(),
            "the production dependency readiness phase Aggregate failed"
        );
    }
}
