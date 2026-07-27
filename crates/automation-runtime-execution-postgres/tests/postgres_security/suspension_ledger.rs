#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn suspension_ledger_is_empty_owner_only_and_fail_closed() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;

    let counts = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
            (SELECT pg_catalog.count(*) FROM public.runtime_suspend_attempt_operations_v2), \
            (SELECT pg_catalog.count(*) FROM public.runtime_suspended_attempts_v2), \
            (SELECT pg_catalog.count(*) FROM public.runtime_suspend_attempt_completions_v2)",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0));

    for table in [
        "public.runtime_suspend_attempt_operations_v2",
        "public.runtime_suspended_attempts_v2",
        "public.runtime_suspend_attempt_completions_v2",
    ] {
        let denied = sqlx::query(&format!("SELECT * FROM {table}"))
            .fetch_all(&database.executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&denied, "42501");
    }

    let rejected = sqlx::query(
        "INSERT INTO public.runtime_suspend_attempt_operations_v2 (\
            suspension_id, tenant_id, installation_id, deployment_id, \
            deployment_revision, convergence_attempt_no, \
            suspend_attempt_request_bytes, suspend_attempt_digest\
         ) VALUES (\
            '00112233445566778899aabbccddeeff', \
            'tenant:1', 'installation:1', 'deployment:1', 1, 1, \
            pg_catalog.convert_to('{}', 'UTF8'), \
            pg_catalog.repeat('a', 64)\
         )",
    )
    .execute(&database.owner_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&rejected, "23514");

    let truncate_rejected = sqlx::query("TRUNCATE public.runtime_suspended_attempts_v2")
        .execute(&database.owner_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&truncate_rejected, "23514");

    let executor_oid = sqlx::query_scalar::<_, i64>(
        "SELECT role.oid::BIGINT \
         FROM pg_catalog.pg_roles AS role \
         WHERE role.rolname = $1",
    )
    .bind(&database.role)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let function_grants = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM (VALUES \
            ('public.reject_runtime_suspend_attempt_ledger_mutation_v2()'), \
            ('public.validate_runtime_suspend_attempt_ledger_v2()') \
         ) AS expected(identity) \
         WHERE pg_catalog.has_function_privilege(\
            $1::OID, \
            pg_catalog.to_regprocedure(expected.identity), \
            'EXECUTE'\
         )",
    )
    .bind(executor_oid)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(function_grants, 0);

    assert_cross_runtime_readiness(&mut database).await;
    cleanup(database).await;
}
