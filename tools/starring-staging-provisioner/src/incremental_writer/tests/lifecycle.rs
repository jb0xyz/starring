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
