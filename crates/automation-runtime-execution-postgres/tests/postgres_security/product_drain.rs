#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_observation_is_scope_only_and_function_bound() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let snapshot_value = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT deployment.snapshot FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let snapshot: RuntimeDeploymentSnapshotV1 =
        serde_json::from_value(snapshot_value.0).unwrap();
    let lookup = automation_runtime_controller::RuntimeProductDrainScopeLookupV2::from_locked_snapshot(
        &snapshot,
    )
    .unwrap();
    assert_product_drain_rejects_serializable(&database.executor_pool).await;
    assert_product_drain_snowflake_bounds(&database.executor_pool).await;
    let adapter = verified_execution_adapter(&database).await;
    let observed = automation_runtime_worker::RuntimeProductDrainObservationPortV2::observe_product_drain_scope(
        &adapter,
        lookup,
    )
    .await
    .unwrap();
    assert_eq!(
        observed.kind(),
        automation_runtime_controller::RuntimeProductDrainScopeObservationKindV2::Absent
    );
    assert_eq!(observed.locked_snapshot(), &snapshot);

    let canonical = canonical_product_drain(&snapshot);
    seed_canonical_product_drain(&database.owner_pool, &canonical).await;
    let observed = automation_runtime_worker::RuntimeProductDrainObservationPortV2::observe_product_drain_scope(
        &adapter,
        automation_runtime_controller::RuntimeProductDrainScopeLookupV2::from_locked_snapshot(
            &snapshot,
        )
        .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        observed.kind(),
        automation_runtime_controller::RuntimeProductDrainScopeObservationKindV2::Present
    );
    let persisted = observed.persisted().unwrap();
    assert_eq!(
        persisted.root().product_operation_id(),
        &canonical.product_preimage().operation_id
    );
    assert_eq!(
        persisted.intent().key().intent_id,
        canonical.drain_preimage().key.intent_id
    );
    assert_eq!(
        persisted.root().product_mutation_request_bytes(),
        canonical.product_mutation_request_bytes()
    );
    assert_eq!(
        persisted.root().product_mutation_digest(),
        canonical.product_mutation_digest()
    );
    assert_eq!(
        persisted.root().drain_intent_request_bytes(),
        canonical.drain_intent_request_bytes()
    );
    assert_eq!(
        persisted.root().drain_intent_digest(),
        canonical.drain_intent_digest()
    );

    for statement in [
        "INSERT INTO public.runtime_product_operations_v2 DEFAULT VALUES",
        "UPDATE public.runtime_product_operations_v2 SET product_operation_id = product_operation_id",
        "DELETE FROM public.runtime_product_operations_v2",
        "INSERT INTO public.runtime_drain_intents_v2 DEFAULT VALUES",
        "UPDATE public.runtime_drain_intents_v2 SET drain_intent_id = drain_intent_id",
        "DELETE FROM public.runtime_drain_intents_v2",
        "TRUNCATE public.runtime_slot_writer_fences_v2, public.runtime_drain_intents_v2",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.owner_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
    }
    let error = sqlx::query(
        "TRUNCATE public.runtime_slot_writer_fences_v2, public.runtime_drain_intents_v2, \
         public.runtime_product_operations_v2",
    )
    .execute(&database.owner_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&error, "23514");

    for statement in [
        "SELECT * FROM public.runtime_product_operations_v2",
        "SELECT * FROM public.runtime_drain_intents_v2",
        "INSERT INTO public.runtime_product_operations_v2 DEFAULT VALUES",
        "UPDATE public.runtime_product_operations_v2 SET product_operation_id = product_operation_id",
        "DELETE FROM public.runtime_product_operations_v2",
        "TRUNCATE public.runtime_product_operations_v2",
        "INSERT INTO public.runtime_drain_intents_v2 DEFAULT VALUES",
        "UPDATE public.runtime_drain_intents_v2 SET drain_intent_id = drain_intent_id",
        "DELETE FROM public.runtime_drain_intents_v2",
        "TRUNCATE public.runtime_slot_writer_fences_v2, public.runtime_drain_intents_v2",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "42501");
    }

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_observation_refreshes_after_the_scope_lock_wait() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let snapshot = product_drain_snapshot(&database.owner_pool).await;
    let canonical = canonical_product_drain(&snapshot);
    let adapter = verified_execution_adapter(&database).await;
    set_product_drain_row_triggers(&database.owner_pool, false).await;

    let mut writer = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock_shared(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *writer)
    .await
    .unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended(\
                pg_catalog.concat('starring-runtime-serving-slot-v1:', $1, ':', $2), 0\
            )\
         )",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *writer)
    .await
    .unwrap();
    sqlx::query(
        "SELECT deployment.deployment_id FROM public.runtime_deployments AS deployment \
         WHERE deployment.tenant_id = $1 AND deployment.installation_id = $2 \
           AND deployment.deployment_id = $3 AND deployment.revision = 1 \
           AND deployment.guild_id = $4 AND deployment.ruleset_key = $5 \
         FOR UPDATE",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(&mut *writer)
    .await
    .unwrap();
    insert_canonical_product_drain(&mut writer, &canonical).await;

    let observer = tokio::spawn(async move {
        automation_runtime_worker::RuntimeProductDrainObservationPortV2::observe_product_drain_scope(
            &adapter,
            automation_runtime_controller::RuntimeProductDrainScopeLookupV2::from_locked_snapshot(
                &snapshot,
            )
            .unwrap(),
        )
        .await
    });
    wait_for_advisory_lock_waiter(&database.owner_pool).await;
    writer.commit().await.unwrap();
    let observed = tokio::time::timeout(Duration::from_secs(3), observer)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    set_product_drain_row_triggers(&database.owner_pool, true).await;
    assert_eq!(
        observed.kind(),
        automation_runtime_controller::RuntimeProductDrainScopeObservationKindV2::Present
    );
    assert_eq!(
        observed
            .persisted()
            .unwrap()
            .root()
            .product_operation_id(),
        &canonical.product_preimage().operation_id
    );

    cleanup(database).await;
    drop(server);
}

async fn product_drain_snapshot(pool: &PgPool) -> RuntimeDeploymentSnapshotV1 {
    let snapshot = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT deployment.snapshot FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap();
    serde_json::from_value(snapshot.0).unwrap()
}

async fn wait_for_advisory_lock_waiter(pool: &PgPool) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let waiting = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (\
                    SELECT 1 FROM pg_catalog.pg_locks AS lock \
                    WHERE lock.locktype = 'advisory' AND NOT lock.granted\
                 )",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn assert_product_drain_rejects_serializable(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let error = sqlx::query(
        "SELECT * FROM public.starring_runtime_product_drain_observe_v2($1,$2,$3,$4,$5,$6)",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(1_i64)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_all(&mut *transaction)
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    transaction.rollback().await.unwrap();
}

async fn assert_product_drain_snowflake_bounds(pool: &PgPool) {
    for guild_id in ["0", "01", "18446744073709551616"] {
        let error = sqlx::query(
            "SELECT * FROM public.starring_runtime_product_drain_observe_v2($1,$2,$3,$4,$5,$6)",
        )
        .bind(TENANT)
        .bind(INSTALLATION)
        .bind(DEPLOYMENT)
        .bind(1_i64)
        .bind(guild_id)
        .bind(RULESET)
        .fetch_all(pool)
        .await
        .unwrap_err();
        assert_sqlstate(&error, "RX002");
    }
    let error = sqlx::query(
        "SELECT * FROM public.starring_runtime_product_drain_observe_v2($1,$2,$3,$4,$5,$6)",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(1_i64)
    .bind("18446744073709551615")
    .bind(RULESET)
    .fetch_all(pool)
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX001");
}

fn canonical_product_drain(
    snapshot: &RuntimeDeploymentSnapshotV1,
) -> automation_runtime_controller::RuntimeCanonicalProductDrainV2 {
    let preimage = automation_runtime_controller::RuntimeProductMutationPreimageV2 {
        operation_id: automation_runtime_controller::RuntimeProductOperationIdV2::parse(
            "00112233445566778899aabbccddeeff",
        )
        .unwrap(),
        scope: automation_runtime_controller::RuntimeDeploymentScopeV1::from_identity(
            &snapshot.identity,
        ),
        expected_revision: snapshot.revision,
        slot: automation_runtime_controller::RuntimeServingSlotV2::from_target(&snapshot.target),
        expected_target: snapshot.target.clone(),
        mutation_kind: automation_runtime_controller::RuntimeProductMutationKindV2::Teardown,
        product_semantic_request_digest:
            automation_runtime_controller::RuntimeProductSemanticRequestDigestV2::parse(
                "4".repeat(64),
            )
            .unwrap(),
    };
    automation_runtime_controller::RuntimeCanonicalProductDrainV2::new(
        preimage,
        automation_runtime_controller::RuntimeDrainIntentIdV2::parse(
            "ffeeddccbbaa99887766554433221100",
        )
        .unwrap(),
    )
    .unwrap()
}

async fn seed_canonical_product_drain(
    pool: &PgPool,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) {
    set_product_drain_row_triggers(pool, false).await;
    let mut transaction = pool.begin().await.unwrap();
    insert_canonical_product_drain(&mut transaction, canonical).await;
    transaction.commit().await.unwrap();
    set_product_drain_row_triggers(pool, true).await;
}

async fn set_product_drain_row_triggers(pool: &PgPool, enabled: bool) {
    let statements = if enabled {
        [
            "ALTER TABLE public.runtime_product_operations_v2 ENABLE TRIGGER runtime_product_operations_v2_reject_row_mutation",
            "ALTER TABLE public.runtime_drain_intents_v2 ENABLE TRIGGER runtime_drain_intents_v2_reject_row_mutation",
        ]
    } else {
        [
            "ALTER TABLE public.runtime_product_operations_v2 DISABLE TRIGGER runtime_product_operations_v2_reject_row_mutation",
            "ALTER TABLE public.runtime_drain_intents_v2 DISABLE TRIGGER runtime_drain_intents_v2_reject_row_mutation",
        ]
    };
    for statement in statements {
        sqlx::query(statement).execute(pool).await.unwrap();
    }
}

async fn insert_canonical_product_drain(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) {
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    let writer_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT fence.writer_epoch \
         FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2($1,$2) \
              AS fence",
    )
    .bind(drain.key.slot.guild_id.to_string())
    .bind(drain.key.slot.ruleset_key.as_str())
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_product_operations_v2 \
         (product_operation_id, tenant_id, installation_id, deployment_id, expected_revision, \
          expected_target_guild_id, expected_target_ruleset_key, expected_target_version, \
          expected_target_content_hash, expected_target_binding_revision, \
          expected_target_binding_fingerprint, product_mutation_request_bytes, \
          product_mutation_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(product.operation_id.as_str())
    .bind(product.scope.tenant_id.as_str())
    .bind(product.scope.installation_id.as_str())
    .bind(product.scope.deployment_id.as_str())
    .bind(i64::try_from(product.expected_revision.get()).unwrap())
    .bind(product.expected_target.guild_id.to_string())
    .bind(product.expected_target.ruleset_key.as_str())
    .bind(i64::from(product.expected_target.version.get()))
    .bind(product.expected_target.content_hash.to_hex())
    .bind(i64::try_from(product.expected_target.binding_revision.get()).unwrap())
    .bind(product.expected_target.binding_fingerprint.as_str())
    .bind(canonical.product_mutation_request_bytes())
    .bind(canonical.product_mutation_digest().as_str())
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_drain_intents_v2 \
         (drain_intent_id, tenant_id, installation_id, deployment_id, slot_guild_id, \
          slot_ruleset_key, expected_revision, product_operation_id, product_mutation_digest, \
          drain_intent_request_bytes, drain_intent_digest, intent_revision, intent_state) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,1,'pending')",
    )
    .bind(drain.key.intent_id.as_str())
    .bind(drain.key.scope.tenant_id.as_str())
    .bind(drain.key.scope.installation_id.as_str())
    .bind(drain.key.scope.deployment_id.as_str())
    .bind(drain.key.slot.guild_id.to_string())
    .bind(drain.key.slot.ruleset_key.as_str())
    .bind(i64::try_from(drain.key.expected_revision.get()).unwrap())
    .bind(drain.key.product_operation_id.as_str())
    .bind(drain.key.product_mutation_digest.as_str())
    .bind(canonical.drain_intent_request_bytes())
    .bind(canonical.drain_intent_digest().as_str())
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
             starring_runtime_slot_writer_fence_mark_drain_v2(\
                 $1,$2,$3,$4,$5,$6,$7,$8,$9\
             )",
    )
    .bind(drain.key.slot.guild_id.to_string())
    .bind(drain.key.slot.ruleset_key.as_str())
    .bind(writer_epoch)
    .bind(drain.key.intent_id.as_str())
    .bind(drain.key.product_operation_id.as_str())
    .bind(drain.key.scope.tenant_id.as_str())
    .bind(drain.key.scope.installation_id.as_str())
    .bind(drain.key.scope.deployment_id.as_str())
    .bind(i64::try_from(drain.key.expected_revision.get()).unwrap())
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
}
