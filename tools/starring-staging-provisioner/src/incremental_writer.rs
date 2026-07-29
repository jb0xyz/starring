use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, Postgres, Row};

use crate::crypto::{DatabaseSecretV1, SecretItemRefV1};
use crate::final_verify::{
    verify_admin_connection, verify_admin_target_connection, verify_application_connection,
    verify_final_hba,
};
use crate::identity::{
    exact_admin_connect_options, exact_admin_target_connect_options,
    exact_database_connect_options, KeychainIdentityV1, ADMIN_KEYCHAIN_IDENTITY,
    APPLICATION_DATABASE_IDENTITIES, AUTHORING_WRITER_IDENTITY, CLUSTER_ADMIN_ROLE, DATABASE_NAME,
    OWNER_ROLE,
};
use crate::keychain::KeychainClientV1;
use crate::postgres::alter_role_password_sql;
use crate::{ProvisionerErrorV1, StagingAcknowledgementV1};

const WRITER_ADVISORY_LOCK: i64 = 7_613_482_661_059_301_407;
const AUTHORING_WRITER_FUNCTIONS: [&str; 5] = [
    "public.starring_authoring_session_writer_database_identity_v1()",
    "public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])",
    "public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)",
    "public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)",
    "public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])",
];
const AUTHORING_WRITER_FUNCTION_NAMES: [&str; 5] = [
    "starring_authoring_session_writer_database_identity_v1",
    "starring_authoring_session_writer_check_v1",
    "starring_authoring_session_writer_load_v1",
    "starring_authoring_session_writer_commit_v1",
    "starring_authoring_session_writer_key_coverage_v1",
];
const SNAPSHOT_READER_V1: &str =
    "public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)";
const SNAPSHOT_READER_V2: &str =
    "public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)";
const VERIFY_SNAPSHOT_CAPABILITY_SQL: &str = r#"
WITH expected(version, identity) AS (
    VALUES
        (1, 'public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)'),
        (2, 'public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)')
),
resolved AS (
    SELECT
        expected.version,
        function_row.oid,
        function_row.proowner,
        function_row.prosecdef,
        function_row.prokind,
        function_row.proacl
    FROM expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
),
function_acl AS (
    SELECT resolved.version, privilege.*
    FROM resolved
    CROSS JOIN LATERAL pg_catalog.aclexplode(
        COALESCE(
            resolved.proacl,
            pg_catalog.acldefault('f', resolved.proowner)
        )
    ) AS privilege
),
contract AS (
    SELECT
        (SELECT pg_catalog.count(*) = 2
            AND pg_catalog.bool_and(
                resolved.oid IS NOT NULL
                AND resolved.proowner = pg_catalog.to_regrole('starring_owner')
                AND resolved.prosecdef
                AND resolved.prokind = 'f'
            )
         FROM resolved)
        AND (SELECT pg_catalog.count(*) = 3 FROM function_acl)
        AND (SELECT pg_catalog.count(*) = 2
             FROM function_acl
             WHERE function_acl.grantee =
                    pg_catalog.to_regrole('starring_owner'))
        AND NOT EXISTS (
            SELECT 1
            FROM function_acl
            WHERE function_acl.grantee NOT IN (
                    pg_catalog.to_regrole('starring_owner'),
                    pg_catalog.to_regrole('starring_authorized_snapshot_reader')
                )
                OR function_acl.grantor <>
                    pg_catalog.to_regrole('starring_owner')
                OR function_acl.privilege_type <> 'EXECUTE'
                OR function_acl.is_grantable
        ) AS common_exact
)
SELECT
    contract.common_exact
        AND (SELECT pg_catalog.count(*) = 1
             FROM function_acl
             WHERE function_acl.version = 1
                AND function_acl.grantee = pg_catalog.to_regrole(
                    'starring_authorized_snapshot_reader'
                ))
        AND (SELECT pg_catalog.count(*) = 0
             FROM function_acl
             WHERE function_acl.version = 2
                AND function_acl.grantee = pg_catalog.to_regrole(
                    'starring_authorized_snapshot_reader'
                )) AS legacy_exact,
    contract.common_exact
        AND (SELECT pg_catalog.count(*) = 0
             FROM function_acl
             WHERE function_acl.version = 1
                AND function_acl.grantee = pg_catalog.to_regrole(
                    'starring_authorized_snapshot_reader'
                ))
        AND (SELECT pg_catalog.count(*) = 1
             FROM function_acl
             WHERE function_acl.version = 2
                AND function_acl.grantee = pg_catalog.to_regrole(
                    'starring_authorized_snapshot_reader'
                )) AS cutover_exact
FROM contract
"#;
const VERIFY_MIGRATION_CONTRACT_SQL: &str = r#"
WITH expected(identity) AS (
    SELECT pg_catalog.unnest($1::TEXT[])
),
resolved AS (
    SELECT
        expected.identity,
        function_row.oid,
        function_row.proowner,
        function_row.prosecdef,
        function_row.prokind,
        function_row.proname,
        function_row.prolang,
        function_row.provolatile,
        function_row.proparallel,
        function_row.proconfig,
        function_row.proretset,
        function_row.proisstrict,
        function_row.proacl
    FROM expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
),
function_acl AS (
    SELECT privilege.*
    FROM resolved
    CROSS JOIN LATERAL pg_catalog.aclexplode(
        COALESCE(
            resolved.proacl,
            pg_catalog.acldefault('f', resolved.proowner)
        )
    ) AS privilege
)
SELECT
    (SELECT pg_catalog.count(*) = 5
        AND pg_catalog.bool_and(
            resolved.oid IS NOT NULL
            AND resolved.proowner = pg_catalog.to_regrole('starring_owner')
            AND resolved.prosecdef
            AND resolved.prokind = 'f'
            AND resolved.prolang = (
                SELECT language.oid
                FROM pg_catalog.pg_language AS language
                WHERE language.lanname = CASE
                    WHEN resolved.proname =
                        'starring_authoring_session_writer_database_identity_v1'
                    THEN 'sql'
                    ELSE 'plpgsql'
                END
            )
            AND resolved.provolatile = 'v'
            AND resolved.proparallel = 'u'
            AND resolved.proconfig IS NOT DISTINCT FROM CASE
                WHEN resolved.proname =
                    'starring_authoring_session_writer_commit_v1'
                THEN ARRAY['search_path=pg_catalog, public']::TEXT[]
                ELSE ARRAY['search_path=pg_catalog']::TEXT[]
            END
            AND resolved.proretset IS NOT DISTINCT FROM (
                resolved.proname <>
                    'starring_authoring_session_writer_database_identity_v1'
            )
            AND resolved.proisstrict IS NOT DISTINCT FROM (
                resolved.proname <>
                    'starring_authoring_session_writer_commit_v1'
            )
        )
     FROM resolved)
    AND (
        SELECT pg_catalog.count(*) = 5
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname = 'public'
            AND function_row.proname = ANY($2::TEXT[])
    )
    AND (
        SELECT pg_catalog.count(*) = CASE WHEN $3::BOOLEAN THEN 10 ELSE 5 END
            AND pg_catalog.bool_and(
                function_acl.grantee IN (
                    pg_catalog.to_regrole('starring_owner'),
                    CASE
                        WHEN $3::BOOLEAN
                        THEN pg_catalog.to_regrole(
                            'starring_authoring_session_writer'
                        )
                        ELSE pg_catalog.to_regrole('starring_owner')
                    END
                )
                AND function_acl.grantor = pg_catalog.to_regrole('starring_owner')
                AND function_acl.privilege_type = 'EXECUTE'
                AND NOT function_acl.is_grantable
            )
        FROM function_acl
    )
    AND pg_catalog.to_regprocedure(
        'public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)'
    ) IS NOT NULL
    AND (
        SELECT pg_catalog.count(*) = 5
            AND pg_catalog.bool_and(
                attribute.attnum IS NOT NULL
                AND pg_catalog.format_type(
                    attribute.atttypid,
                    attribute.atttypmod
                ) = expected_column.type_name
                AND NOT attribute.attnotnull
                AND NOT attribute.atthasdef
            )
        FROM (
            VALUES
                ('writer_semantic_request_digest', 'text'),
                ('writer_digest_key_id', 'text'),
                ('writer_digest_key_fingerprint', 'text'),
                ('safe_turn_projection', 'bytea'),
                ('safe_turn_projection_digest', 'text')
        ) AS expected_column(column_name, type_name)
        LEFT JOIN pg_catalog.pg_attribute AS attribute
            ON attribute.attrelid = pg_catalog.to_regclass(
                    'public.authoring_session_generations'
                )
                AND attribute.attname = expected_column.column_name
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
    )
    AND (
        SELECT pg_catalog.count(*) = 6
        FROM pg_catalog.pg_constraint AS constraint_row
        WHERE constraint_row.conrelid = pg_catalog.to_regclass(
                'public.authoring_session_generations'
            )
            AND constraint_row.contype = 'c'
            AND constraint_row.convalidated
            AND constraint_row.conname = ANY(ARRAY[
                'authoring_generations_writer_metadata_presence_valid',
                'authoring_generations_writer_semantic_digest_valid',
                'authoring_generations_writer_key_identity_valid',
                'authoring_generations_safe_projection_valid',
                'authoring_generations_trusted_stage_valid',
                'authoring_generations_trusted_candidate_valid'
            ]::TEXT[])
    )
    AND EXISTS (
        SELECT 1
        FROM public._sqlx_migrations AS migration
        WHERE migration.version = 202607300001
            AND migration.success
    )
"#;
const VERIFY_WRITER_CONTRACT_SQL: &str = r#"
WITH expected(identity) AS (
    SELECT pg_catalog.unnest($1::TEXT[])
),
resolved AS (
    SELECT
        function_row.oid,
        function_row.proowner,
        function_row.proacl
    FROM expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
),
function_acl AS (
    SELECT privilege.*
    FROM resolved
    CROSS JOIN LATERAL pg_catalog.aclexplode(
        COALESCE(
            resolved.proacl,
            pg_catalog.acldefault('f', resolved.proowner)
        )
    ) AS privilege
),
writer AS (
    SELECT role.*
    FROM pg_catalog.pg_authid AS role
    WHERE role.rolname = 'starring_authoring_session_writer'
)
SELECT
    (SELECT pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(
            writer.rolcanlogin
            AND NOT writer.rolsuper
            AND NOT writer.rolcreatedb
            AND NOT writer.rolcreaterole
            AND NOT writer.rolinherit
            AND NOT writer.rolreplication
            AND NOT writer.rolbypassrls
            AND writer.rolconnlimit = 4
            AND writer.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE
            AND writer.rolpassword LIKE 'SCRAM-SHA-256$4096:%'
        )
     FROM writer)
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_auth_members AS membership
        WHERE membership.roleid = pg_catalog.to_regrole(
                'starring_authoring_session_writer'
            )
            OR membership.member = pg_catalog.to_regrole(
                'starring_authoring_session_writer'
            )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_db_role_setting AS setting
        WHERE setting.setrole = pg_catalog.to_regrole(
            'starring_authoring_session_writer'
        )
    )
    AND pg_catalog.has_database_privilege(
        'starring_authoring_session_writer',
        'starring_runtime_staging',
        'CONNECT'
    )
    AND NOT pg_catalog.has_database_privilege(
        'starring_authoring_session_writer',
        'starring_runtime_staging',
        'CREATE,TEMPORARY'
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_database AS database_row
        WHERE database_row.datname <> 'starring_runtime_staging'
            AND pg_catalog.has_database_privilege(
                'starring_authoring_session_writer',
                database_row.oid,
                'CONNECT,CREATE,TEMPORARY'
            )
    )
    AND pg_catalog.has_schema_privilege(
        'starring_authoring_session_writer',
        'public',
        'USAGE'
    )
    AND NOT pg_catalog.has_schema_privilege(
        'starring_authoring_session_writer',
        'public',
        'CREATE'
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_namespace AS namespace
        WHERE namespace.nspname <> 'information_schema'
            AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
            AND namespace.nspname <> 'public'
            AND pg_catalog.has_schema_privilege(
                'starring_authoring_session_writer',
                namespace.oid,
                'USAGE,CREATE'
            )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public'
            AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
            AND pg_catalog.has_table_privilege(
                'starring_authoring_session_writer',
                relation.oid,
                'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'
            )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public'
            AND CASE
                WHEN relation.relkind = 'S'
                THEN pg_catalog.has_sequence_privilege(
                    'starring_authoring_session_writer',
                    relation.oid,
                    'USAGE,SELECT,UPDATE'
                )
                ELSE FALSE
            END
    )
    AND (
        SELECT pg_catalog.count(*) = 5
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname <> 'information_schema'
            AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
            AND pg_catalog.has_function_privilege(
                'starring_authoring_session_writer',
                function_row.oid,
                'EXECUTE'
            )
            AND function_row.oid IN (SELECT resolved.oid FROM resolved)
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname <> 'information_schema'
            AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
            AND pg_catalog.has_function_privilege(
                'starring_authoring_session_writer',
                function_row.oid,
                'EXECUTE'
            )
            AND function_row.oid NOT IN (SELECT resolved.oid FROM resolved)
    )
    AND NOT EXISTS (
        SELECT 1
        FROM resolved
        WHERE pg_catalog.has_function_privilege(
            'starring_authoring_session_writer',
            resolved.oid,
            'EXECUTE WITH GRANT OPTION'
        )
    )
    AND (
        SELECT pg_catalog.count(*) = 10
            AND pg_catalog.bool_and(
                function_acl.grantee IN (
                    pg_catalog.to_regrole('starring_owner'),
                    pg_catalog.to_regrole('starring_authoring_session_writer')
                )
                AND function_acl.grantor = pg_catalog.to_regrole('starring_owner')
                AND function_acl.privilege_type = 'EXECUTE'
                AND NOT function_acl.is_grantable
            )
        FROM function_acl
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_database AS database_row
        WHERE database_row.datdba = pg_catalog.to_regrole(
            'starring_authoring_session_writer'
        )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_namespace AS namespace
        WHERE namespace.nspowner = pg_catalog.to_regrole(
            'starring_authoring_session_writer'
        )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_class AS relation
        WHERE relation.relowner = pg_catalog.to_regrole(
            'starring_authoring_session_writer'
        )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.proowner = pg_catalog.to_regrole(
            'starring_authoring_session_writer'
        )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS defaults
        WHERE defaults.defaclrole = pg_catalog.to_regrole(
            'starring_authoring_session_writer'
        )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS defaults
        CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
        WHERE privilege.grantee IN (
            0,
            pg_catalog.to_regrole('starring_authoring_session_writer')
        )
    )
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncrementalAuthoringWriterOutcomeV1 {
    Created,
    ExactReplay,
}

impl IncrementalAuthoringWriterOutcomeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::ExactReplay => "exact_replay",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrementalAuthoringWriterReportV1 {
    outcome: IncrementalAuthoringWriterOutcomeV1,
}

impl IncrementalAuthoringWriterReportV1 {
    pub const fn outcome(self) -> IncrementalAuthoringWriterOutcomeV1 {
        self.outcome
    }

    pub const fn database(self) -> &'static str {
        DATABASE_NAME
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IncrementalStateV1 {
    Fresh,
    ExactReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapshotCapabilityV1 {
    Legacy,
    Cutover,
}

pub async fn provision_authoring_writer(
    acknowledgement: StagingAcknowledgementV1,
) -> Result<IncrementalAuthoringWriterReportV1, ProvisionerErrorV1> {
    let keychain = KeychainClientV1::new()?;
    let admin_value = keychain.read_required(ADMIN_KEYCHAIN_IDENTITY)?;
    let mut admin = PgConnection::connect_with(&exact_admin_connect_options(&admin_value)?)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseConnection)?;
    verify_admin_connection(&mut admin, acknowledgement.system_identifier()).await?;
    verify_final_hba(&mut admin).await?;
    drop(admin);

    let mut target = PgConnection::connect_with(&exact_admin_target_connect_options(&admin_value)?)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseConnection)?;
    verify_admin_target_connection(&mut target).await?;
    acquire_incremental_lock(&mut target).await?;
    verify_existing_cluster_contract(&mut target).await?;

    let role_exists = authoring_writer_role_exists(&mut target).await?;
    let writer_identity = writer_keychain_identity();
    let existing_secret = keychain.read_optional(writer_identity)?;
    if role_exists != existing_secret.is_some() {
        return Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState);
    }
    verify_migration_contract(&mut target, role_exists).await?;
    let snapshot_capability = load_snapshot_capability(&mut target).await?;
    let state =
        classify_incremental_state(role_exists, existing_secret.is_some(), snapshot_capability)?;

    if state == IncrementalStateV1::ExactReplay {
        let existing_secret =
            existing_secret.ok_or(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)?;
        verify_writer_contract(&mut target).await?;
        verify_writer_connection(&existing_secret).await?;
        return Ok(IncrementalAuthoringWriterReportV1 {
            outcome: IncrementalAuthoringWriterOutcomeV1::ExactReplay,
        });
    }

    let secret = DatabaseSecretV1::generate(AUTHORING_WRITER_IDENTITY)?;
    let item = SecretItemRefV1 {
        identity: writer_identity,
        value: secret.url(),
    };
    let keychain_update = keychain.begin_create(item)?;
    match apply_authoring_writer(&mut target, &secret).await {
        Ok(()) => {}
        Err(ProvisionerErrorV1::DatabaseCommitIndeterminate) => {
            keychain_update.commit();
            return Err(ProvisionerErrorV1::DatabaseCommitIndeterminate);
        }
        Err(error) => {
            keychain_update.rollback()?;
            return Err(error);
        }
    }

    let final_verification = async {
        verify_migration_contract(&mut target, true).await?;
        verify_snapshot_capability(&mut target, SnapshotCapabilityV1::Cutover).await?;
        verify_writer_contract(&mut target).await?;
        verify_writer_connection(secret.url()).await
    }
    .await;
    if let Err(error) = final_verification {
        if rollback_authoring_writer(&mut target, secret.verifier())
            .await
            .is_err()
        {
            keychain_update.commit();
            return Err(ProvisionerErrorV1::IncrementalAuthoringWriterRollback);
        }
        keychain_update.rollback()?;
        return Err(error);
    }
    keychain_update.commit();
    Ok(IncrementalAuthoringWriterReportV1 {
        outcome: IncrementalAuthoringWriterOutcomeV1::Created,
    })
}

fn writer_keychain_identity() -> KeychainIdentityV1 {
    KeychainIdentityV1 {
        service: AUTHORING_WRITER_IDENTITY.service,
        account: AUTHORING_WRITER_IDENTITY.account,
    }
}

fn classify_incremental_state(
    role_exists: bool,
    keychain_exists: bool,
    snapshot_capability: SnapshotCapabilityV1,
) -> Result<IncrementalStateV1, ProvisionerErrorV1> {
    match (role_exists, keychain_exists, snapshot_capability) {
        (false, false, SnapshotCapabilityV1::Legacy) => Ok(IncrementalStateV1::Fresh),
        (true, true, SnapshotCapabilityV1::Cutover) => Ok(IncrementalStateV1::ExactReplay),
        _ => Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState),
    }
}

async fn load_snapshot_capability<'e, E>(
    executor: E,
) -> Result<SnapshotCapabilityV1, ProvisionerErrorV1>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = sqlx::query(VERIFY_SNAPSHOT_CAPABILITY_SQL)
        .fetch_one(executor)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?;
    let legacy: bool = row
        .try_get(0)
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?;
    let cutover: bool = row
        .try_get(1)
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?;
    classify_snapshot_capability(legacy, cutover)
}

fn classify_snapshot_capability(
    legacy: bool,
    cutover: bool,
) -> Result<SnapshotCapabilityV1, ProvisionerErrorV1> {
    match (legacy, cutover) {
        (true, false) => Ok(SnapshotCapabilityV1::Legacy),
        (false, true) => Ok(SnapshotCapabilityV1::Cutover),
        _ => Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState),
    }
}

async fn verify_snapshot_capability<'e, E>(
    executor: E,
    expected: SnapshotCapabilityV1,
) -> Result<(), ProvisionerErrorV1>
where
    E: Executor<'e, Database = Postgres>,
{
    if load_snapshot_capability(executor).await? == expected {
        Ok(())
    } else {
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
    }
}

async fn acquire_incremental_lock(connection: &mut PgConnection) -> Result<(), ProvisionerErrorV1> {
    let acquired: bool = sqlx::query_scalar("SELECT pg_catalog.pg_try_advisory_lock($1)")
        .bind(WRITER_ADVISORY_LOCK)
        .fetch_one(connection)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?;
    if acquired {
        Ok(())
    } else {
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterBusy)
    }
}

async fn authoring_writer_role_exists(
    connection: &mut PgConnection,
) -> Result<bool, ProvisionerErrorV1> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_regrole('starring_authoring_session_writer') IS NOT NULL",
    )
    .fetch_one(connection)
    .await
    .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)
}

async fn verify_existing_cluster_contract(
    connection: &mut PgConnection,
) -> Result<(), ProvisionerErrorV1> {
    let roles = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .filter(|identity| **identity != AUTHORING_WRITER_IDENTITY)
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    let row = sqlx::query(
        "SELECT \
            (SELECT pg_catalog.count(*) = 19 AND pg_catalog.bool_and(role.rolcanlogin AND NOT role.rolsuper AND NOT role.rolcreatedb AND NOT role.rolcreaterole AND NOT role.rolinherit AND NOT role.rolreplication AND NOT role.rolbypassrls AND role.rolconnlimit = 4 AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND role.rolpassword LIKE 'SCRAM-SHA-256$4096:%') FROM pg_catalog.pg_authid AS role WHERE role.rolname = ANY($1::TEXT[])), \
            NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid IN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[])) OR membership.member IN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[]))), \
            NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting WHERE setting.setrole IN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[]))), \
            (SELECT NOT role.rolcanlogin AND NOT role.rolsuper AND NOT role.rolcreatedb AND NOT role.rolcreaterole AND NOT role.rolinherit AND NOT role.rolreplication AND NOT role.rolbypassrls AND role.rolconnlimit = 0 AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND role.rolpassword IS NULL AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid = role.oid OR membership.member = role.oid) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting WHERE setting.setrole = role.oid) FROM pg_catalog.pg_authid AS role WHERE role.rolname = $2), \
            (SELECT role.rolcanlogin AND role.rolsuper AND NOT role.rolcreatedb AND NOT role.rolcreaterole AND NOT role.rolinherit AND NOT role.rolreplication AND NOT role.rolbypassrls AND role.rolconnlimit = 2 AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND role.rolpassword LIKE 'SCRAM-SHA-256$4096:%' AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid = role.oid OR membership.member = role.oid) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting WHERE setting.setrole = role.oid) FROM pg_catalog.pg_authid AS role WHERE role.rolname = $3)",
    )
    .bind(&roles)
    .bind(OWNER_ROLE)
    .bind(CLUSTER_ADMIN_ROLE)
    .fetch_one(connection)
    .await
    .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?;
    for index in 0..5 {
        if !row
            .try_get::<bool, _>(index)
            .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?
        {
            return Err(ProvisionerErrorV1::IncrementalAuthoringWriterContract);
        }
    }
    Ok(())
}

async fn verify_migration_contract(
    connection: &mut PgConnection,
    provisioned: bool,
) -> Result<(), ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(VERIFY_MIGRATION_CONTRACT_SQL)
        .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
        .bind(AUTHORING_WRITER_FUNCTION_NAMES.as_slice())
        .bind(provisioned)
        .fetch_one(connection)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?;
    if exact {
        Ok(())
    } else {
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterContract)
    }
}

async fn verify_writer_contract(connection: &mut PgConnection) -> Result<(), ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(VERIFY_WRITER_CONTRACT_SQL)
        .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
        .fetch_one(connection)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterContract)?;
    if exact {
        Ok(())
    } else {
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
    }
}

async fn apply_authoring_writer(
    connection: &mut PgConnection,
    secret: &DatabaseSecretV1,
) -> Result<(), ProvisionerErrorV1> {
    let mut transaction = connection
        .begin()
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    for setting in [
        "SET LOCAL lock_timeout = '5s'",
        "SET LOCAL statement_timeout = '60s'",
        "SET LOCAL idle_in_transaction_session_timeout = '60s'",
    ] {
        sqlx::query(setting)
            .execute(&mut *transaction)
            .await
            .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    }
    let absent: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regrole('starring_authoring_session_writer') IS NULL",
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    if !absent {
        return Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState);
    }
    let migration_exact: bool = sqlx::query_scalar(VERIFY_MIGRATION_CONTRACT_SQL)
        .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
        .bind(AUTHORING_WRITER_FUNCTION_NAMES.as_slice())
        .bind(false)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    if !migration_exact {
        return Err(ProvisionerErrorV1::IncrementalAuthoringWriterContract);
    }
    verify_snapshot_capability(&mut *transaction, SnapshotCapabilityV1::Legacy).await?;
    sqlx::query(
        "CREATE ROLE starring_authoring_session_writer NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL 'infinity' PASSWORD NULL",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    let password_sql = alter_role_password_sql(AUTHORING_WRITER_IDENTITY.role, secret.verifier())?;
    sqlx::query(password_sql.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    sqlx::query("ALTER ROLE starring_authoring_session_writer LOGIN")
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    sqlx::query(
        "GRANT CONNECT ON DATABASE starring_runtime_staging TO starring_authoring_session_writer",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    sqlx::query("GRANT USAGE ON SCHEMA public TO starring_authoring_session_writer")
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    for function in AUTHORING_WRITER_FUNCTIONS {
        let grant =
            format!("GRANT EXECUTE ON FUNCTION {function} TO starring_authoring_session_writer");
        sqlx::query(&grant)
            .execute(&mut *transaction)
            .await
            .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    }
    cutover_snapshot_capability(&mut transaction).await?;
    let migration_exact: bool = sqlx::query_scalar(VERIFY_MIGRATION_CONTRACT_SQL)
        .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
        .bind(AUTHORING_WRITER_FUNCTION_NAMES.as_slice())
        .bind(true)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseVerification)?;
    if !migration_exact {
        return Err(ProvisionerErrorV1::DatabaseVerification);
    }
    verify_snapshot_capability(&mut *transaction, SnapshotCapabilityV1::Cutover).await?;
    let exact: bool = sqlx::query_scalar(VERIFY_WRITER_CONTRACT_SQL)
        .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseVerification)?;
    if !exact {
        return Err(ProvisionerErrorV1::DatabaseVerification);
    }
    transaction
        .commit()
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseCommitIndeterminate)
}

async fn verify_writer_connection(value: &[u8]) -> Result<(), ProvisionerErrorV1> {
    let options = exact_database_connect_options(value, AUTHORING_WRITER_IDENTITY)
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)?;
    let mut writer = PgConnection::connect_with(&options)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)?;
    verify_application_connection(&mut writer, AUTHORING_WRITER_IDENTITY.role)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)?;
    let capability_works: bool = sqlx::query_scalar(
        "SELECT pg_catalog.length(public.starring_authoring_session_writer_database_identity_v1()) BETWEEN 1 AND 128",
    )
    .fetch_one(&mut writer)
    .await
    .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)?;
    if capability_works {
        Ok(())
    } else {
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
    }
}

async fn cutover_snapshot_capability(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<(), ProvisionerErrorV1> {
    let revoke_snapshot_v1 = format!(
        "REVOKE EXECUTE ON FUNCTION {SNAPSHOT_READER_V1} FROM starring_authorized_snapshot_reader"
    );
    sqlx::query(&revoke_snapshot_v1)
        .execute(&mut **transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    let grant_snapshot_v2 = format!(
        "GRANT EXECUTE ON FUNCTION {SNAPSHOT_READER_V2} TO starring_authorized_snapshot_reader"
    );
    sqlx::query(&grant_snapshot_v2)
        .execute(&mut **transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
    Ok(())
}

async fn restore_snapshot_capability(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<(), ProvisionerErrorV1> {
    let revoke_snapshot_v2 = format!(
        "REVOKE EXECUTE ON FUNCTION {SNAPSHOT_READER_V2} FROM starring_authorized_snapshot_reader"
    );
    sqlx::query(&revoke_snapshot_v2)
        .execute(&mut **transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    let grant_snapshot_v1 = format!(
        "GRANT EXECUTE ON FUNCTION {SNAPSHOT_READER_V1} TO starring_authorized_snapshot_reader"
    );
    sqlx::query(&grant_snapshot_v1)
        .execute(&mut **transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    Ok(())
}

async fn rollback_authoring_writer(
    connection: &mut PgConnection,
    verifier: &str,
) -> Result<(), ProvisionerErrorV1> {
    let exact_password: bool = sqlx::query_scalar(
        "SELECT rolpassword = $1 FROM pg_catalog.pg_authid WHERE rolname = 'starring_authoring_session_writer'",
    )
    .bind(verifier)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?
    .ok_or(ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    if !exact_password
        || verify_migration_contract(connection, true).await.is_err()
        || verify_snapshot_capability(&mut *connection, SnapshotCapabilityV1::Cutover)
            .await
            .is_err()
        || verify_writer_contract(connection).await.is_err()
    {
        return Err(ProvisionerErrorV1::IncrementalAuthoringWriterRollback);
    }
    let mut transaction = connection
        .begin()
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    for setting in [
        "SET LOCAL lock_timeout = '5s'",
        "SET LOCAL statement_timeout = '60s'",
        "SET LOCAL idle_in_transaction_session_timeout = '60s'",
    ] {
        sqlx::query(setting)
            .execute(&mut *transaction)
            .await
            .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    }
    restore_snapshot_capability(&mut transaction).await?;
    verify_snapshot_capability(&mut *transaction, SnapshotCapabilityV1::Legacy)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    sqlx::query("ALTER ROLE starring_authoring_session_writer NOLOGIN")
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    for function in AUTHORING_WRITER_FUNCTIONS {
        let revoke = format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION {function} FROM starring_authoring_session_writer"
        );
        sqlx::query(&revoke)
            .execute(&mut *transaction)
            .await
            .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    }
    sqlx::query("REVOKE ALL PRIVILEGES ON SCHEMA public FROM starring_authoring_session_writer")
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    sqlx::query(
        "REVOKE ALL PRIVILEGES ON DATABASE starring_runtime_staging FROM starring_authoring_session_writer",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    let migration_restored: bool = sqlx::query_scalar(VERIFY_MIGRATION_CONTRACT_SQL)
        .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
        .bind(AUTHORING_WRITER_FUNCTION_NAMES.as_slice())
        .bind(false)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    if !migration_restored {
        return Err(ProvisionerErrorV1::IncrementalAuthoringWriterRollback);
    }
    sqlx::query("DROP ROLE starring_authoring_session_writer")
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)?;
    transaction
        .commit()
        .await
        .map_err(|_| ProvisionerErrorV1::IncrementalAuthoringWriterRollback)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERIFY_WRITER_DEFAULT_ACL_CONTRACT_SQL: &str = r#"
SELECT
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS defaults
        WHERE defaults.defaclrole = pg_catalog.to_regrole(
            'starring_authoring_session_writer'
        )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS defaults
        CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
        WHERE privilege.grantee IN (
            0,
            pg_catalog.to_regrole('starring_authoring_session_writer')
        )
    )
"#;

    #[test]
    fn state_machine_accepts_only_fresh_or_complete_replay() {
        assert_eq!(
            classify_snapshot_capability(true, false).unwrap(),
            SnapshotCapabilityV1::Legacy
        );
        assert_eq!(
            classify_snapshot_capability(false, true).unwrap(),
            SnapshotCapabilityV1::Cutover
        );
        assert!(classify_snapshot_capability(false, false).is_err());
        assert!(classify_snapshot_capability(true, true).is_err());
        assert_eq!(
            classify_incremental_state(false, false, SnapshotCapabilityV1::Legacy).unwrap(),
            IncrementalStateV1::Fresh
        );
        assert_eq!(
            classify_incremental_state(true, true, SnapshotCapabilityV1::Cutover).unwrap(),
            IncrementalStateV1::ExactReplay
        );
        assert_eq!(
            classify_incremental_state(false, true, SnapshotCapabilityV1::Legacy),
            Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
        );
        assert_eq!(
            classify_incremental_state(true, false, SnapshotCapabilityV1::Cutover),
            Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
        );
        assert_eq!(
            classify_incremental_state(false, false, SnapshotCapabilityV1::Cutover),
            Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
        );
        assert_eq!(
            classify_incremental_state(true, true, SnapshotCapabilityV1::Legacy),
            Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
        );
    }

    #[test]
    fn incremental_manifest_is_one_identity_and_five_exact_functions() {
        assert_eq!(
            writer_keychain_identity(),
            KeychainIdentityV1 {
                service: "starring-api.staging",
                account: "database.authoring-session-writer",
            }
        );
        assert_eq!(AUTHORING_WRITER_FUNCTIONS.len(), 5);
        assert_eq!(AUTHORING_WRITER_FUNCTION_NAMES.len(), 5);
        assert!(AUTHORING_WRITER_FUNCTIONS
            .iter()
            .all(|identity| identity.starts_with("public.starring_authoring_session_writer_")));
        assert_eq!(
            AUTHORING_WRITER_FUNCTIONS
                .iter()
                .filter(|identity| identity.contains("_commit_v1("))
                .count(),
            1
        );
        assert!(SNAPSHOT_READER_V1
            .ends_with("starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)"));
        assert!(SNAPSHOT_READER_V2
            .ends_with("starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)"));
        assert!(VERIFY_WRITER_CONTRACT_SQL.contains(
            "WHEN relation.relkind = 'S'\n                THEN pg_catalog.has_sequence_privilege"
        ));
    }

    #[test]
    fn writer_contract_rejects_owned_direct_and_public_default_acl_paths() {
        assert!(VERIFY_WRITER_CONTRACT_SQL.contains(
            "WHERE defaults.defaclrole = pg_catalog.to_regrole(\n            'starring_authoring_session_writer'\n        )"
        ));
        assert!(VERIFY_WRITER_CONTRACT_SQL.contains(
            "CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege\n        WHERE privilege.grantee IN (\n            0,\n            pg_catalog.to_regrole('starring_authoring_session_writer')\n        )"
        ));
        assert_eq!(
            VERIFY_WRITER_CONTRACT_SQL
                .matches("FROM pg_catalog.pg_default_acl AS defaults")
                .count(),
            2
        );
    }

    #[test]
    fn mutation_surface_names_only_the_incremental_role() {
        let source = include_str!("incremental_writer.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for statement in [
            "CREATE ROLE ",
            "ALTER ROLE ",
            "GRANT CONNECT ",
            "GRANT USAGE ",
            "GRANT EXECUTE ",
            "REVOKE ALL PRIVILEGES ",
            "DROP ROLE ",
        ] {
            for line in source.lines().filter(|line| line.contains(statement)) {
                assert!(
                    line.contains("starring_authoring_session_writer")
                        || line.contains("starring_authorized_snapshot_reader"),
                    "{line}"
                );
            }
        }
        for transition in [
            "REVOKE EXECUTE ON FUNCTION {SNAPSHOT_READER_V1} FROM starring_authorized_snapshot_reader",
            "GRANT EXECUTE ON FUNCTION {SNAPSHOT_READER_V2} TO starring_authorized_snapshot_reader",
            "REVOKE EXECUTE ON FUNCTION {SNAPSHOT_READER_V2} FROM starring_authorized_snapshot_reader",
            "GRANT EXECUTE ON FUNCTION {SNAPSHOT_READER_V1} TO starring_authorized_snapshot_reader",
        ] {
            assert_eq!(source.matches(transition).count(), 1, "{transition}");
        }
        assert!(!source.contains("keyring.product-action"));
        assert!(!source.contains("keyring.snapshot-envelope"));
        assert!(!source.contains("database.cluster-admin\""));
        assert!(source.contains("starring_authorized_snapshot_reader"));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore]
    async fn live_existing_staging_incremental_contract_queries_parse() {
        let keychain = KeychainClientV1::new().unwrap();
        let admin_value = keychain.read_required(ADMIN_KEYCHAIN_IDENTITY).unwrap();
        let mut target =
            PgConnection::connect_with(&exact_admin_target_connect_options(&admin_value).unwrap())
                .await
                .unwrap();
        verify_admin_target_connection(&mut target).await.unwrap();
        acquire_incremental_lock(&mut target).await.unwrap();
        verify_existing_cluster_contract(&mut target).await.unwrap();
        let initial_role_exists = authoring_writer_role_exists(&mut target).await.unwrap();
        let initial_keychain_exists = keychain
            .read_optional(writer_keychain_identity())
            .unwrap()
            .is_some();
        let initial_snapshot = sqlx::query(VERIFY_SNAPSHOT_CAPABILITY_SQL)
            .fetch_one(&mut target)
            .await
            .unwrap();
        let initial_snapshot = (
            initial_snapshot.try_get::<bool, _>(0).unwrap(),
            initial_snapshot.try_get::<bool, _>(1).unwrap(),
        );
        let _: bool = sqlx::query_scalar(VERIFY_MIGRATION_CONTRACT_SQL)
            .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
            .bind(AUTHORING_WRITER_FUNCTION_NAMES.as_slice())
            .bind(false)
            .fetch_one(&mut target)
            .await
            .unwrap();
        let mut snapshot_transaction = target.begin().await.unwrap();
        let snapshot_v2_exists: bool =
            sqlx::query_scalar("SELECT pg_catalog.to_regprocedure($1) IS NOT NULL")
                .bind(SNAPSHOT_READER_V2)
                .fetch_one(&mut *snapshot_transaction)
                .await
                .unwrap();
        if !snapshot_v2_exists {
            sqlx::query(
                "CREATE FUNCTION public.starring_product_authorized_snapshot_read_v2(TEXT,TEXT,BYTEA,TEXT,TEXT) RETURNS SETOF TEXT LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE SECURITY DEFINER SET search_path = pg_catalog AS 'SELECT NULL::TEXT WHERE FALSE'",
            )
            .execute(&mut *snapshot_transaction)
            .await
            .unwrap();
            sqlx::query(
                "ALTER FUNCTION public.starring_product_authorized_snapshot_read_v2(TEXT,TEXT,BYTEA,TEXT,TEXT) OWNER TO starring_owner",
            )
            .execute(&mut *snapshot_transaction)
            .await
            .unwrap();
            sqlx::query(
                "REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_authorized_snapshot_read_v2(TEXT,TEXT,BYTEA,TEXT,TEXT) FROM PUBLIC",
            )
            .execute(&mut *snapshot_transaction)
            .await
            .unwrap();
        }
        match load_snapshot_capability(&mut *snapshot_transaction)
            .await
            .unwrap()
        {
            SnapshotCapabilityV1::Legacy => {
                cutover_snapshot_capability(&mut snapshot_transaction)
                    .await
                    .unwrap();
                verify_snapshot_capability(
                    &mut *snapshot_transaction,
                    SnapshotCapabilityV1::Cutover,
                )
                .await
                .unwrap();
                restore_snapshot_capability(&mut snapshot_transaction)
                    .await
                    .unwrap();
                verify_snapshot_capability(
                    &mut *snapshot_transaction,
                    SnapshotCapabilityV1::Legacy,
                )
                .await
                .unwrap();
            }
            SnapshotCapabilityV1::Cutover => {
                restore_snapshot_capability(&mut snapshot_transaction)
                    .await
                    .unwrap();
                verify_snapshot_capability(
                    &mut *snapshot_transaction,
                    SnapshotCapabilityV1::Legacy,
                )
                .await
                .unwrap();
                cutover_snapshot_capability(&mut snapshot_transaction)
                    .await
                    .unwrap();
                verify_snapshot_capability(
                    &mut *snapshot_transaction,
                    SnapshotCapabilityV1::Cutover,
                )
                .await
                .unwrap();
            }
        }
        snapshot_transaction.rollback().await.unwrap();
        if initial_role_exists {
            let _: bool = sqlx::query_scalar(VERIFY_WRITER_CONTRACT_SQL)
                .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
                .fetch_one(&mut target)
                .await
                .unwrap();
            let exact: bool = sqlx::query_scalar(VERIFY_WRITER_DEFAULT_ACL_CONTRACT_SQL)
                .fetch_one(&mut target)
                .await
                .unwrap();
            assert!(exact);
            for statement in [
                "ALTER DEFAULT PRIVILEGES FOR ROLE starring_authoring_session_writer REVOKE ALL PRIVILEGES ON TABLES FROM starring_authoring_session_writer",
                "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner IN SCHEMA public GRANT SELECT ON TABLES TO starring_authoring_session_writer",
                "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner IN SCHEMA public GRANT USAGE ON SEQUENCES TO PUBLIC",
            ] {
                let mut transaction = target.begin().await.unwrap();
                sqlx::query(statement)
                    .execute(&mut *transaction)
                    .await
                    .unwrap();
                let exact: bool = sqlx::query_scalar(VERIFY_WRITER_DEFAULT_ACL_CONTRACT_SQL)
                    .fetch_one(&mut *transaction)
                    .await
                    .unwrap();
                assert!(!exact, "{statement}");
                transaction.rollback().await.unwrap();
            }
        } else {
            let verifier =
                crate::crypto::scram_verifier(b"contract-parse", b"0123456789abcdef").unwrap();
            let mut transaction = target.begin().await.unwrap();
            sqlx::query(
                "CREATE ROLE starring_authoring_session_writer LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL 'infinity' PASSWORD NULL",
            )
            .execute(&mut *transaction)
            .await
            .unwrap();
            let password_sql =
                alter_role_password_sql(AUTHORING_WRITER_IDENTITY.role, &verifier).unwrap();
            sqlx::query(password_sql.as_str())
                .execute(&mut *transaction)
                .await
                .unwrap();
            let _: bool = sqlx::query_scalar(VERIFY_WRITER_CONTRACT_SQL)
                .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
                .fetch_one(&mut *transaction)
                .await
                .unwrap();
            transaction.rollback().await.unwrap();
        }
        assert_eq!(
            authoring_writer_role_exists(&mut target).await.unwrap(),
            initial_role_exists
        );
        assert_eq!(
            keychain
                .read_optional(writer_keychain_identity())
                .unwrap()
                .is_some(),
            initial_keychain_exists
        );
        let final_snapshot = sqlx::query(VERIFY_SNAPSHOT_CAPABILITY_SQL)
            .fetch_one(&mut target)
            .await
            .unwrap();
        assert_eq!(
            (
                final_snapshot.try_get::<bool, _>(0).unwrap(),
                final_snapshot.try_get::<bool, _>(1).unwrap(),
            ),
            initial_snapshot
        );
    }
}
