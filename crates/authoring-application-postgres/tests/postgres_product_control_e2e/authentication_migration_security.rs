#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn authentication_scope_migration_rejects_split_relation_owners() {
    let mut database = isolated_product_control_database("authentication_owners").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_190_014)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let split_owner = format!("starring_auth_split_{}", suffix());
    sqlx::query(&format!(
        "CREATE ROLE {split_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "ALTER TABLE public.product_auth_sessions OWNER TO {split_owner}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_015)
            .unwrap();
        let mut transaction = database.pool.begin().await.unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await.unwrap();
        let remaining_functions = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM (VALUES \
              (pg_catalog.to_regprocedure( \
               'public.starring_product_session_read_v1(bytea)')), \
              (pg_catalog.to_regprocedure( \
               'public.starring_product_session_mutation_read_v1(bytea)')), \
              (pg_catalog.to_regprocedure( \
               'public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)')) \
             ) AS expected(function_oid) WHERE function_oid IS NOT NULL",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(remaining_functions, 0);
    })
    .catch_unwind()
    .await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {split_owner}"))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn authentication_scope_migration_strips_hostile_default_grants() {
    let mut database = isolated_product_control_database("authentication_grants").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_190_014)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let role_suffix = suffix();
    let owner_role = format!("starring_auth_owner_{role_suffix}");
    let migrator_role = format!("starring_auth_migrator_{role_suffix}");
    let hostile_role = format!("starring_auth_hostile_{role_suffix}");
    let migrator_password = database_role_password();
    for role in [&owner_role, &migrator_role, &hostile_role] {
        assert!(
            role.len() <= 63
                && role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_')
        );
    }
    for role in [&owner_role, &hostile_role] {
        sqlx::query(&format!(
            "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    let password_literal =
        sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
            .bind(&migrator_password)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {migrator_role} LOGIN PASSWORD {password_literal} \
         NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
         NOBYPASSRLS CONNECTION LIMIT 2"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for relation in ["product_principals", "product_auth_sessions"] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {migrator_role}",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE, CREATE ON SCHEMA public TO {owner_role}, {migrator_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT {owner_role} TO {migrator_role}"))
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "ALTER DEFAULT PRIVILEGES FOR ROLE {migrator_role} IN SCHEMA public \
         GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let migrator_pool =
        database_role_login_pool(&database.name, &migrator_role, &migrator_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let identity = sqlx::query_as::<_, (String, String)>(
            "SELECT current_user::TEXT, session_user::TEXT",
        )
        .fetch_one(&migrator_pool)
        .await
        .unwrap();
        assert_eq!(identity, (migrator_role.clone(), migrator_role.clone()));
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_015)
            .unwrap();
        let mut transaction = migrator_pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        for function_identity in [
            "public.starring_product_session_read_v1(bytea)",
            "public.starring_product_session_mutation_read_v1(bytea)",
            "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
        ] {
            let function_contract = sqlx::query_as::<_, (String, bool, bool, bool, i64)>(
                "SELECT pg_catalog.pg_get_userbyid(function_row.proowner), \
                  function_row.prosecdef, \
                  pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE'), \
                  EXISTS ( \
                   SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
                    function_row.proacl, \
                    pg_catalog.acldefault('f', function_row.proowner) \
                   )) AS privilege \
                   WHERE privilege.grantee = 0 \
                    AND privilege.privilege_type = 'EXECUTE' \
                  ), \
                  (SELECT pg_catalog.count(*) \
                   FROM pg_catalog.aclexplode(COALESCE( \
                    function_row.proacl, \
                    pg_catalog.acldefault('f', function_row.proowner) \
                   )) AS privilege \
                   WHERE privilege.grantee <> function_row.proowner) \
                 FROM pg_catalog.pg_proc AS function_row \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
            )
            .bind(function_identity)
            .bind(&hostile_role)
            .fetch_one(&database.pool)
            .await
            .unwrap();
            assert_eq!(
                function_contract,
                (owner_role.clone(), true, false, false, 0)
            );
        }
    })
    .catch_unwind()
    .await;
    migrator_pool.close().await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    let _ = sqlx::query(&format!(
        "REVOKE {owner_role} FROM {migrator_role}"
    ))
    .execute(&mut database.administrator)
    .await;
    for role in [&hostile_role, &migrator_role, &owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
