struct RuntimeMigrationDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

async fn isolated_runtime_migration_database() -> RuntimeMigrationDatabase {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let base = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let configured_database = base
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        configured_database.starts_with("starring_")
            && configured_database
                .split('_')
                .any(|segment| segment == "test")
            && configured_database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to create a database outside the strict Starring test namespace"
    );
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("starring_runtime_acl_test_{suffix}");
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    RuntimeMigrationDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_runtime_migration_database(database: RuntimeMigrationDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
}

async fn runtime_authority_function_acl(pool: &PgPool) -> (i64, i64, String, bool, bool, bool) {
    sqlx::query_as(
        "SELECT \
          routine.oid::BIGINT, \
          routine.proowner::BIGINT, \
          owner.rolname, \
          EXISTS (\
           SELECT 1 FROM pg_catalog.aclexplode(routine.proacl) AS privilege \
           INNER JOIN pg_catalog.pg_roles AS grantee ON grantee.oid = privilege.grantee \
           WHERE grantee.rolname = 'pg_read_all_data' \
            AND privilege.privilege_type = 'EXECUTE'), \
          EXISTS (\
           SELECT 1 FROM pg_catalog.aclexplode(routine.proacl) AS privilege \
           INNER JOIN pg_catalog.pg_roles AS grantee ON grantee.oid = privilege.grantee \
           WHERE grantee.rolname = 'pg_read_all_data' \
            AND privilege.privilege_type = 'EXECUTE' \
            AND privilege.is_grantable), \
          EXISTS (\
           SELECT 1 FROM pg_catalog.aclexplode(routine.proacl) AS privilege \
           WHERE privilege.grantee = 0 \
            AND privilege.privilege_type = 'EXECUTE') \
         FROM pg_catalog.pg_proc AS routine \
         INNER JOIN pg_catalog.pg_roles AS owner ON owner.oid = routine.proowner \
         WHERE routine.oid = pg_catalog.to_regprocedure(\
          'public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)')",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn authority_lock_upgrade_preserves_owner_and_explicit_execute_acl() {
    let database = isolated_runtime_migration_database().await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version < 202_607_190_011)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        sqlx::raw_sql(
            "GRANT EXECUTE ON FUNCTION public.starring_runtime_lock_current_authority(\
             text,text,text,text,bigint,text,text,bigint,text,bigint,text) \
             TO pg_read_all_data WITH GRANT OPTION",
        )
        .execute(&database.pool)
        .await?;
        let before = runtime_authority_function_acl(&database.pool).await;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_011)
            .expect("runtime authority upgrade migration must exist");
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await?;
        let after = runtime_authority_function_acl(&database.pool).await;
        Ok::<_, sqlx::Error>((before, after))
    }
    .await;
    drop_runtime_migration_database(database).await;
    let (before, after) = outcome.unwrap();
    assert_eq!(before.0, after.0);
    assert_eq!(before.1, after.1);
    assert_eq!(before.2, after.2);
    assert_eq!((before.3, before.4, before.5), (true, true, false));
    assert_eq!((after.3, after.4, after.5), (true, true, false));
}
