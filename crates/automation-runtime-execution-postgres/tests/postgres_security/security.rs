#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_database_is_function_only_and_least_privilege() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;
    assert_cross_runtime_readiness(&mut database).await;
    let owner_pool = database.owner_pool.clone();
    let executor_pool = database.executor_pool.clone();
    let foreign_database_options = database.foreign_database_options.clone();
    let database_name = database.name.clone();
    let role = database.role.clone();
    let administrator_role = database.administrator_role.clone();
    let outcome = tokio::spawn(async move {
        execution_security_scenario(
            owner_pool,
            executor_pool,
            foreign_database_options,
            database_name,
            role,
            administrator_role,
        )
        .await;
    })
    .await;
    cleanup(database).await;
    outcome.expect("restricted execution proof must complete");
    drop(server);
}

async fn execution_security_scenario(
    owner_pool: PgPool,
    executor_pool: PgPool,
    foreign_database_options: PgConnectOptions,
    database_name: String,
    role: String,
    administrator_role: String,
) {
    assert_exact_executor_capabilities(&owner_pool, &executor_pool, &role).await;
    assert_readiness_identity(&owner_pool, &executor_pool, &database_name, &role).await;
    assert_verified_adapter(&owner_pool, &executor_pool, &database_name, &role).await;
    assert_wrong_role_rejected(&owner_pool).await;
    assert_cross_database_rejected(
        &owner_pool,
        &executor_pool,
        &foreign_database_options,
        &role,
    )
    .await;
    assert_raw_sql_rejected(&executor_pool, &administrator_role).await;
    assert_invalid_operations_are_non_mutating(&owner_pool, &executor_pool).await;
    assert_claim_and_renew_success(&owner_pool, &executor_pool, &database_name, &role).await;
    assert_readiness_definition_sha(&owner_pool, EXPECTED_READINESS_DEFINITION_SHA256_V1).await;
}

async fn assert_verified_adapter(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    database_name: &str,
    role: &str,
) {
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let expectation =
        RuntimeExecutionDatabaseExpectationV1::new(database_identity.clone(), database_name, role)
            .unwrap();
    let binding = observe_runtime_execution_database_identity_v1(executor_pool, database_name, role)
        .await
        .unwrap();
    assert_eq!(binding.database_identity(), database_identity);
    assert!(matches!(
        observe_runtime_execution_database_identity_v1(
            executor_pool,
            database_name,
            "starring_wrong_execution_role",
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch)
    ));
    let adapter = PostgresRuntimeExecutionV1::connect_verified_default(
        executor_pool.clone(),
        expectation.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        adapter.initial_readiness().database_identity,
        database_identity
    );
    assert_eq!(adapter.initial_readiness().database_name, database_name);
    assert_eq!(adapter.initial_readiness().executor_role, role);
    assert!(adapter.verify_database_v1().await.is_ok());
    let wrong_expectation = RuntimeExecutionDatabaseExpectationV1::new(
        expectation.database_identity(),
        "starring_wrong_execution_database",
        expectation.executor_role(),
    )
    .unwrap();
    assert!(matches!(
        PostgresRuntimeExecutionV1::connect_verified_default(
            executor_pool.clone(),
            wrong_expectation
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch)
    ));
}

async fn assert_exact_executor_capabilities(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    role: &str,
) {
    let mut actual = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.format( \
            '%I.%I(%s)', namespace.nspname, function_row.proname, \
            pg_catalog.replace( \
                pg_catalog.oidvectortypes(function_row.proargtypes), ', ', ',' \
            ) \
         )::TEXT \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = function_row.pronamespace \
         WHERE function_row.oid >= 16384 \
            AND namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND pg_catalog.has_function_privilege( \
                pg_catalog.to_regrole(session_user), function_row.oid, 'EXECUTE' \
            ) \
         ORDER BY 1",
    )
    .fetch_all(executor_pool)
    .await
    .unwrap();
    let mut expected = EXECUTOR_FUNCTIONS
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);

    let role_oid = sqlx::query_scalar::<_, i32>(
        "SELECT role.oid::INT4 FROM pg_catalog.pg_roles AS role WHERE role.rolname = $1",
    )
    .bind(role)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let (direct_execute_count, grantable_execute_count, public_execute_count) =
        sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT \
                pg_catalog.count(*) FILTER ( \
                    WHERE privilege.grantee = $1 \
                        AND privilege.privilege_type = 'EXECUTE' \
                ), \
                pg_catalog.count(*) FILTER ( \
                    WHERE privilege.grantee = $1 AND privilege.is_grantable \
                ), \
                pg_catalog.count(*) FILTER (WHERE privilege.grantee = 0) \
             FROM pg_catalog.pg_proc AS function_row \
             INNER JOIN pg_catalog.pg_namespace AS namespace \
                ON namespace.oid = function_row.pronamespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE function_row.oid >= 16384 \
                AND namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'",
        )
        .bind(role_oid)
        .fetch_one(owner_pool)
        .await
        .unwrap();
    assert_eq!(direct_execute_count, EXECUTOR_FUNCTIONS.len() as i64);
    assert_eq!(grantable_execute_count, 0);
    assert_eq!(public_execute_count, 0);

    let raw_relation_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND relation.relkind IN ('r', 'p', 'v', 'm', 'f') \
            AND ( \
                pg_catalog.has_table_privilege(session_user, relation.oid, 'SELECT') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'INSERT') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'UPDATE') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'DELETE') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'TRUNCATE') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'REFERENCES') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'TRIGGER') \
            )",
    )
    .fetch_one(executor_pool)
    .await
    .unwrap();
    assert_eq!(raw_relation_capabilities, 0);
    let raw_column_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_attribute AS attribute \
         INNER JOIN pg_catalog.pg_class AS relation \
            ON relation.oid = attribute.attrelid \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND attribute.attnum > 0 \
            AND NOT attribute.attisdropped \
            AND ( \
                pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'SELECT' \
                ) \
                OR pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'INSERT' \
                ) \
                OR pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'UPDATE' \
                ) \
                OR pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'REFERENCES' \
                ) \
            )",
    )
    .fetch_one(executor_pool)
    .await
    .unwrap();
    assert_eq!(raw_column_capabilities, 0);
    let raw_sequence_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_class AS sequence \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = sequence.relnamespace \
         WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND sequence.relkind = 'S' \
            AND ( \
                pg_catalog.has_sequence_privilege(session_user, sequence.oid, 'USAGE') \
                OR pg_catalog.has_sequence_privilege(session_user, sequence.oid, 'SELECT') \
                OR pg_catalog.has_sequence_privilege(session_user, sequence.oid, 'UPDATE') \
            )",
    )
    .fetch_one(executor_pool)
    .await
    .unwrap();
    assert_eq!(raw_sequence_capabilities, 0);

    let (connect, create, temporary, schema_usage, schema_create) =
        sqlx::query_as::<_, (bool, bool, bool, bool, bool)>(
            "SELECT \
                pg_catalog.has_database_privilege(session_user, current_database(), 'CONNECT'), \
                pg_catalog.has_database_privilege(session_user, current_database(), 'CREATE'), \
                pg_catalog.has_database_privilege(session_user, current_database(), 'TEMPORARY'), \
                pg_catalog.has_schema_privilege(session_user, 'public', 'USAGE'), \
                pg_catalog.has_schema_privilege(session_user, 'public', 'CREATE')",
        )
        .fetch_one(executor_pool)
        .await
        .unwrap();
    assert!(connect);
    assert!(!create);
    assert!(!temporary);
    assert!(schema_usage);
    assert!(!schema_create);

    let (public_database_acl, public_schema_acl, membership_count, owned_object_count) =
        sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT \
                (SELECT pg_catalog.count(*) \
                 FROM pg_catalog.pg_database AS database_row \
                 CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                    database_row.datacl, \
                    pg_catalog.acldefault('d', database_row.datdba) \
                 )) AS privilege \
                 WHERE database_row.datname = current_database() \
                    AND privilege.grantee = 0), \
                (SELECT pg_catalog.count(*) \
                 FROM pg_catalog.pg_namespace AS namespace \
                 CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                    namespace.nspacl, \
                    pg_catalog.acldefault('n', namespace.nspowner) \
                 )) AS privilege \
                 WHERE namespace.nspname = 'public' \
                    AND privilege.grantee = 0), \
                (SELECT pg_catalog.count(*) \
                 FROM pg_catalog.pg_auth_members AS membership \
                 WHERE membership.member = pg_catalog.to_regrole(session_user) \
                    OR membership.roleid = pg_catalog.to_regrole(session_user)), \
                (SELECT pg_catalog.count(*) FROM ( \
                    SELECT relation.oid FROM pg_catalog.pg_class AS relation \
                        WHERE relation.relowner = pg_catalog.to_regrole(session_user) \
                    UNION ALL \
                    SELECT namespace.oid FROM pg_catalog.pg_namespace AS namespace \
                        WHERE namespace.nspowner = pg_catalog.to_regrole(session_user) \
                    UNION ALL \
                    SELECT function_row.oid FROM pg_catalog.pg_proc AS function_row \
                        WHERE function_row.proowner = pg_catalog.to_regrole(session_user) \
                    UNION ALL \
                    SELECT database_row.oid FROM pg_catalog.pg_database AS database_row \
                        WHERE database_row.datdba = pg_catalog.to_regrole(session_user) \
                 ) AS owned)",
        )
        .fetch_one(executor_pool)
        .await
        .unwrap();
    assert_eq!(public_database_acl, 0);
    assert_eq!(public_schema_acl, 0);
    assert_eq!(membership_count, 0);
    assert_eq!(owned_object_count, 0);

    let (superuser, inherit, create_role, create_db, can_login, replication, bypass_rls, config) =
        sqlx::query_as::<
            _,
            (
                bool,
                bool,
                bool,
                bool,
                bool,
                bool,
                bool,
                Option<Vec<String>>,
            ),
        >(
            "SELECT role.rolsuper, role.rolinherit, role.rolcreaterole, role.rolcreatedb, \
                role.rolcanlogin, role.rolreplication, role.rolbypassrls, role.rolconfig \
             FROM pg_catalog.pg_roles AS role WHERE role.rolname = session_user",
        )
        .fetch_one(executor_pool)
        .await
        .unwrap();
    assert!(!superuser);
    assert!(!inherit);
    assert!(!create_role);
    assert!(!create_db);
    assert!(can_login);
    assert!(!replication);
    assert!(!bypass_rls);
    assert!(config.is_none_or(|entries| entries.is_empty()));
}

async fn assert_readiness_identity(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    database_name: &str,
    role: &str,
) {
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let rows = sqlx::query_as::<_, (String, String, String, DateTime<Utc>)>(
        "SELECT * FROM public.starring_runtime_execution_database_readiness_v1()",
    )
    .fetch_all(executor_pool)
    .await
    .unwrap();
    let [(observed_identity, observed_database, observed_role, checked_at)] = rows.as_slice()
    else {
        panic!("readiness must return exactly one row")
    };
    assert_eq!(observed_identity, &database_identity);
    assert_eq!(observed_database, database_name);
    assert_eq!(observed_role, role);
    assert!(*checked_at <= Utc::now());
    let identity_rows = sqlx::query_scalar::<_, String>(
        "SELECT * FROM public.starring_runtime_execution_database_identity_v1()",
    )
    .fetch_all(executor_pool)
    .await
    .unwrap();
    assert_eq!(identity_rows, [database_identity]);
}

async fn assert_wrong_role_rejected(owner_pool: &PgPool) {
    let error = sqlx::query(format!("SELECT * FROM {READINESS_FUNCTION}").as_str())
        .fetch_all(owner_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RE001");
}

async fn assert_cross_database_rejected(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    foreign_database_options: &PgConnectOptions,
    role: &str,
) {
    let foreign_database = foreign_database_options.get_database().unwrap();
    let foreign_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_database AS database_row \
         WHERE database_row.datallowconn \
            AND database_row.datname <> current_database() \
            AND ( \
                pg_catalog.has_database_privilege($1, database_row.oid, 'CONNECT') \
                OR pg_catalog.has_database_privilege($1, database_row.oid, 'CREATE') \
                OR pg_catalog.has_database_privilege($1, database_row.oid, 'TEMPORARY') \
            )",
    )
    .bind(role)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    assert_eq!(foreign_capabilities, 0);
    let error = match PgConnection::connect_with(foreign_database_options).await {
        Ok(connection) => {
            connection.close().await.unwrap();
            panic!("execution role connected to foreign database {foreign_database}")
        }
        Err(error) => error,
    };
    assert_sqlstate(&error, "42501");
    let (quoted_foreign_database, quoted_role) = sqlx::query_as::<_, (String, String)>(
        "SELECT pg_catalog.quote_ident($1), pg_catalog.quote_ident($2)",
    )
    .bind(foreign_database)
    .bind(role)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    owner_pool
        .execute(
            format!("GRANT CONNECT ON DATABASE {quoted_foreign_database} TO {quoted_role}")
                .as_str(),
        )
        .await
        .unwrap();
    let readiness_error = sqlx::query(format!("SELECT * FROM {READINESS_FUNCTION}").as_str())
        .fetch_all(executor_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&readiness_error, "RE001");
    PgConnection::connect_with(foreign_database_options)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    owner_pool
        .execute(
            format!("REVOKE CONNECT ON DATABASE {quoted_foreign_database} FROM {quoted_role}")
                .as_str(),
        )
        .await
        .unwrap();
    let readiness_rows = sqlx::query(format!("SELECT * FROM {READINESS_FUNCTION}").as_str())
        .fetch_all(executor_pool)
        .await
        .unwrap();
    assert_eq!(readiness_rows.len(), 1);
}

async fn assert_raw_sql_rejected(executor_pool: &PgPool, administrator_role: &str) {
    let statements = [
        "SELECT deployment_id FROM public.runtime_deployments LIMIT 1".to_string(),
        "INSERT INTO public.runtime_deployments DEFAULT VALUES".to_string(),
        "UPDATE public.runtime_deployments SET revision = revision".to_string(),
        "DELETE FROM public.runtime_deployments".to_string(),
        "SELECT deployment_id FROM public.runtime_execution_mutation_markers LIMIT 1".to_string(),
        "UPDATE public.runtime_execution_mutation_markers \
         SET mutation_revision = mutation_revision"
            .to_string(),
        "SELECT fence_state FROM public.runtime_writer_fence".to_string(),
        "UPDATE public.runtime_writer_fence SET fence_generation = fence_generation".to_string(),
        "DELETE FROM public.runtime_writer_fence".to_string(),
        "CREATE TABLE public.runtime_execution_escape(value BIGINT)".to_string(),
        "CREATE TEMP TABLE runtime_execution_escape(value BIGINT)".to_string(),
        "CREATE ROLE runtime_execution_escape".to_string(),
        format!("SET ROLE {administrator_role}"),
    ];
    for statement in statements {
        let error = sqlx::query(&statement)
            .execute(executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "42501");
    }
}

async fn assert_invalid_operations_are_non_mutating(owner_pool: &PgPool, executor_pool: &PgPool) {
    let before = protected_counts(owner_pool).await;
    for statement in [
        "SELECT * FROM public.starring_runtime_execution_claim_next_v1('', 1000)",
        "SELECT * FROM public.starring_runtime_execution_mutate_v1( \
            '', '', '', 1, '', 1, 1, 1, 'preflight', '{}'::JSONB \
         )",
    ] {
        let error = sqlx::query(statement)
            .fetch_all(executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "RX002");
    }
    assert_eq!(protected_counts(owner_pool).await, before);
}

async fn protected_counts(pool: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT \
            (SELECT pg_catalog.count(*) FROM public.runtime_deployments), \
            (SELECT pg_catalog.count(*) FROM public.runtime_attestations), \
            (SELECT pg_catalog.count(*) FROM public.runtime_serving_leases), \
            (SELECT pg_catalog.count(*) \
             FROM public.runtime_execution_mutation_markers)",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_claim_and_renew_success(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    database_name: &str,
    role: &str,
) {
    seed_claimable_deployment(owner_pool).await;
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let expectation =
        RuntimeExecutionDatabaseExpectationV1::new(database_identity, database_name, role).unwrap();
    let adapter =
        PostgresRuntimeExecutionV1::connect_verified_default(executor_pool.clone(), expectation)
            .await
            .unwrap();
    let controller_id = ControllerId::parse("runtime-execution-controller").unwrap();
    let claim_lease = Duration::from_secs(90);
    let claim_request = RuntimeClaimNextExecutionV1 {
        controller_id: controller_id.clone(),
        lease_for: claim_lease,
    };
    let claimed = adapter
        .claim_next_execution(claim_request.clone())
        .await
        .unwrap()
        .expect("seeded deployment must be claimable");
    assert_execution_receipt(&claimed, &controller_id, 2, 1, 1, claim_lease);
    let replayed_claim = adapter
        .claim_next_execution(claim_request)
        .await
        .unwrap()
        .expect("owned deployment must replay its claim");
    assert_eq!(replayed_claim, claimed);
    assert_persisted_execution(
        owner_pool,
        2,
        "requested",
        &controller_id,
        1,
        1,
        claim_lease,
    )
    .await;

    let mut session = RuntimeConvergenceSessionV1::from_claim(claimed).unwrap();
    let renewal_lease = Duration::from_secs(120);
    let renewal_request = session.begin_renewal(renewal_lease).unwrap();
    let renewed = adapter
        .renew_execution(renewal_request.clone())
        .await
        .unwrap();
    assert_eq!(renewed.action_id, renewal_request.action_id);
    assert_execution_receipt(&renewed.execution, &controller_id, 3, 2, 1, renewal_lease);
    let replayed_renewal = adapter.renew_execution(renewal_request).await.unwrap();
    assert_eq!(replayed_renewal, renewed);
    session.apply_renewal(renewed.clone()).unwrap();
    assert_eq!(session.snapshot(), &renewed.execution.snapshot);
    assert_eq!(session.fencing_token(), renewed.execution.fencing_token);
    assert_eq!(
        session.convergence_attempt(),
        renewed.execution.convergence_attempt
    );
    assert_eq!(session.acquired_at(), renewed.execution.acquired_at);
    assert_eq!(session.expires_at(), renewed.execution.expires_at);
    assert_persisted_execution(
        owner_pool,
        3,
        "requested",
        &controller_id,
        2,
        1,
        renewal_lease,
    )
    .await;
    let competing_renewal = session.begin_renewal(Duration::from_secs(150)).unwrap();
    apply_cancel_successor(executor_pool, &competing_renewal.guard).await;
    let cancelled = persisted_cancelled_execution(owner_pool).await;
    assert_cancelled_execution(&cancelled, &controller_id);
    let error = adapter
        .renew_execution(competing_renewal)
        .await
        .unwrap_err();
    assert_eq!(error, RuntimeExecutionPersistenceErrorV1::OwnershipLost);
    assert_eq!(persisted_cancelled_execution(owner_pool).await, cancelled);
    assert_eq!(protected_counts(owner_pool).await, (1, 0, 0, 1));
}

fn assert_execution_receipt(
    receipt: &RuntimeExecutionReceiptV1,
    controller_id: &ControllerId,
    expected_revision: u64,
    expected_fencing_token: u64,
    expected_attempt: u32,
    expected_duration: Duration,
) {
    assert_eq!(receipt.snapshot.identity.deployment_id.as_str(), DEPLOYMENT);
    assert_eq!(receipt.snapshot.identity.tenant_id.as_str(), TENANT);
    assert_eq!(
        receipt.snapshot.identity.installation_id.as_str(),
        INSTALLATION
    );
    assert_eq!(receipt.snapshot.revision.get(), expected_revision);
    assert_eq!(&receipt.controller_id, controller_id);
    assert_eq!(receipt.fencing_token.get(), expected_fencing_token);
    assert_eq!(receipt.convergence_attempt.get(), expected_attempt);
    assert_eq!(
        receipt.expires_at - receipt.acquired_at,
        TimeDelta::from_std(expected_duration).unwrap()
    );
    let lease = receipt
        .snapshot
        .controller_lease
        .as_ref()
        .expect("claimed snapshot must embed its controller lease");
    assert_eq!(&lease.controller_id, controller_id);
    assert_eq!(lease.fencing_token, receipt.fencing_token);
    assert_eq!(lease.acquired_at, receipt.acquired_at);
    assert_eq!(lease.expires_at, receipt.expires_at);
    assert_eq!(
        receipt.snapshot.last_fencing_token,
        Some(receipt.fencing_token)
    );
}

async fn assert_persisted_execution(
    owner_pool: &PgPool,
    expected_revision: i64,
    expected_phase: &str,
    expected_controller: &ControllerId,
    expected_fencing_token: i64,
    expected_attempt: i64,
    expected_duration: Duration,
) {
    let count =
        sqlx::query_scalar::<_, i64>("SELECT pg_catalog.count(*) FROM public.runtime_deployments")
            .fetch_one(owner_pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let state = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            i64,
            i64,
            String,
            i64,
            i64,
            i64,
            i64,
            String,
        ),
    >(
        "SELECT revision, phase, controller_id, controller_fencing_token, \
            last_fencing_token, last_controller_id, convergence_attempt_no, \
            (EXTRACT(EPOCH FROM (controller_lease_expires_at \
                - controller_acquired_at)) * 1000)::BIGINT, \
            (snapshot ->> 'revision')::BIGINT, \
            (snapshot #>> '{controller_lease,fencing_token}')::BIGINT, \
            snapshot #>> '{controller_lease,controller_id}' \
         FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    assert_eq!(state.0, expected_revision);
    assert_eq!(state.1, expected_phase);
    assert_eq!(state.2, expected_controller.as_str());
    assert_eq!(state.3, expected_fencing_token);
    assert_eq!(state.4, expected_fencing_token);
    assert_eq!(state.5, expected_controller.as_str());
    assert_eq!(state.6, expected_attempt);
    assert_eq!(
        state.7,
        i64::try_from(expected_duration.as_millis()).unwrap()
    );
    assert_eq!(state.8, expected_revision);
    assert_eq!(state.9, expected_fencing_token);
    assert_eq!(state.10, expected_controller.as_str());
}

async fn apply_cancel_successor(
    executor_pool: &PgPool,
    guard: &automation_runtime_controller::RuntimeExecutionGuardV1,
) {
    let mut transaction = executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let rows = sqlx::query(
        "SELECT * FROM public.starring_runtime_execution_mutate_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, 'cancel', $9 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(Json(json!({"reason": "renewal-race-successor"})))
    .fetch_all(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    transaction.commit().await.unwrap();
}

type CancelledExecutionState = (
    i64,
    String,
    Option<String>,
    Option<i64>,
    i64,
    String,
    i64,
    Json<Value>,
);

async fn persisted_cancelled_execution(owner_pool: &PgPool) -> CancelledExecutionState {
    sqlx::query_as(
        "SELECT revision, phase, controller_id, controller_fencing_token, \
            last_fencing_token, last_controller_id, convergence_attempt_no, snapshot \
         FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(owner_pool)
    .await
    .unwrap()
}

fn assert_cancelled_execution(state: &CancelledExecutionState, controller_id: &ControllerId) {
    assert_eq!(state.0, 4);
    assert_eq!(state.1, "cancelled");
    assert_eq!(state.2, None);
    assert_eq!(state.3, None);
    assert_eq!(state.4, 2);
    assert_eq!(state.5, controller_id.as_str());
    assert_eq!(state.6, 1);
    assert_eq!(state.7["revision"], json!(4));
    assert_eq!(state.7["phase"]["phase"], "cancelled");
    assert_eq!(state.7["phase"]["reason"], "renewal-race-successor");
    assert_eq!(state.7["controller_lease"], Value::Null);
    assert_eq!(state.7["last_fencing_token"], json!(2));
}

async fn assert_readiness_definition_sha(owner_pool: &PgPool, expected_digest: &str) {
    let digest = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to( \
            pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure( \
                'public.starring_runtime_execution_database_readiness_v1()' \
            )), 'UTF8' \
         )), 'hex')",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    assert!(canonical_sha256(&digest));
    eprintln!("runtime execution readiness definition sha256: {digest}");
    assert_ne!(expected_digest, "PENDING");
    assert_eq!(digest, expected_digest);
}
