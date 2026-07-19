use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application_postgres::{
    PostgresProductIdentityRetention, PostgresProductIdentityRetentionConfig,
    ProductDatabaseFailureV1, ProductIdentityRetentionError, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Acquire, Postgres, Transaction};

const REDIRECT_URI: &str = "https://starring.example/oauth/discord/callback";

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct PurgeOutcome {
    deleted_sessions: i32,
    deleted_oauth_flows: i32,
    backlog_remaining: bool,
}

struct FlowSeed {
    state_digest: Vec<u8>,
    browser_nonce_digest: Vec<u8>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}

struct SessionSeed {
    session_digest: Vec<u8>,
    csrf_digest: Vec<u8>,
    oauth_state_digest: Option<Vec<u8>>,
    authenticated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    revocation_reason: Option<String>,
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
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to PostgreSQL test database");
    let current_database = sqlx::query_scalar::<_, String>("SELECT pg_catalog.current_database()")
        .fetch_one(&pool)
        .await
        .expect("read current PostgreSQL test database");
    assert_test_database_name(&current_database);
    assert_eq!(current_database, expected_database);
    MIGRATOR
        .run(&pool)
        .await
        .expect("run PostgreSQL migrations");
    pool
}

fn fixture_key(label: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("retention_{label}_{timestamp:x}_{sequence:x}")
}

fn bytes_digest(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}

fn hex_digest(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

fn numeric_id(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes).max(1).to_string()
}

fn stable_session_subject(tenant_id: &str, principal_id: &str, session_digest: &[u8]) -> Vec<u8> {
    let mut digest = Sha256::new();
    for value in [
        b"starring.product.session.subject.v1".as_slice(),
        tenant_id.as_bytes(),
        principal_id.as_bytes(),
        session_digest,
    ] {
        Digest::update(
            &mut digest,
            u64::try_from(value.len()).unwrap().to_be_bytes(),
        );
        Digest::update(&mut digest, value);
    }
    digest.finalize().to_vec()
}

async fn assert_retention_indexes(connection: &mut PgConnection) {
    let names = sqlx::query_scalar::<_, String>(
        "SELECT indexname FROM pg_catalog.pg_indexes \
         WHERE schemaname = 'public' \
           AND indexname IN (\
             'product_auth_sessions_terminal_retention_index', \
             'product_oauth_flows_consumed_retention_index', \
             'product_oauth_flows_unconsumed_retention_index'\
           ) ORDER BY indexname",
    )
    .fetch_all(connection)
    .await
    .expect("read product identity retention indexes");
    assert_eq!(
        names,
        vec![
            "product_auth_sessions_terminal_retention_index".to_string(),
            "product_oauth_flows_consumed_retention_index".to_string(),
            "product_oauth_flows_unconsumed_retention_index".to_string(),
        ]
    );
}

fn flow_seed(
    fixture: &str,
    label: &str,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
) -> FlowSeed {
    FlowSeed {
        state_digest: bytes_digest(&format!("{fixture}:flow:{label}:state")),
        browser_nonce_digest: bytes_digest(&format!("{fixture}:flow:{label}:nonce")),
        created_at,
        expires_at,
        consumed_at,
    }
}

fn session_identity(fixture: &str, label: &str) -> (Vec<u8>, Vec<u8>) {
    (
        bytes_digest(&format!("{fixture}:session:{label}:credential")),
        bytes_digest(&format!("{fixture}:session:{label}:csrf")),
    )
}

async fn insert_principal(connection: &mut PgConnection, fixture: &str) -> String {
    let principal_id = format!("principal:{fixture}");
    let discord_user_id = numeric_id(&format!("{fixture}:discord-user"));
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) \
         VALUES ($1, $2, '{}'::JSONB)",
    )
    .bind(&principal_id)
    .bind(discord_user_id)
    .execute(connection)
    .await
    .expect("insert retention test principal");
    principal_id
}

async fn insert_flows(connection: &mut PgConnection, flows: &[FlowSeed]) {
    for flow in flows {
        let terminal_result_code = flow.consumed_at.as_ref().map(|_| "callback_claimed");
        sqlx::query(
            "INSERT INTO public.product_oauth_flows \
             (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, \
              expires_at, consumed_at, terminal_result_code) \
             VALUES ($1, $2, $3, '/', $4, $5, $6, $7)",
        )
        .bind(&flow.state_digest)
        .bind(&flow.browser_nonce_digest)
        .bind(REDIRECT_URI)
        .bind(flow.created_at)
        .bind(flow.expires_at)
        .bind(flow.consumed_at)
        .bind(terminal_result_code)
        .execute(&mut *connection)
        .await
        .expect("insert retention test OAuth flow");
    }
}

async fn seed_sessions(
    transaction: &mut Transaction<'_, Postgres>,
    principal_id: &str,
    sessions: &[SessionSeed],
) {
    let mut seed_transaction = transaction
        .begin()
        .await
        .expect("begin session seed savepoint");
    sqlx::query("LOCK TABLE public.product_auth_sessions IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *seed_transaction)
        .await
        .expect("lock product authentication sessions for historical seed");
    sqlx::query(
        "ALTER TABLE public.product_auth_sessions \
         DISABLE TRIGGER product_auth_sessions_enforce_oauth_binding",
    )
    .execute(&mut *seed_transaction)
    .await
    .expect("disable OAuth binding insert trigger for historical seed");
    for session in sessions {
        sqlx::query(
            "INSERT INTO public.product_auth_sessions \
             (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
              created_at, last_seen_at, idle_expires_at, absolute_expires_at, revoked_at, \
              revocation_reason) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&session.session_digest)
        .bind(principal_id)
        .bind(&session.csrf_digest)
        .bind(session.oauth_state_digest.as_deref())
        .bind(session.authenticated_at)
        .bind(session.created_at)
        .bind(session.last_seen_at)
        .bind(session.idle_expires_at)
        .bind(session.absolute_expires_at)
        .bind(session.revoked_at)
        .bind(session.revocation_reason.as_deref())
        .execute(&mut *seed_transaction)
        .await
        .expect("insert historical retention test session");
    }
    sqlx::query(
        "ALTER TABLE public.product_auth_sessions \
         ENABLE TRIGGER product_auth_sessions_enforce_oauth_binding",
    )
    .execute(&mut *seed_transaction)
    .await
    .expect("restore OAuth binding insert trigger after historical seed");
    seed_transaction
        .commit()
        .await
        .expect("commit historical session seed savepoint");
}

async fn purge(
    connection: &mut PgConnection,
    batch_limit: i32,
) -> Result<PurgeOutcome, sqlx::Error> {
    sqlx::query_as::<_, PurgeOutcome>(
        "SELECT deleted_sessions, deleted_oauth_flows, backlog_remaining \
         FROM public.starring_purge_product_identity_v1($1)",
    )
    .bind(batch_limit)
    .fetch_one(connection)
    .await
}

async fn retention_gate(connection: &mut PgConnection) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT pg_catalog.current_setting(\
         'starring.product_identity_retention_gate', TRUE)",
    )
    .fetch_one(connection)
    .await
    .expect("read product identity retention gate")
}

async fn assert_gate_cleared(connection: &mut PgConnection) {
    assert_eq!(retention_gate(connection).await, Some(String::new()));
}

async fn drain_eligible_rows(connection: &mut PgConnection) {
    loop {
        let outcome = purge(connection, 1000)
            .await
            .expect("drain eligible identity rows inside rollback scope");
        assert_gate_cleared(connection).await;
        if !outcome.backlog_remaining {
            break;
        }
        assert!(outcome.deleted_sessions + outcome.deleted_oauth_flows > 0);
    }
}

async fn isolated_transaction(pool: &PgPool) -> Transaction<'_, Postgres> {
    let mut transaction = pool
        .begin()
        .await
        .expect("begin retention test transaction");
    sqlx::query(
        "LOCK TABLE public.product_oauth_flows, public.product_auth_sessions \
         IN ACCESS EXCLUSIVE MODE",
    )
    .execute(&mut *transaction)
    .await
    .expect("isolate product identity retention test");
    drain_eligible_rows(&mut transaction).await;
    transaction
}

fn assert_database_error(error: sqlx::Error, sqlstate: &str, message: &str) {
    let sqlx::Error::Database(database_error) = error else {
        panic!("expected database error, received {error:?}");
    };
    assert_eq!(database_error.code().as_deref(), Some(sqlstate));
    assert_eq!(database_error.message(), message);
}

async fn assert_purge_rejected(transaction: &mut Transaction<'_, Postgres>, batch_limit: i32) {
    let mut attempt = transaction
        .begin()
        .await
        .expect("begin invalid purge savepoint");
    let error = purge(&mut attempt, batch_limit)
        .await
        .expect_err("invalid purge batch must fail");
    assert_database_error(
        error,
        "22023",
        "product identity purge batch limit is invalid",
    );
    attempt
        .rollback()
        .await
        .expect("rollback invalid purge savepoint");
}

async fn assert_null_purge_rejected(transaction: &mut Transaction<'_, Postgres>) {
    let mut attempt = transaction
        .begin()
        .await
        .expect("begin null purge savepoint");
    let error = sqlx::query_as::<_, PurgeOutcome>(
        "SELECT deleted_sessions, deleted_oauth_flows, backlog_remaining \
         FROM public.starring_purge_product_identity_v1(NULL::INTEGER)",
    )
    .fetch_one(&mut *attempt)
    .await
    .expect_err("null purge batch must fail");
    assert_database_error(
        error,
        "22023",
        "product identity purge batch limit is invalid",
    );
    attempt
        .rollback()
        .await
        .expect("rollback null purge savepoint");
}

async fn assert_session_delete_rejected(
    transaction: &mut Transaction<'_, Postgres>,
    session_digest: &[u8],
) {
    let mut attempt = transaction
        .begin()
        .await
        .expect("begin direct session delete savepoint");
    let error = sqlx::query(
        "DELETE FROM public.product_auth_sessions \
         WHERE session_digest = $1",
    )
    .bind(session_digest)
    .execute(&mut *attempt)
    .await
    .expect_err("direct eligible session deletion must fail");
    assert_database_error(
        error,
        "23514",
        "product authentication sessions cannot be deleted directly",
    );
    attempt
        .rollback()
        .await
        .expect("rollback direct session delete savepoint");
}

async fn assert_flow_delete_rejected(
    transaction: &mut Transaction<'_, Postgres>,
    state_digest: &[u8],
) {
    let mut attempt = transaction
        .begin()
        .await
        .expect("begin direct OAuth flow delete savepoint");
    let error = sqlx::query(
        "DELETE FROM public.product_oauth_flows \
         WHERE state_digest = $1",
    )
    .bind(state_digest)
    .execute(&mut *attempt)
    .await
    .expect_err("direct eligible OAuth flow deletion must fail");
    assert_database_error(
        error,
        "23514",
        "product OAuth flows cannot be deleted directly",
    );
    attempt
        .rollback()
        .await
        .expect("rollback direct OAuth flow delete savepoint");
}

async fn session_exists(connection: &mut PgConnection, session_digest: &[u8]) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
         SELECT 1 FROM public.product_auth_sessions WHERE session_digest = $1)",
    )
    .bind(session_digest)
    .fetch_one(connection)
    .await
    .expect("check retained product session")
}

async fn flow_exists(connection: &mut PgConnection, state_digest: &[u8]) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (\
         SELECT 1 FROM public.product_oauth_flows WHERE state_digest = $1)",
    )
    .bind(state_digest)
    .fetch_one(connection)
    .await
    .expect("check retained product OAuth flow")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn retention_guards_direct_deletes_validates_bounds_and_clears_gate() {
    let pool = pool().await;
    let mut transaction = isolated_transaction(&pool).await;
    assert_retention_indexes(&mut transaction).await;
    let fixture = fixture_key("guards");
    let principal_id = insert_principal(&mut transaction, &fixture).await;
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let eligible_flow = flow_seed(
        &fixture,
        "eligible",
        now - TimeDelta::hours(3),
        now - TimeDelta::hours(3) + TimeDelta::minutes(5),
        None,
    );
    insert_flows(&mut transaction, std::slice::from_ref(&eligible_flow)).await;
    let (session_digest, csrf_digest) = session_identity(&fixture, "eligible");
    let eligible_session = SessionSeed {
        session_digest: session_digest.clone(),
        csrf_digest,
        oauth_state_digest: None,
        authenticated_at: now - TimeDelta::days(9),
        created_at: now - TimeDelta::days(9),
        last_seen_at: now - TimeDelta::days(8) - TimeDelta::hours(1),
        idle_expires_at: now + TimeDelta::hours(1),
        absolute_expires_at: now + TimeDelta::hours(2),
        revoked_at: Some(now - TimeDelta::days(8)),
        revocation_reason: Some("logout".to_string()),
    };
    seed_sessions(
        &mut transaction,
        &principal_id,
        std::slice::from_ref(&eligible_session),
    )
    .await;

    assert_session_delete_rejected(&mut transaction, &session_digest).await;
    assert_flow_delete_rejected(&mut transaction, &eligible_flow.state_digest).await;
    assert_null_purge_rejected(&mut transaction).await;
    assert!(session_exists(&mut transaction, &session_digest).await);
    assert!(flow_exists(&mut transaction, &eligible_flow.state_digest).await);
    assert_purge_rejected(&mut transaction, 0).await;
    assert_purge_rejected(&mut transaction, 1001).await;
    assert_gate_cleared(&mut transaction).await;

    let first = purge(&mut transaction, 1).await.unwrap();
    assert_eq!(
        first,
        PurgeOutcome {
            deleted_sessions: 1,
            deleted_oauth_flows: 0,
            backlog_remaining: true,
        }
    );
    assert_gate_cleared(&mut transaction).await;
    assert_flow_delete_rejected(&mut transaction, &eligible_flow.state_digest).await;

    let second = purge(&mut transaction, 1).await.unwrap();
    assert_eq!(
        second,
        PurgeOutcome {
            deleted_sessions: 0,
            deleted_oauth_flows: 1,
            backlog_remaining: false,
        }
    );
    assert_gate_cleared(&mut transaction).await;
    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn retention_purge_applies_windows_and_preserves_live_references() {
    let pool = pool().await;
    let mut transaction = isolated_transaction(&pool).await;
    let fixture = fixture_key("windows");
    let principal_id = insert_principal(&mut transaction, &fixture).await;
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();

    let old_absolute_flow = flow_seed(
        &fixture,
        "old-absolute-session",
        now - TimeDelta::days(8) - TimeDelta::minutes(2),
        now - TimeDelta::days(8) + TimeDelta::minutes(5),
        Some(now - TimeDelta::days(8) - TimeDelta::minutes(1)),
    );
    let recent_absolute_flow = flow_seed(
        &fixture,
        "recent-absolute-session",
        now - TimeDelta::days(6) - TimeDelta::minutes(2),
        now - TimeDelta::days(6) + TimeDelta::minutes(5),
        Some(now - TimeDelta::days(6) - TimeDelta::minutes(1)),
    );
    let active_flow = flow_seed(
        &fixture,
        "active-session",
        now - TimeDelta::minutes(3),
        now + TimeDelta::minutes(5),
        Some(now - TimeDelta::minutes(2)),
    );
    let referenced_old_flow = flow_seed(
        &fixture,
        "referenced-old",
        now - TimeDelta::days(8) - TimeDelta::minutes(2),
        now - TimeDelta::days(8) + TimeDelta::minutes(5),
        Some(now - TimeDelta::days(8) - TimeDelta::minutes(1)),
    );
    let old_unconsumed_flow = flow_seed(
        &fixture,
        "old-unconsumed",
        now - TimeDelta::hours(3),
        now - TimeDelta::hours(3) + TimeDelta::minutes(5),
        None,
    );
    let recent_unconsumed_flow = flow_seed(
        &fixture,
        "recent-unconsumed",
        now - TimeDelta::minutes(35),
        now - TimeDelta::minutes(30),
        None,
    );
    let live_unconsumed_flow = flow_seed(
        &fixture,
        "live-unconsumed",
        now - TimeDelta::minutes(2),
        now + TimeDelta::minutes(5),
        None,
    );
    let old_consumed_flow = flow_seed(
        &fixture,
        "old-consumed",
        now - TimeDelta::days(8) - TimeDelta::minutes(2),
        now - TimeDelta::days(8) + TimeDelta::minutes(5),
        Some(now - TimeDelta::days(8) - TimeDelta::minutes(1)),
    );
    let recent_consumed_flow = flow_seed(
        &fixture,
        "recent-consumed",
        now - TimeDelta::days(6) - TimeDelta::minutes(2),
        now - TimeDelta::days(6) + TimeDelta::minutes(5),
        Some(now - TimeDelta::days(6) - TimeDelta::minutes(1)),
    );
    let idle_expired_flow = flow_seed(
        &fixture,
        "idle-expired-session",
        now - TimeDelta::days(7) - TimeDelta::hours(1) - TimeDelta::minutes(2),
        now - TimeDelta::days(7) - TimeDelta::hours(1) + TimeDelta::minutes(5),
        Some(now - TimeDelta::days(7) - TimeDelta::hours(1) - TimeDelta::minutes(1)),
    );
    let flows = vec![
        old_absolute_flow,
        recent_absolute_flow,
        active_flow,
        referenced_old_flow,
        old_unconsumed_flow,
        recent_unconsumed_flow,
        live_unconsumed_flow,
        old_consumed_flow,
        recent_consumed_flow,
        idle_expired_flow,
    ];
    insert_flows(&mut transaction, &flows).await;

    let (old_revoked_digest, old_revoked_csrf) = session_identity(&fixture, "old-revoked");
    let (old_absolute_digest, old_absolute_csrf) = session_identity(&fixture, "old-absolute");
    let (recent_revoked_digest, recent_revoked_csrf) = session_identity(&fixture, "recent-revoked");
    let (recent_absolute_digest, recent_absolute_csrf) =
        session_identity(&fixture, "recent-absolute");
    let (active_digest, active_csrf) = session_identity(&fixture, "active");
    let (referencing_digest, referencing_csrf) = session_identity(&fixture, "referencing");
    let (idle_expired_digest, idle_expired_csrf) = session_identity(&fixture, "idle-expired");
    let sessions = vec![
        SessionSeed {
            session_digest: old_revoked_digest.clone(),
            csrf_digest: old_revoked_csrf,
            oauth_state_digest: None,
            authenticated_at: now - TimeDelta::days(9),
            created_at: now - TimeDelta::days(9),
            last_seen_at: now - TimeDelta::days(8) - TimeDelta::hours(1),
            idle_expires_at: now + TimeDelta::hours(1),
            absolute_expires_at: now + TimeDelta::hours(2),
            revoked_at: Some(now - TimeDelta::days(8)),
            revocation_reason: Some("logout".to_string()),
        },
        SessionSeed {
            session_digest: old_absolute_digest.clone(),
            csrf_digest: old_absolute_csrf,
            oauth_state_digest: Some(flows[0].state_digest.clone()),
            authenticated_at: now - TimeDelta::days(8),
            created_at: now - TimeDelta::days(8),
            last_seen_at: now - TimeDelta::days(8),
            idle_expires_at: now - TimeDelta::days(8) + TimeDelta::minutes(30),
            absolute_expires_at: now - TimeDelta::days(8) + TimeDelta::hours(1),
            revoked_at: None,
            revocation_reason: None,
        },
        SessionSeed {
            session_digest: recent_revoked_digest.clone(),
            csrf_digest: recent_revoked_csrf,
            oauth_state_digest: None,
            authenticated_at: now - TimeDelta::days(2),
            created_at: now - TimeDelta::days(2),
            last_seen_at: now - TimeDelta::days(1),
            idle_expires_at: now + TimeDelta::days(1),
            absolute_expires_at: now + TimeDelta::days(2),
            revoked_at: Some(now - TimeDelta::hours(6)),
            revocation_reason: Some("logout".to_string()),
        },
        SessionSeed {
            session_digest: recent_absolute_digest.clone(),
            csrf_digest: recent_absolute_csrf,
            oauth_state_digest: Some(flows[1].state_digest.clone()),
            authenticated_at: now - TimeDelta::days(6),
            created_at: now - TimeDelta::days(6),
            last_seen_at: now - TimeDelta::days(6),
            idle_expires_at: now - TimeDelta::days(6) + TimeDelta::minutes(30),
            absolute_expires_at: now - TimeDelta::days(6) + TimeDelta::hours(1),
            revoked_at: None,
            revocation_reason: None,
        },
        SessionSeed {
            session_digest: active_digest.clone(),
            csrf_digest: active_csrf,
            oauth_state_digest: Some(flows[2].state_digest.clone()),
            authenticated_at: now - TimeDelta::minutes(1),
            created_at: now - TimeDelta::minutes(1),
            last_seen_at: now - TimeDelta::minutes(1),
            idle_expires_at: now + TimeDelta::minutes(20),
            absolute_expires_at: now + TimeDelta::hours(1),
            revoked_at: None,
            revocation_reason: None,
        },
        SessionSeed {
            session_digest: referencing_digest.clone(),
            csrf_digest: referencing_csrf,
            oauth_state_digest: Some(flows[3].state_digest.clone()),
            authenticated_at: now - TimeDelta::minutes(1),
            created_at: now - TimeDelta::minutes(1),
            last_seen_at: now - TimeDelta::minutes(1),
            idle_expires_at: now + TimeDelta::minutes(20),
            absolute_expires_at: now + TimeDelta::hours(1),
            revoked_at: None,
            revocation_reason: None,
        },
        SessionSeed {
            session_digest: idle_expired_digest.clone(),
            csrf_digest: idle_expired_csrf,
            oauth_state_digest: Some(flows[9].state_digest.clone()),
            authenticated_at: now - TimeDelta::days(7) - TimeDelta::hours(1),
            created_at: now - TimeDelta::days(7) - TimeDelta::hours(1),
            last_seen_at: now - TimeDelta::days(7) - TimeDelta::hours(1),
            idle_expires_at: now - TimeDelta::days(7) - TimeDelta::minutes(30),
            absolute_expires_at: now - TimeDelta::days(6) - TimeDelta::hours(14),
            revoked_at: None,
            revocation_reason: None,
        },
    ];
    seed_sessions(&mut transaction, &principal_id, &sessions).await;

    let outcome = purge(&mut transaction, 1000).await.unwrap();
    assert_eq!(
        outcome,
        PurgeOutcome {
            deleted_sessions: 3,
            deleted_oauth_flows: 4,
            backlog_remaining: false,
        }
    );
    assert_gate_cleared(&mut transaction).await;

    assert!(!session_exists(&mut transaction, &old_revoked_digest).await);
    assert!(!session_exists(&mut transaction, &old_absolute_digest).await);
    assert!(!session_exists(&mut transaction, &idle_expired_digest).await);
    for digest in [
        &recent_revoked_digest,
        &recent_absolute_digest,
        &active_digest,
        &referencing_digest,
    ] {
        assert!(session_exists(&mut transaction, digest).await);
    }
    assert!(!flow_exists(&mut transaction, &flows[0].state_digest).await);
    assert!(flow_exists(&mut transaction, &flows[1].state_digest).await);
    assert!(flow_exists(&mut transaction, &flows[2].state_digest).await);
    assert!(flow_exists(&mut transaction, &flows[3].state_digest).await);
    assert!(!flow_exists(&mut transaction, &flows[4].state_digest).await);
    assert!(flow_exists(&mut transaction, &flows[5].state_digest).await);
    assert!(flow_exists(&mut transaction, &flows[6].state_digest).await);
    assert!(!flow_exists(&mut transaction, &flows[7].state_digest).await);
    assert!(flow_exists(&mut transaction, &flows[8].state_digest).await);
    assert!(!flow_exists(&mut transaction, &flows[9].state_digest).await);

    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn retention_purge_shares_batch_limit_and_reports_backlog() {
    let pool = pool().await;
    let mut transaction = isolated_transaction(&pool).await;
    let fixture = fixture_key("batch");
    let principal_id = insert_principal(&mut transaction, &fixture).await;
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let flows = (0..3)
        .map(|index| {
            flow_seed(
                &fixture,
                &format!("eligible-{index}"),
                now - TimeDelta::hours(4) + TimeDelta::minutes(index),
                now - TimeDelta::hours(4) + TimeDelta::minutes(index + 5),
                None,
            )
        })
        .collect::<Vec<_>>();
    insert_flows(&mut transaction, &flows).await;
    let sessions = (0..2)
        .map(|index| {
            let (session_digest, csrf_digest) =
                session_identity(&fixture, &format!("eligible-{index}"));
            SessionSeed {
                session_digest,
                csrf_digest,
                oauth_state_digest: None,
                authenticated_at: now - TimeDelta::days(10),
                created_at: now - TimeDelta::days(10),
                last_seen_at: now - TimeDelta::days(9),
                idle_expires_at: now + TimeDelta::hours(1),
                absolute_expires_at: now + TimeDelta::hours(2),
                revoked_at: Some(now - TimeDelta::days(8) - TimeDelta::minutes(index)),
                revocation_reason: Some("logout".to_string()),
            }
        })
        .collect::<Vec<_>>();
    seed_sessions(&mut transaction, &principal_id, &sessions).await;

    let first = purge(&mut transaction, 3).await.unwrap();
    assert_eq!(first.deleted_sessions + first.deleted_oauth_flows, 3);
    assert_eq!(first.deleted_sessions, 2);
    assert_eq!(first.deleted_oauth_flows, 1);
    assert!(first.backlog_remaining);
    assert_gate_cleared(&mut transaction).await;

    let second = purge(&mut transaction, 2).await.unwrap();
    assert_eq!(
        second,
        PurgeOutcome {
            deleted_sessions: 0,
            deleted_oauth_flows: 2,
            backlog_remaining: false,
        }
    );
    assert_gate_cleared(&mut transaction).await;

    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn retention_purge_preserves_stable_session_subject_audit_history() {
    let pool = pool().await;
    let mut transaction = isolated_transaction(&pool).await;
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let fixture = fixture_key("audit");
    let principal_id = insert_principal(&mut transaction, &fixture).await;
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let session_flow = flow_seed(
        &fixture,
        "audit-session",
        now - TimeDelta::days(8) - TimeDelta::minutes(2),
        now - TimeDelta::days(8) + TimeDelta::minutes(5),
        Some(now - TimeDelta::days(8) - TimeDelta::minutes(1)),
    );
    insert_flows(&mut transaction, std::slice::from_ref(&session_flow)).await;
    let (session_digest, csrf_digest) = session_identity(&fixture, "audit-session");
    let session = SessionSeed {
        session_digest: session_digest.clone(),
        csrf_digest,
        oauth_state_digest: Some(session_flow.state_digest.clone()),
        authenticated_at: now - TimeDelta::days(8),
        created_at: now - TimeDelta::days(8),
        last_seen_at: now - TimeDelta::days(8),
        idle_expires_at: now - TimeDelta::days(8) + TimeDelta::minutes(30),
        absolute_expires_at: now - TimeDelta::days(8) + TimeDelta::hours(1),
        revoked_at: None,
        revocation_reason: None,
    };
    seed_sessions(
        &mut transaction,
        &principal_id,
        std::slice::from_ref(&session),
    )
    .await;

    let tenant_id = format!("tenant:{fixture}");
    let installation_id = format!("installation:{fixture}");
    let application_id = numeric_id(&format!("{fixture}:application"));
    let guild_id = numeric_id(&format!("{fixture}:guild"));
    let ruleset_key = format!("retention_{}", &hex_digest(&fixture)[..24]);
    let resource_bindings = ResourceBindingMap::default();
    let binding_fingerprint = resource_binding_fingerprint_v2(&resource_bindings).into_string();
    let authority_payload_digest = hex_digest(&format!("{fixture}:authority"));
    let authority_request_digest = hex_digest(&format!("{fixture}:authority-request"));
    let receipt_id = hex_digest(&format!("{fixture}:receipt"));
    let event_id = hex_digest(&format!("{fixture}:event"));
    let idempotency_digest = hex_digest(&format!("{fixture}:idempotency"));
    let request_digest = hex_digest(&format!("{fixture}:request"));
    let observation_digest = hex_digest(&format!("{fixture}:observation"));
    let session_subject = stable_session_subject(&tenant_id, &principal_id, &session_digest);
    assert_ne!(session_subject, session_digest);

    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', $2)",
    )
    .bind(&tenant_id)
    .bind(format!("Retention {fixture}"))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(&application_id)
    .bind(&guild_id)
    .bind(&ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, \
          pg_catalog.jsonb_build_object(\
           'role_bindings', '{}'::JSONB, 'channel_bindings', '{}'::JSONB), \
          $3, 1, 1, 3600, $4, $5, $6)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(&binding_fingerprint)
    .bind(&authority_payload_digest)
    .bind(&principal_id)
    .bind(&authority_request_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_action_receipts \
         (receipt_id, tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, request_digest, target_resource_type, target_resource_id, \
          resulting_state, result_code, http_disposition_class) \
         VALUES ($1, $2, $3, $4, 'retention_test', $5, $6, 'session', $7, \
          'retained', 'ok', 2)",
    )
    .bind(&receipt_id)
    .bind(&tenant_id)
    .bind(&installation_id)
    .bind(&principal_id)
    .bind(&idempotency_digest)
    .bind(&request_digest)
    .bind(&principal_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_audit_events \
         (event_id, tenant_id, installation_id, principal_id, session_subject_digest, action, \
          target_resource_type, target_resource_id, request_id, receipt_id, \
          authority_observation_digest, effective_permission_bits, authority_observed_at, \
          installation_authority_revision, resulting_state, result_code) \
         VALUES ($1, $2, $3, $4, $5, 'session.retention', 'session', $6, $7, $8, $9, \
          0, $10, 1, 'retained', 'ok')",
    )
    .bind(&event_id)
    .bind(&tenant_id)
    .bind(&installation_id)
    .bind(&principal_id)
    .bind(&session_subject)
    .bind(&principal_id)
    .bind(format!("retention.request.{fixture}"))
    .bind(&receipt_id)
    .bind(&observation_digest)
    .bind(now - TimeDelta::seconds(1))
    .execute(&mut *transaction)
    .await
    .unwrap();

    let session_foreign_keys = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_constraint AS constraint_row \
         JOIN pg_catalog.pg_class AS child_table \
           ON child_table.oid = constraint_row.conrelid \
         JOIN pg_catalog.pg_namespace AS child_schema \
           ON child_schema.oid = child_table.relnamespace \
         WHERE constraint_row.contype = 'f' \
           AND child_schema.nspname = 'public' \
           AND child_table.relname = 'product_audit_events' \
           AND constraint_row.confrelid = \
               'public.product_auth_sessions'::pg_catalog.regclass",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(session_foreign_keys, 0);

    let outcome = purge(&mut transaction, 10).await.unwrap();
    assert_eq!(
        outcome,
        PurgeOutcome {
            deleted_sessions: 1,
            deleted_oauth_flows: 1,
            backlog_remaining: false,
        }
    );
    assert_gate_cleared(&mut transaction).await;
    assert!(!session_exists(&mut transaction, &session_digest).await);
    let retained_subject = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT session_subject_digest \
         FROM public.product_audit_events WHERE event_id = $1",
    )
    .bind(&event_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(retained_subject, session_subject);

    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn retention_adapter_bounds_inputs_database_locks_and_reports_committed_work() {
    let pool = pool().await;
    let fixture = fixture_key("adapter");
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let eligible = flow_seed(
        &fixture,
        "eligible",
        now - TimeDelta::days(10) - TimeDelta::minutes(5),
        now - TimeDelta::days(10),
        None,
    );
    let mut connection = pool.acquire().await.unwrap();
    insert_flows(&mut connection, &[eligible]).await;
    drop(connection);

    let retention = PostgresProductIdentityRetention::with_config(
        pool.clone(),
        PostgresProductIdentityRetentionConfig::new(
            Duration::from_secs(1),
            Duration::from_millis(50),
        )
        .unwrap(),
    );
    assert_eq!(
        retention.purge(0).await.unwrap_err(),
        ProductIdentityRetentionError::InvalidBatchLimit
    );
    let report = retention.purge(1).await.unwrap();
    assert_eq!(report.deleted_sessions(), 0);
    assert_eq!(report.deleted_oauth_flows(), 1);
    assert!(!report.backlog_remaining());

    let mut locker = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE public.product_auth_sessions IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *locker)
        .await
        .unwrap();
    let bounded = tokio::time::timeout(Duration::from_secs(2), retention.purge(1))
        .await
        .expect("retention adapter exceeded its outer test deadline")
        .unwrap_err();
    assert_eq!(
        bounded,
        ProductIdentityRetentionError::Database(ProductDatabaseFailureV1::Timeout)
    );
    locker.rollback().await.unwrap();
    pool.close().await;
}
