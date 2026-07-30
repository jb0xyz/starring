const SLOT_WRITER_FENCE_MIGRATION: &str = include_str!(
    "../../../../migrations/202607240010_persist_runtime_slot_writer_fence.sql"
);

const SLOT_WRITER_FENCE_GUCS: [&str; 11] = [
    "starring.runtime_slot_writer_fence_action_v2",
    "starring.runtime_slot_writer_fence_slot_guild_id_v2",
    "starring.runtime_slot_writer_fence_slot_ruleset_key_v2",
    "starring.runtime_slot_writer_fence_expected_epoch_v2",
    "starring.runtime_slot_writer_fence_drain_intent_id_v2",
    "starring.runtime_slot_writer_fence_product_operation_id_v2",
    "starring.runtime_slot_writer_fence_tenant_id_v2",
    "starring.runtime_slot_writer_fence_installation_id_v2",
    "starring.runtime_slot_writer_fence_deployment_id_v2",
    "starring.runtime_slot_writer_fence_expected_revision_v2",
    "starring.runtime_slot_writer_fence_marked_at_v2",
];

type SlotWriterFenceRow = (
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<DateTime<Utc>>,
);

async fn slot_writer_fence_row(
    pool: &PgPool,
    guild_id: &str,
    ruleset_key: &str,
) -> SlotWriterFenceRow {
    sqlx::query_as(
        "SELECT fence.slot_guild_id, fence.slot_ruleset_key, fence.writer_epoch, \
                fence.pending_drain_intent_id, fence.pending_product_operation_id, \
                fence.pending_tenant_id, fence.pending_installation_id, \
                fence.pending_deployment_id, fence.pending_expected_revision, \
                fence.pending_marked_at \
         FROM public.runtime_slot_writer_fences_v2 AS fence \
         WHERE fence.slot_guild_id = $1 AND fence.slot_ruleset_key = $2",
    )
    .bind(guild_id)
    .bind(ruleset_key)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn assert_open_slot_writer_fence(
    row: &SlotWriterFenceRow,
    guild_id: &str,
    ruleset_key: &str,
    expected_epoch: i64,
) {
    assert_eq!(row.0, guild_id);
    assert_eq!(row.1, ruleset_key);
    assert_eq!(row.2, expected_epoch);
    assert_eq!(row.3, None);
    assert_eq!(row.4, None);
    assert_eq!(row.5, None);
    assert_eq!(row.6, None);
    assert_eq!(row.7, None);
    assert_eq!(row.8, None);
    assert_eq!(row.9, None);
}

async fn assert_slot_writer_fence_gates_clear(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
) {
    let values = sqlx::query_scalar::<_, Vec<Option<String>>>(
        "SELECT ARRAY[\
            NULLIF(pg_catalog.current_setting($1, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($2, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($3, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($4, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($5, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($6, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($7, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($8, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($9, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($10, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($11, TRUE), '')\
         ]",
    )
    .bind(SLOT_WRITER_FENCE_GUCS[0])
    .bind(SLOT_WRITER_FENCE_GUCS[1])
    .bind(SLOT_WRITER_FENCE_GUCS[2])
    .bind(SLOT_WRITER_FENCE_GUCS[3])
    .bind(SLOT_WRITER_FENCE_GUCS[4])
    .bind(SLOT_WRITER_FENCE_GUCS[5])
    .bind(SLOT_WRITER_FENCE_GUCS[6])
    .bind(SLOT_WRITER_FENCE_GUCS[7])
    .bind(SLOT_WRITER_FENCE_GUCS[8])
    .bind(SLOT_WRITER_FENCE_GUCS[9])
    .bind(SLOT_WRITER_FENCE_GUCS[10])
    .fetch_one(executor)
    .await
    .unwrap();
    assert_eq!(values, vec![None; SLOT_WRITER_FENCE_GUCS.len()]);
}

async fn slot_writer_fence_manifests(pool: &PgPool) -> (bool, bool, bool, bool) {
    let exact_target_v2 = sqlx::query_scalar::<_, bool>(
        "SELECT pg_catalog.to_regprocedure(\
            'public.starring_runtime_exact_target_schema_manifest_v2()'\
         ) IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let query = if exact_target_v2 {
        "SELECT public.starring_runtime_interaction_schema_manifest_v1(), \
                public.starring_runtime_exact_target_schema_manifest_v2(), \
                public.starring_runtime_serving_schema_manifest_v1(), \
                public.starring_runtime_execution_schema_manifest_v1()"
    } else {
        "SELECT public.starring_runtime_interaction_schema_manifest_v1(), \
                public.starring_runtime_exact_target_schema_manifest_v1(), \
                public.starring_runtime_serving_schema_manifest_v1(), \
                public.starring_runtime_execution_schema_manifest_v1()"
    };
    sqlx::query_as(query)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn pre_slot_writer_fence_database(
    server: &PostgresTestServer,
    prefix: &str,
) -> (String, PgConnection, PgPool) {
    let base = server.connect_options();
    let database_name = format!("{prefix}_{}", unique_suffix());
    assert!(canonical_identifier(&database_name));
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    administrator
        .execute(format!("CREATE DATABASE {database_name}").as_str())
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(base.database(&database_name))
        .await
        .unwrap();
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_240_009)
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
    (database_name, administrator, pool)
}

async fn drop_slot_writer_fence_test_database(
    database_name: &str,
    mut administrator: PgConnection,
    pool: PgPool,
) {
    pool.close().await;
    administrator
        .execute(format!("DROP DATABASE {database_name} WITH (FORCE)").as_str())
        .await
        .unwrap();
}

async fn assert_slot_writer_fence_catalog_contract(database: &IsolatedDatabase) {
    assert_eq!(
        slot_writer_fence_manifests(&database.owner_pool).await,
        (true, true, true, true)
    );

    let table_shape = sqlx::query_as::<_, (bool, bool, bool, bool, i64, i64)>(
        "SELECT \
            relation.relkind = 'r', \
            relation.relpersistence = 'p', \
            NOT relation.relrowsecurity, \
            NOT relation.relforcerowsecurity, \
            pg_catalog.count(*) FILTER (WHERE privilege.grantee = 0), \
            pg_catalog.count(*) FILTER (WHERE privilege.grantee = role_row.oid) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = relation.relnamespace \
         INNER JOIN pg_catalog.pg_roles AS role_row ON role_row.rolname = $1 \
         LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
            relation.relacl, pg_catalog.acldefault('r', relation.relowner) \
         )) AS privilege ON TRUE \
         WHERE namespace.nspname = 'public' \
            AND relation.relname = 'runtime_slot_writer_fences_v2' \
         GROUP BY relation.oid, role_row.oid",
    )
    .bind(&database.role)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(table_shape, (true, true, true, true, 0, 0));

    let trigger_shapes = sqlx::query_as::<_, (String, i16, bool, bool, String)>(
        "SELECT trigger_row.tgname::TEXT, trigger_row.tgtype, \
                trigger_row.tgdeferrable, trigger_row.tginitdeferred, \
                trigger_row.tgfoid::REGPROCEDURE::TEXT \
         FROM pg_catalog.pg_trigger AS trigger_row \
         WHERE trigger_row.tgrelid = \
            'public.runtime_slot_writer_fences_v2'::REGCLASS \
            AND NOT trigger_row.tgisinternal \
         ORDER BY trigger_row.tgname",
    )
    .fetch_all(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        trigger_shapes,
        vec![
            (
                "runtime_slot_writer_fences_v2_assert_pending_symmetry".to_string(),
                29,
                true,
                true,
                "validate_runtime_slot_writer_fence_symmetry_v2()".to_string(),
            ),
            (
                "runtime_slot_writer_fences_v2_reject_row_mutation".to_string(),
                31,
                false,
                false,
                "reject_runtime_slot_writer_fence_mutation_v2()".to_string(),
            ),
            (
                "runtime_slot_writer_fences_v2_reject_truncate".to_string(),
                34,
                false,
                false,
                "reject_runtime_slot_writer_fence_mutation_v2()".to_string(),
            ),
        ]
    );

    let installation_trigger_shape = sqlx::query_as::<_, (i16, bool, bool, String)>(
        "SELECT trigger_row.tgtype, trigger_row.tgdeferrable, \
                trigger_row.tginitdeferred, trigger_row.tgfoid::REGPROCEDURE::TEXT \
         FROM pg_catalog.pg_trigger AS trigger_row \
         WHERE trigger_row.tgrelid = 'public.automation_installations'::REGCLASS \
            AND trigger_row.tgname = \
                'automation_installations_create_runtime_slot_writer_fence_v2' \
            AND NOT trigger_row.tgisinternal",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        installation_trigger_shape,
        (
            5,
            false,
            false,
            "starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_installation_insert_v2()"
                .to_string(),
        )
    );

    let drain_symmetry_shape = sqlx::query_as::<_, (i16, bool, bool, String)>(
        "SELECT trigger_row.tgtype, trigger_row.tgdeferrable, \
                trigger_row.tginitdeferred, trigger_row.tgfoid::REGPROCEDURE::TEXT \
         FROM pg_catalog.pg_trigger AS trigger_row \
         WHERE trigger_row.tgrelid = 'public.runtime_drain_intents_v2'::REGCLASS \
            AND trigger_row.tgname = \
                'runtime_drain_intents_v2_assert_slot_writer_fence_symmetry' \
            AND NOT trigger_row.tgisinternal",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        drain_symmetry_shape,
        (
            29,
            true,
            true,
            "validate_runtime_slot_writer_fence_symmetry_v2()".to_string(),
        )
    );

    let functions = sqlx::query_as::<_, (String, bool, String, String, bool, i64)>(
        "SELECT \
            pg_catalog.format( \
                '%I.%I(%s)', namespace.nspname, function_row.proname, \
                pg_catalog.replace( \
                    pg_catalog.oidvectortypes(function_row.proargtypes), ', ', ',' \
                ) \
            )::TEXT, \
            function_row.prosecdef, function_row.provolatile::TEXT, \
            function_row.proparallel::TEXT, \
            function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[], \
            pg_catalog.count(*) FILTER ( \
                WHERE privilege.grantee = 0 \
                    OR privilege.grantee = role_row.oid \
            ) \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = function_row.pronamespace \
         INNER JOIN pg_catalog.pg_roles AS role_row ON role_row.rolname = $1 \
         LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
            function_row.proacl, \
            pg_catalog.acldefault('f', function_row.proowner) \
         )) AS privilege ON TRUE \
         WHERE (namespace.nspname, function_row.proname) IN ( \
            ('public', 'reject_runtime_slot_writer_fence_mutation_v2'), \
            ('public', 'validate_runtime_slot_writer_fence_symmetry_v2'), \
            ('starring_runtime_private_v2', \
                'starring_runtime_slot_writer_fence_create_v2'), \
            ('starring_runtime_private_v2', \
                'starring_runtime_slot_writer_fence_lock_v2'), \
            ('starring_runtime_private_v2', \
                'starring_runtime_slot_writer_fence_begin_unsafe_v2'), \
            ('starring_runtime_private_v2', \
                'starring_runtime_slot_writer_fence_mark_drain_v2'), \
            ('starring_runtime_private_v2', \
                'starring_runtime_slot_writer_fence_installation_insert_v2') \
         ) \
         GROUP BY function_row.oid, namespace.nspname, role_row.oid \
         ORDER BY 1",
    )
    .bind(&database.role)
    .fetch_all(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(functions.len(), 7);
    for (identity, security_definer, volatility, parallel, fixed_path, exposed_acl) in functions {
        assert_eq!(volatility, "v", "{identity}");
        assert_eq!(parallel, "u", "{identity}");
        assert!(fixed_path, "{identity}");
        assert_eq!(exposed_acl, 0, "{identity}");
        assert_eq!(
            security_definer,
            identity == "public.validate_runtime_slot_writer_fence_symmetry_v2()",
            "{identity}"
        );
    }
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn slot_writer_fence_installation_security_and_catalog_are_closed() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let guild_id = GUILD.to_string();
    let initial = slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await;
    assert_open_slot_writer_fence(&initial, &guild_id, RULESET, 1);

    let second_guild = "9200102";
    let second_ruleset = "runtime_execution_second";
    let second_installation = "runtime-execution-installation-second";
    let mut installation = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *installation)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations (\
            installation_id, tenant_id, discord_application_id, discord_guild_id, \
            ruleset_key, lifecycle_state, current_authority_revision\
         ) VALUES ($1,$2,$3,$4,$5,'provisioning',1)",
    )
    .bind(second_installation)
    .bind(TENANT)
    .bind("9200302")
    .bind(second_guild)
    .bind(second_ruleset)
    .execute(&mut *installation)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions (\
            installation_id, revision, tenant_id, binding_revision, resource_bindings, \
            binding_fingerprint, policy_revision, required_approvals, \
            activation_ttl_seconds, authority_payload_digest, \
            created_by_principal_id, created_by_request_digest\
         ) VALUES ($1,1,$2,1,'{}'::JSONB,$3,1,1,3600,$4,$5,$6)",
    )
    .bind(second_installation)
    .bind(TENANT)
    .bind(BINDING_FINGERPRINT)
    .bind("5".repeat(64))
    .bind(PRINCIPAL)
    .bind("6".repeat(64))
    .execute(&mut *installation)
    .await
    .unwrap();
    installation.commit().await.unwrap();
    let second = slot_writer_fence_row(&database.owner_pool, second_guild, second_ruleset).await;
    assert_open_slot_writer_fence(&second, second_guild, second_ruleset, 1);

    for statement in [
        "INSERT INTO public.runtime_slot_writer_fences_v2 (\
            slot_guild_id, slot_ruleset_key, writer_epoch, updated_at\
         ) VALUES ('9200199','runtime_execution_rejected',1,\
                   pg_catalog.clock_timestamp())",
        "UPDATE public.runtime_slot_writer_fences_v2 \
         SET writer_epoch = writer_epoch \
         WHERE slot_guild_id = '9200101'",
        "DELETE FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = '9200101'",
        "TRUNCATE public.runtime_slot_writer_fences_v2",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.owner_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
    }

    for statement in [
        "SELECT * FROM public.runtime_slot_writer_fences_v2",
        "SELECT * FROM starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_lock_v2(\
                 '9200101','runtime_execution_ruleset'\
             )",
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_begin_unsafe_v2(\
                 '9200101','runtime_execution_ruleset',1\
             )",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "42501");
    }

    assert_slot_writer_fence_catalog_contract(&database).await;
    assert_cross_runtime_readiness(&mut database).await;
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn slot_writer_fence_epoch_and_first_apply_are_atomic_and_replay_stable() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let session = gateway_ready_session(&database, "runtime-slot-fence-controller").await;
    let snapshot = session.snapshot().clone();
    let canonical = canonical_product_drain(&snapshot);
    let guild_id = GUILD.to_string();
    let initial = slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await;
    let initial_epoch = initial.2;
    assert_open_slot_writer_fence(&initial, &guild_id, RULESET, initial_epoch);

    let mut hostile = database.owner_pool.begin().await.unwrap();
    sqlx::raw_sql(
        "SET LOCAL search_path = pg_temp, public; \
         CREATE TEMPORARY TABLE runtime_slot_writer_fences_v2 (writer_epoch BIGINT); \
         INSERT INTO runtime_slot_writer_fences_v2 VALUES (99)",
    )
    .execute(&mut *hostile)
    .await
    .unwrap();
    let locked_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT fence.writer_epoch \
         FROM starring_runtime_private_v2.\
              starring_runtime_slot_writer_fence_lock_v2($1,$2) AS fence",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .fetch_one(&mut *hostile)
    .await
    .unwrap();
    assert_eq!(locked_epoch, initial_epoch);
    let advanced = sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_begin_unsafe_v2($1,$2,$3)",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .bind(locked_epoch)
    .fetch_one(&mut *hostile)
    .await
    .unwrap();
    assert_eq!(advanced, initial_epoch + 1);
    let decoy_epoch =
        sqlx::query_scalar::<_, i64>("SELECT writer_epoch FROM runtime_slot_writer_fences_v2")
            .fetch_one(&mut *hostile)
            .await
            .unwrap();
    assert_eq!(decoy_epoch, 99);
    assert_slot_writer_fence_gates_clear(&mut *hostile).await;
    hostile.commit().await.unwrap();

    let stale = sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_begin_unsafe_v2($1,$2,$3)",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .bind(initial_epoch)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap_err();
    assert_database_error(&stale, "RX001", "runtime_execution_slot_writer_epoch_stale");
    assert_open_slot_writer_fence(
        &slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await,
        &guild_id,
        RULESET,
        initial_epoch + 1,
    );

    let mut first_apply = begin_product_drain_first_apply(&database.owner_pool).await;
    let inserted = call_product_drain_first_apply(&mut *first_apply, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    assert_slot_writer_fence_gates_clear(&mut *first_apply).await;
    first_apply.commit().await.unwrap();

    let pending = slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await;
    assert_eq!(pending.2, initial_epoch + 2);
    assert_eq!(
        pending.3.as_deref(),
        Some(canonical.drain_preimage().key.intent_id.as_str())
    );
    assert_eq!(
        pending.4.as_deref(),
        Some(canonical.product_preimage().operation_id.as_str())
    );
    assert_eq!(
        pending.5.as_deref(),
        Some(canonical.product_preimage().scope.tenant_id.as_str())
    );
    assert_eq!(
        pending.6.as_deref(),
        Some(canonical.product_preimage().scope.installation_id.as_str())
    );
    assert_eq!(
        pending.7.as_deref(),
        Some(canonical.product_preimage().scope.deployment_id.as_str())
    );
    assert_eq!(
        pending.8,
        Some(i64::try_from(canonical.product_preimage().expected_revision.get()).unwrap())
    );
    assert!(pending.9.is_some());

    let blocked = sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_begin_unsafe_v2($1,$2,$3)",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .bind(pending.2)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap_err();
    assert_database_error(
        &blocked,
        "RX007",
        "runtime_execution_product_drain_pending",
    );
    assert_eq!(
        slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await,
        pending
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;

    let replayed = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(replayed.outcome_name, "replayed");
    let replay_fence = slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await;
    assert_eq!(replay_fence, pending);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn slot_writer_fence_deferred_symmetry_rejects_both_orphan_directions() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let session = gateway_ready_session(&database, "runtime-slot-symmetry-controller").await;
    let canonical = canonical_product_drain(session.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");

    let mut missing_fence = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_slot_writer_fences_v2 DISABLE TRIGGER \
            runtime_slot_writer_fences_v2_reject_row_mutation",
    )
    .execute(&mut *missing_fence)
    .await
    .unwrap();
    sqlx::query("SAVEPOINT missing_fence")
        .execute(&mut *missing_fence)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *missing_fence)
    .await
    .unwrap();
    let fence_orphan = sqlx::query(
        "SET CONSTRAINTS \
            runtime_slot_writer_fences_v2_assert_pending_symmetry IMMEDIATE",
    )
    .execute(&mut *missing_fence)
    .await
    .unwrap_err();
    assert_database_error(
        &fence_orphan,
        "23514",
        "runtime_slot_writer_fence_symmetry_invalid",
    );
    sqlx::query("ROLLBACK TO SAVEPOINT missing_fence")
        .execute(&mut *missing_fence)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_slot_writer_fences_v2 ENABLE TRIGGER \
            runtime_slot_writer_fences_v2_reject_row_mutation",
    )
    .execute(&mut *missing_fence)
    .await
    .unwrap();
    missing_fence.commit().await.unwrap();

    let mut mismatched_drain = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_drain_intents_v2 DISABLE TRIGGER \
            runtime_drain_intents_v2_reject_row_mutation",
    )
    .execute(&mut *mismatched_drain)
    .await
    .unwrap();
    sqlx::query("SAVEPOINT mismatched_drain")
        .execute(&mut *mismatched_drain)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_drain_intents_v2 \
         SET drain_intent_id = $2 WHERE drain_intent_id = $1",
    )
    .bind(canonical.drain_preimage().key.intent_id.as_str())
    .bind("f".repeat(32))
    .execute(&mut *mismatched_drain)
    .await
    .unwrap();
    let drain_symmetry = sqlx::query(
        "SET CONSTRAINTS \
            runtime_drain_intents_v2_assert_slot_writer_fence_symmetry IMMEDIATE",
    )
    .execute(&mut *mismatched_drain)
    .await
    .unwrap_err();
    assert_database_error(
        &drain_symmetry,
        "23514",
        "runtime_slot_writer_fence_symmetry_invalid",
    );
    sqlx::query("ROLLBACK TO SAVEPOINT mismatched_drain")
        .execute(&mut *mismatched_drain)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_drain_intents_v2 ENABLE TRIGGER \
            runtime_drain_intents_v2_reject_row_mutation",
    )
    .execute(&mut *mismatched_drain)
    .await
    .unwrap();
    mismatched_drain.commit().await.unwrap();

    let mut missing_drain = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_drain_intents_v2 DISABLE TRIGGER \
            runtime_drain_intents_v2_reject_row_mutation",
    )
    .execute(&mut *missing_drain)
    .await
    .unwrap();
    sqlx::query("SAVEPOINT missing_drain")
        .execute(&mut *missing_drain)
        .await
        .unwrap();
    let drain_orphan = sqlx::query(
        "DELETE FROM public.runtime_drain_intents_v2 \
         WHERE drain_intent_id = $1",
    )
    .bind(canonical.drain_preimage().key.intent_id.as_str())
    .execute(&mut *missing_drain)
    .await
    .unwrap_err();
    assert_sqlstate(&drain_orphan, "23503");
    sqlx::query("ROLLBACK TO SAVEPOINT missing_drain")
        .execute(&mut *missing_drain)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_drain_intents_v2 ENABLE TRIGGER \
            runtime_drain_intents_v2_reject_row_mutation",
    )
    .execute(&mut *missing_drain)
    .await
    .unwrap();
    missing_drain.commit().await.unwrap();

    let fence = slot_writer_fence_row(&database.owner_pool, &GUILD.to_string(), RULESET).await;
    assert_eq!(
        fence.3.as_deref(),
        Some(canonical.drain_preimage().key.intent_id.as_str())
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM public.runtime_drain_intents_v2",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        1
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn slot_writer_fence_physical_epoch_update_aborts_a_stale_serializable_writer() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let session = gateway_ready_session(&database, "runtime-slot-serial-controller").await;
    let canonical = canonical_product_drain(session.snapshot());
    let guild_id = GUILD.to_string();

    let mut stale = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *stale)
        .await
        .unwrap();
    let stale_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT writer_epoch FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .fetch_one(&mut *stale)
    .await
    .unwrap();
    assert!(stale_epoch > 0);

    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let pending = slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await;
    assert_eq!(pending.2, stale_epoch + 1);

    let serialization = sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_begin_unsafe_v2($1,$2,$3)",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .bind(stale_epoch)
    .fetch_one(&mut *stale)
    .await
    .unwrap_err();
    assert_sqlstate(&serialization, "40001");
    stale.rollback().await.unwrap();

    let retry = sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_begin_unsafe_v2($1,$2,$3)",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .bind(stale_epoch)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap_err();
    assert_database_error(
        &retry,
        "RX007",
        "runtime_execution_product_drain_pending",
    );
    assert_eq!(
        slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await,
        pending
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn slot_writer_fence_inverse_corruption_fails_closed_without_epoch_advance() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let session = gateway_ready_session(&database, "runtime-slot-inverse-controller").await;
    let canonical = canonical_product_drain(session.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let guild_id = GUILD.to_string();
    let pending = slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await;

    let mut corrupt = database.owner_pool.begin().await.unwrap();
    for trigger in [
        "runtime_slot_writer_fences_v2_reject_row_mutation",
        "runtime_slot_writer_fences_v2_assert_pending_symmetry",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.runtime_slot_writer_fences_v2 DISABLE TRIGGER {trigger}"
        ))
        .execute(&mut *corrupt)
        .await
        .unwrap();
    }
    sqlx::query(
        "UPDATE public.runtime_slot_writer_fences_v2 \
         SET pending_drain_intent_id = NULL, \
             pending_product_operation_id = NULL, pending_tenant_id = NULL, \
             pending_installation_id = NULL, pending_deployment_id = NULL, \
             pending_expected_revision = NULL, pending_marked_at = NULL \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .execute(&mut *corrupt)
    .await
    .unwrap();
    for trigger in [
        "runtime_slot_writer_fences_v2_assert_pending_symmetry",
        "runtime_slot_writer_fences_v2_reject_row_mutation",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.runtime_slot_writer_fences_v2 ENABLE TRIGGER {trigger}"
        ))
        .execute(&mut *corrupt)
        .await
        .unwrap();
    }
    corrupt.commit().await.unwrap();

    let lock_error = sqlx::query_scalar::<_, i64>(
        "SELECT fence.writer_epoch FROM starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_lock_v2($1,$2) AS fence",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap_err();
    assert_database_error(
        &lock_error,
        "RX004",
        "runtime_execution_product_drain_state_invalid",
    );
    let begin_error = sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_begin_unsafe_v2($1,$2,$3)",
    )
    .bind(&guild_id)
    .bind(RULESET)
    .bind(pending.2)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap_err();
    assert_database_error(
        &begin_error,
        "RX004",
        "runtime_execution_product_drain_state_invalid",
    );
    let corrupted = slot_writer_fence_row(&database.owner_pool, &guild_id, RULESET).await;
    assert_open_slot_writer_fence(&corrupted, &guild_id, RULESET, pending.2);
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn slot_writer_fence_upgrade_backfills_existing_slots_and_rerun_is_atomic() {
    let server = PostgresTestServer::start();
    let (database_name, administrator, pool) =
        pre_slot_writer_fence_database(&server, "st_re_sf_upgrade").await;
    seed_claimable_deployment(&pool).await;
    assert!(
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT pg_catalog.to_regclass(\
                'public.runtime_slot_writer_fences_v2'\
             )::TEXT",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
        .is_none()
    );

    let mut upgrade = pool.begin().await.unwrap();
    sqlx::raw_sql(SLOT_WRITER_FENCE_MIGRATION)
        .execute(&mut *upgrade)
        .await
        .unwrap();
    upgrade.commit().await.unwrap();
    let guild_id = GUILD.to_string();
    let upgraded = slot_writer_fence_row(&pool, &guild_id, RULESET).await;
    assert_open_slot_writer_fence(&upgraded, &guild_id, RULESET, 1);
    assert_eq!(
        slot_writer_fence_manifests(&pool).await,
        (true, true, true, true)
    );

    let mut rerun = pool.begin().await.unwrap();
    let error = sqlx::raw_sql(SLOT_WRITER_FENCE_MIGRATION)
        .execute(&mut *rerun)
        .await
        .unwrap_err();
    rerun.rollback().await.unwrap();
    assert_database_error(&error, "RE001", "runtime_slot_writer_fence_preflight_drift");
    assert_eq!(
        slot_writer_fence_row(&pool, &guild_id, RULESET).await,
        upgraded
    );
    assert_eq!(
        slot_writer_fence_manifests(&pool).await,
        (true, true, true, true)
    );

    drop_slot_writer_fence_test_database(&database_name, administrator, pool).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn slot_writer_fence_upgrade_rejects_nonempty_roots_without_catalog_drift() {
    let server = PostgresTestServer::start();
    let (database_name, administrator, pool) =
        pre_slot_writer_fence_database(&server, "st_re_sf_nonempty").await;
    seed_claimable_deployment(&pool).await;
    let snapshot = product_drain_snapshot(&pool).await;
    let canonical = canonical_product_drain(&snapshot);
    insert_product_only(&pool, &canonical).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM public.runtime_product_operations_v2",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    let before_manifests = slot_writer_fence_manifests(&pool).await;
    assert_eq!(before_manifests, (true, true, true, true));

    let mut upgrade = pool.begin().await.unwrap();
    let error = sqlx::raw_sql(SLOT_WRITER_FENCE_MIGRATION)
        .execute(&mut *upgrade)
        .await
        .unwrap_err();
    upgrade.rollback().await.unwrap();
    assert_database_error(&error, "RE001", "runtime_slot_writer_fence_preflight_drift");
    assert!(
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT pg_catalog.to_regclass(\
                'public.runtime_slot_writer_fences_v2'\
             )::TEXT",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(slot_writer_fence_manifests(&pool).await, before_manifests);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM public.runtime_product_operations_v2",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );

    drop_slot_writer_fence_test_database(&database_name, administrator, pool).await;
    drop(server);
}
