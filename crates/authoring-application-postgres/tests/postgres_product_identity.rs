use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application::{
    AuthenticationBackendFailureV1, AuthenticationError, AuthenticationPort,
};
use authoring_application_postgres::{
    digest_opaque_session_credential_v1, PostgresAuthentication, PostgresAuthenticationConfig,
    PostgresProductIdentityConfig, PostgresProductIdentityStore, ProductDatabaseFailureV1,
    ProductIdentityError, ProductIdentityLifetimesV1, ProductLogoutDispositionV1,
    ProductSecretGenerator, ProductSecretGeneratorError, ProductSessionRevocationReasonV1,
    MIGRATOR,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use discord_model::UserId;
use futures::join;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::types::Json;
use sqlx::Connection;

const REDIRECT_URI: &str = "https://starring.example/oauth/discord/callback";

#[derive(Clone)]
struct DeterministicGenerator {
    counter: Arc<AtomicU64>,
}

impl DeterministicGenerator {
    fn new(seed: u64) -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(seed)),
        }
    }
}

impl ProductSecretGenerator for DeterministicGenerator {
    fn fill_secret(&self, destination: &mut [u8; 32]) -> Result<(), ProductSecretGeneratorError> {
        let value = self.counter.fetch_add(1, Ordering::SeqCst);
        for (index, chunk) in destination.chunks_exact_mut(8).enumerate() {
            let part = value.wrapping_add(u64::try_from(index).unwrap());
            chunk.copy_from_slice(&part.to_be_bytes());
        }
        Ok(())
    }
}

fn assert_test_database_name(database_name: &str) {
    assert!(
        database_name.starts_with("starring_")
            && database_name.split('_').any(|segment| segment == "test")
            && database_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to use a database outside the strict Starring test namespace"
    );
}

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let options = url
        .parse::<PgConnectOptions>()
        .unwrap_or_else(|_| panic!("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL"));
    let database_name = options
        .get_database()
        .unwrap_or_else(|| panic!("STARRING_TEST_DATABASE_URL must name a database"));
    assert_test_database_name(database_name);
    url
}

async fn pool() -> PgPool {
    let url = database_url();
    let expected_database = url
        .parse::<PgConnectOptions>()
        .unwrap()
        .get_database()
        .unwrap()
        .to_string();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .expect("connect");
    let current_database = sqlx::query_scalar::<_, String>("SELECT pg_catalog.current_database()")
        .fetch_one(&pool)
        .await
        .expect("read current test database");
    assert_test_database_name(&current_database);
    assert_eq!(current_database, expected_database);
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

async fn shadow_search_path_pool(setup_pool: &PgPool) -> PgPool {
    sqlx::query("CREATE SCHEMA IF NOT EXISTS authoring_identity_shadow")
        .execute(setup_pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION authoring_identity_shadow.clock_timestamp() \
         RETURNS TIMESTAMPTZ LANGUAGE SQL IMMUTABLE SET search_path = pg_catalog \
         AS 'SELECT ''2000-01-01T00:00:00Z''::TIMESTAMPTZ'",
    )
    .execute(setup_pool)
    .await
    .unwrap();
    PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET search_path = authoring_identity_shadow, pg_catalog")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&database_url())
        .await
        .unwrap()
}

fn production_store(
    pool: PgPool,
    seed: u64,
) -> PostgresProductIdentityStore<DeterministicGenerator> {
    let config = PostgresProductIdentityConfig::production(
        REDIRECT_URI,
        ["/".to_string(), "/app".to_string()],
    )
    .unwrap();
    PostgresProductIdentityStore::new(pool, DeterministicGenerator::new(seed), config)
}

fn configurable_store(
    pool: PgPool,
    seed: u64,
    redirect_uri: &str,
    allowed_return_paths: impl IntoIterator<Item = String>,
    lifetimes: ProductIdentityLifetimesV1,
) -> PostgresProductIdentityStore<DeterministicGenerator> {
    let config =
        PostgresProductIdentityConfig::new(redirect_uri, allowed_return_paths, lifetimes).unwrap();
    PostgresProductIdentityStore::new(pool, DeterministicGenerator::new(seed), config)
}

fn unique_user_id() -> UserId {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let value = u64::try_from(value).unwrap();
    UserId(value)
}

fn unrelated_secret(seed: u8) -> String {
    URL_SAFE_NO_PAD.encode([seed; 32])
}

fn credential_from_seed(seed: u64, domain: u64) -> String {
    let mut bytes = [0_u8; 32];
    for (index, chunk) in bytes.chunks_exact_mut(8).enumerate() {
        chunk.copy_from_slice(
            &seed
                .wrapping_add(domain)
                .wrapping_add(u64::try_from(index).unwrap())
                .to_be_bytes(),
        );
    }
    URL_SAFE_NO_PAD.encode(bytes)
}

struct DirectSessionFixture {
    user_id: UserId,
    principal_id: String,
    session: String,
    csrf: String,
    oauth_state_digest: [u8; 32],
}

async fn insert_direct_product_session(
    pool: &PgPool,
    store: &PostgresProductIdentityStore<DeterministicGenerator>,
    return_path: &str,
    display_name: &str,
    idle_seconds: f64,
    absolute_seconds: f64,
) -> DirectSessionFixture {
    let flow = store.create_oauth_flow(return_path).await.unwrap();
    let oauth_state_digest = digest_opaque_session_credential_v1(flow.state().expose_secret())
        .unwrap()
        .into_bytes();
    store
        .consume_oauth_flow(
            flow.state().expose_secret(),
            flow.browser_nonce().expose_secret(),
        )
        .await
        .unwrap();
    let user_id = unique_user_id();
    let principal_id = format!("discord:{user_id}");
    let session = credential_from_seed(user_id.0, 41);
    let csrf = credential_from_seed(user_id.0, 73);
    let session_digest = digest_opaque_session_credential_v1(&session).unwrap();
    let csrf_digest = digest_opaque_session_credential_v1(&csrf).unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) VALUES ($1, $2, $3)",
    )
    .bind(&principal_id)
    .bind(user_id.to_string())
    .bind(Json(serde_json::json!({"display_name": display_name})))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "WITH session_clock AS MATERIALIZED ( \
          SELECT pg_catalog.clock_timestamp() AS issued_at \
         ) \
         INSERT INTO public.product_auth_sessions \
         (session_digest, principal_id, csrf_digest, oauth_state_digest, \
          authenticated_at, created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
         SELECT $1, $2, $3, $4, issued_at, issued_at, issued_at, \
          issued_at + pg_catalog.make_interval(secs => $5::DOUBLE PRECISION), \
          issued_at + pg_catalog.make_interval(secs => $6::DOUBLE PRECISION) \
         FROM session_clock",
    )
    .bind(session_digest.as_bytes().as_slice())
    .bind(&principal_id)
    .bind(csrf_digest.as_bytes().as_slice())
    .bind(oauth_state_digest.as_slice())
    .bind(idle_seconds)
    .bind(absolute_seconds)
    .execute(pool)
    .await
    .unwrap();
    DirectSessionFixture {
        user_id,
        principal_id,
        session,
        csrf,
        oauth_state_digest,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn migration_retires_legacy_sessions_for_a_coordinated_cutover() {
    let database_name = format!(
        "starring_identity_rolling_test_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let base_options = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator = PgConnection::connect_with(&base_options.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {database_name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let rolling_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(base_options.database(&database_name))
        .await;
    let rolling_pool = match rolling_pool {
        Ok(pool) => pool,
        Err(_) => {
            sqlx::query(&format!("DROP DATABASE {database_name} WITH (FORCE)"))
                .execute(&mut administrator)
                .await
                .unwrap();
            panic!("connect to generated rolling test database failed");
        }
    };
    let outcome = async {
        let current_database =
            sqlx::query_scalar::<_, String>("SELECT pg_catalog.current_database()")
                .fetch_one(&rolling_pool)
                .await?;
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_001)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&rolling_pool)
                .await?;
        }
        sqlx::query(
            "INSERT INTO public.product_principals \
             (principal_id, discord_user_id, display_profile) \
             VALUES ('legacy-principal', '18446744073709550002', \
              '{\"display_name\":\"Legacy\"}'::JSONB)",
        )
        .execute(&rolling_pool)
        .await?;
        sqlx::query(
            "INSERT INTO public.product_oauth_flows \
             (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, expires_at) \
             VALUES ($1, $2, $3, '/', CURRENT_TIMESTAMP, \
              CURRENT_TIMESTAMP + INTERVAL '1 hour')",
        )
        .bind([31_u8; 32].as_slice())
        .bind([32_u8; 32].as_slice())
        .bind(REDIRECT_URI)
        .execute(&rolling_pool)
        .await?;
        sqlx::query(
            "INSERT INTO public.product_auth_sessions \
             (session_digest, principal_id, csrf_digest, authenticated_at, created_at, \
              last_seen_at, idle_expires_at, absolute_expires_at) \
             VALUES ($1, 'legacy-principal', $2, CURRENT_TIMESTAMP - INTERVAL '1 hour', \
              CURRENT_TIMESTAMP - INTERVAL '1 hour', CURRENT_TIMESTAMP, \
              CURRENT_TIMESTAMP + INTERVAL '2 hours', CURRENT_TIMESTAMP + INTERVAL '24 hours')",
        )
        .bind([33_u8; 32].as_slice())
        .bind([34_u8; 32].as_slice())
        .execute(&rolling_pool)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../../migrations/202607190002_create_runtime_convergence.sql"
        ))
        .execute(&rolling_pool)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../../migrations/202607190003_bind_product_sessions_to_oauth_flows.sql"
        ))
        .execute(&rolling_pool)
        .await?;
        let legacy_session = sqlx::query_as::<_, (bool, bool, Option<String>)>(
            "SELECT oauth_state_digest IS NULL, revoked_at IS NOT NULL, revocation_reason \
             FROM public.product_auth_sessions WHERE principal_id = 'legacy-principal'",
        )
        .fetch_one(&rolling_pool)
        .await?;
        let legacy_flow_eligible = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
              SELECT 1 FROM public.product_oauth_flows \
              WHERE state_digest = $1 \
              AND expires_at <= created_at + INTERVAL '10 minutes' \
             )",
        )
        .bind([31_u8; 32].as_slice())
        .fetch_one(&rolling_pool)
        .await?;
        let unbound_insert = sqlx::query(
            "INSERT INTO public.product_auth_sessions \
             (session_digest, principal_id, csrf_digest, authenticated_at, created_at, \
              last_seen_at, idle_expires_at, absolute_expires_at) \
             VALUES ($1, 'legacy-principal', $2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, \
              CURRENT_TIMESTAMP, CURRENT_TIMESTAMP + INTERVAL '20 minutes', \
              CURRENT_TIMESTAMP + INTERVAL '2 hours')",
        )
        .bind([35_u8; 32].as_slice())
        .bind([36_u8; 32].as_slice())
        .execute(&rolling_pool)
        .await;
        let unbound_insert_rejected = matches!(
            unbound_insert,
            Err(sqlx::Error::Database(database))
                if database.constraint() == Some("product_auth_sessions_oauth_binding_valid")
        );
        let oversized_flow_insert = sqlx::query(
            "INSERT INTO public.product_oauth_flows \
             (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, expires_at) \
             VALUES ($1, $2, $3, '/', CURRENT_TIMESTAMP, \
              CURRENT_TIMESTAMP + INTERVAL '11 minutes')",
        )
        .bind([37_u8; 32].as_slice())
        .bind([38_u8; 32].as_slice())
        .bind(REDIRECT_URI)
        .execute(&rolling_pool)
        .await;
        let oversized_flow_rejected = matches!(
            oversized_flow_insert,
            Err(sqlx::Error::Database(database))
                if database.constraint() == Some("product_oauth_flows_lifetime_bounded")
        );
        Ok::<_, sqlx::Error>((
            current_database,
            legacy_session,
            legacy_flow_eligible,
            unbound_insert_rejected,
            oversized_flow_rejected,
        ))
    }
    .await;
    rolling_pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database_name} WITH (FORCE)"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let (
        current_database,
        legacy_session,
        legacy_flow_eligible,
        unbound_insert_rejected,
        oversized_flow_rejected,
    ) = outcome.unwrap();
    assert_test_database_name(&current_database);
    assert_eq!(current_database, database_name);
    assert_eq!(
        legacy_session,
        (true, true, Some("oauth_rebinding_required".to_string()))
    );
    assert!(!legacy_flow_eligible);
    assert!(unbound_insert_rejected);
    assert!(oversized_flow_rejected);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_identity_and_authentication_ignore_a_shadow_search_path() {
    let setup_pool = pool().await;
    let shadow_pool = shadow_search_path_pool(&setup_pool).await;
    let store = production_store(shadow_pool, unique_user_id().0);
    let fixture = insert_direct_product_session(
        &setup_pool,
        &store,
        "/app",
        "Shadow Safe Principal",
        1_800.0,
        43_200.0,
    )
    .await;
    let current = store
        .current_principal(&fixture.session, &fixture.csrf)
        .await
        .unwrap();
    assert_eq!(
        current.session_fingerprint(),
        &digest_opaque_session_credential_v1(&fixture.session).unwrap()
    );
    store
        .authentication()
        .authenticate(&fixture.session)
        .await
        .unwrap();
    assert_eq!(
        store.logout(&fixture.session, &fixture.csrf).await.unwrap(),
        ProductLogoutDispositionV1::Revoked
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn postgres_oauth_flow_persists_only_digests_and_has_one_race_winner() {
    let pool = pool().await;
    let oauth_binding_search_path = sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(array_to_string(routine.proconfig, ','), '') \
         FROM pg_proc AS routine \
         INNER JOIN pg_namespace AS namespace ON namespace.oid = routine.pronamespace \
         WHERE namespace.nspname = 'public' \
         AND routine.proname = 'enforce_product_auth_session_oauth_binding'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(oauth_binding_search_path, "search_path=pg_catalog");
    let fixed_identity_transition_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pg_proc AS routine \
         INNER JOIN pg_namespace AS namespace ON namespace.oid = routine.pronamespace \
         WHERE namespace.nspname = 'public' \
         AND routine.proname = ANY($1::TEXT[]) \
         AND routine.proconfig @> ARRAY['search_path=pg_catalog']::TEXT[]",
    )
    .bind([
        "enforce_product_principal_transition",
        "enforce_product_oauth_flow_transition",
        "enforce_product_auth_session_transition",
    ])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(fixed_identity_transition_count, 3);
    let oauth_binding_column_not_null = sqlx::query_scalar::<_, bool>(
        "SELECT attribute.attnotnull FROM pg_attribute AS attribute \
         INNER JOIN pg_class AS relation ON relation.oid = attribute.attrelid \
         INNER JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname = 'public' AND relation.relname = 'product_auth_sessions' \
         AND attribute.attname = 'oauth_state_digest' AND NOT attribute.attisdropped",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!oauth_binding_column_not_null);
    let oauth_binding_presence_validated = sqlx::query_scalar::<_, bool>(
        "SELECT constraint_record.convalidated FROM pg_constraint AS constraint_record \
         INNER JOIN pg_class AS relation ON relation.oid = constraint_record.conrelid \
         INNER JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname = 'public' AND relation.relname = 'product_auth_sessions' \
         AND constraint_record.conname = 'product_auth_sessions_oauth_binding_presence'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(oauth_binding_presence_validated);
    let active_unbound_sessions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.product_auth_sessions \
         WHERE oauth_state_digest IS NULL AND revoked_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_unbound_sessions, 0);
    let store = production_store(pool.clone(), unique_user_id().0);
    let flow = store.create_oauth_flow("/app").await.unwrap();
    let state = flow.state().expose_secret().to_string();
    let nonce = flow.browser_nonce().expose_secret().to_string();
    assert_eq!(state.len(), 43);
    assert_eq!(nonce.len(), 43);
    assert_ne!(state, nonce);
    assert!(!format!("{flow:?}").contains(&state));
    assert!(!format!("{flow:?}").contains(&nonce));
    assert_eq!(flow.redirect_uri(), REDIRECT_URI);
    assert_eq!(flow.return_path(), "/app");
    assert!((1..=600).contains(&flow.max_age_seconds()));

    let state_digest = digest_opaque_session_credential_v1(&state).unwrap();
    let nonce_digest = digest_opaque_session_credential_v1(&nonce).unwrap();
    let persisted = sqlx::query_as::<_, (Vec<u8>, Vec<u8>, String, String)>(
        "SELECT state_digest, browser_nonce_digest, redirect_uri, return_path \
         FROM product_oauth_flows WHERE state_digest = $1",
    )
    .bind(state_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, state_digest.as_bytes());
    assert_eq!(persisted.1, nonce_digest.as_bytes());
    assert_ne!(persisted.0, state.as_bytes());
    assert_ne!(persisted.1, nonce.as_bytes());
    assert_eq!(persisted.2, REDIRECT_URI);
    assert_eq!(persisted.3, "/app");

    let wrong_redirect = configurable_store(
        pool.clone(),
        unique_user_id().0,
        "https://starring.example/oauth/discord/other-callback",
        ["/app".to_string()],
        ProductIdentityLifetimesV1::production(),
    );
    assert!(matches!(
        wrong_redirect.consume_oauth_flow(&state, &nonce).await,
        Err(authoring_application_postgres::OAuthFlowError::InvalidOrConsumed)
    ));
    assert!(matches!(
        store
            .consume_oauth_flow(&state, &unrelated_secret(201))
            .await,
        Err(authoring_application_postgres::OAuthFlowError::InvalidOrConsumed)
    ));

    let (left, right) = join!(
        store.consume_oauth_flow(&state, &nonce),
        store.consume_oauth_flow(&state, &nonce)
    );
    let _consumed = match (left, right) {
        (Ok(consumed), Err(_)) | (Err(_), Ok(consumed)) => consumed,
        _ => panic!("exactly one OAuth claim must win"),
    };
    assert!(matches!(
        store.consume_oauth_flow(&state, &nonce).await,
        Err(authoring_application_postgres::OAuthFlowError::InvalidOrConsumed)
    ));
    let terminal = sqlx::query_as::<_, (bool, Option<String>)>(
        "SELECT consumed_at IS NOT NULL, terminal_result_code \
         FROM product_oauth_flows WHERE state_digest = $1",
    )
    .bind(state_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(terminal, (true, Some("callback_claimed".to_string())));

    let linked_user_id = unique_user_id();
    let linked_principal_id = format!("oauth-link-{linked_user_id}");
    sqlx::query(
        "INSERT INTO product_principals \
         (principal_id, discord_user_id, display_profile) \
         VALUES ($1, $2, '{\"display_name\":\"Linked\"}'::JSONB)",
    )
    .bind(&linked_principal_id)
    .bind(linked_user_id.to_string())
    .execute(&pool)
    .await
    .unwrap();
    let linked_session = digest_opaque_session_credential_v1(&unrelated_secret(203)).unwrap();
    let linked_csrf = digest_opaque_session_credential_v1(&unrelated_secret(204)).unwrap();
    let unbound_error = sqlx::query(
        "INSERT INTO product_auth_sessions \
         (session_digest, principal_id, csrf_digest, authenticated_at, created_at, \
          last_seen_at, idle_expires_at, absolute_expires_at) \
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, \
          CURRENT_TIMESTAMP + INTERVAL '20 minutes', CURRENT_TIMESTAMP + INTERVAL '2 hours')",
    )
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(205))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .bind(&linked_principal_id)
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(206))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(matches!(
        unbound_error,
        sqlx::Error::Database(database)
            if database.constraint() == Some("product_auth_sessions_oauth_binding_valid")
    ));
    let causal_error = sqlx::query(
        "INSERT INTO product_auth_sessions \
         (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
          created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
         VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP - INTERVAL '1 hour', \
          CURRENT_TIMESTAMP - INTERVAL '1 hour', CURRENT_TIMESTAMP - INTERVAL '1 hour', \
          CURRENT_TIMESTAMP - INTERVAL '40 minutes', CURRENT_TIMESTAMP + INTERVAL '1 hour')",
    )
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(207))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .bind(&linked_principal_id)
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(208))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .bind(state_digest.as_bytes().as_slice())
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(matches!(
        causal_error,
        sqlx::Error::Database(database)
            if database.constraint() == Some("product_auth_sessions_oauth_binding_valid")
    ));
    let idle_cap_error = sqlx::query(
        "INSERT INTO product_auth_sessions \
         (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
          created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
         VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, \
          CURRENT_TIMESTAMP + INTERVAL '31 minutes', CURRENT_TIMESTAMP + INTERVAL '2 hours')",
    )
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(209))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .bind(&linked_principal_id)
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(210))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .bind(state_digest.as_bytes().as_slice())
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(matches!(
        idle_cap_error,
        sqlx::Error::Database(database)
            if database.constraint() == Some("product_auth_sessions_initial_activity_valid")
    ));
    let future_activity_error = sqlx::query(
        "INSERT INTO product_auth_sessions \
         (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
          created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
         VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, \
          CURRENT_TIMESTAMP + INTERVAL '1 hour', CURRENT_TIMESTAMP + INTERVAL '80 minutes', \
          CURRENT_TIMESTAMP + INTERVAL '2 hours')",
    )
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(211))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .bind(&linked_principal_id)
    .bind(
        digest_opaque_session_credential_v1(&unrelated_secret(212))
            .unwrap()
            .as_bytes()
            .as_slice(),
    )
    .bind(state_digest.as_bytes().as_slice())
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(matches!(
        future_activity_error,
        sqlx::Error::Database(database)
            if database.constraint() == Some("product_auth_sessions_initial_activity_valid")
    ));
    sqlx::query(
        "INSERT INTO product_auth_sessions \
         (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
          created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
         VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, \
          CURRENT_TIMESTAMP + INTERVAL '20 minutes', CURRENT_TIMESTAMP + INTERVAL '2 hours')",
    )
    .bind(linked_session.as_bytes().as_slice())
    .bind(&linked_principal_id)
    .bind(linked_csrf.as_bytes().as_slice())
    .bind(state_digest.as_bytes().as_slice())
    .execute(&pool)
    .await
    .unwrap();
    let future_touch_error = sqlx::query(
        "UPDATE product_auth_sessions \
         SET last_seen_at = CURRENT_TIMESTAMP + INTERVAL '1 hour', \
          idle_expires_at = CURRENT_TIMESTAMP + INTERVAL '80 minutes' \
         WHERE session_digest = $1",
    )
    .bind(linked_session.as_bytes().as_slice())
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(matches!(
        future_touch_error,
        sqlx::Error::Database(database)
            if database.constraint() == Some("product_auth_sessions_transition_valid")
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn postgres_session_issuance_projects_touches_verifies_and_revokes() {
    let pool = pool().await;
    let lifetimes = ProductIdentityLifetimesV1::new(
        Duration::from_secs(60),
        Duration::from_secs(60),
        Duration::from_secs(300),
        Duration::from_millis(1),
        Duration::from_millis(250),
    )
    .unwrap();
    let store = configurable_store(
        pool.clone(),
        unique_user_id().0,
        REDIRECT_URI,
        ["/app".to_string()],
        lifetimes,
    );
    let fixture =
        insert_direct_product_session(&pool, &store, "/app", "Product Owner", 60.0, 300.0).await;
    let user_id = fixture.user_id;
    let session = fixture.session;
    let csrf = fixture.csrf;
    assert_ne!(session, csrf);

    let session_digest = digest_opaque_session_credential_v1(&session).unwrap();
    let csrf_digest = digest_opaque_session_credential_v1(&csrf).unwrap();
    let persisted = sqlx::query_as::<_, (Vec<u8>, Vec<u8>, String, Vec<u8>)>(
        "SELECT session_digest, csrf_digest, principal_id, oauth_state_digest \
         FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, session_digest.as_bytes());
    assert_eq!(persisted.1, csrf_digest.as_bytes());
    assert_ne!(persisted.0, session.as_bytes());
    assert_ne!(persisted.1, csrf.as_bytes());
    assert_ne!(persisted.0, persisted.1);
    assert_eq!(persisted.2, fixture.principal_id);
    assert_eq!(persisted.3, fixture.oauth_state_digest);

    let mut shared_reader = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT session_digest FROM product_auth_sessions WHERE session_digest = $1 FOR SHARE",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&mut *shared_reader)
    .await
    .unwrap();
    let concurrent_reader = PostgresAuthentication::with_config(
        pool.clone(),
        PostgresAuthenticationConfig::new(
            Duration::from_secs(60),
            Duration::from_secs(30),
            Duration::from_millis(250),
        )
        .unwrap(),
    );
    tokio::time::timeout(
        Duration::from_secs(1),
        concurrent_reader.authenticate(&session),
    )
    .await
    .unwrap()
    .unwrap();
    shared_reader.rollback().await.unwrap();

    let before = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
        "SELECT last_seen_at FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("SELECT pg_sleep(0.01)")
        .execute(&pool)
        .await
        .unwrap();
    let current = store.current_principal(&session, &csrf).await.unwrap();
    assert!(!format!("{current:?}").contains("Product Owner"));
    assert!(!format!("{current:?}").contains(&user_id.to_string()));
    assert_eq!(
        current.principal_id().as_str(),
        format!("discord:{user_id}")
    );
    let after = sqlx::query_as::<
        _,
        (
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
            bool,
        ),
    >(
        "SELECT last_seen_at, idle_expires_at, absolute_expires_at, \
          idle_expires_at <= last_seen_at + INTERVAL '30 minutes' \
         FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(after.0 > before);
    assert!(after.0 < after.1);
    assert!(after.1 <= after.2);
    assert!(after.3);

    assert_eq!(store.verify_csrf(&session, &csrf).await.unwrap(), current);
    let last_seen_before_invalid_csrf = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
        "SELECT last_seen_at FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(matches!(
        store.verify_csrf(&session, &unrelated_secret(202)).await,
        Err(ProductIdentityError::InvalidCsrf)
    ));
    let last_seen_after_invalid_csrf = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
        "SELECT last_seen_at FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(last_seen_after_invalid_csrf, last_seen_before_invalid_csrf);
    let authenticated = store
        .authentication()
        .authenticate(session.as_str())
        .await
        .unwrap();
    assert!(!format!("{authenticated:?}").contains(&format!("discord:{user_id}")));

    let mut exclusive_writer = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT session_digest FROM product_auth_sessions WHERE session_digest = $1 FOR UPDATE",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&mut *exclusive_writer)
    .await
    .unwrap();
    assert_eq!(
        store.current_principal(&session, &csrf).await,
        Err(ProductIdentityError::Database(
            ProductDatabaseFailureV1::Timeout
        ))
    );
    assert_eq!(
        store.authentication().authenticate(&session).await,
        Err(AuthenticationError::Backend(
            AuthenticationBackendFailureV1::Timeout
        ))
    );
    exclusive_writer.rollback().await.unwrap();

    assert_eq!(
        store.logout(&session, &unrelated_secret(202)).await,
        Err(ProductIdentityError::InvalidCsrf)
    );
    let (left, right) = join!(store.logout(&session, &csrf), store.logout(&session, &csrf));
    assert!(matches!(
        (left, right),
        (
            Ok(ProductLogoutDispositionV1::Revoked),
            Ok(ProductLogoutDispositionV1::ExactReplay)
        ) | (
            Ok(ProductLogoutDispositionV1::ExactReplay),
            Ok(ProductLogoutDispositionV1::Revoked)
        )
    ));
    assert!(matches!(
        store.current_principal(&session, &csrf).await,
        Err(ProductIdentityError::Revoked)
    ));
    assert!(matches!(
        store.authentication().authenticate(session.as_str()).await,
        Err(AuthenticationError::Revoked)
    ));
    let revocation_reason = sqlx::query_scalar::<_, String>(
        "SELECT revocation_reason FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revocation_reason, "user_logout");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn postgres_principal_is_canonical_revised_and_disabled_fail_closed() {
    let pool = pool().await;
    let store = production_store(pool.clone(), unique_user_id().0);
    let fixture =
        insert_direct_product_session(&pool, &store, "/", "First Name", 1_800.0, 43_200.0).await;
    sqlx::query(
        "WITH principal_clock AS MATERIALIZED ( \
          SELECT GREATEST(pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
           AS changed_at FROM public.product_principals WHERE principal_id = $1 \
         ) \
         UPDATE public.product_principals SET \
         identity_revision = identity_revision + 1, \
         display_profile = '{\"display_name\":\"Second Name\"}'::JSONB, \
         last_authenticated_at = principal_clock.changed_at, \
         updated_at = principal_clock.changed_at \
         FROM principal_clock WHERE principal_id = $1",
    )
    .bind(&fixture.principal_id)
    .execute(&pool)
    .await
    .unwrap();
    let current_first = store
        .current_principal(&fixture.session, &fixture.csrf)
        .await
        .unwrap();
    assert_eq!(current_first.identity_revision(), 2);
    assert_eq!(current_first.display_name(), "Second Name");

    sqlx::query(
        "UPDATE product_principals SET disabled = TRUE, \
         identity_revision = identity_revision + 1, \
         updated_at = GREATEST(clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
         WHERE principal_id = $1",
    )
    .bind(&fixture.principal_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .current_principal(&fixture.session, &fixture.csrf)
            .await,
        Err(ProductIdentityError::InvalidCredential)
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn postgres_database_time_expires_oauth_and_idle_sessions() {
    let pool = pool().await;
    let flow_lifetimes = ProductIdentityLifetimesV1::new(
        Duration::from_secs(1),
        Duration::from_secs(30),
        Duration::from_secs(60),
        Duration::from_secs(5),
        Duration::from_secs(2),
    )
    .unwrap();
    let flow_store = configurable_store(
        pool.clone(),
        unique_user_id().0,
        REDIRECT_URI,
        ["/".to_string()],
        flow_lifetimes,
    );
    let blocked_flow = flow_store.create_oauth_flow("/").await.unwrap();
    let blocked_state = blocked_flow.state().expose_secret().to_string();
    let blocked_nonce = blocked_flow.browser_nonce().expose_secret().to_string();
    let blocked_state_digest = digest_opaque_session_credential_v1(&blocked_state).unwrap();
    let mut flow_blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT state_digest FROM product_oauth_flows WHERE state_digest = $1 FOR UPDATE")
        .bind(blocked_state_digest.as_bytes().as_slice())
        .fetch_one(&mut *flow_blocker)
        .await
        .unwrap();
    let blocked_store = flow_store.clone();
    let blocked_consume = tokio::spawn(async move {
        blocked_store
            .consume_oauth_flow(&blocked_state, &blocked_nonce)
            .await
    });
    sqlx::query("SELECT pg_sleep(1.05)")
        .execute(&pool)
        .await
        .unwrap();
    flow_blocker.commit().await.unwrap();
    assert!(matches!(
        blocked_consume.await.unwrap(),
        Err(authoring_application_postgres::OAuthFlowError::InvalidOrConsumed)
    ));
    let session_lifetimes = ProductIdentityLifetimesV1::new(
        Duration::from_secs(60),
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_millis(100),
        Duration::from_secs(2),
    )
    .unwrap();
    let session_store = configurable_store(
        pool.clone(),
        unique_user_id().0,
        REDIRECT_URI,
        ["/".to_string()],
        session_lifetimes,
    );
    let fixture =
        insert_direct_product_session(&pool, &session_store, "/", "Idle Session", 1.0, 2.0).await;
    let session = fixture.session;
    let csrf = fixture.csrf;
    let session_digest = digest_opaque_session_credential_v1(&session).unwrap();
    let mut session_blocker = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT session_digest FROM product_auth_sessions WHERE session_digest = $1 FOR UPDATE",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&mut *session_blocker)
    .await
    .unwrap();
    let blocked_store = session_store.clone();
    let blocked_session = session.clone();
    let blocked_csrf = csrf.clone();
    let blocked_current = tokio::spawn(async move {
        blocked_store
            .current_principal(&blocked_session, &blocked_csrf)
            .await
    });
    sqlx::query("SELECT pg_sleep(1.05)")
        .execute(&pool)
        .await
        .unwrap();
    session_blocker.commit().await.unwrap();
    assert!(matches!(
        blocked_current.await.unwrap(),
        Err(ProductIdentityError::Expired)
    ));
    session_store
        .revoke_session(
            &session,
            ProductSessionRevocationReasonV1::SecurityRevocation,
        )
        .await
        .unwrap();
}
