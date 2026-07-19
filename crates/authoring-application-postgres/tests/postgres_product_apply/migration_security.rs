#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_preflight_refuses_ambiguous_applied_history() {
    let database = isolated_database("upgrade").await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_007)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let fixture = seed_fixture(&database.pool).await;
        sqlx::query(
            "UPDATE public.activation_requests \
             SET state = 'applied', applied_at = pg_catalog.clock_timestamp(), applied_by = $2, \
              completion_kind = 'already_active', activation_notices = '[]'::JSONB, \
              product_revision = product_revision + 1 \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .bind(&fixture.actor.user_id)
        .execute(&database.pool)
        .await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_008)
            .unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject Applied history without a deployment");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected migration preflight database error");
        };
        assert_eq!(
            database_error.constraint(),
            Some("atomic_product_apply_upgrade_deployment_complete")
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

async fn function_execute(pool: &PgPool, role: &str, identity: &str) -> bool {
    sqlx::query_scalar("SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')")
        .bind(role)
        .bind(identity)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn function_grantable(pool: &PgPool, role: &str, identity: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (\
         SELECT 1 \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(\
          COALESCE(function_row.proacl, pg_catalog.acldefault('f', function_row.proowner))\
         ) AS privilege \
         INNER JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee \
         WHERE function_row.oid = pg_catalog.to_regprocedure($2) \
          AND role.rolname = $1 \
          AND privilege.privilege_type = 'EXECUTE' \
          AND privilege.is_grantable)",
    )
    .bind(role)
    .bind(identity)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn public_function_execute(pool: &PgPool, identity: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (\
         SELECT 1 \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(\
          COALESCE(function_row.proacl, pg_catalog.acldefault('f', function_row.proowner))\
         ) AS privilege \
         WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
          AND privilege.grantee = 0 \
          AND privilege.privilege_type = 'EXECUTE')",
    )
    .bind(identity)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn run_migration_as(
    pool: &PgPool,
    migration_sql: &str,
    role: &str,
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query(&format!("SET LOCAL ROLE {role}"))
        .execute(&mut *transaction)
        .await?;
    if let Err(error) = sqlx::raw_sql(migration_sql)
        .execute(&mut *transaction)
        .await
    {
        transaction.rollback().await?;
        return Err(error);
    }
    transaction.commit().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_requires_explicit_apply_authority_intersection() {
    let database = isolated_database("acl").await;
    let role_suffix = suffix();
    let migration_role = format!("starring_apply_migrator_{role_suffix}");
    let lock_only_role = format!("starring_apply_lock_only_{role_suffix}");
    let finalizer_only_role = format!("starring_apply_final_only_{role_suffix}");
    let intersection_role = format!("starring_apply_intersect_{role_suffix}");
    let grantable_role = format!("starring_apply_grantable_{role_suffix}");
    let mismatch_role = format!("starring_apply_mismatch_{role_suffix}");
    let post_lock_only_role = format!("starring_apply_post_lock_{role_suffix}");
    let roles = [
        &migration_role,
        &lock_only_role,
        &finalizer_only_role,
        &intersection_role,
        &grantable_role,
        &mismatch_role,
        &post_lock_only_role,
    ];
    let owner = sqlx::query_scalar::<_, String>("SELECT current_user")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    let quoted_owner = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_ident($1)")
        .bind(&owner)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    for role in roles {
        sqlx::query(&format!("CREATE ROLE {role} NOLOGIN"))
            .execute(&database.pool)
            .await
            .unwrap();
    }
    sqlx::query(&format!("GRANT {quoted_owner} TO {migration_role}"))
        .execute(&database.pool)
        .await
        .unwrap();
    let outcome = async {
        let lock_identity = "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)";
        let core_identity = "public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)";
        let finalizer_identity = "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)";
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_009)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let original = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT lock.oid::BIGINT, finalizer.oid::BIGINT, lock.proowner::BIGINT \
             FROM pg_catalog.pg_proc AS lock \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             WHERE lock.oid = pg_catalog.to_regprocedure($1) \
              AND finalizer.oid = pg_catalog.to_regprocedure($2)",
        )
        .bind(lock_identity)
        .bind(finalizer_identity)
        .fetch_one(&database.pool)
        .await?;
        for (identity, role, grantable) in [
            (lock_identity, lock_only_role.as_str(), false),
            (finalizer_identity, finalizer_only_role.as_str(), false),
            (lock_identity, intersection_role.as_str(), false),
            (finalizer_identity, intersection_role.as_str(), true),
            (lock_identity, grantable_role.as_str(), true),
            (finalizer_identity, grantable_role.as_str(), true),
            (lock_identity, mismatch_role.as_str(), true),
            (finalizer_identity, mismatch_role.as_str(), false),
        ] {
            sqlx::raw_sql(&format!(
                "GRANT EXECUTE ON FUNCTION {identity} TO {role}{}",
                if grantable { " WITH GRANT OPTION" } else { "" }
            ))
            .execute(&database.pool)
            .await?;
        }
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {finalizer_identity} TO PUBLIC"
        ))
        .execute(&database.pool)
        .await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_010)
            .unwrap();
        let error = run_migration_as(&database.pool, migration.sql.as_ref(), &migration_role)
            .await
            .expect_err("lock-only authority must block the upgrade");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("55000"));
        assert_eq!(
            database_error.message(),
            "product apply lock grantee lacks explicit finalizer authorization"
        );
        let rolled_back = sqlx::query_as::<_, (i64, Option<i64>, i64, i64)>(
            "SELECT lock.oid::BIGINT, core.oid::BIGINT, finalizer.oid::BIGINT, \
              lock.proowner::BIGINT \
             FROM pg_catalog.pg_proc AS lock \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             LEFT JOIN pg_catalog.pg_proc AS core \
              ON core.oid = pg_catalog.to_regprocedure($2) \
             WHERE lock.oid = pg_catalog.to_regprocedure($1) \
              AND finalizer.oid = pg_catalog.to_regprocedure($3)",
        )
        .bind(lock_identity)
        .bind(core_identity)
        .bind(finalizer_identity)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(rolled_back, (original.0, None, original.1, original.2));
        assert!(public_function_execute(&database.pool, finalizer_identity).await);
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {finalizer_identity} TO {lock_only_role}"
        ))
        .execute(&database.pool)
        .await?;
        let error = run_migration_as(&database.pool, migration.sql.as_ref(), &migration_role)
            .await
            .expect_err("inconsistent grant option must block the upgrade");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("55000"));
        assert_eq!(
            database_error.message(),
            "product apply grant option lacks explicit finalizer authorization"
        );
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {finalizer_identity} TO {mismatch_role} WITH GRANT OPTION"
        ))
        .execute(&database.pool)
        .await?;
        run_migration_as(&database.pool, migration.sql.as_ref(), &migration_role).await?;
        assert!(function_execute(&database.pool, &intersection_role, lock_identity).await);
        assert!(!function_grantable(&database.pool, &intersection_role, lock_identity).await);
        assert!(function_execute(&database.pool, &grantable_role, lock_identity).await);
        assert!(function_grantable(&database.pool, &grantable_role, lock_identity).await);
        assert!(function_execute(&database.pool, &mismatch_role, lock_identity).await);
        assert!(function_grantable(&database.pool, &mismatch_role, lock_identity).await);
        assert!(function_execute(&database.pool, &finalizer_only_role, finalizer_identity).await);
        assert!(!function_execute(&database.pool, &finalizer_only_role, lock_identity).await);
        assert!(!function_execute(&database.pool, &finalizer_only_role, core_identity).await);
        assert!(!public_function_execute(&database.pool, lock_identity).await);
        assert!(!public_function_execute(&database.pool, core_identity).await);
        assert!(!public_function_execute(&database.pool, finalizer_identity).await);
        let upgraded = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64)>(
            "SELECT wrapper.oid::BIGINT, core.oid::BIGINT, finalizer.oid::BIGINT, \
              wrapper.proowner::BIGINT, core.proowner::BIGINT, finalizer.proowner::BIGINT \
             FROM pg_catalog.pg_proc AS wrapper \
             CROSS JOIN pg_catalog.pg_proc AS core \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             WHERE wrapper.oid = pg_catalog.to_regprocedure($1) \
              AND core.oid = pg_catalog.to_regprocedure($2) \
              AND finalizer.oid = pg_catalog.to_regprocedure($3)",
        )
        .bind(lock_identity)
        .bind(core_identity)
        .bind(finalizer_identity)
        .fetch_one(&database.pool)
        .await?;
        assert_ne!(upgraded.0, original.0);
        assert_eq!(upgraded.1, original.0);
        assert_eq!(upgraded.2, original.1);
        assert_eq!(
            (upgraded.3, upgraded.4, upgraded.5),
            (original.2, original.2, original.2)
        );
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {core_identity} TO {post_lock_only_role}"
        ))
        .execute(&database.pool)
        .await?;
        assert!(function_execute(&database.pool, &post_lock_only_role, core_identity).await);
        assert!(!function_execute(&database.pool, &post_lock_only_role, lock_identity).await);
        let fixture = seed_fixture(&database.pool).await;
        let before = sqlx::query_as::<_, (String, i64)>(
            "SELECT state, product_revision \
             FROM public.activation_requests \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .fetch_one(&database.pool)
        .await?;
        let operation = Operation::new("acl-core-only-denied");
        let mut transaction = begin_serializable(&database.pool).await;
        sqlx::query(&format!("SET LOCAL ROLE {post_lock_only_role}"))
            .execute(&mut *transaction)
            .await?;
        let error = lock_apply(
            &mut transaction,
            &fixture,
            &operation,
            &Call::valid(&fixture),
        )
        .await
        .expect_err("core-only role must not execute the terminal wrapper");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("42501"));
        transaction.rollback().await?;
        let after = sqlx::query_as::<_, (String, i64)>(
            "SELECT state, product_revision \
             FROM public.activation_requests \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(after, before);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
    sqlx::query(&format!("REVOKE {quoted_owner} FROM {migration_role}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    for role in roles {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut administrator)
            .await
            .unwrap();
    }
    outcome.unwrap();
}
