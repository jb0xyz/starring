use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha384};
use sqlx::postgres::{PgConnectOptions, PgSslMode};

use super::*;

struct DisposableClusterV1 {
    bootstrap: PgConnection,
    base: PgConnectOptions,
    base_database: String,
    role_verifier: zeroize::Zeroizing<String>,
    admin_password: zeroize::Zeroizing<String>,
}

impl DisposableClusterV1 {
    async fn connect() -> Self {
        let raw_url = zeroize::Zeroizing::new(
            std::env::var("STARRING_PROVISIONER_LIFECYCLE_POSTGRES")
                .expect("STARRING_PROVISIONER_LIFECYCLE_POSTGRES required for ignored tests"),
        );
        let base = raw_url
            .as_str()
            .parse::<PgConnectOptions>()
            .expect("lifecycle PostgreSQL endpoint is invalid")
            .ssl_mode(PgSslMode::Disable);
        let base_database = base
            .get_database()
            .expect("lifecycle PostgreSQL endpoint must name a database")
            .to_owned();
        assert!(
            base_database.starts_with("starring_")
                && base_database.split('_').any(|segment| segment == "test")
                && base_database
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        );
        let mut bootstrap = PgConnection::connect_with(&base.clone().database("postgres"))
            .await
            .unwrap();
        let data_directory: String =
            sqlx::query_scalar("SELECT pg_catalog.current_setting('data_directory')")
                .fetch_one(&mut bootstrap)
                .await
                .unwrap();
        let server_version: i32 =
            sqlx::query_scalar("SELECT pg_catalog.current_setting('server_version_num')::INTEGER")
                .fetch_one(&mut bootstrap)
                .await
                .unwrap();
        let isolated: bool = sqlx::query_scalar(
            "SELECT current_setting('is_superuser') = 'on' \
             AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname LIKE 'starring\\_%' ESCAPE '\\') \
             AND pg_catalog.to_regrole('starring_owner') IS NULL \
             AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_database WHERE datname = 'starring_runtime_staging')",
        )
        .fetch_one(&mut bootstrap)
        .await
        .unwrap();
        assert!(
            (160_000..170_000).contains(&server_version)
                && disposable_data_directory(&data_directory)
                && isolated
        );
        let role_secret = DatabaseSecretV1::generate(AUTHORING_WRITER_IDENTITY).unwrap();
        let admin_password = zeroize::Zeroizing::new(
            DatabaseSecretV1::generate(AUTHORING_WRITER_IDENTITY)
                .unwrap()
                .password()
                .to_owned(),
        );
        let mut salt = [0_u8; 16];
        getrandom::fill(&mut salt).unwrap();
        let role_verifier =
            crate::crypto::scram_verifier(role_secret.password().as_bytes(), &salt).unwrap();
        Self {
            bootstrap,
            base,
            base_database,
            role_verifier,
            admin_password,
        }
    }

    async fn bootstrap_staging(&mut self) {
        create_fixed_role(
            &mut self.bootstrap,
            OWNER_ROLE,
            "NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0 VALID UNTIL 'infinity' PASSWORD NULL",
            None,
        )
        .await;
        let admin_verifier = {
            let mut salt = [0_u8; 16];
            getrandom::fill(&mut salt).unwrap();
            crate::crypto::scram_verifier(self.admin_password.as_bytes(), &salt).unwrap()
        };
        create_fixed_role(
            &mut self.bootstrap,
            CLUSTER_ADMIN_ROLE,
            "NOLOGIN SUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 2 VALID UNTIL 'infinity' PASSWORD NULL",
            Some(admin_verifier.as_str()),
        )
        .await;
        sqlx::query("ALTER ROLE starring_cluster_admin LOGIN")
            .execute(&mut self.bootstrap)
            .await
            .unwrap();
        for identity in APPLICATION_DATABASE_IDENTITIES
            .iter()
            .filter(|identity| **identity != AUTHORING_WRITER_IDENTITY)
        {
            create_fixed_role(
                &mut self.bootstrap,
                identity.role,
                "NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL 'infinity' PASSWORD NULL",
                Some(self.role_verifier.as_str()),
            )
            .await;
            sqlx::query(&format!("ALTER ROLE {} LOGIN", identity.role))
                .execute(&mut self.bootstrap)
                .await
                .unwrap();
        }
        sqlx::query("CREATE DATABASE starring_runtime_staging OWNER starring_owner")
            .execute(&mut self.bootstrap)
            .await
            .unwrap();
        let databases =
            sqlx::query_scalar::<_, String>("SELECT datname FROM pg_catalog.pg_database")
                .fetch_all(&mut self.bootstrap)
                .await
                .unwrap();
        for database in databases {
            assert_safe_identifier(&database);
            sqlx::query(&format!(
                "REVOKE CONNECT,CREATE,TEMPORARY ON DATABASE {database} FROM PUBLIC"
            ))
            .execute(&mut self.bootstrap)
            .await
            .unwrap();
        }
        for identity in APPLICATION_DATABASE_IDENTITIES
            .iter()
            .filter(|identity| **identity != AUTHORING_WRITER_IDENTITY)
        {
            sqlx::query(&format!(
                "GRANT CONNECT ON DATABASE starring_runtime_staging TO {}",
                identity.role
            ))
            .execute(&mut self.bootstrap)
            .await
            .unwrap();
        }
        let mut owner = PgConnection::connect_with(&self.base.clone().database(DATABASE_NAME))
            .await
            .unwrap();
        sqlx::query("SET ROLE starring_owner")
            .execute(&mut owner)
            .await
            .unwrap();
        sqlx::query("REVOKE CREATE ON SCHEMA public FROM PUBLIC")
            .execute(&mut owner)
            .await
            .unwrap();
        apply_all_migrations(&mut owner).await;
        for statement in [
            "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner \
             REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC",
            "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner \
             REVOKE USAGE ON TYPES FROM PUBLIC",
        ] {
            sqlx::query(statement).execute(&mut owner).await.unwrap();
        }
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {SNAPSHOT_READER_V1} TO starring_authorized_snapshot_reader"
        ))
        .execute(&mut owner)
        .await
        .unwrap();
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION {SNAPSHOT_READER_V2} FROM starring_authorized_snapshot_reader"
        ))
        .execute(&mut owner)
        .await
        .unwrap();
    }

    async fn admin_target(&self) -> PgConnection {
        PgConnection::connect_with(
            &self
                .base
                .clone()
                .database(DATABASE_NAME)
                .username(CLUSTER_ADMIN_ROLE)
                .password(self.admin_password.as_str()),
        )
        .await
        .unwrap()
    }

    async fn writer_target(&self, secret: &DatabaseSecretV1) -> Result<PgConnection, sqlx::Error> {
        PgConnection::connect_with(
            &self
                .base
                .clone()
                .database(DATABASE_NAME)
                .username(AUTHORING_WRITER_IDENTITY.role)
                .password(secret.password()),
        )
        .await
    }

    async fn cleanup(mut self) {
        sqlx::query("DROP DATABASE starring_runtime_staging WITH (FORCE)")
            .execute(&mut self.bootstrap)
            .await
            .unwrap();
        for identity in APPLICATION_DATABASE_IDENTITIES.iter().rev() {
            if authoring_writer_role_exists(&mut self.bootstrap)
                .await
                .unwrap_or(false)
                && *identity == AUTHORING_WRITER_IDENTITY
            {
                sqlx::query("DROP ROLE starring_authoring_session_writer")
                    .execute(&mut self.bootstrap)
                    .await
                    .unwrap();
            } else if *identity != AUTHORING_WRITER_IDENTITY {
                sqlx::query(&format!("DROP ROLE {}", identity.role))
                    .execute(&mut self.bootstrap)
                    .await
                    .unwrap();
            }
        }
        sqlx::query("DROP ROLE starring_cluster_admin")
            .execute(&mut self.bootstrap)
            .await
            .unwrap();
        sqlx::query("DROP ROLE starring_owner")
            .execute(&mut self.bootstrap)
            .await
            .unwrap();
        let remains: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname LIKE 'starring\\_%' ESCAPE '\\') \
             OR EXISTS (SELECT 1 FROM pg_catalog.pg_database WHERE datname = 'starring_runtime_staging')",
        )
        .fetch_one(&mut self.bootstrap)
        .await
        .unwrap();
        assert!(!remains);
        assert!(self
            .base_database
            .split('_')
            .any(|segment| segment == "test"));
    }
}

fn disposable_data_directory(value: &str) -> bool {
    Path::new(value).starts_with("/tmp") || Path::new(value).starts_with("/private/tmp")
}

fn assert_safe_identifier(value: &str) {
    assert!(
        !value.is_empty()
            && value.len() <= 63
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
}

async fn create_fixed_role(
    connection: &mut PgConnection,
    role: &str,
    attributes: &str,
    verifier: Option<&str>,
) {
    assert_safe_identifier(role);
    sqlx::query(&format!("CREATE ROLE {role} {attributes}"))
        .execute(&mut *connection)
        .await
        .unwrap();
    if let Some(verifier) = verifier {
        let password_sql = alter_role_password_sql(role, verifier).unwrap();
        sqlx::query(&password_sql)
            .execute(&mut *connection)
            .await
            .unwrap();
    }
}

fn migration_files() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let mut files = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

async fn apply_all_migrations(connection: &mut PgConnection) {
    sqlx::query(
        "CREATE TABLE public._sqlx_migrations (\
         version BIGINT PRIMARY KEY,\
         description TEXT NOT NULL,\
         installed_on TIMESTAMPTZ NOT NULL DEFAULT pg_catalog.now(),\
         success BOOLEAN NOT NULL,\
         checksum BYTEA NOT NULL,\
         execution_time BIGINT NOT NULL)",
    )
    .execute(&mut *connection)
    .await
    .unwrap();
    let files = migration_files();
    assert_eq!(files.len(), 89);
    for path in files {
        let filename = path.file_name().unwrap().to_str().unwrap();
        let stem = filename.strip_suffix(".sql").unwrap();
        let (version, description) = stem.split_once('_').unwrap();
        let version = version.parse::<i64>().unwrap();
        let migration = fs::read(&path).unwrap();
        let checksum = Sha384::digest(&migration);
        let migration = std::str::from_utf8(&migration).unwrap();
        let mut transaction = connection.begin().await.unwrap();
        sqlx::raw_sql(migration)
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO public._sqlx_migrations \
             (version,description,success,checksum,execution_time) \
             VALUES ($1,$2,TRUE,$3,0)",
        )
        .bind(version)
        .bind(description.replace('_', " "))
        .bind(checksum.as_slice())
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }
}

async fn assert_writer_connection(cluster: &DisposableClusterV1, secret: &DatabaseSecretV1) {
    let mut writer = cluster.writer_target(secret).await.unwrap();
    let exact: bool = sqlx::query_scalar(
        "SELECT current_user = session_user \
         AND current_user = 'starring_authoring_session_writer' \
         AND pg_catalog.current_database() = 'starring_runtime_staging' \
         AND pg_catalog.length(public.starring_authoring_session_writer_database_identity_v1()) BETWEEN 1 AND 128",
    )
    .fetch_one(&mut writer)
    .await
    .unwrap();
    assert!(exact);
}

fn assert_sqlstate(error: sqlx::Error, expected: &str, name: &str) {
    let code = error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned());
    assert_eq!(code.as_deref(), Some(expected), "{name}: {error}");
}

async fn assert_fixed_roles_cannot_execute_writer_functions(target: &mut PgConnection) {
    let roles = [
        "starring_identity_session",
        "starring_authorized_snapshot_reader",
        "starring_runtime_execution",
    ];
    let isolated: bool = sqlx::query_scalar(
        r#"
WITH expected(identity) AS (
    SELECT pg_catalog.unnest($1::TEXT[])
),
resolved AS (
    SELECT function_row.oid,function_row.proowner,function_row.proacl
    FROM expected
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
),
function_acl AS (
    SELECT privilege.*
    FROM resolved
    CROSS JOIN LATERAL pg_catalog.aclexplode(
        COALESCE(
            resolved.proacl,
            pg_catalog.acldefault('f',resolved.proowner)
        )
    ) AS privilege
)
SELECT
    NOT EXISTS (
        SELECT 1
        FROM function_acl
        WHERE function_acl.grantee = 0
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_catalog.unnest($2::TEXT[]) AS role(role_name)
        CROSS JOIN resolved
        WHERE pg_catalog.has_function_privilege(
            role.role_name,
            resolved.oid,
            'EXECUTE'
        )
    )
"#,
    )
    .bind(AUTHORING_WRITER_FUNCTIONS.as_slice())
    .bind(roles.as_slice())
    .fetch_one(&mut *target)
    .await
    .unwrap();
    assert!(isolated);
    for role in roles {
        let mut transaction = target.begin().await.unwrap();
        sqlx::query(&format!("SET LOCAL ROLE {role}"))
            .execute(&mut *transaction)
            .await
            .unwrap();
        let error = sqlx::query_scalar::<_, String>(
            "SELECT public.starring_authoring_session_writer_database_identity_v1()",
        )
        .fetch_one(&mut *transaction)
        .await
        .unwrap_err();
        assert_sqlstate(error, "42501", role);
        transaction.rollback().await.unwrap();
    }
    verify_writer_contract(target).await.unwrap();
}

async fn assert_writer_cannot_mutate_relations(
    cluster: &DisposableClusterV1,
    secret: &DatabaseSecretV1,
) {
    for (name, statement) in [
        (
            "writer_relation_select",
            "SELECT * FROM public.authoring_sessions LIMIT 1",
        ),
        (
            "writer_relation_insert",
            "INSERT INTO public.authoring_sessions (\
             session_id,tenant_id,installation_id,owner_principal_id,\
             current_generation,lifecycle_state\
             ) VALUES (\
             'writer-matrix','tenant-matrix','installation-matrix',\
             'principal-matrix',1,'active'\
             )",
        ),
        (
            "writer_relation_update",
            "UPDATE public.authoring_sessions \
             SET lifecycle_state = 'closed' WHERE FALSE",
        ),
        (
            "writer_relation_delete",
            "DELETE FROM public.authoring_sessions WHERE FALSE",
        ),
        (
            "writer_outside_function_execute",
            "SELECT public.starring_runtime_mutation_clock()",
        ),
    ] {
        let mut writer = cluster.writer_target(secret).await.unwrap();
        let mut transaction = writer.begin().await.unwrap();
        let error = sqlx::raw_sql(statement)
            .execute(&mut *transaction)
            .await
            .unwrap_err();
        assert_sqlstate(error, "42501", name);
        transaction.rollback().await.unwrap();
    }
}

async fn assert_fresh_contract(target: &mut PgConnection) {
    assert!(!authoring_writer_role_exists(target).await.unwrap());
    verify_migration_contract(target, false).await.unwrap();
    verify_snapshot_capability(&mut *target, SnapshotCapabilityV1::Legacy)
        .await
        .unwrap();
    assert_eq!(
        inspect_incremental_state(target, false).await.unwrap(),
        IncrementalStateV1::Fresh
    );
}

async fn assert_exact_replay_contract(
    cluster: &DisposableClusterV1,
    target: &mut PgConnection,
    secret: &DatabaseSecretV1,
) {
    assert!(authoring_writer_role_exists(target).await.unwrap());
    verify_migration_contract(target, true).await.unwrap();
    verify_snapshot_capability(&mut *target, SnapshotCapabilityV1::Cutover)
        .await
        .unwrap();
    verify_writer_contract(target).await.unwrap();
    assert_eq!(
        inspect_incremental_state(target, true).await.unwrap(),
        IncrementalStateV1::ExactReplay
    );
    assert_writer_connection(cluster, secret).await;
}

async fn assert_writer_contract_rejects_mutation(
    target: &mut PgConnection,
    name: &str,
    mutation: &str,
) {
    let mut transaction = target.begin().await.unwrap();
    sqlx::raw_sql(mutation)
        .execute(&mut *transaction)
        .await
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(
        verify_writer_contract(&mut transaction).await,
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState),
        "{name}"
    );
    transaction.rollback().await.unwrap();
    verify_migration_contract(target, true)
        .await
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    verify_snapshot_capability(&mut *target, SnapshotCapabilityV1::Cutover)
        .await
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    verify_writer_contract(target)
        .await
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    assert_eq!(
        inspect_incremental_state(target, true).await,
        Ok(IncrementalStateV1::ExactReplay),
        "{name}"
    );
}

async fn assert_writer_negative_capability_matrix(target: &mut PgConnection) {
    let mut mutations = vec![
        (
            "role_no_login",
            "ALTER ROLE starring_authoring_session_writer NOLOGIN".to_owned(),
        ),
        (
            "role_superuser",
            "ALTER ROLE starring_authoring_session_writer SUPERUSER".to_owned(),
        ),
        (
            "role_create_database",
            "ALTER ROLE starring_authoring_session_writer CREATEDB".to_owned(),
        ),
        (
            "role_create_role",
            "ALTER ROLE starring_authoring_session_writer CREATEROLE".to_owned(),
        ),
        (
            "role_inherit",
            "ALTER ROLE starring_authoring_session_writer INHERIT".to_owned(),
        ),
        (
            "role_replication",
            "ALTER ROLE starring_authoring_session_writer REPLICATION".to_owned(),
        ),
        (
            "role_bypass_rls",
            "ALTER ROLE starring_authoring_session_writer BYPASSRLS".to_owned(),
        ),
        (
            "role_connection_limit",
            "ALTER ROLE starring_authoring_session_writer CONNECTION LIMIT 5".to_owned(),
        ),
        (
            "role_valid_until",
            "ALTER ROLE starring_authoring_session_writer VALID UNTIL '2030-01-01'".to_owned(),
        ),
        (
            "role_password",
            "ALTER ROLE starring_authoring_session_writer PASSWORD NULL".to_owned(),
        ),
        (
            "membership_writer_as_member",
            "GRANT starring_authorized_snapshot_reader TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "membership_writer_as_granted_role",
            "GRANT starring_authoring_session_writer TO starring_authorized_snapshot_reader"
                .to_owned(),
        ),
        (
            "global_role_setting",
            "ALTER ROLE starring_authoring_session_writer SET statement_timeout TO '1s'".to_owned(),
        ),
        (
            "database_role_setting",
            "ALTER ROLE starring_authoring_session_writer IN DATABASE \
             starring_runtime_staging SET statement_timeout TO '1s'"
                .to_owned(),
        ),
        (
            "parameter_set",
            "GRANT SET ON PARAMETER statement_timeout \
             TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "parameter_alter_system",
            "GRANT ALTER SYSTEM ON PARAMETER statement_timeout \
             TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "parameter_set_public",
            "GRANT SET ON PARAMETER statement_timeout TO PUBLIC".to_owned(),
        ),
        (
            "parameter_alter_system_public",
            "GRANT ALTER SYSTEM ON PARAMETER statement_timeout TO PUBLIC".to_owned(),
        ),
        (
            "staging_database_create",
            "GRANT CREATE ON DATABASE starring_runtime_staging \
             TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "staging_database_temporary",
            "GRANT TEMPORARY ON DATABASE starring_runtime_staging \
             TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "other_database_connect",
            "GRANT CONNECT ON DATABASE postgres TO starring_authoring_session_writer".to_owned(),
        ),
        (
            "public_schema_create_direct",
            "GRANT CREATE ON SCHEMA public TO starring_authoring_session_writer".to_owned(),
        ),
        (
            "public_schema_create_inherited",
            "GRANT CREATE ON SCHEMA public TO PUBLIC".to_owned(),
        ),
        (
            "other_schema_usage",
            "CREATE SCHEMA writer_matrix_other AUTHORIZATION starring_owner; \
             GRANT USAGE ON SCHEMA writer_matrix_other \
             TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "other_schema_create",
            "CREATE SCHEMA writer_matrix_other AUTHORIZATION starring_owner; \
             GRANT CREATE ON SCHEMA writer_matrix_other \
             TO starring_authoring_session_writer"
                .to_owned(),
        ),
    ];
    for privilege in [
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
        "TRUNCATE",
        "REFERENCES",
        "TRIGGER",
    ] {
        mutations.push((
            match privilege {
                "SELECT" => "table_select",
                "INSERT" => "table_insert",
                "UPDATE" => "table_update",
                "DELETE" => "table_delete",
                "TRUNCATE" => "table_truncate",
                "REFERENCES" => "table_references",
                "TRIGGER" => "table_trigger",
                _ => unreachable!(),
            },
            format!(
                "GRANT {privilege} ON TABLE public.authoring_sessions \
                 TO starring_authoring_session_writer"
            ),
        ));
    }
    for privilege in ["SELECT", "INSERT", "UPDATE", "REFERENCES"] {
        mutations.push((
            match privilege {
                "SELECT" => "column_select",
                "INSERT" => "column_insert",
                "UPDATE" => "column_update",
                "REFERENCES" => "column_references",
                _ => unreachable!(),
            },
            format!(
                "GRANT {privilege} (session_id) ON TABLE public.authoring_sessions \
                 TO starring_authoring_session_writer"
            ),
        ));
    }
    for privilege in ["USAGE", "SELECT", "UPDATE"] {
        mutations.push((
            match privilege {
                "USAGE" => "sequence_usage",
                "SELECT" => "sequence_select",
                "UPDATE" => "sequence_update",
                _ => unreachable!(),
            },
            format!(
                "CREATE SEQUENCE public.writer_matrix_sequence; \
                 ALTER SEQUENCE public.writer_matrix_sequence OWNER TO starring_owner; \
                 REVOKE ALL PRIVILEGES ON SEQUENCE public.writer_matrix_sequence FROM PUBLIC; \
                 GRANT {privilege} ON SEQUENCE public.writer_matrix_sequence \
                 TO starring_authoring_session_writer"
            ),
        ));
    }
    mutations.extend([
        (
            "outside_function_execute",
            "CREATE FUNCTION public.writer_matrix_outside() RETURNS INTEGER \
             LANGUAGE SQL AS 'SELECT 1'; \
             ALTER FUNCTION public.writer_matrix_outside() OWNER TO starring_owner; \
             REVOKE ALL PRIVILEGES ON FUNCTION public.writer_matrix_outside() FROM PUBLIC; \
             GRANT EXECUTE ON FUNCTION public.writer_matrix_outside() \
             TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "allowed_function_grant_option",
            format!(
                "GRANT EXECUTE ON FUNCTION {} \
                 TO starring_authoring_session_writer WITH GRANT OPTION",
                AUTHORING_WRITER_FUNCTIONS[0]
            ),
        ),
        (
            "database_ownership",
            "ALTER DATABASE starring_runtime_staging \
             OWNER TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "schema_ownership",
            "ALTER SCHEMA public OWNER TO starring_authoring_session_writer".to_owned(),
        ),
        (
            "relation_ownership",
            "ALTER TABLE public.authoring_sessions \
             OWNER TO starring_authoring_session_writer"
                .to_owned(),
        ),
        (
            "function_ownership",
            format!(
                "ALTER FUNCTION {} OWNER TO starring_authoring_session_writer",
                AUTHORING_WRITER_FUNCTIONS[0]
            ),
        ),
    ]);
    let default_acl_cases = [
        ("tables", "SELECT", "TABLES"),
        ("sequences", "USAGE", "SEQUENCES"),
        ("functions", "EXECUTE", "FUNCTIONS"),
        ("types", "USAGE", "TYPES"),
        ("schemas", "USAGE", "SCHEMAS"),
    ];
    for (name, privilege, object_type) in default_acl_cases {
        mutations.push((
            match name {
                "tables" => "writer_default_acl_tables",
                "sequences" => "writer_default_acl_sequences",
                "functions" => "writer_default_acl_functions",
                "types" => "writer_default_acl_types",
                "schemas" => "writer_default_acl_schemas",
                _ => unreachable!(),
            },
            format!(
                "ALTER DEFAULT PRIVILEGES FOR ROLE starring_authoring_session_writer \
                 GRANT {privilege} ON {object_type} \
                 TO starring_authorized_snapshot_reader"
            ),
        ));
        mutations.push((
            match name {
                "tables" => "owner_default_acl_writer_tables",
                "sequences" => "owner_default_acl_writer_sequences",
                "functions" => "owner_default_acl_writer_functions",
                "types" => "owner_default_acl_writer_types",
                "schemas" => "owner_default_acl_writer_schemas",
                _ => unreachable!(),
            },
            format!(
                "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner \
                 GRANT {privilege} ON {object_type} \
                 TO starring_authoring_session_writer"
            ),
        ));
    }
    for (name, privilege, object_type) in [
        ("tables", "SELECT", "TABLES"),
        ("sequences", "USAGE", "SEQUENCES"),
        ("functions", "EXECUTE", "FUNCTIONS"),
        ("types", "USAGE", "TYPES"),
        ("schemas", "USAGE", "SCHEMAS"),
    ] {
        mutations.push((
            match name {
                "tables" => "owner_default_acl_public_tables",
                "sequences" => "owner_default_acl_public_sequences",
                "functions" => "owner_default_acl_public_functions",
                "types" => "owner_default_acl_public_types",
                "schemas" => "owner_default_acl_public_schemas",
                _ => unreachable!(),
            },
            format!(
                "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner \
                 GRANT {privilege} ON {object_type} TO PUBLIC"
            ),
        ));
    }
    assert_eq!(mutations.len(), 60);
    for (name, mutation) in mutations {
        assert_writer_contract_rejects_mutation(target, name, &mutation).await;
    }
}

#[tokio::test]
#[ignore = "requires an isolated disposable PostgreSQL 16 cluster"]
async fn lifecycle_proves_fresh_replay_partial_rollback_and_restore() {
    let mut cluster = DisposableClusterV1::connect().await;
    cluster.bootstrap_staging().await;
    let mut target = cluster.admin_target().await;
    verify_existing_cluster_contract(&mut target).await.unwrap();
    assert_fresh_contract(&mut target).await;

    sqlx::query(
        "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner \
         GRANT SELECT ON TABLES TO PUBLIC",
    )
    .execute(&mut target)
    .await
    .unwrap();
    let failed_secret = DatabaseSecretV1::generate(AUTHORING_WRITER_IDENTITY).unwrap();
    assert_eq!(
        apply_authoring_writer(&mut target, &failed_secret).await,
        Err(ProvisionerErrorV1::DatabaseVerification)
    );
    assert_fresh_contract(&mut target).await;
    sqlx::query(
        "ALTER DEFAULT PRIVILEGES FOR ROLE starring_owner \
         REVOKE SELECT ON TABLES FROM PUBLIC",
    )
    .execute(&mut target)
    .await
    .unwrap();

    let secret = DatabaseSecretV1::generate(AUTHORING_WRITER_IDENTITY).unwrap();
    apply_authoring_writer(&mut target, &secret).await.unwrap();
    assert_exact_replay_contract(&cluster, &mut target, &secret).await;
    assert_writer_negative_capability_matrix(&mut target).await;
    assert_fixed_roles_cannot_execute_writer_functions(&mut target).await;
    assert_writer_cannot_mutate_relations(&cluster, &secret).await;
    assert_exact_replay_contract(&cluster, &mut target, &secret).await;
    assert_eq!(
        inspect_incremental_state(&mut target, false).await,
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
    );

    let revoked = AUTHORING_WRITER_FUNCTIONS[0];
    sqlx::query(&format!(
        "REVOKE EXECUTE ON FUNCTION {revoked} FROM starring_authoring_session_writer"
    ))
    .execute(&mut target)
    .await
    .unwrap();
    assert_eq!(
        inspect_incremental_state(&mut target, true).await,
        Err(ProvisionerErrorV1::IncrementalAuthoringWriterContract)
    );
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {revoked} TO starring_authoring_session_writer"
    ))
    .execute(&mut target)
    .await
    .unwrap();
    assert_exact_replay_contract(&cluster, &mut target, &secret).await;

    rollback_authoring_writer(&mut target, secret.verifier())
        .await
        .unwrap();
    assert_fresh_contract(&mut target).await;
    assert!(cluster.writer_target(&secret).await.is_err());
    verify_existing_cluster_contract(&mut target).await.unwrap();

    let restored_secret = DatabaseSecretV1::generate(AUTHORING_WRITER_IDENTITY).unwrap();
    apply_authoring_writer(&mut target, &restored_secret)
        .await
        .unwrap();
    assert_exact_replay_contract(&cluster, &mut target, &restored_secret).await;
    rollback_authoring_writer(&mut target, restored_secret.verifier())
        .await
        .unwrap();
    assert_fresh_contract(&mut target).await;
    drop(target);
    cluster.cleanup().await;
}
