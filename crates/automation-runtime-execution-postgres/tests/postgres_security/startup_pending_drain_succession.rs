const STARTUP_PENDING_DRAIN_SUCCESSION: &str = "SELECT \
    journal_outcome_name, terminal_outcome_name, action_authority_revision, \
    selection_authority_revision, database_now, minimum_database_now, recorded_at, \
    terminal_projection_bytes, terminal_digest \
 FROM public.starring_runtime_startup_recovery_pending_drain_succession_v3(\
    $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,\
    $17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,\
    $33,$34,$35,$36,$37,$38,$39,$40,$41,$42,$43,$44,$45\
 )";
const STARTUP_PENDING_DRAIN_SELECTOR_V3: &str =
    "public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)";
const STARTUP_PENDING_DRAIN_SUCCESSION_V3: &str =
    "public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)";
const STARTUP_PENDING_DRAIN_SUCCESSION_MIGRATION: &str =
    include_str!("../../../../migrations/202607280001_add_pending_drain_succession_v3.sql");

#[derive(Debug, sqlx::FromRow)]
struct PendingDrainSuccessionSelectionRow {
    selection_outcome_name: String,
    observed_database_now: DateTime<Utc>,
    observed_owner_expires_at: DateTime<Utc>,
    selected_drain_intent_id: Option<String>,
    selected_source_intent_revision: Option<i64>,
    selected_source_state_digest: Option<String>,
    predecessor_claim_terminal_digest: Option<String>,
    predecessor_process_instance_id: Option<String>,
    predecessor_lease_epoch: Option<i64>,
    predecessor_claim_revision: Option<i64>,
    predecessor_claim_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
struct PendingDrainSuccessionRow {
    journal_outcome_name: String,
    terminal_outcome_name: String,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    database_now: DateTime<Utc>,
    minimum_database_now: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
    terminal_projection_bytes: Vec<u8>,
    terminal_digest: String,
}

#[derive(Clone)]
struct PendingDrainSuccessionFixture {
    recovery_id: String,
    owner: StartupObservationOwnerTuple,
    minimum_database_now: DateTime<Utc>,
    selected_drain_intent_id: String,
    selected_source_intent_revision: i64,
    selected_source_state_digest: String,
    predecessor_claim_terminal_digest: String,
    predecessor_process_instance_id: String,
    predecessor_lease_epoch: i64,
    predecessor_claim_revision: i64,
    predecessor_claim_expires_at: DateTime<Utc>,
    seal_key: Vec<u8>,
}

async fn select_pending_drain_succession(
    pool: &PgPool,
    owner: &StartupObservationOwnerTuple,
) -> Result<PendingDrainSuccessionSelectionRow, sqlx::Error> {
    let minimum_database_now = database_now(pool).await;
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY")
        .execute(&mut *transaction)
        .await?;
    let selected = sqlx::query_as::<_, PendingDrainSuccessionSelectionRow>(
        "SELECT selection_outcome_name, observed_database_now, \
            observed_owner_expires_at, selected_drain_intent_id, \
            selected_source_intent_revision, selected_source_state_digest, \
            predecessor_claim_terminal_digest, predecessor_process_instance_id, \
            predecessor_lease_epoch, predecessor_claim_revision, \
            predecessor_claim_expires_at \
         FROM public.starring_runtime_startup_recovery_select_pending_drain_v3(\
            $1,$2,$3,$4,$5,$6,$7\
         )",
    )
    .bind(&owner.0)
    .bind(&owner.1)
    .bind(owner.2)
    .bind(&owner.3)
    .bind(owner.4)
    .bind(owner.5)
    .bind(minimum_database_now)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(selected)
}

async fn rewrite_predecessor_claim_expiry(
    pool: &PgPool,
    recovery_id: &str,
    drain_intent_id: &str,
    expires_at: DateTime<Utc>,
) -> String {
    let (terminal_projection_bytes, canonical_state_bytes) =
        sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
            "SELECT action.terminal_projection_bytes, drain.canonical_state_bytes \
             FROM public.runtime_startup_recovery_actions_v2 AS action \
             CROSS JOIN public.runtime_drain_intents_v2 AS drain \
             WHERE action.recovery_id = $1 \
                AND action.action_authority_revision = 2 \
                AND drain.drain_intent_id = $2",
        )
        .bind(recovery_id)
        .bind(drain_intent_id)
        .fetch_one(pool)
        .await
        .unwrap();
    let (source_digest_frame, evidence_frame, product_root_frame) =
        sqlx::query_as::<_, (Vec<u8>, Vec<u8>, Vec<u8>)>(
            "SELECT \
                starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(\
                    $1, 1::SMALLINT, 1::SMALLINT\
                ), \
                starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(\
                    $1, 1::SMALLINT, 3::SMALLINT\
                ), \
                starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(\
                    $1, 1::SMALLINT, 4::SMALLINT\
                )",
        )
        .bind(&terminal_projection_bytes)
        .fetch_one(pool)
        .await
        .unwrap();
    let successor_state_frame = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT pg_catalog.convert_to(\
            pg_catalog.regexp_replace(\
                pg_catalog.convert_from($1, 'UTF8'), \
                '\"claim_expires_at_unix_microseconds\":-?[0-9]+', \
                pg_catalog.concat(\
                    '\"claim_expires_at_unix_microseconds\":', $2::BIGINT\
                ), \
                'g'\
            ), \
            'UTF8'\
        )",
    )
    .bind(&canonical_state_bytes)
    .bind(expires_at.timestamp_micros())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_ne!(successor_state_frame, canonical_state_bytes);
    let progressed_projection = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT starring_runtime_private_v2.\
            starring_runtime_pending_drain_projection_v2(\
                1::SMALLINT,$1,$2,$3,$4\
            )",
    )
    .bind(source_digest_frame)
    .bind(&successor_state_frame)
    .bind(evidence_frame)
    .bind(product_root_frame)
    .fetch_one(pool)
    .await
    .unwrap();
    let successor_digest = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.encode(pg_catalog.sha256($1), 'hex')",
    )
    .bind(&successor_state_frame)
    .fetch_one(pool)
    .await
    .unwrap();

    let mut transaction = pool.begin().await.unwrap();
    for statement in [
        "ALTER TABLE public.runtime_drain_intents_v2 DISABLE TRIGGER USER",
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 DISABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    assert_eq!(
        sqlx::query(
            "UPDATE public.runtime_drain_intents_v2 \
             SET canonical_state_bytes = $2, canonical_state_digest = $3 \
             WHERE drain_intent_id = $1",
        )
        .bind(drain_intent_id)
        .bind(&successor_state_frame)
        .bind(&successor_digest)
        .execute(&mut *transaction)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    assert_eq!(
        sqlx::query(
            "UPDATE public.runtime_startup_recovery_actions_v2 AS action \
             SET owner_expires_at = $3, \
                terminal_projection_bytes = $4, \
                terminal_digest = \
                    starring_runtime_private_v2.\
                    starring_runtime_startup_recovery_terminal_digest_v2(\
                        action.record_format_version, action.recovery_id, \
                        action.originating_emergency_generation, \
                        action.coordinator_generation, \
                        action.action_authority_revision, \
                        action.selection_authority_revision, \
                        action.recovery_class, action.gateway_shard_id, \
                        action.owner_process_instance_id, action.owner_lease_epoch, \
                        action.owner_runtime_build_revision, action.owner_revision, \
                        $3, action.minimum_database_now, action.recorded_at, $4\
                    ) \
             WHERE action.recovery_id = $1 \
                AND action.action_authority_revision = $2",
        )
        .bind(recovery_id)
        .bind(2_i64)
        .bind(expires_at)
        .bind(&progressed_projection)
        .execute(&mut *transaction)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    for statement in [
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 ENABLE TRIGGER USER",
        "ALTER TABLE public.runtime_drain_intents_v2 ENABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
    let exact = sqlx::query_scalar::<_, bool>(
        "SELECT starring_runtime_private_v2.\
            starring_runtime_pending_drain_state_exact_v2(drain) \
         FROM public.runtime_drain_intents_v2 AS drain \
         WHERE drain.drain_intent_id = $1",
    )
    .bind(drain_intent_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(exact);
    sqlx::query_scalar::<_, String>(
        "SELECT terminal_digest \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_id = $1 AND action_authority_revision = 2",
    )
    .bind(recovery_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn acquire_successor_owner(
    database: &IsolatedDatabase,
    predecessor: &StartupObservationOwnerTuple,
) -> StartupObservationOwnerTuple {
    let forced_expiry = database_now(&database.owner_pool).await - TimeDelta::hours(2);
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_gateway_owners DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query(
            "UPDATE public.runtime_gateway_owners \
             SET expires_at = $1 \
             WHERE gateway_shard_id = $2 \
                AND process_instance_id = $3 \
                AND lease_epoch = $4",
        )
        .bind(forced_expiry)
        .bind(&predecessor.0)
        .bind(&predecessor.1)
        .bind(predecessor.2)
        .execute(&mut *transaction)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    sqlx::query("ALTER TABLE public.runtime_gateway_owners ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
            expected_build_revision, owner_revision, expires_at \
         FROM public.starring_runtime_gateway_owner_acquire_v1(\
            'shard:0', 'pending-drain-successor-process', \
            'pending-drain-successor-build', 300000\
         ) \
         WHERE outcome_name = 'acquired'",
    )
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    assert_eq!(owner.0, predecessor.0);
    assert_ne!(owner.1, predecessor.1);
    assert_eq!(owner.2, predecessor.2 + 1);
    owner
}

async fn corrupt_predecessor_journal_identity(
    pool: &PgPool,
    recovery_id: &str,
) -> String {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 \
         DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query(
            "UPDATE public.runtime_startup_recovery_actions_v2 AS action \
             SET owner_process_instance_id = 'tampered-predecessor-process', \
                terminal_digest = \
                    starring_runtime_private_v2.\
                    starring_runtime_startup_recovery_terminal_digest_v2(\
                        action.record_format_version, action.recovery_id, \
                        action.originating_emergency_generation, \
                        action.coordinator_generation, \
                        action.action_authority_revision, \
                        action.selection_authority_revision, \
                        action.recovery_class, action.gateway_shard_id, \
                        'tampered-predecessor-process', action.owner_lease_epoch, \
                        action.owner_runtime_build_revision, action.owner_revision, \
                        action.owner_expires_at, action.minimum_database_now, \
                        action.recorded_at, action.terminal_projection_bytes\
                    ) \
             WHERE action.recovery_id = $1 \
                AND action.action_authority_revision = 2",
        )
        .bind(recovery_id)
        .execute(&mut *transaction)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    sqlx::query(
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 \
         ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    sqlx::query_scalar::<_, String>(
        "SELECT terminal_digest \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_id = $1 AND action_authority_revision = 2",
    )
    .bind(recovery_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn corrupt_predecessor_deployment_fence(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query(
            "UPDATE public.runtime_deployments \
             SET last_fencing_token = last_fencing_token + 1, \
                snapshot = pg_catalog.jsonb_set(\
                    snapshot, '{last_fencing_token}', \
                    pg_catalog.to_jsonb(last_fencing_token + 1), FALSE\
                ) \
             WHERE deployment_id = $1",
        )
        .bind(DEPLOYMENT)
        .execute(&mut *transaction)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    sqlx::query("ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_committed_succession_certification(pool: &PgPool) {
    let operation_id = "abcdefabcdefabcdefabcdefabcdefab";
    let fingerprint = "6".repeat(64);
    let terminal_at = database_now(pool).await;
    let receipt = b"committed-certification";
    let digest = sqlx::query_scalar::<_, String>(
        "SELECT starring_runtime_private_v2.\
            starring_runtime_certification_terminal_digest_v2(\
                2::SMALLINT,$1,$2,$3,$4,$5,1,1,'certification_committed',\
                'live',2,1,$6,$7\
            )",
    )
    .bind(operation_id)
    .bind(&fingerprint)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(terminal_at)
    .bind(receipt.as_slice())
    .fetch_one(pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    for statement in [
        "ALTER TABLE public.runtime_certification_operations_v2 DISABLE TRIGGER USER",
        "ALTER TABLE public.runtime_certification_operation_terminals_v2 DISABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO public.runtime_certification_operations_v2 (\
            operation_id, tenant_id, installation_id, deployment_id, \
            deployment_revision, convergence_attempt_no, \
            certification_intent_bytes, intent_fingerprint\
         ) VALUES ($1,$2,$3,$4,1,1,pg_catalog.convert_to('{}','UTF8'),$5)",
    )
    .bind(operation_id)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(&fingerprint)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_certification_operation_terminals_v2 (\
            record_format_version, operation_id, intent_fingerprint, tenant_id, \
            installation_id, deployment_id, deployment_revision, \
            convergence_attempt_no, terminal_outcome_name, resulting_phase, \
            resulting_deployment_revision, resulting_convergence_attempt_no, \
            terminal_at, terminal_receipt_bytes, terminal_receipt_digest\
         ) VALUES (\
            2,$1,$2,$3,$4,$5,1,1,'certification_committed','live',2,1,$6,$7,$8\
         )",
    )
    .bind(operation_id)
    .bind(&fingerprint)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(terminal_at)
    .bind(receipt.as_slice())
    .bind(&digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for statement in [
        "ALTER TABLE public.runtime_certification_operation_terminals_v2 ENABLE TRIGGER USER",
        "ALTER TABLE public.runtime_certification_operations_v2 ENABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn claimed_predecessor(
    database: &IsolatedDatabase,
    predecessor_recovery_id: &str,
) -> (
    PendingDrainExecutionFixture,
    PendingDrainExecutionRow,
    StartupObservationOwnerTuple,
) {
    seed_pending_drain_execution_candidate(database).await;
    let predecessor =
        pending_drain_execution_fixture(database, predecessor_recovery_id).await;
    let claim = committed_pending_drain_claim(&database.executor_pool, &predecessor)
        .await
        .unwrap();
    assert_eq!(claim.journal_outcome_name, "applied");
    assert_eq!(claim.terminal_outcome_name, "claimed");
    let successor_owner = acquire_successor_owner(database, &predecessor.owner).await;
    (predecessor, claim, successor_owner)
}

async fn expired_succession_fixture(
    database: &IsolatedDatabase,
    predecessor_recovery_id: &str,
    successor_recovery_id: &str,
) -> PendingDrainSuccessionFixture {
    let (predecessor, claim, owner) =
        claimed_predecessor(database, predecessor_recovery_id).await;
    let expired_at = claim.recorded_at + TimeDelta::microseconds(1);
    let predecessor_terminal_digest = rewrite_predecessor_claim_expiry(
        &database.owner_pool,
        predecessor_recovery_id,
        &predecessor.selected_drain_intent_id,
        expired_at,
    )
    .await;
    assert!(database_now(&database.owner_pool).await >= expired_at);
    let selected = select_pending_drain_succession(&database.executor_pool, &owner)
        .await
        .unwrap();
    assert_eq!(
        selected.selection_outcome_name,
        "expired_previous_owner"
    );
    assert_eq!(selected.observed_owner_expires_at, owner.5);
    assert_eq!(selected.predecessor_claim_expires_at, Some(expired_at));
    assert_eq!(
        selected.predecessor_claim_terminal_digest.as_deref(),
        Some(predecessor_terminal_digest.as_str())
    );
    let selected_drain_intent_id = selected.selected_drain_intent_id.unwrap();
    PendingDrainSuccessionFixture {
        recovery_id: successor_recovery_id.to_string(),
        owner,
        minimum_database_now: selected.observed_database_now,
        seal_key: decode_pending_drain_intent_id(&selected_drain_intent_id),
        selected_drain_intent_id,
        selected_source_intent_revision: selected.selected_source_intent_revision.unwrap(),
        selected_source_state_digest: selected.selected_source_state_digest.unwrap(),
        predecessor_claim_terminal_digest: selected
            .predecessor_claim_terminal_digest
            .unwrap(),
        predecessor_process_instance_id: selected.predecessor_process_instance_id.unwrap(),
        predecessor_lease_epoch: selected.predecessor_lease_epoch.unwrap(),
        predecessor_claim_revision: selected.predecessor_claim_revision.unwrap(),
        predecessor_claim_expires_at: selected.predecessor_claim_expires_at.unwrap(),
    }
}

async fn call_pending_drain_succession(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    fixture: &PendingDrainSuccessionFixture,
) -> Result<PendingDrainSuccessionRow, sqlx::Error> {
    sqlx::query_as::<_, PendingDrainSuccessionRow>(STARTUP_PENDING_DRAIN_SUCCESSION)
        .bind(&fixture.recovery_id)
        .bind(1_i64)
        .bind(2_i64)
        .bind(2_i64)
        .bind(1_i64)
        .bind(&fixture.owner.0)
        .bind(&fixture.owner.1)
        .bind(fixture.owner.2)
        .bind(&fixture.owner.3)
        .bind(fixture.owner.4)
        .bind(fixture.owner.5)
        .bind(fixture.minimum_database_now)
        .bind(&fixture.owner.1)
        .bind(1_i64)
        .bind(1_i64)
        .bind("ready")
        .bind(1_i64)
        .bind(2_i64)
        .bind(1_i64)
        .bind(0_i64)
        .bind(&fixture.owner.1)
        .bind(1_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind(&fixture.selected_drain_intent_id)
        .bind(fixture.selected_source_intent_revision)
        .bind(&fixture.selected_source_state_digest)
        .bind(&fixture.predecessor_claim_terminal_digest)
        .bind(false)
        .bind(0_i64)
        .bind(0_i64)
        .bind(&fixture.seal_key)
        .bind(1_i64)
        .bind(1_i64)
        .bind(1_i64)
        .bind(2_i64)
        .bind(1_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind(1_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind(false)
        .fetch_one(executor)
        .await
}

async fn committed_pending_drain_succession(
    pool: &PgPool,
    fixture: &PendingDrainSuccessionFixture,
) -> Result<PendingDrainSuccessionRow, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    prepare_pending_drain_write(&mut transaction).await?;
    match call_pending_drain_succession(&mut *transaction, fixture).await {
        Ok(row) => {
            transaction.commit().await?;
            Ok(row)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn pending_drain_succession_catalog_image(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
) -> (String, bool, i64, String) {
    let catalog_fingerprint = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.encode(\
            pg_catalog.sha256(pg_catalog.convert_to(\
                pg_catalog.string_agg(\
                    pg_catalog.concat_ws(\
                        '|', namespace.nspname, function_row.proname, \
                        pg_catalog.pg_get_function_identity_arguments(function_row.oid), \
                        function_row.proowner::TEXT, \
                        COALESCE(function_row.proacl::TEXT, ''), \
                        pg_catalog.pg_get_functiondef(function_row.oid)\
                    ), E'\\n' ORDER BY namespace.nspname, function_row.proname, \
                        pg_catalog.pg_get_function_identity_arguments(function_row.oid)\
                ), 'UTF8'\
            )), 'hex') \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = function_row.pronamespace \
         WHERE (\
                namespace.nspname = 'starring_runtime_private_v2' \
                AND function_row.proname LIKE \
                    'starring_runtime_pending_drain%v3'\
            ) OR (\
                namespace.nspname = 'public' \
                AND function_row.proname IN (\
                    'starring_runtime_startup_recovery_select_pending_drain_v3', \
                    'starring_runtime_startup_recovery_pending_drain_succession_v3', \
                    'starring_runtime_execution_schema_manifest_v1', \
                    'starring_runtime_execution_database_readiness_v1'\
                )\
            )",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let manifest = sqlx::query_scalar::<_, bool>(
        "SELECT public.starring_runtime_execution_schema_manifest_v1()",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let readiness = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_runtime_execution_database_readiness_v1()",
    )
    .fetch_one(executor_pool)
    .await
    .unwrap();
    let readiness_digest = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.encode(\
            pg_catalog.sha256(pg_catalog.convert_to(\
                pg_catalog.pg_get_functiondef(function_row.oid), 'UTF8'\
            )), 'hex') \
         FROM pg_catalog.pg_proc AS function_row \
         WHERE function_row.oid = pg_catalog.to_regprocedure(\
            'public.starring_runtime_execution_database_readiness_v1()'\
         )",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    (
        catalog_fingerprint,
        manifest,
        readiness,
        readiness_digest,
    )
}

async fn seed_live_pending_drain_candidate(
    database: &IsolatedDatabase,
) -> automation_runtime_controller::RuntimeCanonicalProductDrainV2 {
    seed_live_for_startup_observation(database, 300_000).await;
    let Json(snapshot) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT snapshot FROM public.runtime_deployments \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(snapshot).unwrap();
    assert_eq!(snapshot.phase, RuntimeDeploymentPhaseV1::Live);
    let canonical = canonical_product_drain(&snapshot);
    disconnect_product_drain_serving_lease(database).await;
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    canonical
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_v3_selector_accepts_its_live_source() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let canonical = seed_live_pending_drain_candidate(&database).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let observed = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(observed["outcome_name"], "observed");
    assert_eq!(observed["serving_state_name"], "empty");
    assert_eq!(observed["pending_runtime_drain_intent_count"], 1);
    let selected = select_pending_drain_succession(&database.executor_pool, &owner)
        .await
        .unwrap();
    assert_eq!(selected.selection_outcome_name, "unclaimed");
    assert_eq!(
        selected.selected_drain_intent_id.as_deref(),
        Some(canonical.drain_preimage().key.intent_id.as_str())
    );
    assert_eq!(selected.selected_source_intent_revision, Some(1));
    let selected_drain_intent_id = selected.selected_drain_intent_id.unwrap();
    let fixture = PendingDrainExecutionFixture {
        recovery_id: "12121212121212121212121212121212".to_string(),
        owner,
        minimum_database_now: selected.observed_database_now,
        seal_key: decode_pending_drain_intent_id(&selected_drain_intent_id),
        selected_drain_intent_id,
        selected_source_intent_revision: selected.selected_source_intent_revision.unwrap(),
        selected_source_state_digest: selected.selected_source_state_digest.unwrap(),
    };
    let claim = committed_pending_drain_claim(&database.executor_pool, &fixture)
        .await
        .unwrap();
    assert_eq!(claim.journal_outcome_name, "applied");
    assert_eq!(claim.terminal_outcome_name, "claimed");
    let (phase, intent_revision, journal_count) = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT deployment.phase, drain.intent_revision, (\
            SELECT pg_catalog.count(*) \
            FROM public.runtime_startup_recovery_actions_v2\
         ) \
         FROM public.runtime_deployments AS deployment \
         CROSS JOIN public.runtime_drain_intents_v2 AS drain \
         WHERE deployment.deployment_id = $1 \
            AND drain.drain_intent_id = $2",
    )
    .bind(DEPLOYMENT)
    .bind(canonical.drain_preimage().key.intent_id.as_str())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(phase, "live");
    assert_eq!(intent_revision, 2);
    assert_eq!(journal_count, 1);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_v3_selector_rejects_an_unrelated_live_deployment() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_pending_drain_candidate(&database).await;
    seed_second_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(&database).await;
    let (unrelated, _) = selector_stale_live_session(
        &database,
        &adapter,
        "pending-drain-unrelated-live-controller",
        "pending-drain-unrelated-live-panel",
        "pending-drain-unrelated-live-process",
    )
    .await;
    assert_eq!(
        unrelated.snapshot().identity.deployment_id.as_str(),
        SELECTOR_DEPLOYMENT
    );
    let before = pending_drain_execution_state(&database.owner_pool).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let error = select_pending_drain_succession(&database.executor_pool, &owner)
        .await
        .unwrap_err();
    assert_database_error(
        &error,
        "RX003",
        "runtime_startup_pending_drain_higher_priority",
    );
    assert_eq!(pending_drain_execution_state(&database.owner_pool).await, before);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_v3_selector_rejects_a_detached_source_fence() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_pending_drain_candidate(&database).await;
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_slot_writer_fences_v2 DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_slot_writer_fences_v2 \
         SET pending_drain_intent_id = NULL, \
             pending_product_operation_id = NULL, \
             pending_tenant_id = NULL, \
             pending_installation_id = NULL, \
             pending_deployment_id = NULL, \
             pending_expected_revision = NULL, \
             pending_marked_at = NULL \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_slot_writer_fences_v2 ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let before = pending_drain_execution_state(&database.owner_pool).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let error = select_pending_drain_succession(&database.executor_pool, &owner)
        .await
        .unwrap_err();
    assert_database_error(
        &error,
        "RX003",
        "runtime_startup_pending_drain_higher_priority",
    );
    assert_eq!(pending_drain_execution_state(&database.owner_pool).await, before);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_v3_selector_classifies_closed_source_states() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let predecessor_owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let none = select_pending_drain_succession(&database.executor_pool, &predecessor_owner)
        .await
        .unwrap();
    assert_eq!(none.selection_outcome_name, "no_candidate");
    assert!(none.selected_drain_intent_id.is_none());
    seed_pending_drain_execution_candidate(&database).await;
    let unclaimed = select_pending_drain_succession(&database.executor_pool, &predecessor_owner)
        .await
        .unwrap();
    assert_eq!(unclaimed.selection_outcome_name, "unclaimed");
    assert_eq!(
        unclaimed.selected_drain_intent_id.as_deref(),
        Some("ffeeddccbbaa99887766554433221100")
    );

    let predecessor = pending_drain_execution_fixture(
        &database,
        "11111111111111111111111111111111",
    )
    .await;
    let claim = committed_pending_drain_claim(&database.executor_pool, &predecessor)
        .await
        .unwrap();
    let successor_owner = acquire_successor_owner(&database, &predecessor_owner).await;
    let fresh_expiry = claim.recorded_at + TimeDelta::hours(1);
    let fresh_terminal_digest = rewrite_predecessor_claim_expiry(
        &database.owner_pool,
        &predecessor.recovery_id,
        &predecessor.selected_drain_intent_id,
        fresh_expiry,
    )
    .await;
    let fresh = select_pending_drain_succession(&database.executor_pool, &successor_owner)
        .await
        .unwrap();
    assert_eq!(fresh.selection_outcome_name, "fresh_previous_owner");
    assert_eq!(fresh.predecessor_claim_expires_at, Some(fresh_expiry));
    assert_eq!(
        fresh.predecessor_claim_terminal_digest.as_deref(),
        Some(fresh_terminal_digest.as_str())
    );
    assert_eq!(
        fresh.predecessor_process_instance_id.as_deref(),
        Some(predecessor_owner.1.as_str())
    );
    assert_eq!(fresh.predecessor_lease_epoch, Some(predecessor_owner.2));

    let expired_at = claim.recorded_at + TimeDelta::microseconds(1);
    rewrite_predecessor_claim_expiry(
        &database.owner_pool,
        &predecessor.recovery_id,
        &predecessor.selected_drain_intent_id,
        expired_at,
    )
    .await;
    assert!(database_now(&database.owner_pool).await >= expired_at);
    let expired = select_pending_drain_succession(&database.executor_pool, &successor_owner)
        .await
        .unwrap();
    assert_eq!(expired.selection_outcome_name, "expired_previous_owner");
    assert!(expired.observed_database_now >= expired_at);
    assert_eq!(expired.predecessor_claim_expires_at, Some(expired_at));
    assert_eq!(expired.predecessor_claim_revision, Some(1));
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_rerun_is_atomic() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let before = pending_drain_succession_catalog_image(
        &database.owner_pool,
        &database.executor_pool,
    )
    .await;
    assert!(before.1);
    assert_eq!(before.2, 1);
    let mut transaction = database.owner_pool.begin().await.unwrap();
    let error = sqlx::raw_sql(STARTUP_PENDING_DRAIN_SUCCESSION_MIGRATION)
        .execute(&mut *transaction)
        .await
        .unwrap_err();
    transaction.rollback().await.unwrap();
    assert_database_error(
        &error,
        "RE001",
        "runtime_pending_drain_succession_preflight_drift",
    );
    assert_eq!(
        pending_drain_succession_catalog_image(
            &database.owner_pool,
            &database.executor_pool,
        )
        .await,
        before
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_applies_once_and_replays_exactly() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let fixture = expired_succession_fixture(
        &database,
        "22222222222222222222222222222222",
        "33333333333333333333333333333333",
    )
    .await;
    let before = pending_drain_execution_state(&database.owner_pool).await;
    let before_fence = before.0["last_fencing_token"].as_i64().unwrap();
    let before_revision = before.1["intent_revision"].as_i64().unwrap();
    let before_journal_count = before.4;

    let applied = committed_pending_drain_succession(&database.executor_pool, &fixture)
        .await
        .unwrap();
    assert_eq!(applied.journal_outcome_name, "applied");
    assert_eq!(
        applied.terminal_outcome_name,
        "route_absent_acknowledged"
    );
    assert_eq!(applied.action_authority_revision, 2);
    assert_eq!(applied.selection_authority_revision, 1);
    assert_eq!(applied.minimum_database_now, fixture.minimum_database_now);
    assert!(applied.recorded_at >= fixture.minimum_database_now);
    assert!(applied.database_now >= applied.recorded_at);
    assert_eq!(applied.terminal_digest.len(), 64);
    assert!(!applied.terminal_projection_bytes.is_empty());

    let after = pending_drain_execution_state(&database.owner_pool).await;
    assert_eq!(after.0["last_fencing_token"], json!(before_fence + 1));
    assert_eq!(after.1["intent_revision"], json!(before_revision + 1));
    assert_eq!(
        after.1["intent_state"],
        json!("route_absent_acknowledged")
    );
    assert_eq!(after.2, before.2);
    assert_eq!(after.3, before.3);
    assert_eq!(after.4, before_journal_count + 1);
    let Json(successor_state) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.convert_from(canonical_state_bytes, 'UTF8')::JSONB \
         FROM public.runtime_drain_intents_v2 \
         WHERE drain_intent_id = $1",
    )
    .bind(&fixture.selected_drain_intent_id)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        successor_state["state"]["acknowledgement"]["claim"]["claim_revision"],
        json!(fixture.predecessor_claim_revision + 1)
    );
    assert_eq!(
        successor_state["state"]["acknowledgement"]["claim"]["controller_fencing_token"],
        json!(before_fence + 1)
    );
    assert_eq!(
        successor_state["state"]["acknowledgement"]["claim"]["process_instance_id"],
        json!(fixture.owner.1)
    );
    assert_ne!(
        successor_state["state"]["acknowledgement"]["claim"]["process_instance_id"],
        json!(fixture.predecessor_process_instance_id)
    );
    assert!(fixture.owner.2 > fixture.predecessor_lease_epoch);
    assert!(
        fixture.minimum_database_now >= fixture.predecessor_claim_expires_at
    );

    let replayed = committed_pending_drain_succession(&database.executor_pool, &fixture)
        .await
        .unwrap();
    assert_eq!(replayed.journal_outcome_name, "replayed");
    assert_eq!(
        replayed.terminal_projection_bytes,
        applied.terminal_projection_bytes
    );
    assert_eq!(replayed.terminal_digest, applied.terminal_digest);
    assert_eq!(replayed.minimum_database_now, applied.minimum_database_now);
    assert_eq!(replayed.recorded_at, applied.recorded_at);
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_concurrent_action_has_one_apply_and_one_replay() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let fixture = expired_succession_fixture(
        &database,
        "88888888888888888888888888888888",
        "99999999999999999999999999999999",
    )
    .await;
    let before = pending_drain_execution_state(&database.owner_pool).await;
    let (first, second) = tokio::join!(
        committed_pending_drain_succession(&database.executor_pool, &fixture),
        committed_pending_drain_succession(&database.executor_pool, &fixture),
    );
    let mut applied = Vec::new();
    let mut replayed = Vec::new();
    let mut serialization_failure = false;
    for outcome in [first, second] {
        match outcome {
            Ok(row) if row.journal_outcome_name == "applied" => applied.push(row),
            Ok(row) if row.journal_outcome_name == "replayed" => replayed.push(row),
            Ok(row) => panic!(
                "unexpected pending drain succession race outcome {}",
                row.journal_outcome_name
            ),
            Err(error) => {
                assert_sqlstate(&error, "40001");
                serialization_failure = true;
            }
        }
    }
    assert_eq!(applied.len(), 1);
    if serialization_failure {
        replayed.push(
            committed_pending_drain_succession(&database.executor_pool, &fixture)
                .await
                .unwrap(),
        );
    }
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].journal_outcome_name, "replayed");
    assert_eq!(
        replayed[0].terminal_projection_bytes,
        applied[0].terminal_projection_bytes
    );
    assert_eq!(replayed[0].terminal_digest, applied[0].terminal_digest);
    let after = pending_drain_execution_state(&database.owner_pool).await;
    assert_eq!(
        after.0["last_fencing_token"],
        json!(before.0["last_fencing_token"].as_i64().unwrap() + 1)
    );
    assert_eq!(
        after.1["intent_revision"],
        json!(before.1["intent_revision"].as_i64().unwrap() + 1)
    );
    assert_eq!(after.4, before.4 + 1);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_rollback_and_tamper_leave_no_writes() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let fixture = expired_succession_fixture(
        &database,
        "44444444444444444444444444444444",
        "55555555555555555555555555555555",
    )
    .await;
    let before = pending_drain_execution_state(&database.owner_pool).await;

    let mut tampered = fixture.clone();
    tampered.predecessor_claim_terminal_digest = "9".repeat(64);
    let error = committed_pending_drain_succession(&database.executor_pool, &tampered)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX003");
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        before
    );

    let mut wrong_source_digest = fixture.clone();
    wrong_source_digest.selected_source_state_digest = "8".repeat(64);
    let source_digest_error =
        committed_pending_drain_succession(&database.executor_pool, &wrong_source_digest)
            .await
            .unwrap_err();
    assert_sqlstate(&source_digest_error, "RX001");
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        before
    );

    let mut wrong_source_revision = fixture.clone();
    wrong_source_revision.selected_source_intent_revision += 1;
    let source_revision_error =
        committed_pending_drain_succession(&database.executor_pool, &wrong_source_revision)
            .await
            .unwrap_err();
    assert_sqlstate(&source_revision_error, "RX001");
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        before
    );

    let mut wrong_seal = fixture.clone();
    wrong_seal.seal_key = vec![0x42; 16];
    let seal_error = committed_pending_drain_succession(&database.executor_pool, &wrong_seal)
        .await
        .unwrap_err();
    assert_sqlstate(&seal_error, "RX002");
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        before
    );

    let mut transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut transaction).await.unwrap();
    let applied = call_pending_drain_succession(&mut *transaction, &fixture)
        .await
        .unwrap();
    assert_eq!(applied.journal_outcome_name, "applied");
    transaction.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        before
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_rejects_corrupt_durable_evidence_without_writes() {
    {
        let server = PostgresTestServer::start();
        let database = isolated_database(server.connect_options()).await;
        let predecessor_recovery_id = "abababababababababababababababab";
        let mut fixture = expired_succession_fixture(
            &database,
            predecessor_recovery_id,
            "bcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbc",
        )
        .await;
        fixture.predecessor_claim_terminal_digest =
            corrupt_predecessor_journal_identity(
                &database.owner_pool,
                predecessor_recovery_id,
            )
            .await;
        let before = pending_drain_execution_state(&database.owner_pool).await;
        let error = committed_pending_drain_succession(&database.executor_pool, &fixture)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "RX003");
        assert_eq!(
            pending_drain_execution_state(&database.owner_pool).await,
            before
        );
        cleanup(database).await;
        drop(server);
    }
    {
        let server = PostgresTestServer::start();
        let database = isolated_database(server.connect_options()).await;
        let fixture = expired_succession_fixture(
            &database,
            "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
            "dededededededededededededededede",
        )
        .await;
        corrupt_predecessor_deployment_fence(&database.owner_pool).await;
        let before = pending_drain_execution_state(&database.owner_pool).await;
        let error = committed_pending_drain_succession(&database.executor_pool, &fixture)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "RX003");
        assert_eq!(
            pending_drain_execution_state(&database.owner_pool).await,
            before
        );
        cleanup(database).await;
        drop(server);
    }
    {
        let server = PostgresTestServer::start();
        let database = isolated_database(server.connect_options()).await;
        let fixture = expired_succession_fixture(
            &database,
            "efefefefefefefefefefefefefefefef",
            "acacacacacacacacacacacacacacacac",
        )
        .await;
        seed_committed_succession_certification(&database.owner_pool).await;
        let before = pending_drain_execution_state(&database.owner_pool).await;
        let error = committed_pending_drain_succession(&database.executor_pool, &fixture)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "RX003");
        assert_eq!(
            pending_drain_execution_state(&database.owner_pool).await,
            before
        );
        cleanup(database).await;
        drop(server);
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_rejects_owner_renewal_without_writes() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let fixture = expired_succession_fixture(
        &database,
        "66666666666666666666666666666666",
        "77777777777777777777777777777777",
    )
    .await;
    renew_pending_drain_owner(&database.executor_pool, &fixture.owner).await;
    let before = pending_drain_execution_state(&database.owner_pool).await;
    let error = committed_pending_drain_succession(&database.executor_pool, &fixture)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX001");
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        before
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_capabilities_are_executor_only() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;
    for identity in [
        STARTUP_PENDING_DRAIN_SELECTOR_V3,
        STARTUP_PENDING_DRAIN_SUCCESSION_V3,
    ] {
        assert!(
            sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege(\
                    current_user, $1, 'EXECUTE'\
                 )",
            )
            .bind(identity)
            .fetch_one(&database.executor_pool)
            .await
            .unwrap()
        );
        let public_execute = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(\
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner)\
             )) AS privilege \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
                AND privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE'",
        )
        .bind(identity)
        .fetch_one(&database.owner_pool)
        .await
        .unwrap();
        assert_eq!(public_execute, 0);
    }
    let private_denied = sqlx::query(
        "SELECT starring_runtime_private_v2.\
            starring_runtime_pending_drain_predecessor_exact_v3(\
                NULL::public.runtime_drain_intents_v2, \
                NULL::public.runtime_startup_recovery_actions_v2\
            )",
    )
    .execute(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&private_denied, "42501");
    let relation_denied =
        sqlx::query("SELECT * FROM public.runtime_startup_recovery_actions_v2")
            .execute(&database.executor_pool)
            .await
            .unwrap_err();
    assert_sqlstate(&relation_denied, "42501");
    assert_cross_runtime_readiness(&mut database).await;
    cleanup(database).await;
    drop(server);
}
