use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::{
    PostgresProductRejections, ProductDecisionDigestKeyV1, ProductDecisionDigestKeyringV1,
    ProductDecisionReadinessErrorV1, MIGRATOR,
};
use futures::FutureExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

const REJECTION_IDENTITY_FUNCTION: &str =
    "public.starring_product_rejection_executor_database_identity_v1()";
const REJECTION_COVERAGE_FUNCTION: &str =
    "public.starring_product_rejection_keyring_coverage_v1(text[],text[])";
const REJECTION_FUNCTION: &str = "public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)";
const REJECTION_PROBE_QUERY: &str = "SELECT outcome, resulting_revision, resulting_state, \
    exact_replay, guild_id FROM public.starring_product_reject_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
    pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
    'invalid', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
    TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
    '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
    ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
    ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
    pg_catalog.repeat('7', 64), pg_catalog.repeat('8', 64), 'probe reason') LIMIT 2";
const RELATIONS: [&str; 16] = [
    "product_control_plane_identity",
    "activation_requests",
    "authoring_promotions",
    "product_tenants",
    "automation_installations",
    "automation_installation_authority_versions",
    "product_principals",
    "product_auth_sessions",
    "product_action_receipts",
    "product_action_receipt_idempotency_aliases",
    "product_audit_events",
    "product_action_receipt_audit_evidence",
    "activation_request_approvals",
    "automation_ruleset_activations",
    "automation_ruleset_versions",
    "runtime_deployments",
];
const SUPPORT_FUNCTIONS: [&str; 22] = [
    "public.assert_atomic_product_apply_runtime_request()",
    "public.assert_no_committed_product_activation_applying()",
    "public.assert_product_approval_receipt_alias()",
    "public.assert_product_approval_receipt_audit()",
    "public.capture_product_action_receipt_audit_evidence()",
    "public.enforce_activation_approval_payload_binding()",
    "public.enforce_activation_approval_scope()",
    "public.enforce_product_action_receipt_alias_capacity()",
    "public.enforce_product_action_receipt_alias_retention()",
    "public.enforce_product_action_receipt_retention()",
    "public.enforce_product_activation_executor()",
    "public.enforce_product_activation_journal_link()",
    "public.enforce_product_activation_scope()",
    "public.guard_legacy_activation_product_slot()",
    "public.guard_product_activation_applied_record()",
    "public.guard_product_ruleset_artifact_transition()",
    "public.reject_activation_approval_mutation()",
    "public.reject_immutable_product_approval_row()",
    "public.starring_runtime_desired_target_digest_v1(jsonb,bigint)",
    "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
];
const PROBE_SESSION_DIGEST: [u8; 32] = [61_u8; 32];
const PROBE_SUBJECT_DIGEST: [u8; 32] = [109_u8; 32];

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct RejectionReadinessDatabase {
    name: String,
    administrator: PgConnection,
    owner_pool: PgPool,
}

struct RejectionReadinessFixture {
    database: RejectionReadinessDatabase,
    owner_role: String,
    rejection_role: String,
    attacker_role: String,
    rejection_pool: PgPool,
}

#[derive(Clone, Copy)]
enum ReadinessDrift {
    MissingExecute,
    ExecuteGrantOption,
    DirectTablePrivilege,
    DirectColumnPrivilege,
    DirectSequencePrivilege,
    UnexpectedOrdinaryFunction,
    OtherSchemaFunction,
    PublicFunctionGrant,
    NamedExpectedFunctionGrant,
    CallerSchemaCreate,
    UntrustedSchemaCreate,
    RoleMembership,
    CreatedbRole,
    RelationOwner,
    RowLevelSecurity,
    FunctionVolatility,
    DisabledTrigger,
    InvalidDatabaseIdentity,
}

impl ReadinessDrift {
    const fn label(self) -> &'static str {
        match self {
            Self::MissingExecute => "missing execute",
            Self::ExecuteGrantOption => "execute grant option",
            Self::DirectTablePrivilege => "direct table privilege",
            Self::DirectColumnPrivilege => "direct column privilege",
            Self::DirectSequencePrivilege => "direct sequence privilege",
            Self::UnexpectedOrdinaryFunction => "unexpected ordinary function",
            Self::OtherSchemaFunction => "other schema function",
            Self::PublicFunctionGrant => "public function grant",
            Self::NamedExpectedFunctionGrant => "named expected function grant",
            Self::CallerSchemaCreate => "caller schema create",
            Self::UntrustedSchemaCreate => "untrusted schema create",
            Self::RoleMembership => "role membership",
            Self::CreatedbRole => "createdb role",
            Self::RelationOwner => "relation owner",
            Self::RowLevelSecurity => "row level security",
            Self::FunctionVolatility => "function volatility",
            Self::DisabledTrigger => "disabled trigger",
            Self::InvalidDatabaseIdentity => "invalid database identity",
        }
    }

    const fn expected(self) -> ProductDecisionReadinessErrorV1 {
        match self {
            Self::MissingExecute => ProductDecisionReadinessErrorV1::CapabilityMissing,
            Self::ExecuteGrantOption
            | Self::DirectTablePrivilege
            | Self::DirectColumnPrivilege
            | Self::DirectSequencePrivilege
            | Self::UnexpectedOrdinaryFunction
            | Self::OtherSchemaFunction
            | Self::PublicFunctionGrant
            | Self::NamedExpectedFunctionGrant
            | Self::CallerSchemaCreate
            | Self::RoleMembership
            | Self::CreatedbRole => ProductDecisionReadinessErrorV1::ExcessCapability,
            Self::UntrustedSchemaCreate
            | Self::RelationOwner
            | Self::RowLevelSecurity
            | Self::FunctionVolatility
            | Self::DisabledTrigger
            | Self::InvalidDatabaseIdentity => ProductDecisionReadinessErrorV1::ContractMismatch,
        }
    }

    async fn apply(self, fixture: &RejectionReadinessFixture) {
        match self {
            Self::MissingExecute => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE EXECUTE ON FUNCTION {REJECTION_FUNCTION} FROM {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::ExecuteGrantOption => {
                fixture
                    .execute_owner(&format!(
                        "GRANT EXECUTE ON FUNCTION {REJECTION_FUNCTION} TO {} WITH GRANT OPTION",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::DirectTablePrivilege => {
                fixture
                    .execute_owner(&format!(
                        "GRANT SELECT ON TABLE public.activation_requests TO {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::DirectColumnPrivilege => {
                fixture
                    .execute_owner(&format!(
                        "GRANT UPDATE (state) ON TABLE public.activation_requests TO {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::DirectSequencePrivilege => {
                fixture
                    .execute_owner(&format!(
                        "GRANT USAGE ON SEQUENCE public.rejection_readiness_sequence TO {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::UnexpectedOrdinaryFunction => {
                fixture
                    .execute_owner(
                        "CREATE FUNCTION public.rejection_readiness_escape() RETURNS INTEGER \
                         LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE \
                         SET search_path = pg_catalog AS 'SELECT 1'",
                    )
                    .await;
                fixture
                    .execute_owner(
                        "REVOKE ALL ON FUNCTION public.rejection_readiness_escape() FROM PUBLIC",
                    )
                    .await;
                fixture
                    .execute_owner(&format!(
                        "ALTER FUNCTION public.rejection_readiness_escape() OWNER TO {}",
                        fixture.owner_role
                    ))
                    .await;
                fixture
                    .execute_owner(&format!(
                        "GRANT EXECUTE ON FUNCTION public.rejection_readiness_escape() TO {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::OtherSchemaFunction => {
                fixture
                    .execute_owner(&format!(
                        "CREATE SCHEMA rejection_readiness_shadow AUTHORIZATION {}",
                        fixture.owner_role
                    ))
                    .await;
                fixture
                    .execute_owner(
                        "CREATE FUNCTION rejection_readiness_shadow.escape() RETURNS INTEGER \
                         LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE \
                         SET search_path = pg_catalog AS 'SELECT 1'",
                    )
                    .await;
                fixture
                    .execute_owner(
                        "REVOKE ALL ON FUNCTION rejection_readiness_shadow.escape() FROM PUBLIC",
                    )
                    .await;
                fixture
                    .execute_owner(&format!(
                        "ALTER FUNCTION rejection_readiness_shadow.escape() OWNER TO {}",
                        fixture.owner_role
                    ))
                    .await;
                fixture
                    .execute_owner(&format!(
                        "GRANT USAGE ON SCHEMA rejection_readiness_shadow TO {}",
                        fixture.rejection_role
                    ))
                    .await;
                fixture
                    .execute_owner(&format!(
                        "GRANT EXECUTE ON FUNCTION rejection_readiness_shadow.escape() TO {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::PublicFunctionGrant => {
                fixture
                    .execute_owner(
                        "CREATE FUNCTION public.rejection_readiness_public_escape() \
                         RETURNS INTEGER LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE \
                         SET search_path = pg_catalog AS 'SELECT 1'",
                    )
                    .await;
                fixture
                    .execute_owner(&format!(
                        "ALTER FUNCTION public.rejection_readiness_public_escape() OWNER TO {}",
                        fixture.owner_role
                    ))
                    .await;
                fixture
                    .execute_owner(
                        "GRANT EXECUTE ON FUNCTION \
                         public.rejection_readiness_public_escape() TO PUBLIC",
                    )
                    .await;
            }
            Self::NamedExpectedFunctionGrant => {
                fixture
                    .execute_owner(&format!(
                        "GRANT EXECUTE ON FUNCTION {REJECTION_IDENTITY_FUNCTION} TO {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::CallerSchemaCreate => {
                fixture
                    .execute_owner(&format!(
                        "GRANT CREATE ON SCHEMA public TO {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::UntrustedSchemaCreate => {
                fixture
                    .execute_owner(&format!(
                        "GRANT CREATE ON SCHEMA public TO {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::RoleMembership => {
                fixture
                    .execute_administrator(&format!(
                        "GRANT {} TO {}",
                        fixture.attacker_role, fixture.rejection_role
                    ))
                    .await;
            }
            Self::CreatedbRole => {
                fixture
                    .execute_administrator(&format!(
                        "ALTER ROLE {} CREATEDB",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::RelationOwner => {
                fixture
                    .execute_owner(&format!(
                        "ALTER TABLE public.runtime_deployments OWNER TO {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::RowLevelSecurity => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.activation_requests ENABLE ROW LEVEL SECURITY",
                    )
                    .await;
            }
            Self::FunctionVolatility => {
                fixture
                    .execute_owner(&format!("ALTER FUNCTION {REJECTION_FUNCTION} STABLE"))
                    .await;
            }
            Self::DisabledTrigger => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.activation_requests DISABLE TRIGGER \
                         activation_requests_enforce_product_executor",
                    )
                    .await;
            }
            Self::InvalidDatabaseIdentity => {
                fixture
                    .execute_owner(
                        "CREATE OR REPLACE FUNCTION \
                         public.starring_product_rejection_executor_database_identity_v1() \
                         RETURNS TEXT LANGUAGE sql VOLATILE STRICT SECURITY DEFINER \
                         PARALLEL UNSAFE SET search_path = pg_catalog \
                         AS 'SELECT ''invalid-database-identity''::TEXT'",
                    )
                    .await;
            }
        }
    }

    async fn restore(self, fixture: &RejectionReadinessFixture) {
        match self {
            Self::MissingExecute => fixture.grant_rejection_execute(REJECTION_FUNCTION).await,
            Self::ExecuteGrantOption => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE ALL ON FUNCTION {REJECTION_FUNCTION} FROM {}",
                        fixture.rejection_role
                    ))
                    .await;
                fixture.grant_rejection_execute(REJECTION_FUNCTION).await;
            }
            Self::DirectTablePrivilege => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE SELECT ON TABLE public.activation_requests FROM {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::DirectColumnPrivilege => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE UPDATE (state) ON TABLE public.activation_requests FROM {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::DirectSequencePrivilege => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE USAGE ON SEQUENCE public.rejection_readiness_sequence FROM {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::UnexpectedOrdinaryFunction => {
                fixture
                    .execute_owner("DROP FUNCTION public.rejection_readiness_escape()")
                    .await;
            }
            Self::OtherSchemaFunction => {
                fixture
                    .execute_owner("DROP SCHEMA rejection_readiness_shadow CASCADE")
                    .await;
            }
            Self::PublicFunctionGrant => {
                fixture
                    .execute_owner("DROP FUNCTION public.rejection_readiness_public_escape()")
                    .await;
            }
            Self::NamedExpectedFunctionGrant => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE EXECUTE ON FUNCTION {REJECTION_IDENTITY_FUNCTION} FROM {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::CallerSchemaCreate => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE CREATE ON SCHEMA public FROM {}",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::UntrustedSchemaCreate => {
                fixture
                    .execute_owner(&format!(
                        "REVOKE CREATE ON SCHEMA public FROM {}",
                        fixture.attacker_role
                    ))
                    .await;
            }
            Self::RoleMembership => {
                fixture
                    .execute_administrator(&format!(
                        "REVOKE {} FROM {}",
                        fixture.attacker_role, fixture.rejection_role
                    ))
                    .await;
            }
            Self::CreatedbRole => {
                fixture
                    .execute_administrator(&format!(
                        "ALTER ROLE {} NOCREATEDB",
                        fixture.rejection_role
                    ))
                    .await;
            }
            Self::RelationOwner => {
                fixture
                    .execute_owner(&format!(
                        "ALTER TABLE public.runtime_deployments OWNER TO {}",
                        fixture.owner_role
                    ))
                    .await;
            }
            Self::RowLevelSecurity => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.activation_requests DISABLE ROW LEVEL SECURITY",
                    )
                    .await;
            }
            Self::FunctionVolatility => {
                fixture
                    .execute_owner(&format!("ALTER FUNCTION {REJECTION_FUNCTION} VOLATILE"))
                    .await;
            }
            Self::DisabledTrigger => {
                fixture
                    .execute_owner(
                        "ALTER TABLE public.activation_requests ENABLE TRIGGER \
                         activation_requests_enforce_product_executor",
                    )
                    .await;
            }
            Self::InvalidDatabaseIdentity => {
                fixture
                    .execute_owner(
                        "CREATE OR REPLACE FUNCTION \
                         public.starring_product_rejection_executor_database_identity_v1() \
                         RETURNS TEXT LANGUAGE sql VOLATILE STRICT SECURITY DEFINER \
                         PARALLEL UNSAFE SET search_path = pg_catalog \
                         AS 'SELECT identity.database_identity::TEXT \
                         FROM public.product_control_plane_identity AS identity \
                         WHERE identity.singleton'",
                    )
                    .await;
            }
        }
    }
}

impl RejectionReadinessFixture {
    async fn new() -> Self {
        let mut database = isolated_database().await;
        MIGRATOR.run(&database.owner_pool).await.unwrap();
        let role_suffix = suffix();
        let owner_role = format!("rejection_owner_{role_suffix}");
        let rejection_role = format!("rejection_executor_{role_suffix}");
        let attacker_role = format!("rejection_attacker_{role_suffix}");
        for role in [&owner_role, &rejection_role, &attacker_role] {
            assert_safe_identifier(role);
        }
        let password = database_role_password();
        let password_literal =
            sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
                .bind(&password)
                .fetch_one(&database.owner_pool)
                .await
                .unwrap();
        for role in [&owner_role, &attacker_role] {
            sqlx::query(&format!(
                "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
                 NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
            ))
            .execute(&mut database.administrator)
            .await
            .unwrap();
        }
        sqlx::query(&format!(
            "CREATE ROLE {rejection_role} LOGIN PASSWORD {password_literal} \
             NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
             NOBYPASSRLS CONNECTION LIMIT 4"
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
        for function in [
            REJECTION_IDENTITY_FUNCTION,
            REJECTION_COVERAGE_FUNCTION,
            REJECTION_FUNCTION,
        ]
        .into_iter()
        .chain(SUPPORT_FUNCTIONS)
        {
            sqlx::query(&format!("ALTER FUNCTION {function} OWNER TO {owner_role}"))
                .execute(&database.owner_pool)
                .await
                .unwrap();
        }
        sqlx::query(&format!("ALTER SCHEMA public OWNER TO {owner_role}"))
            .execute(&database.owner_pool)
            .await
            .unwrap();
        for statement in [
            format!("REVOKE ALL ON DATABASE {} FROM PUBLIC", database.name),
            "REVOKE ALL ON SCHEMA public FROM PUBLIC".to_string(),
            "REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC".to_string(),
            "REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM PUBLIC".to_string(),
            "REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC".to_string(),
            format!(
                "GRANT CONNECT ON DATABASE {} TO {rejection_role}",
                database.name
            ),
            format!("GRANT USAGE ON SCHEMA public TO {owner_role}, {rejection_role}"),
        ] {
            sqlx::query(&statement)
                .execute(&database.owner_pool)
                .await
                .unwrap();
        }
        for function in [
            REJECTION_IDENTITY_FUNCTION,
            REJECTION_COVERAGE_FUNCTION,
            REJECTION_FUNCTION,
        ] {
            sqlx::query(&format!(
                "GRANT EXECUTE ON FUNCTION {function} TO {rejection_role}"
            ))
            .execute(&database.owner_pool)
            .await
            .unwrap();
        }
        for statement in [
            "CREATE SEQUENCE public.rejection_readiness_sequence OWNED BY NONE".to_string(),
            format!("ALTER SEQUENCE public.rejection_readiness_sequence OWNER TO {owner_role}"),
            "REVOKE ALL ON SEQUENCE public.rejection_readiness_sequence FROM PUBLIC".to_string(),
        ] {
            sqlx::query(&statement)
                .execute(&database.owner_pool)
                .await
                .unwrap();
        }
        let rejection_pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(
                database_url()
                    .parse::<PgConnectOptions>()
                    .unwrap()
                    .database(&database.name)
                    .username(&rejection_role)
                    .password(&password),
            )
            .await
            .unwrap();
        Self {
            database,
            owner_role,
            rejection_role,
            attacker_role,
            rejection_pool,
        }
    }

    fn rejections(&self) -> PostgresProductRejections {
        PostgresProductRejections::new(self.rejection_pool.clone(), readiness_keyring()).unwrap()
    }

    async fn execute_owner(&self, statement: &str) {
        self.database.owner_pool.execute(statement).await.unwrap();
    }

    async fn execute_administrator(&self, statement: &str) {
        let base = database_url().parse::<PgConnectOptions>().unwrap();
        let mut administrator = PgConnection::connect_with(&base.database("postgres"))
            .await
            .unwrap();
        administrator.execute(statement).await.unwrap();
    }

    async fn grant_rejection_execute(&self, function: &str) {
        self.execute_owner(&format!(
            "GRANT EXECUTE ON FUNCTION {function} TO {}",
            self.rejection_role
        ))
        .await;
    }

    async fn close(self) {
        self.rejection_pool.close().await;
        self.database.owner_pool.close().await;
        let mut administrator = self.database.administrator;
        sqlx::query(&format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.database.name
        ))
        .execute(&mut administrator)
        .await
        .unwrap();
        for role in [self.rejection_role, self.attacker_role, self.owner_role] {
            assert_safe_identifier(&role);
            sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
                .execute(&mut administrator)
                .await
                .unwrap();
        }
    }
}

fn readiness_keyring() -> ProductDecisionDigestKeyringV1 {
    ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes(
            "rejection-readiness-v1",
            std::array::from_fn(|index| 47_u8.wrapping_add(index as u8)),
        )
        .unwrap(),
        [],
    )
    .unwrap()
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

async fn isolated_database() -> RejectionReadinessDatabase {
    let name = format!("starring_rejection_test_{}", suffix());
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
    RejectionReadinessDatabase {
        name,
        administrator,
        owner_pool,
    }
}

async fn assert_permission_denied(pool: &PgPool, statement: &str) {
    let error = sqlx::query(statement).execute(pool).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("42501")
    );
}

async fn verify_baseline_capabilities(fixture: &RejectionReadinessFixture) {
    fixture
        .rejections()
        .verify_product_rejection_readiness()
        .await
        .unwrap();
    let capability = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool)>(
        "SELECT \
         pg_catalog.has_database_privilege(current_user, current_database(), 'CONNECT'), \
         pg_catalog.has_database_privilege(current_user, current_database(), 'TEMPORARY'), \
         pg_catalog.has_schema_privilege(current_user, 'public', 'USAGE'), \
         pg_catalog.has_schema_privilege(current_user, 'public', 'CREATE'), \
         pg_catalog.has_any_column_privilege(current_user, \
             'public.activation_requests', 'SELECT,INSERT,UPDATE,REFERENCES'), \
         pg_catalog.has_sequence_privilege(current_user, \
             'public.rejection_readiness_sequence', 'USAGE'), \
         EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership \
             CROSS JOIN pg_catalog.pg_roles AS role WHERE role.rolname = current_user \
             AND (membership.member = role.oid OR membership.roleid = role.oid))",
    )
    .fetch_one(&fixture.rejection_pool)
    .await
    .unwrap();
    assert_eq!(capability, (true, false, true, false, false, false, false));
    let relation_privilege_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
         ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname = 'public' \
         AND relation.relname = ANY($1::TEXT[]) \
         AND pg_catalog.has_table_privilege(current_user, relation.oid, \
             'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')",
    )
    .bind(RELATIONS)
    .fetch_one(&fixture.rejection_pool)
    .await
    .unwrap();
    assert_eq!(relation_privilege_count, 0);
    for statement in [
        "SELECT * FROM public.activation_requests LIMIT 1",
        "UPDATE public.activation_requests SET state = state",
        "SELECT pg_catalog.nextval('public.rejection_readiness_sequence')",
        "CREATE TEMPORARY TABLE rejection_readiness_escape(value INTEGER)",
        &format!(
            "CREATE TABLE public.rejection_readiness_escape_{}(value INTEGER)",
            suffix()
        ),
    ] {
        assert_permission_denied(&fixture.rejection_pool, statement).await;
    }
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {REJECTION_FUNCTION} TO PUBLIC"
    ))
    .execute(&fixture.rejection_pool)
    .await
    .unwrap();
    let public_execute = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(function_row.proacl, \
         pg_catalog.acldefault('f', function_row.proowner))) AS privilege \
         WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
         AND privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE')",
    )
    .bind(REJECTION_FUNCTION)
    .fetch_one(&fixture.database.owner_pool)
    .await
    .unwrap();
    assert!(!public_execute);
    let function_access = sqlx::query_as::<_, (i64, i64)>(
        "WITH expected(identity) AS (SELECT pg_catalog.unnest($1::TEXT[])) \
         SELECT pg_catalog.count(*) FILTER (WHERE pg_catalog.has_function_privilege( \
             current_user, pg_catalog.to_regprocedure(expected.identity), 'EXECUTE')), \
         pg_catalog.count(*) FILTER (WHERE pg_catalog.has_function_privilege( \
             current_user, pg_catalog.to_regprocedure(expected.identity), \
             'EXECUTE WITH GRANT OPTION')) FROM expected",
    )
    .bind([
        REJECTION_IDENTITY_FUNCTION,
        REJECTION_COVERAGE_FUNCTION,
        REJECTION_FUNCTION,
    ])
    .fetch_one(&fixture.rejection_pool)
    .await
    .unwrap();
    assert_eq!(function_access, (3, 0));
}

async fn verify_probe_modes_cannot_mutate(fixture: &RejectionReadinessFixture) {
    let initial_receipts = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.product_action_receipts",
    )
    .fetch_one(&fixture.database.owner_pool)
    .await
    .unwrap();
    let row = sqlx::query_as::<_, (String, Option<i64>, Option<String>, bool, Option<String>)>(
        REJECTION_PROBE_QUERY,
    )
    .bind(PROBE_SESSION_DIGEST.as_slice())
    .bind(PROBE_SUBJECT_DIGEST.as_slice())
    .fetch_one(&fixture.rejection_pool)
    .await
    .unwrap();
    assert_eq!(row, ("invalid_input".to_string(), None, None, false, None));
    let mut read_only = fixture.rejection_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ ONLY")
        .execute(&mut *read_only)
        .await
        .unwrap();
    let row = sqlx::query_as::<_, (String, Option<i64>, Option<String>, bool, Option<String>)>(
        REJECTION_PROBE_QUERY,
    )
    .bind(PROBE_SESSION_DIGEST.as_slice())
    .bind(PROBE_SUBJECT_DIGEST.as_slice())
    .fetch_one(&mut *read_only)
    .await
    .unwrap();
    assert_eq!(row, ("invalid_input".to_string(), None, None, false, None));
    read_only.commit().await.unwrap();
    let final_receipts = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.product_action_receipts",
    )
    .fetch_one(&fixture.database.owner_pool)
    .await
    .unwrap();
    assert_eq!(final_receipts, initial_receipts);
}

async fn seed_uncovered_rejection_receipt(fixture: &RejectionReadinessFixture) {
    let mut transaction = fixture.database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_action_receipts (receipt_id, tenant_id, \
         installation_id, principal_id, endpoint_domain, idempotency_key_digest, \
         idempotency_digest_key_id, idempotency_digest_key_fingerprint, request_digest, \
         target_resource_type, target_resource_id, resulting_revision, resulting_state, \
         result_code, http_disposition_class) VALUES (pg_catalog.repeat('a', 64), \
         'coverage_tenant', 'coverage_installation', 'coverage_principal', \
         'product_reject_v1', pg_catalog.repeat('b', 64), 'uncovered-key', \
         pg_catalog.repeat('c', 64), pg_catalog.repeat('d', 64), \
         'authoring_promotion', pg_catalog.repeat('e', 64), 2, 'rejected', \
         'promotion_rejected', 2)",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_action_receipt_idempotency_aliases (tenant_id, \
         installation_id, principal_id, endpoint_domain, idempotency_key_digest, \
         idempotency_digest_key_id, idempotency_digest_key_fingerprint, receipt_id) \
         VALUES ('coverage_tenant', 'coverage_installation', 'coverage_principal', \
         'product_reject_v1', pg_catalog.repeat('b', 64), 'uncovered-key', \
         pg_catalog.repeat('c', 64), pg_catalog.repeat('a', 64))",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn run_security_matrix(fixture: &RejectionReadinessFixture) {
    verify_baseline_capabilities(fixture).await;
    verify_probe_modes_cannot_mutate(fixture).await;
    for drift in [
        ReadinessDrift::MissingExecute,
        ReadinessDrift::ExecuteGrantOption,
        ReadinessDrift::DirectTablePrivilege,
        ReadinessDrift::DirectColumnPrivilege,
        ReadinessDrift::DirectSequencePrivilege,
        ReadinessDrift::UnexpectedOrdinaryFunction,
        ReadinessDrift::OtherSchemaFunction,
        ReadinessDrift::PublicFunctionGrant,
        ReadinessDrift::NamedExpectedFunctionGrant,
        ReadinessDrift::CallerSchemaCreate,
        ReadinessDrift::UntrustedSchemaCreate,
        ReadinessDrift::RoleMembership,
        ReadinessDrift::CreatedbRole,
        ReadinessDrift::RelationOwner,
        ReadinessDrift::RowLevelSecurity,
        ReadinessDrift::FunctionVolatility,
        ReadinessDrift::DisabledTrigger,
        ReadinessDrift::InvalidDatabaseIdentity,
    ] {
        drift.apply(fixture).await;
        assert_eq!(
            fixture
                .rejections()
                .verify_product_rejection_readiness()
                .await,
            Err(drift.expected()),
            "readiness accepted {}",
            drift.label()
        );
        drift.restore(fixture).await;
        fixture
            .rejections()
            .verify_product_rejection_readiness()
            .await
            .unwrap_or_else(|error| {
                panic!("readiness did not recover after {}: {error}", drift.label())
            });
    }
    seed_uncovered_rejection_receipt(fixture).await;
    assert_eq!(
        fixture
            .rejections()
            .verify_product_rejection_readiness()
            .await,
        Err(ProductDecisionReadinessErrorV1::IncompleteCoverage)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_rejection_readiness_security_matrix_runs_serially() {
    let fixture = RejectionReadinessFixture::new().await;
    let outcome = AssertUnwindSafe(run_security_matrix(&fixture))
        .catch_unwind()
        .await;
    fixture.close().await;
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
