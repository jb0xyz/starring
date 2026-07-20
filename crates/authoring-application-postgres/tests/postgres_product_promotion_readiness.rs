use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::{
    PostgresProductPromotions, ProductActionDigestKeyV1, ProductActionDigestKeyringV1,
    ProductPromotionReadinessErrorV1, MIGRATOR,
};
use futures::FutureExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

const EXTERNAL_FUNCTIONS: [&str; 8] = [
    "public.starring_product_promotion_executor_database_identity_v1()",
    "public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])",
    "public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)",
    "public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_keyring_coverage_v1(text[],text[])",
];
const INTERNAL_FUNCTIONS: [&str; 2] = [
    "public.starring_product_promotion_authorize_current_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean)",
    "public.starring_product_promotion_finalize_receipt_v1(jsonb,jsonb,jsonb,jsonb,jsonb)",
];
const SHARED_HELPERS: [&str; 3] = [
    "public.starring_canonical_json_v1(jsonb)",
    "public.starring_ruleset_content_hash_v1(bigint,jsonb)",
    "public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)",
];
const TRIGGER_HELPERS: [&str; 18] = [
    "public.enforce_authoring_promotion_scope()",
    "public.enforce_authoring_promotion_product_admission()",
    "public.enforce_authoring_promotion_product_transition()",
    "public.reject_ruleset_artifact_mutation()",
    "public.enforce_product_activation_journal_link()",
    "public.enforce_product_activation_scope()",
    "public.guard_legacy_activation_product_slot()",
    "public.guard_product_ruleset_artifact_transition()",
    "public.enforce_activation_approval_payload_binding()",
    "public.enforce_activation_approval_scope()",
    "public.reject_activation_approval_mutation()",
    "public.assert_product_approval_receipt_alias()",
    "public.assert_product_approval_receipt_audit()",
    "public.enforce_product_action_receipt_retention()",
    "public.enforce_product_action_receipt_alias_capacity()",
    "public.enforce_product_action_receipt_alias_retention()",
    "public.capture_product_action_receipt_audit_evidence()",
    "public.reject_immutable_product_approval_row()",
];
const RELATIONS: [&str; 18] = [
    "public.product_control_plane_identity",
    "public.product_principals",
    "public.product_auth_sessions",
    "public.product_tenants",
    "public.automation_installations",
    "public.automation_installation_authority_versions",
    "public.authoring_sessions",
    "public.authoring_session_generations",
    "public.authoring_promotions",
    "public.automation_ruleset_heads",
    "public.automation_ruleset_versions",
    "public.automation_ruleset_activations",
    "public.activation_requests",
    "public.activation_request_approvals",
    "public.product_action_receipts",
    "public.product_action_receipt_idempotency_aliases",
    "public.product_audit_events",
    "public.product_action_receipt_audit_evidence",
];
const REPLAY_FUNCTION: &str = EXTERNAL_FUNCTIONS[1];
const COVERAGE_FUNCTION: &str = EXTERNAL_FUNCTIONS[7];
const AUTHORIZE_FUNCTION: &str = INTERNAL_FUNCTIONS[0];
const TRANSITION_TRIGGER: &str = "authoring_promotions_enforce_product_transition";
const AUTHORIZE_DENIAL_QUERY: &str = "SELECT * FROM \
    public.starring_product_promotion_authorize_current_v1(\
        NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::BYTEA, NULL::TEXT, \
        NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::BIGINT, NULL::TEXT, \
        NULL::TEXT, NULL::TIMESTAMPTZ, NULL::TIMESTAMPTZ, NULL::TEXT, \
        NULL::BOOLEAN)";

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct PromotionReadinessDatabase {
    name: String,
    administrator: PgConnection,
    owner_pool: PgPool,
}

struct PromotionReadinessFixture {
    database: PromotionReadinessDatabase,
    owner_role: String,
    executor_role: String,
    executor_pool: PgPool,
}

impl PromotionReadinessFixture {
    async fn new() -> Self {
        let mut database = isolated_database().await;
        MIGRATOR.run(&database.owner_pool).await.unwrap();
        let role_suffix = suffix();
        let owner_role = format!("promotion_owner_{role_suffix}");
        let executor_role = format!("promotion_executor_{role_suffix}");
        assert_safe_identifier(&owner_role);
        assert_safe_identifier(&executor_role);
        let password = database_role_password();
        let password_literal =
            sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
                .bind(&password)
                .fetch_one(&database.owner_pool)
                .await
                .unwrap();
        sqlx::query(&format!(
            "CREATE ROLE {owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
        ))
        .execute(&mut database.administrator)
        .await
        .unwrap();
        sqlx::query(&format!(
            "CREATE ROLE {executor_role} LOGIN PASSWORD {password_literal} \
             NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
             NOBYPASSRLS CONNECTION LIMIT 4"
        ))
        .execute(&mut database.administrator)
        .await
        .unwrap();
        for relation in RELATIONS {
            execute_owner(
                &database.owner_pool,
                &format!("ALTER TABLE {relation} OWNER TO {owner_role}"),
            )
            .await;
        }
        for function in EXTERNAL_FUNCTIONS
            .into_iter()
            .chain(INTERNAL_FUNCTIONS)
            .chain(SHARED_HELPERS)
            .chain(TRIGGER_HELPERS)
        {
            execute_owner(
                &database.owner_pool,
                &format!("ALTER FUNCTION {function} OWNER TO {owner_role}"),
            )
            .await;
            execute_owner(
                &database.owner_pool,
                &format!("REVOKE ALL ON FUNCTION {function} FROM PUBLIC"),
            )
            .await;
        }
        execute_owner(
            &database.owner_pool,
            &format!("REVOKE ALL ON DATABASE {} FROM PUBLIC", database.name),
        )
        .await;
        execute_owner(
            &database.owner_pool,
            "REVOKE ALL ON SCHEMA public FROM PUBLIC",
        )
        .await;
        execute_owner(
            &database.owner_pool,
            &format!(
                "GRANT CONNECT ON DATABASE {} TO {executor_role}",
                database.name
            ),
        )
        .await;
        execute_owner(
            &database.owner_pool,
            &format!("GRANT USAGE ON SCHEMA public TO {owner_role}, {executor_role}"),
        )
        .await;
        execute_owner(
            &database.owner_pool,
            &format!(
                "GRANT EXECUTE ON FUNCTION {} TO {executor_role}",
                EXTERNAL_FUNCTIONS.join(", ")
            ),
        )
        .await;
        let executor_pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(
                database_url()
                    .parse::<PgConnectOptions>()
                    .unwrap()
                    .database(&database.name)
                    .username(&executor_role)
                    .password(&password),
            )
            .await
            .unwrap();
        Self {
            database,
            owner_role,
            executor_role,
            executor_pool,
        }
    }

    fn promotions(&self) -> PostgresProductPromotions {
        let key = ProductActionDigestKeyV1::from_bytes(
            "active-v1",
            std::array::from_fn(|index| index as u8 + 1),
        )
        .unwrap();
        PostgresProductPromotions::new(
            self.executor_pool.clone(),
            ProductActionDigestKeyringV1::new(key, []).unwrap(),
        )
        .unwrap()
    }

    async fn execute_owner(&self, statement: &str) {
        execute_owner(&self.database.owner_pool, statement).await;
    }

    async fn assert_helper_denied(&self) {
        let error = sqlx::query(AUTHORIZE_DENIAL_QUERY)
            .execute(&self.executor_pool)
            .await
            .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("42501")
        );
    }

    async fn install_missing_coverage_function(&self) {
        self.execute_owner(&format!(
            "ALTER FUNCTION {COVERAGE_FUNCTION} RENAME TO \
             starring_product_promotion_keyring_coverage_saved_v1"
        ))
        .await;
        self.execute_owner(&format!(
            "REVOKE EXECUTE ON FUNCTION public.\
             starring_product_promotion_keyring_coverage_saved_v1(TEXT[],TEXT[]) \
             FROM {}",
            self.executor_role
        ))
        .await;
        self.execute_owner(
            "CREATE FUNCTION public.starring_product_promotion_keyring_coverage_v1(\
                idempotency_digest_key_id_candidates TEXT[], \
                idempotency_digest_key_fingerprint_candidates TEXT[]\
             ) RETURNS TABLE(outcome_code TEXT) LANGUAGE sql VOLATILE STRICT \
             SECURITY DEFINER PARALLEL UNSAFE ROWS 1 SET search_path = pg_catalog \
             AS 'SELECT ''missing_key''::TEXT'",
        )
        .await;
        self.execute_owner(&format!(
            "ALTER FUNCTION {COVERAGE_FUNCTION} OWNER TO {}",
            self.owner_role
        ))
        .await;
        self.execute_owner(&format!(
            "REVOKE ALL ON FUNCTION {COVERAGE_FUNCTION} FROM PUBLIC"
        ))
        .await;
        self.execute_owner(&format!(
            "GRANT EXECUTE ON FUNCTION {COVERAGE_FUNCTION} TO {}",
            self.executor_role
        ))
        .await;
    }

    async fn install_rogue_approval_trigger(&self) {
        self.execute_owner(
            "CREATE FUNCTION public.starring_product_promotion_readiness_rogue_trigger() \
             RETURNS trigger LANGUAGE plpgsql VOLATILE PARALLEL UNSAFE \
             SET search_path = pg_catalog AS 'BEGIN RETURN NEW; END'",
        )
        .await;
        self.execute_owner(&format!(
            "ALTER FUNCTION public.starring_product_promotion_readiness_rogue_trigger() \
             OWNER TO {}",
            self.owner_role
        ))
        .await;
        self.execute_owner(
            "REVOKE ALL ON FUNCTION \
             public.starring_product_promotion_readiness_rogue_trigger() FROM PUBLIC",
        )
        .await;
        self.execute_owner(
            "CREATE TRIGGER activation_request_approvals_rogue_test \
             BEFORE INSERT ON public.activation_request_approvals FOR EACH ROW \
             EXECUTE FUNCTION public.starring_product_promotion_readiness_rogue_trigger()",
        )
        .await;
    }

    async fn remove_rogue_approval_trigger(&self) {
        self.execute_owner(
            "DROP TRIGGER activation_request_approvals_rogue_test \
             ON public.activation_request_approvals",
        )
        .await;
        self.execute_owner(
            "DROP FUNCTION public.starring_product_promotion_readiness_rogue_trigger()",
        )
        .await;
    }

    async fn restore_coverage_function(&self) {
        self.execute_owner(&format!("DROP FUNCTION {COVERAGE_FUNCTION}"))
            .await;
        self.execute_owner(
            "ALTER FUNCTION public.\
             starring_product_promotion_keyring_coverage_saved_v1(TEXT[],TEXT[]) \
             RENAME TO starring_product_promotion_keyring_coverage_v1",
        )
        .await;
        self.execute_owner(&format!(
            "GRANT EXECUTE ON FUNCTION {COVERAGE_FUNCTION} TO {}",
            self.executor_role
        ))
        .await;
    }

    async fn close(self) {
        self.executor_pool.close().await;
        self.database.owner_pool.close().await;
        let mut administrator = self.database.administrator;
        sqlx::query(&format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.database.name
        ))
        .execute(&mut administrator)
        .await
        .unwrap();
        for role in [self.executor_role, self.owner_role] {
            sqlx::query(&format!("DROP ROLE {role}"))
                .execute(&mut administrator)
                .await
                .unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn promotion_readiness_enforces_restricted_executor_contract() {
    let fixture = PromotionReadinessFixture::new().await;
    let outcome = AssertUnwindSafe(async {
        fixture.promotions().verify_readiness().await.unwrap();
        fixture.assert_helper_denied().await;

        fixture
            .execute_owner(&format!(
                "GRANT EXECUTE ON FUNCTION {AUTHORIZE_FUNCTION} TO {}",
                fixture.executor_role
            ))
            .await;
        assert_eq!(
            fixture.promotions().verify_readiness().await,
            Err(ProductPromotionReadinessErrorV1::ExcessCapability)
        );
        fixture
            .execute_owner(&format!(
                "REVOKE EXECUTE ON FUNCTION {AUTHORIZE_FUNCTION} FROM {}",
                fixture.executor_role
            ))
            .await;

        fixture
            .execute_owner(&format!("ALTER FUNCTION {REPLAY_FUNCTION} PARALLEL SAFE"))
            .await;
        assert_eq!(
            fixture.promotions().verify_readiness().await,
            Err(ProductPromotionReadinessErrorV1::ContractMismatch)
        );
        fixture
            .execute_owner(&format!("ALTER FUNCTION {REPLAY_FUNCTION} PARALLEL UNSAFE"))
            .await;

        fixture
            .execute_owner(&format!(
                "ALTER TABLE public.authoring_promotions DISABLE TRIGGER {TRANSITION_TRIGGER}"
            ))
            .await;
        assert_eq!(
            fixture.promotions().verify_readiness().await,
            Err(ProductPromotionReadinessErrorV1::ContractMismatch)
        );
        fixture
            .execute_owner(&format!(
                "ALTER TABLE public.authoring_promotions ENABLE TRIGGER {TRANSITION_TRIGGER}"
            ))
            .await;

        fixture.install_rogue_approval_trigger().await;
        assert_eq!(
            fixture.promotions().verify_readiness().await,
            Err(ProductPromotionReadinessErrorV1::ContractMismatch)
        );
        fixture.remove_rogue_approval_trigger().await;

        fixture.install_missing_coverage_function().await;
        assert_eq!(
            fixture.promotions().verify_readiness().await,
            Err(ProductPromotionReadinessErrorV1::IncompleteCoverage)
        );
        fixture.restore_coverage_function().await;
        fixture.promotions().verify_readiness().await.unwrap();
    })
    .catch_unwind()
    .await;
    fixture.close().await;
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

async fn execute_owner(pool: &PgPool, statement: &str) {
    pool.execute(statement).await.unwrap();
}

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let options = url.parse::<PgConnectOptions>().unwrap();
    let database = options.get_database().unwrap();
    assert!(
        database.starts_with("starring_")
            && database.split('_').any(|segment| segment == "test")
            && database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    url
}

async fn isolated_database() -> PromotionReadinessDatabase {
    let name = format!("starring_promotion_readiness_test_{}", suffix());
    assert_safe_identifier(&name);
    let base = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let owner_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    PromotionReadinessDatabase {
        name,
        administrator,
        owner_pool,
    }
}

fn suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{timestamp}{counter}")
}

fn assert_safe_identifier(identifier: &str) {
    assert!(
        !identifier.is_empty()
            && identifier.len() <= 63
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
}

fn database_role_password() -> String {
    let mut material = [0_u8; 32];
    getrandom::fill(&mut material).unwrap();
    material.iter().map(|byte| format!("{byte:02x}")).collect()
}
