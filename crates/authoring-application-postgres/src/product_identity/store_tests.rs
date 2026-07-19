use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::TimeDelta;
use discord_model::UserId;
use futures::join;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use super::super::principal::VerifiedIdentityProjection;
use super::super::store::PostgresProductIdentityStore;
use super::super::{ConsumedOAuthFlowV1, PostgresProductIdentityConfig, ProductIdentityError};
use crate::{
    ProductIdentityDatabasePoolsV1, ProductIdentityLifetimesV1, ProductSecretGenerator,
    ProductSecretGeneratorError, MIGRATOR,
};

#[derive(Clone)]
struct DeterministicGenerator {
    counter: Arc<AtomicU64>,
}

impl ProductSecretGenerator for DeterministicGenerator {
    fn fill_secret(&self, destination: &mut [u8; 32]) -> Result<(), ProductSecretGeneratorError> {
        let value = self.counter.fetch_add(1, Ordering::SeqCst);
        for (index, chunk) in destination.chunks_exact_mut(8).enumerate() {
            chunk.copy_from_slice(
                &value
                    .wrapping_add(u64::try_from(index).unwrap())
                    .to_be_bytes(),
            );
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
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
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

fn unique_user_id() -> UserId {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    UserId(u64::try_from(value).unwrap())
}

async fn consumed_flow(
    store: &PostgresProductIdentityStore<DeterministicGenerator>,
) -> ConsumedOAuthFlowV1 {
    let flow = store.create_oauth_flow("/").await.unwrap();
    store
        .consume_oauth_flow(
            flow.state().expose_secret(),
            flow.browser_nonce().expose_secret(),
        )
        .await
        .unwrap()
}

fn copy_consumed_flow(flow: &ConsumedOAuthFlowV1) -> ConsumedOAuthFlowV1 {
    ConsumedOAuthFlowV1 {
        state_digest: flow.state_digest.clone(),
        redirect_uri: flow.redirect_uri.clone(),
        return_path: flow.return_path.clone(),
        consumed_at: flow.consumed_at,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn private_issuer_core_persists_only_an_opaque_verified_projection() {
    let database_url = database_url();
    let expected_database = database_url
        .parse::<PgConnectOptions>()
        .unwrap()
        .get_database()
        .unwrap()
        .to_string();
    let setup_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .unwrap();
    let current_database = sqlx::query_scalar::<_, String>("SELECT pg_catalog.current_database()")
        .fetch_one(&setup_pool)
        .await
        .unwrap();
    assert_test_database_name(&current_database);
    assert_eq!(current_database, expected_database);
    MIGRATOR.run(&setup_pool).await.unwrap();
    sqlx::query("CREATE SCHEMA IF NOT EXISTS authoring_identity_shadow")
        .execute(&setup_pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION authoring_identity_shadow.clock_timestamp() \
         RETURNS TIMESTAMPTZ LANGUAGE SQL IMMUTABLE SET search_path = pg_catalog \
         AS 'SELECT ''2000-01-01T00:00:00Z''::TIMESTAMPTZ'",
    )
    .execute(&setup_pool)
    .await
    .unwrap();
    let shadow_pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET search_path = authoring_identity_shadow, pg_catalog")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&database_url)
        .await
        .unwrap();
    let config = PostgresProductIdentityConfig::new(
        "https://starring.example/oauth/discord/callback",
        ["/".to_string()],
        ProductIdentityLifetimesV1::new(
            Duration::from_secs(600),
            Duration::from_secs(60),
            Duration::from_secs(300),
            Duration::from_secs(1),
            Duration::from_millis(250),
        )
        .unwrap(),
    )
    .unwrap();
    let seed = unique_user_id().0;
    let store = PostgresProductIdentityStore::new(
        ProductIdentityDatabasePoolsV1::new(
            shadow_pool.clone(),
            shadow_pool.clone(),
            shadow_pool.clone(),
            shadow_pool,
        ),
        DeterministicGenerator {
            counter: Arc::new(AtomicU64::new(seed)),
        },
        config,
    );
    let user_id = unique_user_id();
    let invalid_flow = consumed_flow(&store).await;
    let invalid_user_id = unique_user_id();
    let invalid_projection = VerifiedIdentityProjection {
        discord_user_id: invalid_user_id,
        display_name: "Invalid Causality",
    };
    let invalid_claim = ConsumedOAuthFlowV1 {
        state_digest: invalid_flow.state_digest,
        redirect_uri: invalid_flow.redirect_uri,
        return_path: invalid_flow.return_path,
        consumed_at: invalid_flow.consumed_at + TimeDelta::seconds(1),
    };
    assert!(matches!(
        store
            .issue_product_session_core(invalid_claim, invalid_projection)
            .await,
        Err(ProductIdentityError::FlowInvalidOrConsumed)
    ));
    let invalid_principal_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.product_principals WHERE discord_user_id = $1",
    )
    .bind(invalid_user_id.to_string())
    .fetch_one(&setup_pool)
    .await
    .unwrap();
    assert_eq!(invalid_principal_count, 0);
    let mismatched_user_id = unique_user_id();
    let mismatched_principal_id = format!("legacy:{mismatched_user_id}");
    sqlx::query(
        "INSERT INTO public.product_principals \
             (principal_id, discord_user_id, display_profile) \
             VALUES ($1, $2, '{\"display_name\":\"Legacy Mapping\"}'::JSONB)",
    )
    .bind(&mismatched_principal_id)
    .bind(mismatched_user_id.to_string())
    .execute(&setup_pool)
    .await
    .unwrap();
    let mismatched_flow = consumed_flow(&store).await;
    let mismatched_state_digest = mismatched_flow.state_digest.clone();
    assert!(matches!(
        store
            .issue_product_session_core(
                mismatched_flow,
                VerifiedIdentityProjection {
                    discord_user_id: mismatched_user_id,
                    display_name: "Canonical Mapping",
                },
            )
            .await,
        Err(ProductIdentityError::Invariant)
    ));
    let mismatched_state = sqlx::query_as::<_, (i64, i64)>(
        "SELECT principal.identity_revision, \
             (SELECT COUNT(*) FROM public.product_auth_sessions AS authentication_session \
              WHERE authentication_session.oauth_state_digest = $2) \
             FROM public.product_principals AS principal \
             WHERE principal.principal_id = $1",
    )
    .bind(&mismatched_principal_id)
    .bind(mismatched_state_digest.as_bytes().as_slice())
    .fetch_one(&setup_pool)
    .await
    .unwrap();
    assert_eq!(mismatched_state, (1, 0));
    let raced_flow = consumed_flow(&store).await;
    let raced_flow_copy = copy_consumed_flow(&raced_flow);
    let left_user_id = unique_user_id();
    let right_user_id = UserId(left_user_id.0 + 1);
    let (left, right) = join!(
        store.issue_product_session_core(
            raced_flow,
            VerifiedIdentityProjection {
                discord_user_id: left_user_id,
                display_name: "Race Left",
            },
        ),
        store.issue_product_session_core(
            raced_flow_copy,
            VerifiedIdentityProjection {
                discord_user_id: right_user_id,
                display_name: "Race Right",
            },
        )
    );
    match (left, right) {
        (Ok(_), Err(ProductIdentityError::FlowInvalidOrConsumed))
        | (Err(ProductIdentityError::FlowInvalidOrConsumed), Ok(_)) => {}
        outcome => panic!("unexpected session issue race outcome: {outcome:?}"),
    }
    let raced_principal_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.product_principals \
         WHERE discord_user_id = ANY($1::TEXT[])",
    )
    .bind([left_user_id.to_string(), right_user_id.to_string()])
    .fetch_one(&setup_pool)
    .await
    .unwrap();
    assert_eq!(raced_principal_count, 1);
    let first = store
        .issue_product_session_core(
            consumed_flow(&store).await,
            VerifiedIdentityProjection {
                discord_user_id: user_id,
                display_name: "First Name",
            },
        )
        .await
        .unwrap();
    let first_session = first.session().expose_secret().to_string();
    let first_csrf = first.csrf().expose_secret().to_string();
    assert_eq!(first.principal().discord_user_id(), user_id);
    assert_eq!(first.principal().identity_revision(), 1);
    assert!((1..=60).contains(&first.max_age_seconds()));
    assert!(!format!("{first:?}").contains("First Name"));
    assert!(!format!("{first:?}").contains(&user_id.to_string()));
    let second = store
        .issue_product_session_core(
            consumed_flow(&store).await,
            VerifiedIdentityProjection {
                discord_user_id: user_id,
                display_name: "Second Name",
            },
        )
        .await
        .unwrap();
    assert_eq!(second.principal().identity_revision(), 2);
    assert_eq!(second.principal().display_name(), "Second Name");
    let current = store.current_principal(&first_session).await.unwrap();
    assert_eq!(current.identity_revision(), 2);
    assert_eq!(current.display_name(), "Second Name");
    sqlx::query(
        "WITH principal_clock AS MATERIALIZED ( \
          SELECT pg_catalog.clock_timestamp() AS disabled_at \
         ) \
         UPDATE public.product_principals AS principal \
         SET disabled = TRUE, identity_revision = principal.identity_revision + 1, \
          last_authenticated_at = GREATEST( \
           disabled_at, principal.updated_at + INTERVAL '1 microsecond'), \
          updated_at = GREATEST( \
           disabled_at, principal.updated_at + INTERVAL '1 microsecond') \
         FROM principal_clock WHERE principal.discord_user_id = $1",
    )
    .bind(user_id.to_string())
    .execute(&setup_pool)
    .await
    .unwrap();
    let disabled_flow = consumed_flow(&store).await;
    let disabled_state_digest = disabled_flow.state_digest.clone();
    assert!(matches!(
        store
            .issue_product_session_core(
                disabled_flow,
                VerifiedIdentityProjection {
                    discord_user_id: user_id,
                    display_name: "Disabled Name",
                },
            )
            .await,
        Err(ProductIdentityError::PrincipalDisabled)
    ));
    let disabled_session_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.product_auth_sessions WHERE oauth_state_digest = $1",
    )
    .bind(disabled_state_digest.as_bytes().as_slice())
    .fetch_one(&setup_pool)
    .await
    .unwrap();
    assert_eq!(disabled_session_count, 0);
    store.logout(&first_session, &first_csrf).await.unwrap();
}
