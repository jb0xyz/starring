mod keychain;

use std::collections::BTreeSet;
use std::str::FromStr;

use automation_instance_postgres::MIGRATOR;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, Row};
use thiserror::Error;

pub use keychain::{
    read_admin_url_from_keychain, AdminKeychainErrorV1, ADMIN_KEYCHAIN_ACCOUNT,
    ADMIN_KEYCHAIN_SERVICE,
};

pub const DATABASE_NAME: &str = "starring_runtime_staging";
pub const OWNER_ROLE: &str = "starring_owner";
pub const RELATION_COUNT: i64 = 171;
pub const CAPABILITY_FUNCTION_COUNT: usize = 108;
pub const CLUSTER_ADMIN_ROLE: &str = "starring_cluster_admin";
pub const PEER_MAP_NAME: &str = "starring_bootstrap";
pub const PEER_SOCKET_DIRECTORY: &str = "/private/tmp/starring-bootstrap";
pub const PEER_PORT: u16 = 5432;

const ADMIN_DATABASE: &str = "postgres";
const ADMIN_HOST: &str = "127.0.0.1";
const ADMIN_PORT: u16 = 5432;
const APPLICATION_NAME: &str = "starring-db-bootstrap";
const API_ROLE_BOOTSTRAP: &str =
    include_str!("../../../ops/postgres/staging-api-role-bootstrap.sql");
const RUNTIME_ROLE_BOOTSTRAP: &str =
    include_str!("../../../ops/postgres/staging-runtime-role-bootstrap.sql");

const CREATE_OWNER_SQL: &str = r#"
DO $starring_owner$
BEGIN
    IF pg_catalog.to_regrole('starring_owner') IS NULL THEN
        CREATE ROLE starring_owner
            NOLOGIN
            NOSUPERUSER
            NOCREATEDB
            NOCREATEROLE
            NOINHERIT
            NOREPLICATION
            NOBYPASSRLS
            CONNECTION LIMIT 0;
    END IF;
END;
$starring_owner$;

ALTER ROLE starring_owner
    NOLOGIN
    NOSUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOINHERIT
    NOREPLICATION
    NOBYPASSRLS
    CONNECTION LIMIT 0
    VALID UNTIL 'infinity'
    PASSWORD NULL;

ALTER ROLE starring_owner RESET ALL;
"#;

const NORMALIZE_CLUSTER_ADMIN_SQL: &str = r#"
DO $starring_cluster_admin_guard$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_authid AS role
        WHERE role.rolname = 'starring_cluster_admin'
            AND role.rolpassword IS NULL
    ) THEN
        RAISE EXCEPTION 'cluster administrator password precondition failed'
            USING ERRCODE = '55000';
    END IF;
END;
$starring_cluster_admin_guard$;

ALTER ROLE starring_cluster_admin
    LOGIN
    SUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOINHERIT
    NOREPLICATION
    NOBYPASSRLS
    CONNECTION LIMIT 2
    VALID UNTIL 'infinity'
    PASSWORD NULL;

ALTER ROLE starring_cluster_admin RESET ALL;

DO $starring_cluster_admin_database_settings$
DECLARE
    database_entry RECORD;
BEGIN
    FOR database_entry IN
        SELECT database_row.datname
        FROM pg_catalog.pg_database AS database_row
        ORDER BY database_row.datname
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER ROLE starring_cluster_admin IN DATABASE %I RESET ALL',
            database_entry.datname
        );
    END LOOP;
END;
$starring_cluster_admin_database_settings$;

DO $starring_cluster_admin_memberships$
DECLARE
    membership_entry RECORD;
BEGIN
    FOR membership_entry IN
        SELECT
            granted_role.rolname AS granted_role_name,
            member_role.rolname AS member_role_name,
            grantor_role.rolname AS grantor_role_name
        FROM pg_catalog.pg_auth_members AS membership
        INNER JOIN pg_catalog.pg_roles AS granted_role
            ON granted_role.oid = membership.roleid
        INNER JOIN pg_catalog.pg_roles AS member_role
            ON member_role.oid = membership.member
        INNER JOIN pg_catalog.pg_roles AS grantor_role
            ON grantor_role.oid = membership.grantor
        WHERE membership.roleid =
                pg_catalog.to_regrole('starring_cluster_admin')
            OR membership.member =
                pg_catalog.to_regrole('starring_cluster_admin')
        ORDER BY granted_role.rolname, member_role.rolname, grantor_role.rolname
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE %I FROM %I GRANTED BY %I CASCADE',
            membership_entry.granted_role_name,
            membership_entry.member_role_name,
            membership_entry.grantor_role_name
        );
    END LOOP;
END;
$starring_cluster_admin_memberships$;
"#;

const CLEAN_OWNER_MEMBERSHIPS_SQL: &str = r#"
DO $starring_owner_memberships$
DECLARE
    membership_entry RECORD;
BEGIN
    FOR membership_entry IN
        SELECT
            granted_role.rolname AS granted_role_name,
            member_role.rolname AS member_role_name,
            grantor_role.rolname AS grantor_role_name
        FROM pg_catalog.pg_auth_members AS membership
        INNER JOIN pg_catalog.pg_roles AS granted_role
            ON granted_role.oid = membership.roleid
        INNER JOIN pg_catalog.pg_roles AS member_role
            ON member_role.oid = membership.member
        INNER JOIN pg_catalog.pg_roles AS grantor_role
            ON grantor_role.oid = membership.grantor
        WHERE membership.roleid = pg_catalog.to_regrole('starring_owner')
            OR membership.member = pg_catalog.to_regrole('starring_owner')
        ORDER BY granted_role.rolname, member_role.rolname, grantor_role.rolname
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE %I FROM %I GRANTED BY %I CASCADE',
            membership_entry.granted_role_name,
            membership_entry.member_role_name,
            membership_entry.grantor_role_name
        );
    END LOOP;
END;
$starring_owner_memberships$;
"#;

const CREATE_DATABASE_SQL: &str = r#"
CREATE DATABASE starring_runtime_staging
    WITH OWNER = starring_owner
    TEMPLATE = template0
    ENCODING = 'UTF8'
"#;

const VERIFY_ADMIN_SQL: &str = r#"
SELECT
    pg_catalog.current_setting('server_version_num')::INTEGER,
    current_user = session_user,
    current_user = 'starring_cluster_admin',
    control.system_identifier::TEXT = $1,
    role.rolsuper
FROM pg_catalog.pg_roles AS role
CROSS JOIN pg_catalog.pg_control_system() AS control
WHERE role.rolname = current_user
"#;

const VERIFY_EXACT_ADMIN_SQL: &str = r#"
SELECT
    role.rolcanlogin
        AND role.rolsuper
        AND NOT role.rolcreatedb
        AND NOT role.rolcreaterole
        AND NOT role.rolinherit
        AND NOT role.rolreplication
        AND NOT role.rolbypassrls
        AND role.rolconnlimit = 2
        AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE
        AND role.rolpassword IS NULL,
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_db_role_setting AS setting
        WHERE setting.setrole = role.oid
    ),
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_auth_members AS membership
        WHERE membership.roleid = role.oid
            OR membership.member = role.oid
    )
FROM pg_catalog.pg_authid AS role
WHERE role.rolname = 'starring_cluster_admin'
"#;

const VERIFY_PEER_CONNECTION_SQL: &str = r#"
WITH hba_contract AS (
    SELECT
        pg_catalog.count(*) FILTER (WHERE rule.error IS NOT NULL) = 0
            AND pg_catalog.count(*) = 7
            AND pg_catalog.count(*) FILTER (
                WHERE rule.rule_number = 1
                    AND rule.type = 'local'
                    AND rule.database = ARRAY[
                        'postgres',
                        'starring_runtime_staging'
                    ]::TEXT[]
                    AND rule.user_name =
                        ARRAY['starring_cluster_admin']::TEXT[]
                    AND rule.address IS NULL
                    AND rule.netmask IS NULL
                    AND rule.auth_method = 'peer'
                    AND rule.options =
                        ARRAY['map=starring_bootstrap']::TEXT[]
            ) = 1
            AND pg_catalog.count(*) FILTER (
                WHERE rule.rule_number = 2
                    AND rule.type = 'host'
                    AND rule.database = ARRAY['all']::TEXT[]
                    AND rule.user_name = ARRAY['all']::TEXT[]
                    AND rule.address = '0.0.0.0'
                    AND rule.netmask = '0.0.0.0'
                    AND rule.auth_method = 'reject'
                    AND rule.options IS NULL
            ) = 1
            AND pg_catalog.count(*) FILTER (
                WHERE rule.rule_number = 3
                    AND rule.type = 'host'
                    AND rule.database = ARRAY['all']::TEXT[]
                    AND rule.user_name = ARRAY['all']::TEXT[]
                    AND rule.address = '::'
                    AND rule.netmask = '::'
                    AND rule.auth_method = 'reject'
                    AND rule.options IS NULL
            ) = 1
            AND pg_catalog.count(*) FILTER (
                WHERE rule.rule_number = 4
                    AND rule.type = 'local'
                    AND rule.database = ARRAY['all']::TEXT[]
                    AND rule.user_name = ARRAY['all']::TEXT[]
                    AND rule.address IS NULL
                    AND rule.netmask IS NULL
                    AND rule.auth_method = 'reject'
                    AND rule.options IS NULL
            ) = 1
            AND pg_catalog.count(*) FILTER (
                WHERE rule.rule_number = 5
                    AND rule.type = 'host'
                    AND rule.database = ARRAY['replication']::TEXT[]
                    AND rule.user_name = ARRAY['all']::TEXT[]
                    AND rule.address = '0.0.0.0'
                    AND rule.netmask = '0.0.0.0'
                    AND rule.auth_method = 'reject'
                    AND rule.options IS NULL
            ) = 1
            AND pg_catalog.count(*) FILTER (
                WHERE rule.rule_number = 6
                    AND rule.type = 'host'
                    AND rule.database = ARRAY['replication']::TEXT[]
                    AND rule.user_name = ARRAY['all']::TEXT[]
                    AND rule.address = '::'
                    AND rule.netmask = '::'
                    AND rule.auth_method = 'reject'
                    AND rule.options IS NULL
            ) = 1
            AND pg_catalog.count(*) FILTER (
                WHERE rule.rule_number = 7
                    AND rule.type = 'local'
                    AND rule.database = ARRAY['replication']::TEXT[]
                    AND rule.user_name = ARRAY['all']::TEXT[]
                    AND rule.address IS NULL
                    AND rule.netmask IS NULL
                    AND rule.auth_method = 'reject'
                    AND rule.options IS NULL
            ) = 1 AS exact
    FROM pg_catalog.pg_hba_file_rules AS rule
),
ident_contract AS (
    SELECT
        pg_catalog.count(*) FILTER (WHERE mapping.error IS NOT NULL) = 0
            AND pg_catalog.count(*) = 1
            AND pg_catalog.count(*) FILTER (
                WHERE mapping.map_name = 'starring_bootstrap'
                    AND mapping.sys_name = 'jungbogeon'
                    AND mapping.pg_username = 'starring_cluster_admin'
            ) = 1 AS exact
    FROM pg_catalog.pg_ident_file_mappings AS mapping
)
SELECT
    pg_catalog.inet_client_addr() IS NULL,
    current_user = 'starring_cluster_admin',
    hba_contract.exact,
    ident_contract.exact
FROM hba_contract
CROSS JOIN ident_contract
"#;

const VERIFY_OWNER_SQL: &str = r#"
SELECT
    NOT role.rolcanlogin
        AND NOT role.rolsuper
        AND NOT role.rolcreatedb
        AND NOT role.rolcreaterole
        AND NOT role.rolinherit
        AND NOT role.rolreplication
        AND NOT role.rolbypassrls
        AND role.rolconnlimit = 0
        AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE
        AND role.rolpassword IS NULL,
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_db_role_setting AS setting
        WHERE setting.setrole = role.oid
    ),
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_auth_members AS membership
        WHERE membership.roleid = role.oid
            OR membership.member = role.oid
    )
FROM pg_catalog.pg_authid AS role
WHERE role.rolname = 'starring_owner'
"#;

const VERIFY_DATABASE_SQL: &str = r#"
SELECT
    owner.rolname = 'starring_owner',
    database_row.datallowconn,
    NOT database_row.datistemplate,
    pg_catalog.pg_encoding_to_char(database_row.encoding) = 'UTF8'
FROM pg_catalog.pg_database AS database_row
INNER JOIN pg_catalog.pg_roles AS owner
    ON owner.oid = database_row.datdba
WHERE database_row.datname = 'starring_runtime_staging'
"#;

const VERIFY_DATABASE_ISOLATION_SQL: &str = r#"
SELECT
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.datname = 'starring_runtime_staging'
            AND activity.pid <> pg_catalog.pg_backend_pid()
            AND activity.backend_type = 'client backend'
    ),
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts AS prepared
        WHERE prepared.database = 'starring_runtime_staging'
    )
"#;

const VERIFY_RELATION_OWNERSHIP_SQL: &str = r#"
SELECT
    pg_catalog.count(*)::BIGINT,
    pg_catalog.count(*) FILTER (
        WHERE owner.rolname IS DISTINCT FROM 'starring_owner'
    )::BIGINT
FROM pg_catalog.pg_class AS relation
INNER JOIN pg_catalog.pg_namespace AS namespace
    ON namespace.oid = relation.relnamespace
LEFT JOIN pg_catalog.pg_roles AS owner
    ON owner.oid = relation.relowner
WHERE namespace.nspname <> 'information_schema'
    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
"#;

const VERIFY_CAPABILITY_OWNERSHIP_SQL: &str = r#"
SELECT
    expected.ordinality::BIGINT,
    function_row.oid IS NOT NULL
        AND namespace.nspname = 'public'
        AND function_row.prokind = 'f'
        AND owner.rolname = 'starring_owner'
FROM pg_catalog.unnest($1::TEXT[]) WITH ORDINALITY
    AS expected(identity, ordinality)
LEFT JOIN pg_catalog.pg_proc AS function_row
    ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
LEFT JOIN pg_catalog.pg_namespace AS namespace
    ON namespace.oid = function_row.pronamespace
LEFT JOIN pg_catalog.pg_roles AS owner
    ON owner.oid = function_row.proowner
ORDER BY expected.ordinality
"#;

const NORMALIZE_PUBLIC_SCHEMA_SQL: &str = r#"
ALTER SCHEMA public OWNER TO starring_owner
"#;

const VERIFY_PUBLIC_SCHEMA_SQL: &str = r#"
SELECT
    pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(owner.rolname = 'starring_owner')
FROM pg_catalog.pg_namespace AS namespace
INNER JOIN pg_catalog.pg_roles AS owner
    ON owner.oid = namespace.nspowner
WHERE namespace.nspname = 'public'
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum BootstrapErrorV1 {
    #[error("invalid_admin_url")]
    InvalidAdminUrl,
    #[error("admin_connection_failed")]
    AdminConnection,
    #[error("cluster_contract_failed")]
    ClusterContract,
    #[error("cluster_admin_bootstrap_failed")]
    ClusterAdminBootstrap,
    #[error("staging_acknowledgement_failed")]
    StagingAcknowledgement,
    #[error("owner_bootstrap_failed")]
    OwnerBootstrap,
    #[error("owner_membership_cleanup_failed")]
    OwnerMembershipCleanup,
    #[error("database_bootstrap_failed")]
    DatabaseBootstrap,
    #[error("public_schema_bootstrap_failed")]
    PublicSchemaBootstrap,
    #[error("database_isolation_failed")]
    DatabaseIsolation,
    #[error("target_connection_failed")]
    TargetConnection,
    #[error("target_role_failed")]
    TargetRole,
    #[error("migration_failed")]
    Migration,
    #[error("migration_ledger_failed")]
    MigrationLedger,
    #[error("relation_ownership_failed")]
    RelationOwnership,
    #[error("capability_manifest_failed")]
    CapabilityManifest,
    #[error("capability_ownership_failed")]
    CapabilityOwnership,
}

impl BootstrapErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidAdminUrl => "invalid_admin_url",
            Self::AdminConnection => "admin_connection_failed",
            Self::ClusterContract => "cluster_contract_failed",
            Self::ClusterAdminBootstrap => "cluster_admin_bootstrap_failed",
            Self::StagingAcknowledgement => "staging_acknowledgement_failed",
            Self::OwnerBootstrap => "owner_bootstrap_failed",
            Self::OwnerMembershipCleanup => "owner_membership_cleanup_failed",
            Self::DatabaseBootstrap => "database_bootstrap_failed",
            Self::PublicSchemaBootstrap => "public_schema_bootstrap_failed",
            Self::DatabaseIsolation => "database_isolation_failed",
            Self::TargetConnection => "target_connection_failed",
            Self::TargetRole => "target_role_failed",
            Self::Migration => "migration_failed",
            Self::MigrationLedger => "migration_ledger_failed",
            Self::RelationOwnership => "relation_ownership_failed",
            Self::CapabilityManifest => "capability_manifest_failed",
            Self::CapabilityOwnership => "capability_ownership_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootstrapReportV1 {
    migration_count: usize,
    relation_count: i64,
    capability_function_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapAuthenticationV1 {
    AuthenticatedUrl,
    TemporaryPeer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingAcknowledgementV1 {
    system_identifier: String,
}

impl StagingAcknowledgementV1 {
    pub fn parse(system_identifier: &str, acknowledgement: &str) -> Result<Self, BootstrapErrorV1> {
        if system_identifier.is_empty()
            || system_identifier.len() > 20
            || !system_identifier.bytes().all(|byte| byte.is_ascii_digit())
            || system_identifier.starts_with('0')
        {
            return Err(BootstrapErrorV1::StagingAcknowledgement);
        }
        let expected = format!(
            "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
        );
        if acknowledgement != expected {
            return Err(BootstrapErrorV1::StagingAcknowledgement);
        }
        Ok(Self {
            system_identifier: system_identifier.to_owned(),
        })
    }

    pub fn system_identifier(&self) -> &str {
        &self.system_identifier
    }
}

impl BootstrapReportV1 {
    pub const fn migration_count(self) -> usize {
        self.migration_count
    }

    pub const fn relation_count(self) -> i64 {
        self.relation_count
    }

    pub const fn capability_function_count(self) -> usize {
        self.capability_function_count
    }
}

pub fn parse_admin_connect_options(input: &str) -> Result<PgConnectOptions, BootstrapErrorV1> {
    let query = input.split_once('?').map(|(_, query)| query);
    if input.contains('#')
        || query.is_some_and(|query| {
            !matches!(
                query,
                "sslmode=disable"
                    | "sslmode=prefer"
                    | "sslmode=require"
                    | "sslmode=verify-ca"
                    | "sslmode=verify-full"
            )
        })
    {
        return Err(BootstrapErrorV1::InvalidAdminUrl);
    }
    PgConnectOptions::from_str(input).map_err(|_| BootstrapErrorV1::InvalidAdminUrl)
}

pub fn parse_keychain_admin_connect_options(
    input: &str,
) -> Result<PgConnectOptions, BootstrapErrorV1> {
    let prefix = format!("postgresql://{CLUSTER_ADMIN_ROLE}:");
    let suffix = format!("@{ADMIN_HOST}:{ADMIN_PORT}/{ADMIN_DATABASE}?sslmode=disable");
    let password = input
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
        .ok_or(BootstrapErrorV1::InvalidAdminUrl)?;
    if password.len() != 43
        || !password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(BootstrapErrorV1::InvalidAdminUrl);
    }
    let options = parse_admin_connect_options(input)?;
    if options.get_host() != ADMIN_HOST
        || options.get_port() != ADMIN_PORT
        || options.get_username() != CLUSTER_ADMIN_ROLE
        || options.get_database() != Some(ADMIN_DATABASE)
    {
        return Err(BootstrapErrorV1::InvalidAdminUrl);
    }
    Ok(options)
}

pub fn peer_bootstrap_connect_options() -> PgConnectOptions {
    PgConnectOptions::new()
        .host(PEER_SOCKET_DIRECTORY)
        .port(PEER_PORT)
        .username(CLUSTER_ADMIN_ROLE)
        .database(ADMIN_DATABASE)
        .ssl_mode(PgSslMode::Disable)
}

pub fn capability_function_identities() -> Result<Vec<&'static str>, BootstrapErrorV1> {
    let mut identities = extract_manifest(API_ROLE_BOOTSTRAP);
    identities.extend(extract_manifest(RUNTIME_ROLE_BOOTSTRAP));
    let unique = identities.iter().copied().collect::<BTreeSet<_>>();
    if identities.len() != CAPABILITY_FUNCTION_COUNT || unique.len() != CAPABILITY_FUNCTION_COUNT {
        return Err(BootstrapErrorV1::CapabilityManifest);
    }
    Ok(identities)
}

pub async fn bootstrap_staging_database(
    options: PgConnectOptions,
    acknowledgement: StagingAcknowledgementV1,
) -> Result<BootstrapReportV1, BootstrapErrorV1> {
    bootstrap_staging_database_with_authentication(
        options,
        BootstrapAuthenticationV1::AuthenticatedUrl,
        acknowledgement,
    )
    .await
}

pub async fn bootstrap_staging_database_with_authentication(
    options: PgConnectOptions,
    authentication: BootstrapAuthenticationV1,
    acknowledgement: StagingAcknowledgementV1,
) -> Result<BootstrapReportV1, BootstrapErrorV1> {
    let admin_options = options
        .clone()
        .database(ADMIN_DATABASE)
        .application_name(APPLICATION_NAME);
    let mut admin = PgConnection::connect_with(&admin_options)
        .await
        .map_err(|_| BootstrapErrorV1::AdminConnection)?;

    verify_cluster_admin(&mut admin, &acknowledgement).await?;
    if authentication == BootstrapAuthenticationV1::TemporaryPeer {
        verify_peer_connection(&mut admin).await?;
        normalize_cluster_admin(&mut admin).await?;
        verify_exact_cluster_admin(&mut admin).await?;
    }
    create_owner(&mut admin).await?;
    cleanup_owner_memberships(&mut admin).await?;
    verify_owner(&mut admin).await?;
    create_or_verify_database(&mut admin).await?;
    verify_database_isolation(&mut admin).await?;

    let target_options = options
        .database(DATABASE_NAME)
        .application_name(APPLICATION_NAME);
    let mut target = match PgConnection::connect_with(&target_options).await {
        Ok(target) => target,
        Err(_) => {
            cleanup_owner_memberships(&mut admin).await?;
            verify_owner(&mut admin).await?;
            return Err(BootstrapErrorV1::TargetConnection);
        }
    };
    if authentication == BootstrapAuthenticationV1::TemporaryPeer {
        if let Err(error) = verify_peer_connection(&mut target).await {
            drop(target);
            cleanup_owner_memberships(&mut admin).await?;
            verify_owner(&mut admin).await?;
            return Err(error);
        }
    }
    let operation = async {
        sqlx::query("SET ROLE starring_owner")
            .execute(&mut target)
            .await
            .map_err(|_| BootstrapErrorV1::TargetRole)?;
        let role_is_owner: bool = sqlx::query_scalar(
            "SELECT current_user = 'starring_owner' AND session_user <> current_user",
        )
        .fetch_one(&mut target)
        .await
        .map_err(|_| BootstrapErrorV1::TargetRole)?;
        if !role_is_owner {
            return Err(BootstrapErrorV1::TargetRole);
        }
        normalize_public_schema(&mut target).await?;
        migrate_and_verify(&mut target).await
    }
    .await;
    let reset = sqlx::query("RESET ROLE").execute(&mut target).await;
    drop(target);
    let cleanup = cleanup_owner_memberships(&mut admin).await;
    let owner = verify_owner(&mut admin).await;

    cleanup?;
    owner?;
    if reset.is_err() {
        return Err(BootstrapErrorV1::TargetRole);
    }
    operation
}

async fn verify_peer_connection(connection: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    let row = sqlx::query(VERIFY_PEER_CONNECTION_SQL)
        .fetch_optional(connection)
        .await
        .map_err(|_| BootstrapErrorV1::ClusterContract)?
        .ok_or(BootstrapErrorV1::ClusterContract)?;
    let unix_socket: bool = row
        .try_get(0)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    let exact_role: bool = row
        .try_get(1)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    let exact_hba_contract: bool = row
        .try_get(2)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    let exact_ident_contract: bool = row
        .try_get(3)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    if !unix_socket || !exact_role || !exact_hba_contract || !exact_ident_contract {
        return Err(BootstrapErrorV1::ClusterContract);
    }
    Ok(())
}

async fn verify_cluster_admin(
    admin: &mut PgConnection,
    acknowledgement: &StagingAcknowledgementV1,
) -> Result<(), BootstrapErrorV1> {
    let row = sqlx::query(VERIFY_ADMIN_SQL)
        .bind(acknowledgement.system_identifier())
        .fetch_optional(admin)
        .await
        .map_err(|_| BootstrapErrorV1::ClusterContract)?
        .ok_or(BootstrapErrorV1::ClusterContract)?;
    let version: i32 = row
        .try_get(0)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    let direct_session: bool = row
        .try_get(1)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    let exact_role: bool = row
        .try_get(2)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    let exact_system_identifier: bool = row
        .try_get(3)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    let superuser: bool = row
        .try_get(4)
        .map_err(|_| BootstrapErrorV1::ClusterContract)?;
    if !(160000..170000).contains(&version)
        || !direct_session
        || !exact_role
        || !exact_system_identifier
        || !superuser
    {
        return Err(BootstrapErrorV1::ClusterContract);
    }
    Ok(())
}

async fn normalize_cluster_admin(admin: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    sqlx::raw_sql(NORMALIZE_CLUSTER_ADMIN_SQL)
        .execute(admin)
        .await
        .map_err(|_| BootstrapErrorV1::ClusterAdminBootstrap)?;
    Ok(())
}

async fn verify_exact_cluster_admin(admin: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    let row = sqlx::query(VERIFY_EXACT_ADMIN_SQL)
        .fetch_optional(admin)
        .await
        .map_err(|_| BootstrapErrorV1::ClusterAdminBootstrap)?
        .ok_or(BootstrapErrorV1::ClusterAdminBootstrap)?;
    let attributes: bool = row
        .try_get(0)
        .map_err(|_| BootstrapErrorV1::ClusterAdminBootstrap)?;
    let settings: bool = row
        .try_get(1)
        .map_err(|_| BootstrapErrorV1::ClusterAdminBootstrap)?;
    let memberships: bool = row
        .try_get(2)
        .map_err(|_| BootstrapErrorV1::ClusterAdminBootstrap)?;
    if !attributes || !settings || !memberships {
        return Err(BootstrapErrorV1::ClusterAdminBootstrap);
    }
    Ok(())
}

async fn create_owner(admin: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    sqlx::raw_sql(CREATE_OWNER_SQL)
        .execute(admin)
        .await
        .map_err(|_| BootstrapErrorV1::OwnerBootstrap)?;
    Ok(())
}

async fn cleanup_owner_memberships(admin: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    sqlx::raw_sql(CLEAN_OWNER_MEMBERSHIPS_SQL)
        .execute(admin)
        .await
        .map_err(|_| BootstrapErrorV1::OwnerMembershipCleanup)?;
    Ok(())
}

async fn verify_owner(admin: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    let row = sqlx::query(VERIFY_OWNER_SQL)
        .fetch_optional(admin)
        .await
        .map_err(|_| BootstrapErrorV1::OwnerBootstrap)?
        .ok_or(BootstrapErrorV1::OwnerBootstrap)?;
    let attributes: bool = row
        .try_get(0)
        .map_err(|_| BootstrapErrorV1::OwnerBootstrap)?;
    let settings: bool = row
        .try_get(1)
        .map_err(|_| BootstrapErrorV1::OwnerBootstrap)?;
    let memberships: bool = row
        .try_get(2)
        .map_err(|_| BootstrapErrorV1::OwnerBootstrap)?;
    if !attributes || !settings || !memberships {
        return Err(BootstrapErrorV1::OwnerBootstrap);
    }
    Ok(())
}

async fn create_or_verify_database(admin: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    let exists: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) = 1 FROM pg_catalog.pg_database WHERE datname = 'starring_runtime_staging'",
    )
    .fetch_one(&mut *admin)
    .await
    .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?;
    if !exists {
        sqlx::raw_sql(CREATE_DATABASE_SQL)
            .execute(&mut *admin)
            .await
            .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?;
    }
    sqlx::query("ALTER ROLE starring_owner IN DATABASE starring_runtime_staging RESET ALL")
        .execute(&mut *admin)
        .await
        .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?;
    let row = sqlx::query(VERIFY_DATABASE_SQL)
        .fetch_optional(&mut *admin)
        .await
        .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?
        .ok_or(BootstrapErrorV1::DatabaseBootstrap)?;
    let exact_owner: bool = row
        .try_get(0)
        .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?;
    let allows_connections: bool = row
        .try_get(1)
        .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?;
    let not_template: bool = row
        .try_get(2)
        .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?;
    let utf8: bool = row
        .try_get(3)
        .map_err(|_| BootstrapErrorV1::DatabaseBootstrap)?;
    if !exact_owner || !allows_connections || !not_template || !utf8 {
        return Err(BootstrapErrorV1::DatabaseBootstrap);
    }
    Ok(())
}

async fn verify_database_isolation(admin: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    let row = sqlx::query(VERIFY_DATABASE_ISOLATION_SQL)
        .fetch_one(admin)
        .await
        .map_err(|_| BootstrapErrorV1::DatabaseIsolation)?;
    let no_sessions: bool = row
        .try_get(0)
        .map_err(|_| BootstrapErrorV1::DatabaseIsolation)?;
    let no_prepared: bool = row
        .try_get(1)
        .map_err(|_| BootstrapErrorV1::DatabaseIsolation)?;
    if !no_sessions || !no_prepared {
        return Err(BootstrapErrorV1::DatabaseIsolation);
    }
    Ok(())
}

async fn migrate_and_verify(
    target: &mut PgConnection,
) -> Result<BootstrapReportV1, BootstrapErrorV1> {
    MIGRATOR
        .run_direct(&mut *target)
        .await
        .map_err(|_| BootstrapErrorV1::Migration)?;
    let migration_count = verify_migration_ledger(&mut *target).await?;
    let relation_count = verify_relation_ownership(&mut *target).await?;
    let capability_function_count = verify_capability_ownership(&mut *target).await?;
    Ok(BootstrapReportV1 {
        migration_count,
        relation_count,
        capability_function_count,
    })
}

async fn normalize_public_schema(target: &mut PgConnection) -> Result<(), BootstrapErrorV1> {
    sqlx::raw_sql(NORMALIZE_PUBLIC_SCHEMA_SQL)
        .execute(&mut *target)
        .await
        .map_err(|_| BootstrapErrorV1::PublicSchemaBootstrap)?;
    let exact: bool = sqlx::query_scalar(VERIFY_PUBLIC_SCHEMA_SQL)
        .fetch_one(target)
        .await
        .map_err(|_| BootstrapErrorV1::PublicSchemaBootstrap)?;
    if !exact {
        return Err(BootstrapErrorV1::PublicSchemaBootstrap);
    }
    Ok(())
}

async fn verify_migration_ledger(target: &mut PgConnection) -> Result<usize, BootstrapErrorV1> {
    let expected = MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .collect::<Vec<_>>();
    let applied = sqlx::query(
        "SELECT version, success, checksum FROM public._sqlx_migrations ORDER BY version",
    )
    .fetch_all(target)
    .await
    .map_err(|_| BootstrapErrorV1::MigrationLedger)?;
    if applied.len() != expected.len() {
        return Err(BootstrapErrorV1::MigrationLedger);
    }
    for (row, migration) in applied.iter().zip(expected.iter()) {
        let version: i64 = row
            .try_get(0)
            .map_err(|_| BootstrapErrorV1::MigrationLedger)?;
        let success: bool = row
            .try_get(1)
            .map_err(|_| BootstrapErrorV1::MigrationLedger)?;
        let checksum: Vec<u8> = row
            .try_get(2)
            .map_err(|_| BootstrapErrorV1::MigrationLedger)?;
        if version != migration.version
            || !success
            || checksum.as_slice() != migration.checksum.as_ref()
        {
            return Err(BootstrapErrorV1::MigrationLedger);
        }
    }
    Ok(expected.len())
}

async fn verify_relation_ownership(target: &mut PgConnection) -> Result<i64, BootstrapErrorV1> {
    let row = sqlx::query(VERIFY_RELATION_OWNERSHIP_SQL)
        .fetch_one(target)
        .await
        .map_err(|_| BootstrapErrorV1::RelationOwnership)?;
    let relation_count: i64 = row
        .try_get(0)
        .map_err(|_| BootstrapErrorV1::RelationOwnership)?;
    let invalid_count: i64 = row
        .try_get(1)
        .map_err(|_| BootstrapErrorV1::RelationOwnership)?;
    if relation_count != RELATION_COUNT || invalid_count != 0 {
        return Err(BootstrapErrorV1::RelationOwnership);
    }
    Ok(relation_count)
}

async fn verify_capability_ownership(target: &mut PgConnection) -> Result<usize, BootstrapErrorV1> {
    let identities = capability_function_identities()?;
    let rows = sqlx::query(VERIFY_CAPABILITY_OWNERSHIP_SQL)
        .bind(&identities)
        .fetch_all(target)
        .await
        .map_err(|_| BootstrapErrorV1::CapabilityOwnership)?;
    if rows.len() != CAPABILITY_FUNCTION_COUNT {
        return Err(BootstrapErrorV1::CapabilityOwnership);
    }
    for (index, row) in rows.iter().enumerate() {
        let ordinal: i64 = row
            .try_get(0)
            .map_err(|_| BootstrapErrorV1::CapabilityOwnership)?;
        let valid: bool = row
            .try_get(1)
            .map_err(|_| BootstrapErrorV1::CapabilityOwnership)?;
        if ordinal != index as i64 + 1 || !valid {
            return Err(BootstrapErrorV1::CapabilityOwnership);
        }
    }
    Ok(rows.len())
}

fn extract_manifest(sql: &'static str) -> Vec<&'static str> {
    sql.lines()
        .filter_map(|line| {
            let start = line.find(", 'public.")? + 3;
            let remaining = &line[start..];
            let end = remaining.find("')")?;
            Some(&remaining[..end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_exact_and_unique() {
        let identities = capability_function_identities().unwrap();
        assert_eq!(identities.len(), 108);
        assert_eq!(
            identities.iter().copied().collect::<BTreeSet<_>>().len(),
            108
        );
        assert_eq!(extract_manifest(API_ROLE_BOOTSTRAP).len(), 53);
        assert_eq!(extract_manifest(RUNTIME_ROLE_BOOTSTRAP).len(), 55);
    }

    #[test]
    fn migration_ledger_source_is_ordered_and_unique() {
        let versions = MIGRATOR
            .iter()
            .filter(|migration| migration.migration_type.is_up_migration())
            .map(|migration| migration.version)
            .collect::<Vec<_>>();
        assert!(!versions.is_empty());
        assert!(versions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(MIGRATOR
            .iter()
            .all(|migration| migration.checksum.len() == 48));
    }

    #[test]
    fn admin_url_parser_accepts_only_connection_scoped_ssl_mode() {
        let options = parse_admin_connect_options(
            "postgresql://bootstrap:test-value@127.0.0.1:5432/postgres?sslmode=disable",
        )
        .unwrap();
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), 5432);
        assert_eq!(options.get_username(), "bootstrap");
        assert_eq!(options.get_database(), Some("postgres"));
        assert!(matches!(
            parse_admin_connect_options(
                "postgresql://bootstrap@127.0.0.1/postgres?sslkey=/tmp/client.key"
            ),
            Err(BootstrapErrorV1::InvalidAdminUrl)
        ));
        assert!(matches!(
            parse_admin_connect_options(
                "postgresql://bootstrap@127.0.0.1/postgres?options=-c%20role%3Dstarring_owner"
            ),
            Err(BootstrapErrorV1::InvalidAdminUrl)
        ));
    }

    #[test]
    fn keychain_admin_url_parser_accepts_only_the_fixed_staging_shape() {
        let password = "A".repeat(43);
        let valid = format!(
            "postgresql://{CLUSTER_ADMIN_ROLE}:{password}@{ADMIN_HOST}:{ADMIN_PORT}/{ADMIN_DATABASE}?sslmode=disable"
        );
        let options = parse_keychain_admin_connect_options(&valid).unwrap();
        assert_eq!(options.get_host(), ADMIN_HOST);
        assert_eq!(options.get_port(), ADMIN_PORT);
        assert_eq!(options.get_username(), CLUSTER_ADMIN_ROLE);
        assert_eq!(options.get_database(), Some(ADMIN_DATABASE));
        let wrong_database = format!(
            "postgresql://{CLUSTER_ADMIN_ROLE}:{password}@{ADMIN_HOST}:{ADMIN_PORT}/{DATABASE_NAME}?sslmode=disable"
        );
        for invalid in [
            valid.replace(CLUSTER_ADMIN_ROLE, "other"),
            valid.replace(ADMIN_HOST, "localhost"),
            valid.replace(&ADMIN_PORT.to_string(), "5433"),
            wrong_database,
            valid.replace("sslmode=disable", "sslmode=require"),
            valid.replace(&password, "short"),
            valid.replace(&password, &format!("{}+", "A".repeat(42))),
        ] {
            assert!(matches!(
                parse_keychain_admin_connect_options(&invalid),
                Err(BootstrapErrorV1::InvalidAdminUrl)
            ));
        }
    }

    #[test]
    fn fixed_identities_are_not_configurable() {
        assert_eq!(DATABASE_NAME, "starring_runtime_staging");
        assert_eq!(OWNER_ROLE, "starring_owner");
        assert_eq!(RELATION_COUNT, 171);
        assert_eq!(CAPABILITY_FUNCTION_COUNT, 108);
        assert_eq!(CLUSTER_ADMIN_ROLE, "starring_cluster_admin");
        assert_eq!(PEER_MAP_NAME, "starring_bootstrap");
        assert_eq!(PEER_SOCKET_DIRECTORY, "/private/tmp/starring-bootstrap");
        assert_eq!(PEER_PORT, 5432);
        assert_eq!(ADMIN_KEYCHAIN_SERVICE, "starring.postgres.staging");
        assert_eq!(ADMIN_KEYCHAIN_ACCOUNT, "database.cluster-admin");
        let peer = peer_bootstrap_connect_options();
        assert_eq!(peer.get_host(), "/private/tmp/starring-bootstrap");
        assert_eq!(peer.get_port(), 5432);
        assert_eq!(peer.get_username(), "starring_cluster_admin");
        assert_eq!(peer.get_database(), Some("postgres"));
    }

    #[test]
    fn staging_acknowledgement_is_exact() {
        let system_identifier = "7623456789012345678";
        let acknowledgement = format!(
            "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
        );
        assert_eq!(
            StagingAcknowledgementV1::parse(system_identifier, &acknowledgement)
                .unwrap()
                .system_identifier(),
            system_identifier
        );
        assert!(matches!(
            StagingAcknowledgementV1::parse(
                system_identifier,
                "starring-runtime-dedicated-staging-cluster-v2:wrong"
            ),
            Err(BootstrapErrorV1::StagingAcknowledgement)
        ));
        assert!(matches!(
            StagingAcknowledgementV1::parse("0", &acknowledgement),
            Err(BootstrapErrorV1::StagingAcknowledgement)
        ));
        assert!(matches!(
            StagingAcknowledgementV1::parse("not-a-system-id", &acknowledgement),
            Err(BootstrapErrorV1::StagingAcknowledgement)
        ));
    }

    #[test]
    fn errors_are_stable_and_redacted() {
        for error in [
            BootstrapErrorV1::InvalidAdminUrl,
            BootstrapErrorV1::AdminConnection,
            BootstrapErrorV1::ClusterContract,
            BootstrapErrorV1::ClusterAdminBootstrap,
            BootstrapErrorV1::StagingAcknowledgement,
            BootstrapErrorV1::OwnerBootstrap,
            BootstrapErrorV1::OwnerMembershipCleanup,
            BootstrapErrorV1::DatabaseBootstrap,
            BootstrapErrorV1::PublicSchemaBootstrap,
            BootstrapErrorV1::DatabaseIsolation,
            BootstrapErrorV1::TargetConnection,
            BootstrapErrorV1::TargetRole,
            BootstrapErrorV1::Migration,
            BootstrapErrorV1::MigrationLedger,
            BootstrapErrorV1::RelationOwnership,
            BootstrapErrorV1::CapabilityManifest,
            BootstrapErrorV1::CapabilityOwnership,
        ] {
            assert_eq!(error.to_string(), error.code());
        }
    }

    #[test]
    fn peer_bootstrap_contract_is_exact_and_hardened() {
        for required in [
            "LOGIN\n    SUPERUSER\n    NOCREATEDB\n    NOCREATEROLE\n    NOINHERIT\n    NOREPLICATION\n    NOBYPASSRLS",
            "CONNECTION LIMIT 2",
            "VALID UNTIL 'infinity'",
            "PASSWORD NULL",
            "ALTER ROLE starring_cluster_admin RESET ALL",
            "REVOKE %I FROM %I GRANTED BY %I CASCADE",
            "pg_catalog.pg_db_role_setting",
            "pg_catalog.pg_auth_members",
            "pg_catalog.count(*) = 7",
            "rule.rule_number = 7",
            "mapping.map_name = 'starring_bootstrap'",
            "mapping.sys_name = 'jungbogeon'",
            "mapping.pg_username = 'starring_cluster_admin'",
            "pg_catalog.count(*) = 1",
        ] {
            assert!(
                NORMALIZE_CLUSTER_ADMIN_SQL.contains(required)
                    || VERIFY_EXACT_ADMIN_SQL.contains(required)
                    || VERIFY_PEER_CONNECTION_SQL.contains(required),
                "{required}"
            );
        }
        assert!(NORMALIZE_CLUSTER_ADMIN_SQL.contains("role.rolpassword IS NULL"));
        assert!(VERIFY_EXACT_ADMIN_SQL.contains("NOT role.rolcreaterole"));
        assert!(VERIFY_EXACT_ADMIN_SQL.contains("role.rolconnlimit = 2"));
    }

    #[test]
    fn schema_and_relation_contracts_are_exact() {
        assert_eq!(
            NORMALIZE_PUBLIC_SCHEMA_SQL.trim(),
            "ALTER SCHEMA public OWNER TO starring_owner"
        );
        assert!(VERIFY_PUBLIC_SCHEMA_SQL.contains("owner.rolname = 'starring_owner'"));
        assert_eq!(RELATION_COUNT, 171);
    }
}
