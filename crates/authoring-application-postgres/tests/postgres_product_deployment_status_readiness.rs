use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::{
    PostgresProductDeploymentStatuses, ProductDeploymentStatusReadinessErrorV1, MIGRATOR,
};
use futures::FutureExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;

const STATUS_IDENTITY_FUNCTION: &str =
    "public.starring_product_deployment_status_reader_database_identity_v1()";
const STATUS_READ_FUNCTION: &str = "public.starring_product_deployment_status_read_v1(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BYTEA)";
const UNEXPECTED_FUNCTION: &str = "public.validate_runtime_deployment_projection()";
const CANONICAL_HELPER: &str = "public.starring_canonical_json_v1(JSONB)";

const RELATIONS: [&str; 13] = [
    "product_control_plane_identity",
    "product_principals",
    "product_auth_sessions",
    "runtime_deployments",
    "activation_requests",
    "authoring_promotions",
    "product_tenants",
    "automation_installations",
    "automation_installation_authority_versions",
    "automation_ruleset_activations",
    "automation_ruleset_versions",
    "runtime_attestations",
    "runtime_serving_leases",
];

const OWNED_FUNCTIONS: [&str; 13] = [
    STATUS_IDENTITY_FUNCTION,
    STATUS_READ_FUNCTION,
    "public.validate_runtime_deployment_projection()",
    "public.enforce_runtime_deployment_policy_shadow()",
    "public.guard_runtime_ruleset_artifact_transition()",
    "public.reject_runtime_deployment_delete()",
    "public.validate_runtime_attestation_projection()",
    "public.reject_immutable_product_row()",
    "public.validate_runtime_serving_lease_transition()",
    "public.reject_runtime_serving_lease_delete()",
    "public.reject_ruleset_artifact_mutation()",
    CANONICAL_HELPER,
    "public.starring_ruleset_content_hash_v1(BIGINT,JSONB)",
];

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct StatusReadinessDatabase {
    name: String,
    administrator: PgConnection,
    owner_pool: PgPool,
}

struct StatusReadinessFixture {
    database: StatusReadinessDatabase,
    owner_role: String,
    reader_role: String,
    attacker_role: String,
    reader_pool: PgPool,
}

#[derive(Clone, Copy)]
enum ReadinessDrift {
    MissingExecute,
    ExecuteGrantOption,
    DirectTablePrivilege,
    UnexpectedExecute,
    FunctionOverload,
    StatusFunctionOverload,
    SchemaCreateGrant,
    DivergentOwner,
    RowLevelSecurity,
    DisabledTrigger,
    HelperVolatility,
    InvalidTopologyIdentity,
}

impl ReadinessDrift {
    const fn label(self) -> &'static str {
        match self {
            Self::MissingExecute => "missing execute",
            Self::ExecuteGrantOption => "execute grant option",
            Self::DirectTablePrivilege => "direct table privilege",
            Self::UnexpectedExecute => "unexpected executable function",
            Self::FunctionOverload => "function overload",
            Self::StatusFunctionOverload => "status function overload",
            Self::SchemaCreateGrant => "untrusted schema create grant",
            Self::DivergentOwner => "divergent owner",
            Self::RowLevelSecurity => "row level security drift",
            Self::DisabledTrigger => "disabled trigger",
            Self::HelperVolatility => "digest helper volatility drift",
            Self::InvalidTopologyIdentity => "invalid topology identity",
        }
    }

    const fn expected(self) -> ProductDeploymentStatusReadinessErrorV1 {
        match self {
            Self::MissingExecute => ProductDeploymentStatusReadinessErrorV1::CapabilityMissing,
            Self::ExecuteGrantOption | Self::DirectTablePrivilege | Self::UnexpectedExecute => {
                ProductDeploymentStatusReadinessErrorV1::ExcessCapability
            }
            Self::FunctionOverload
            | Self::StatusFunctionOverload
            | Self::SchemaCreateGrant
            | Self::DivergentOwner
            | Self::RowLevelSecurity
            | Self::DisabledTrigger
            | Self::HelperVolatility
            | Self::InvalidTopologyIdentity => {
                ProductDeploymentStatusReadinessErrorV1::ContractMismatch
            }
        }
    }

    async fn apply(self, fixture: &StatusReadinessFixture) {
        match self {
            Self::MissingExecute => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE EXECUTE ON FUNCTION {STATUS_READ_FUNCTION} FROM {}",
                        fixture.reader_role
                    ))
                    .await;
            }
            Self::ExecuteGrantOption => {
                fixture
                    .execute_owner(&format!(
                        "GRANT EXECUTE ON FUNCTION {STATUS_READ_FUNCTION} TO {} WITH GRANT OPTION",
                        fixture.reader_role
                    ))
                    .await;
            }
            Self::DirectTablePrivilege => {
                fixture
                    .execute_owner(&format!(
                        "GRANT SELECT ON TABLE public.runtime_deployments TO {}",
                        fixture.reader_role
                    ))
                    .await;
            }
            Self::UnexpectedExecute => {
                fixture
                    .execute_owner(&format!(
                        "GRANT EXECUTE ON FUNCTION {UNEXPECTED_FUNCTION} TO {}",
                        fixture.reader_role
                    ))
                    .await;
            }
            Self::FunctionOverload => {
                fixture
                    .execute_owner(
                        "CREATE FUNCTION public.starring_canonical_json_v1(TEXT) \
                         RETURNS TEXT LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE \
                         SET search_path = pg_catalog AS 'SELECT $1'",
                    )
                    .await;
                fixture
                    .execute_owner(
                        "REVOKE ALL ON FUNCTION public.starring_canonical_json_v1(TEXT) FROM PUBLIC",
                    )
                    .await;
                fixture
                    .execute_owner(&format!(
                        "ALTER FUNCTION public.starring_canonical_json_v1(TEXT) OWNER TO {}",
                        fixture.owner_role
                    ))
                    .await;
            }
            Self::StatusFunctionOverload => {
                fixture
                    .execute_owner(
                        "CREATE FUNCTION public.\
                         starring_product_deployment_status_reader_database_identity_v1(TEXT) \
                         RETURNS TEXT LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE \
                         SET search_path = pg_catalog AS 'SELECT $1'",
                    )
                    .await;
                fixture
                    .execute_owner(
                        "REVOKE ALL ON FUNCTION public.\
                         starring_product_deployment_status_reader_database_identity_v1(TEXT) \
                         FROM PUBLIC",
                    )
                    .await;
                fixture
                    .execute_owner(&format!(
                        "ALTER FUNCTION public.\
                         starring_product_deployment_status_reader_database_identity_v1(TEXT) \
                         OWNER TO {}",
                        fixture.owner_role
                    ))
                    .await;
            }
            Self::SchemaCreateGrant => {
                fixture
                    .execute_owner(&format!(
                        "GRANT CREATE ON SCHEMA public TO {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::DivergentOwner => {
                fixture
                    .execute_owner(&format!(
                        "ALTER TABLE public.runtime_serving_leases OWNER TO {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::RowLevelSecurity => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.runtime_deployments ENABLE ROW LEVEL SECURITY",
                    )
                    .await;
            }
            Self::DisabledTrigger => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.runtime_deployments \
                         DISABLE TRIGGER runtime_deployments_validate_projection",
                    )
                    .await;
            }
            Self::HelperVolatility => {
                fixture
                    .execute_owner(&format!("ALTER FUNCTION {CANONICAL_HELPER} VOLATILE"))
                    .await;
            }
            Self::InvalidTopologyIdentity => {
                fixture
                    .execute_owner(
                        "CREATE OR REPLACE FUNCTION \
                         public.starring_product_deployment_status_reader_database_identity_v1() \
                         RETURNS TEXT LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE \
                         SECURITY DEFINER SET search_path = pg_catalog \
                         AS 'SELECT ''invalid-database-identity''::TEXT'",
                    )
                    .await;
            }
        }
    }

    async fn restore(self, fixture: &StatusReadinessFixture) {
        match self {
            Self::MissingExecute => {
                fixture.grant_status_execute().await;
            }
            Self::ExecuteGrantOption => {
                fixture.revoke_status_execute().await;
                fixture.grant_status_execute().await;
            }
            Self::DirectTablePrivilege => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE SELECT ON TABLE public.runtime_deployments FROM {}",
                        fixture.reader_role
                    ))
                    .await;
            }
            Self::UnexpectedExecute => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE EXECUTE ON FUNCTION {UNEXPECTED_FUNCTION} FROM {}",
                        fixture.reader_role
                    ))
                    .await;
            }
            Self::FunctionOverload => {
                fixture
                    .execute_owner("DROP FUNCTION public.starring_canonical_json_v1(TEXT)")
                    .await;
            }
            Self::StatusFunctionOverload => {
                fixture
                    .execute_owner(
                        "DROP FUNCTION public.\
                         starring_product_deployment_status_reader_database_identity_v1(TEXT)",
                    )
                    .await;
            }
            Self::SchemaCreateGrant => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE CREATE ON SCHEMA public FROM {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::DivergentOwner => {
                fixture
                    .execute_owner(&format!(
                        "ALTER TABLE public.runtime_serving_leases OWNER TO {}",
                        fixture.owner_role
                    ))
                    .await;
            }
            Self::RowLevelSecurity => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.runtime_deployments DISABLE ROW LEVEL SECURITY",
                    )
                    .await;
            }
            Self::DisabledTrigger => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.runtime_deployments \
                         ENABLE TRIGGER runtime_deployments_validate_projection",
                    )
                    .await;
            }
            Self::HelperVolatility => {
                fixture
                    .execute_owner(&format!("ALTER FUNCTION {CANONICAL_HELPER} IMMUTABLE"))
                    .await;
            }
            Self::InvalidTopologyIdentity => {
                fixture
                    .execute_owner(
                        "CREATE OR REPLACE FUNCTION \
                         public.starring_product_deployment_status_reader_database_identity_v1() \
                         RETURNS TEXT LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE \
                         SECURITY DEFINER SET search_path = pg_catalog \
                         AS 'SELECT identity.database_identity::TEXT \
                         FROM public.product_control_plane_identity AS identity \
                         WHERE identity.singleton'",
                    )
                    .await;
            }
        }
    }
}

impl StatusReadinessFixture {
    async fn new(label: &str) -> Self {
        let mut database = isolated_database(label).await;
        MIGRATOR.run(&database.owner_pool).await.unwrap();
        let role_suffix = suffix();
        let owner_role = format!("status_owner_{role_suffix}");
        let reader_role = format!("status_reader_{role_suffix}");
        let attacker_role = format!("status_attacker_{role_suffix}");
        for role in [&owner_role, &reader_role, &attacker_role] {
            assert_safe_identifier(role);
        }
        let reader_password = database_role_password();
        let password_literal =
            sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
                .bind(&reader_password)
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
            "CREATE ROLE {reader_role} LOGIN PASSWORD {password_literal} \
             NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
             NOBYPASSRLS CONNECTION LIMIT 4"
        ))
        .execute(&mut database.administrator)
        .await
        .unwrap();
        sqlx::query(&format!(
            "CREATE ROLE {attacker_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
        ))
        .execute(&mut database.administrator)
        .await
        .unwrap();
        for relation in RELATIONS {
            sqlx::query(&format!(
                "ALTER TABLE public.{relation} OWNER TO {owner_role}"
            ))
            .execute(&database.owner_pool)
            .await
            .unwrap();
        }
        for function in OWNED_FUNCTIONS {
            sqlx::query(&format!("ALTER FUNCTION {function} OWNER TO {owner_role}"))
                .execute(&database.owner_pool)
                .await
                .unwrap();
        }
        sqlx::query(&format!(
            "REVOKE ALL ON DATABASE {} FROM PUBLIC",
            database.name
        ))
        .execute(&database.owner_pool)
        .await
        .unwrap();
        sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
            .execute(&database.owner_pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE {} TO {reader_role}",
            database.name
        ))
        .execute(&database.owner_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT USAGE ON SCHEMA public TO {owner_role}, {reader_role}"
        ))
        .execute(&database.owner_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {STATUS_IDENTITY_FUNCTION}, \
             {STATUS_READ_FUNCTION} TO {reader_role}"
        ))
        .execute(&database.owner_pool)
        .await
        .unwrap();
        let reader_pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(
                database_url()
                    .parse::<PgConnectOptions>()
                    .unwrap()
                    .database(&database.name)
                    .username(&reader_role)
                    .password(&reader_password),
            )
            .await
            .unwrap();
        Self {
            database,
            owner_role,
            reader_role,
            attacker_role,
            reader_pool,
        }
    }

    fn statuses(&self) -> PostgresProductDeploymentStatuses {
        PostgresProductDeploymentStatuses::new(self.reader_pool.clone())
    }

    async fn execute_owner(&self, statement: &str) {
        sqlx::query(statement)
            .execute(&self.database.owner_pool)
            .await
            .unwrap();
    }

    async fn grant_status_execute(&self) {
        self.execute_owner(&format!(
            "GRANT EXECUTE ON FUNCTION {STATUS_READ_FUNCTION} TO {}",
            self.reader_role
        ))
        .await;
    }

    async fn revoke_status_execute(&self) {
        self.execute_owner(&format!(
            "REVOKE EXECUTE ON FUNCTION {STATUS_READ_FUNCTION} FROM {}",
            self.reader_role
        ))
        .await;
    }

    async fn close(self) {
        self.reader_pool.close().await;
        self.database.owner_pool.close().await;
        let mut administrator = self.database.administrator;
        sqlx::query(&format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.database.name
        ))
        .execute(&mut administrator)
        .await
        .unwrap();
        for role in [self.reader_role, self.attacker_role, self.owner_role] {
            assert_safe_identifier(&role);
            sqlx::query(&format!("DROP ROLE {role}"))
                .execute(&mut administrator)
                .await
                .unwrap();
        }
    }
}

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let options = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let database = options
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        database.starts_with("starring_")
            && database.split('_').any(|segment| segment == "test")
            && database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to use a database outside the strict Starring test namespace"
    );
    url
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

async fn isolated_database(label: &str) -> StatusReadinessDatabase {
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let label = label.chars().take(14).collect::<String>();
    let name = format!("starring_status_{label}_test_{}", suffix());
    assert_safe_identifier(&name);
    assert!(name.split('_').any(|segment| segment == "test"));
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
    StatusReadinessDatabase {
        name,
        administrator,
        owner_pool,
    }
}

async fn exercise_cases(label: &str, cases: &[ReadinessDrift]) {
    let fixture = StatusReadinessFixture::new(label).await;
    let outcome = AssertUnwindSafe(async {
        fixture.statuses().verify_readiness().await.unwrap();
        for case in cases {
            case.apply(&fixture).await;
            assert_eq!(
                fixture.statuses().verify_readiness().await,
                Err(case.expected()),
                "readiness accepted {}",
                case.label()
            );
            case.restore(&fixture).await;
            fixture
                .statuses()
                .verify_readiness()
                .await
                .unwrap_or_else(|error| {
                    panic!("readiness did not recover after {}: {error}", case.label())
                });
        }
    })
    .catch_unwind()
    .await;
    fixture.close().await;
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn status_readiness_rejects_missing_and_excess_capabilities() {
    exercise_cases(
        "capability",
        &[
            ReadinessDrift::MissingExecute,
            ReadinessDrift::ExecuteGrantOption,
            ReadinessDrift::DirectTablePrivilege,
            ReadinessDrift::UnexpectedExecute,
        ],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn status_readiness_rejects_persisted_contract_drift() {
    exercise_cases(
        "contract",
        &[
            ReadinessDrift::FunctionOverload,
            ReadinessDrift::StatusFunctionOverload,
            ReadinessDrift::SchemaCreateGrant,
            ReadinessDrift::DivergentOwner,
            ReadinessDrift::RowLevelSecurity,
            ReadinessDrift::DisabledTrigger,
            ReadinessDrift::HelperVolatility,
        ],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn status_readiness_rejects_invalid_database_topology() {
    exercise_cases("topology", &[ReadinessDrift::InvalidTopologyIdentity]).await;
}
