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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_moves_explicit_apply_execute_privileges_to_the_wrapper() {
    let database = isolated_database("acl").await;
    let migration_role = format!("starring_apply_migrator_{}", suffix());
    let owner = sqlx::query_scalar::<_, String>("SELECT current_user")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    let quoted_owner = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_ident($1)")
        .bind(&owner)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE ROLE {migration_role} NOLOGIN"))
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!("GRANT {quoted_owner} TO {migration_role}"))
        .execute(&database.pool)
        .await
        .unwrap();
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_009)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        sqlx::raw_sql(
            "GRANT EXECUTE ON FUNCTION public.starring_product_apply_lock_v1(\
             text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,\
             timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,\
             text,text,text) TO pg_read_all_data WITH GRANT OPTION",
        )
        .execute(&database.pool)
        .await?;
        let mut delegated_grant = database.pool.begin().await?;
        sqlx::query("SET LOCAL ROLE pg_read_all_data")
            .execute(&mut *delegated_grant)
            .await?;
        sqlx::raw_sql(
            "GRANT EXECUTE ON FUNCTION public.starring_product_apply_lock_v1(\
             text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,\
             timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,\
             text,text,text) TO pg_write_all_data",
        )
        .execute(&mut *delegated_grant)
        .await?;
        delegated_grant.commit().await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_010)
            .unwrap();
        let mut migration_transaction = database.pool.begin().await?;
        sqlx::query(&format!("SET LOCAL ROLE {migration_role}"))
            .execute(&mut *migration_transaction)
            .await?;
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *migration_transaction)
            .await?;
        migration_transaction.commit().await?;
        let privileges = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool, bool)>(
            "SELECT \
              pg_catalog.has_function_privilege(\
               'pg_read_all_data', \
               'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              pg_catalog.has_function_privilege(\
               'pg_read_all_data', \
               'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               INNER JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND role.rolname = 'pg_read_all_data' \
                AND privilege.privilege_type = 'EXECUTE' \
                AND privilege.is_grantable), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE'), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE'), \
              wrapper.proowner = core.proowner, \
              wrapper.proowner = finalizer.proowner, \
              wrapper.proowner <> migration_role.oid \
             FROM pg_catalog.pg_proc AS wrapper \
             CROSS JOIN pg_catalog.pg_proc AS core \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             CROSS JOIN pg_catalog.pg_roles AS migration_role \
             WHERE wrapper.oid = pg_catalog.to_regprocedure(\
              'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
               bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
               text,text,text[],text[],text[],text,text,text,text,text,text)') \
              AND core.oid = pg_catalog.to_regprocedure(\
              'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
               bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
               text,text,text[],text[],text[],text,text,text,text,text,text)') \
              AND finalizer.oid = pg_catalog.to_regprocedure(\
              'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,\
               bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
               text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)') \
              AND migration_role.rolname = $1",
        )
        .bind(&migration_role)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(
            privileges,
            (true, false, true, false, false, true, true, true)
        );
        let delegated = sqlx::query_as::<_, (bool, bool, bool)>(
            "SELECT \
              pg_catalog.has_function_privilege(\
               'pg_write_all_data', \
               'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              pg_catalog.has_function_privilege(\
               'pg_write_all_data', \
               'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               INNER JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND role.rolname = 'pg_write_all_data' \
                AND privilege.privilege_type = 'EXECUTE' \
                AND privilege.is_grantable)",
        )
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(delegated, (true, false, false));
        let fixture = seed_fixture(&database.pool).await;
        let operation = Operation::new("acl-nested-core");
        let mut transaction = begin_serializable(&database.pool).await;
        sqlx::query("SET LOCAL ROLE pg_read_all_data")
            .execute(&mut *transaction)
            .await?;
        let locked = lock_apply(
            &mut transaction,
            &fixture,
            &operation,
            &Call::valid(&fixture),
        )
        .await?;
        assert_eq!(locked.outcome, "ready");
        transaction.rollback().await?;
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
    sqlx::query(&format!("DROP ROLE {migration_role}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    outcome.unwrap();
}
