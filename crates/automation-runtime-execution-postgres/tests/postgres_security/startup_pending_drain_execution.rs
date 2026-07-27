const STARTUP_PENDING_DRAIN_EXECUTION: &str = "SELECT \
    journal_outcome_name, terminal_outcome_name, action_authority_revision, \
    selection_authority_revision, database_now, minimum_database_now, recorded_at, \
    terminal_projection_bytes, terminal_digest \
 FROM public.starring_runtime_startup_recovery_execute_pending_drain_v2(\
    $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,\
    $17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,\
    $33,$34,$35,$36,$37,$38,$39,$40,$41,$42,$43,$44,$45,$46,$47,$48\
 )";
const STARTUP_PENDING_DRAIN_NONE: &str = "SELECT \
    journal_outcome_name, terminal_outcome_name, action_authority_revision, \
    selection_authority_revision, database_now, minimum_database_now, recorded_at, \
    terminal_projection_bytes, terminal_digest \
 FROM public.starring_runtime_startup_recovery_record_pending_drain_none_v2(\
    $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,\
    $13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24\
 )";

#[derive(Clone)]
struct PendingDrainExecutionFixture {
    recovery_id: String,
    owner: StartupObservationOwnerTuple,
    minimum_database_now: DateTime<Utc>,
    selected_drain_intent_id: String,
    selected_source_intent_revision: i64,
    selected_source_state_digest: String,
    seal_key: Vec<u8>,
}

#[derive(Debug, sqlx::FromRow)]
struct PendingDrainExecutionRow {
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

#[derive(Debug, sqlx::FromRow)]
struct PendingDrainSelectionRow {
    selection_outcome_name: String,
    observed_database_now: DateTime<Utc>,
    observed_owner_expires_at: DateTime<Utc>,
    selected_drain_intent_id: Option<String>,
    selected_source_intent_revision: Option<i64>,
    selected_source_state_digest: Option<String>,
    selected_slot_guild_id: Option<String>,
    selected_slot_ruleset_key: Option<String>,
    selected_target_version: Option<i64>,
    selected_target_content_hash: Option<String>,
    selected_target_binding_revision: Option<i64>,
    selected_target_binding_fingerprint: Option<String>,
}

fn decode_pending_drain_intent_id(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()
        })
        .collect()
}

async fn seed_pending_drain_execution_candidate(
    database: &IsolatedDatabase,
) -> automation_runtime_controller::RuntimeCanonicalProductDrainV2 {
    seed_claimable_deployment(&database.owner_pool).await;
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET snapshot = pg_catalog.jsonb_set(\
                snapshot, '{last_fencing_token}', '1'::JSONB, FALSE\
             ), \
             last_fencing_token = 1, \
             last_controller_id = 'pending-drain-baseline-controller', \
             convergence_attempt_no = 1 \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let Json(snapshot) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT snapshot FROM public.runtime_deployments \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(snapshot).unwrap();
    let canonical = canonical_product_drain(&snapshot);
    seed_canonical_product_drain(&database.owner_pool, &canonical).await;
    canonical
}

async fn select_pending_drain_execution(
    pool: &PgPool,
    owner: &StartupObservationOwnerTuple,
) -> PendingDrainSelectionRow {
    let minimum_database_now = database_now(pool).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let selected = sqlx::query_as::<_, PendingDrainSelectionRow>(
        "SELECT selection_outcome_name, observed_database_now, \
            observed_owner_expires_at, selected_drain_intent_id, \
            selected_source_intent_revision, selected_source_state_digest, \
            selected_slot_guild_id, selected_slot_ruleset_key, \
            selected_target_version, selected_target_content_hash, \
            selected_target_binding_revision, selected_target_binding_fingerprint \
         FROM public.starring_runtime_startup_recovery_select_pending_drain_v2(\
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
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    selected
}

async fn pending_drain_execution_fixture(
    database: &IsolatedDatabase,
    recovery_id: &str,
) -> PendingDrainExecutionFixture {
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let selected = select_pending_drain_execution(&database.executor_pool, &owner).await;
    assert_eq!(selected.selection_outcome_name, "candidate");
    assert_eq!(selected.observed_owner_expires_at, owner.5);
    assert_eq!(selected.selected_slot_guild_id, Some(GUILD.to_string()));
    assert_eq!(selected.selected_slot_ruleset_key.as_deref(), Some(RULESET));
    assert_eq!(selected.selected_target_version, Some(1));
    assert_eq!(
        selected.selected_target_content_hash.as_deref(),
        Some(CONTENT_HASH)
    );
    assert_eq!(selected.selected_target_binding_revision, Some(1));
    assert_eq!(
        selected.selected_target_binding_fingerprint.as_deref(),
        Some(BINDING_FINGERPRINT)
    );
    let selected_drain_intent_id = selected.selected_drain_intent_id.unwrap();
    let seal_key = decode_pending_drain_intent_id(&selected_drain_intent_id);
    PendingDrainExecutionFixture {
        recovery_id: recovery_id.to_string(),
        owner,
        minimum_database_now: selected.observed_database_now,
        selected_drain_intent_id,
        selected_source_intent_revision: selected.selected_source_intent_revision.unwrap(),
        selected_source_state_digest: selected.selected_source_state_digest.unwrap(),
        seal_key,
    }
}

async fn call_pending_drain_none(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    recovery_id: &str,
    owner: &StartupObservationOwnerTuple,
    minimum_database_now: DateTime<Utc>,
) -> Result<PendingDrainExecutionRow, sqlx::Error> {
    sqlx::query_as::<_, PendingDrainExecutionRow>(STARTUP_PENDING_DRAIN_NONE)
        .bind(recovery_id)
        .bind(1_i64)
        .bind(2_i64)
        .bind(2_i64)
        .bind(1_i64)
        .bind(&owner.0)
        .bind(&owner.1)
        .bind(owner.2)
        .bind(&owner.3)
        .bind(owner.4)
        .bind(owner.5)
        .bind(minimum_database_now)
        .bind(&owner.1)
        .bind(1_i64)
        .bind(1_i64)
        .bind("ready")
        .bind(1_i64)
        .bind(2_i64)
        .bind(1_i64)
        .bind(0_i64)
        .bind(&owner.1)
        .bind(1_i64)
        .bind(0_i64)
        .bind(0_i64)
        .fetch_one(executor)
        .await
}

async fn renew_pending_drain_owner(pool: &PgPool, owner: &StartupObservationOwnerTuple) {
    let renewed = sqlx::query_as::<_, (String, i64, DateTime<Utc>)>(
        "SELECT outcome_name, owner_revision, expires_at \
         FROM public.starring_runtime_gateway_owner_renew_v1(\
            $1,$2,$3,$4,$5,300000\
         )",
    )
    .bind(&owner.0)
    .bind(&owner.1)
    .bind(owner.2)
    .bind(&owner.3)
    .bind(owner.4)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(renewed.0, "renewed");
    assert_eq!(renewed.1, owner.4 + 1);
    assert_ne!(renewed.2, owner.5);
}

async fn call_pending_drain_execution(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    fixture: &PendingDrainExecutionFixture,
    stage: &str,
    prior_claim_terminal_digest: &str,
) -> Result<PendingDrainExecutionRow, sqlx::Error> {
    sqlx::query_as::<_, PendingDrainExecutionRow>(STARTUP_PENDING_DRAIN_EXECUTION)
        .bind(&fixture.recovery_id)
        .bind(1_i64)
        .bind(2_i64)
        .bind(2_i64)
        .bind(1_i64)
        .bind(3_i64)
        .bind(2_i64)
        .bind(stage)
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
        .bind(prior_claim_terminal_digest)
        .fetch_one(executor)
        .await
}

async fn prepare_pending_drain_write(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut **transaction)
        .await?;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn pending_drain_history_gucs(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
) -> Vec<String> {
    sqlx::query_scalar::<_, Vec<String>>(
        "SELECT ARRAY[\
            COALESCE(pg_catalog.current_setting(\
                'starring.runtime_pending_drain_deployment_action_v2', TRUE\
            ), ''), \
            COALESCE(pg_catalog.current_setting(\
                'starring.runtime_pending_drain_deployment_id_v2', TRUE\
            ), ''), \
            COALESCE(pg_catalog.current_setting(\
                'starring.runtime_pending_drain_source_fence_v2', TRUE\
            ), ''), \
            COALESCE(pg_catalog.current_setting(\
                'starring.runtime_pending_drain_successor_fence_v2', TRUE\
            ), ''), \
            COALESCE(pg_catalog.current_setting(\
                'starring.runtime_pending_drain_successor_controller_v2', TRUE\
            ), '')\
        ]::TEXT[]",
    )
    .fetch_one(executor)
    .await
    .unwrap()
}

async fn committed_pending_drain_claim(
    pool: &PgPool,
    fixture: &PendingDrainExecutionFixture,
) -> Result<PendingDrainExecutionRow, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    prepare_pending_drain_write(&mut transaction).await?;
    match call_pending_drain_execution(&mut *transaction, fixture, "claim", "").await {
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

async fn pending_drain_execution_state(pool: &PgPool) -> (Value, Value, Value, Value, i64) {
    let Json(deployment) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(deployment) \
         FROM public.runtime_deployments AS deployment \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap();
    let Json(drain) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(drain) \
         FROM public.runtime_drain_intents_v2 AS drain",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let Json(product) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(product) \
         FROM public.runtime_product_operations_v2 AS product",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let Json(slot) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(slot) \
         FROM public.runtime_slot_writer_fences_v2 AS slot \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(pool)
    .await
    .unwrap();
    let journal_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_startup_recovery_actions_v2",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    (deployment, drain, product, slot, journal_count)
}

fn deployment_without_pending_drain_history(mut deployment: Value) -> Value {
    let object = deployment.as_object_mut().unwrap();
    object.remove("snapshot");
    object.remove("last_fencing_token");
    object.remove("last_controller_id");
    deployment
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_selector_none_and_record_none_are_replayable() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let selected = select_pending_drain_execution(&database.executor_pool, &owner).await;
    assert_eq!(selected.selection_outcome_name, "no_candidate");
    assert_eq!(selected.observed_owner_expires_at, owner.5);
    assert!(selected.selected_drain_intent_id.is_none());
    assert!(selected.selected_source_intent_revision.is_none());
    assert!(selected.selected_source_state_digest.is_none());
    assert!(selected.selected_slot_guild_id.is_none());
    assert!(selected.selected_slot_ruleset_key.is_none());
    assert!(selected.selected_target_version.is_none());
    assert!(selected.selected_target_content_hash.is_none());
    assert!(selected.selected_target_binding_revision.is_none());
    assert!(selected.selected_target_binding_fingerprint.is_none());

    let recovery_id = "01010101010101010101010101010101";
    let mut applied_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut applied_transaction)
        .await
        .unwrap();
    let applied = call_pending_drain_none(
        &mut *applied_transaction,
        recovery_id,
        &owner,
        selected.observed_database_now,
    )
    .await
    .unwrap();
    applied_transaction.commit().await.unwrap();
    assert_eq!(applied.journal_outcome_name, "applied");
    assert_eq!(applied.terminal_outcome_name, "no_candidate");
    assert_eq!(applied.action_authority_revision, 2);
    assert_eq!(applied.selection_authority_revision, 1);

    let mut replay_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut replay_transaction)
        .await
        .unwrap();
    let replayed = call_pending_drain_none(
        &mut *replay_transaction,
        recovery_id,
        &owner,
        selected.observed_database_now,
    )
    .await
    .unwrap();
    replay_transaction.commit().await.unwrap();
    assert_eq!(replayed.journal_outcome_name, "replayed");
    assert_eq!(
        replayed.terminal_projection_bytes,
        applied.terminal_projection_bytes
    );
    assert_eq!(replayed.terminal_digest, applied.terminal_digest);
    let journal_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_id = $1",
    )
    .bind(recovery_id)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(journal_count, 1);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_record_none_rejects_late_candidate_without_journal() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let selected = select_pending_drain_execution(&database.executor_pool, &owner).await;
    assert_eq!(selected.selection_outcome_name, "no_candidate");
    seed_pending_drain_execution_candidate(&database).await;
    let mut transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut transaction).await.unwrap();
    let error = call_pending_drain_none(
        &mut *transaction,
        "02020202020202020202020202020202",
        &owner,
        selected.observed_database_now,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX003");
    transaction.rollback().await.unwrap();
    let journal_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_startup_recovery_actions_v2",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(journal_count, 0);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_claim_rejects_owner_renewal_without_writes() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
    let fixture =
        pending_drain_execution_fixture(&database, "03030303030303030303030303030303").await;
    let before = pending_drain_execution_state(&database.owner_pool).await;
    renew_pending_drain_owner(&database.executor_pool, &fixture.owner).await;
    let mut transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut transaction).await.unwrap();
    let error = call_pending_drain_execution(&mut *transaction, &fixture, "claim", "")
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX001");
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
async fn startup_pending_drain_ack_rejects_owner_renewal_without_writes() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
    let fixture =
        pending_drain_execution_fixture(&database, "04040404040404040404040404040404").await;
    let mut claim_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut claim_transaction)
        .await
        .unwrap();
    let claim = call_pending_drain_execution(&mut *claim_transaction, &fixture, "claim", "")
        .await
        .unwrap();
    assert_eq!(
        pending_drain_history_gucs(&mut *claim_transaction).await,
        vec![String::new(); 5]
    );
    claim_transaction.commit().await.unwrap();
    let mut acknowledgement_fixture = fixture.clone();
    acknowledgement_fixture.minimum_database_now = database_now(&database.owner_pool).await;
    assert!(acknowledgement_fixture.minimum_database_now > claim.recorded_at);
    let after_claim = pending_drain_execution_state(&database.owner_pool).await;
    renew_pending_drain_owner(&database.executor_pool, &fixture.owner).await;
    let mut acknowledgement = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut acknowledgement)
        .await
        .unwrap();
    let error = call_pending_drain_execution(
        &mut *acknowledgement,
        &acknowledgement_fixture,
        "acknowledge",
        &claim.terminal_digest,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX001");
    acknowledgement.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_claim
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_claim_ack_replay_preserves_compound_cas() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let canonical = seed_pending_drain_execution_candidate(&database).await;
    let fixture =
        pending_drain_execution_fixture(&database, "abcabcabcabcabcabcabcabcabcabcab").await;
    assert_eq!(
        fixture.selected_drain_intent_id,
        canonical.drain_preimage().key.intent_id.as_str()
    );
    let before = pending_drain_execution_state(&database.owner_pool).await;

    let mut wrong_seal_fixture = fixture.clone();
    wrong_seal_fixture.seal_key = vec![0x42; 16];
    let mut wrong_seal_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut wrong_seal_transaction)
        .await
        .unwrap();
    let wrong_seal = call_pending_drain_execution(
        &mut *wrong_seal_transaction,
        &wrong_seal_fixture,
        "claim",
        "",
    )
    .await
    .unwrap_err();
    assert_sqlstate(&wrong_seal, "RX002");
    wrong_seal_transaction.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        before
    );

    let mut claim_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut claim_transaction)
        .await
        .unwrap();
    let claim = call_pending_drain_execution(&mut *claim_transaction, &fixture, "claim", "")
        .await
        .unwrap();
    claim_transaction.commit().await.unwrap();
    assert_eq!(claim.journal_outcome_name, "applied");
    assert_eq!(claim.terminal_outcome_name, "claimed");
    assert_eq!(claim.action_authority_revision, 2);
    assert_eq!(claim.selection_authority_revision, 1);
    assert_eq!(claim.minimum_database_now, fixture.minimum_database_now);
    assert!(claim.recorded_at >= claim.minimum_database_now);
    assert!(claim.database_now >= claim.recorded_at);
    assert!(!claim.terminal_projection_bytes.is_empty());
    assert_eq!(claim.terminal_digest.len(), 64);

    let after_claim = pending_drain_execution_state(&database.owner_pool).await;
    assert_eq!(
        deployment_without_pending_drain_history(before.0.clone()),
        deployment_without_pending_drain_history(after_claim.0.clone())
    );
    assert_eq!(after_claim.0["last_fencing_token"], json!(2));
    assert_eq!(
        after_claim.0["last_controller_id"],
        json!("recovery:abcabcabcabcabcabcabcabcabcabcab:2")
    );
    assert_eq!(after_claim.0["snapshot"]["last_fencing_token"], json!(2));
    assert_eq!(after_claim.1["intent_revision"], json!(2));
    assert_eq!(after_claim.1["intent_state"], json!("pending"));
    let claim_state_kind = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.convert_from(\
            canonical_state_bytes, 'UTF8'\
         )::JSONB #>> '{state,kind}' \
         FROM public.runtime_drain_intents_v2",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(claim_state_kind, "pending_claimed");
    assert_eq!(after_claim.2, before.2);
    assert_eq!(after_claim.3, before.3);
    assert_eq!(after_claim.4, 1);

    let mut replay_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut replay_transaction)
        .await
        .unwrap();
    let replay = call_pending_drain_execution(&mut *replay_transaction, &fixture, "claim", "")
        .await
        .unwrap();
    replay_transaction.commit().await.unwrap();
    assert_eq!(replay.journal_outcome_name, "replayed");
    assert_eq!(replay.terminal_digest, claim.terminal_digest);
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_claim
    );

    let mut wrong_candidate_claim_fixture = fixture.clone();
    wrong_candidate_claim_fixture.selected_drain_intent_id =
        "01010101010101010101010101010101".to_string();
    wrong_candidate_claim_fixture.seal_key =
        decode_pending_drain_intent_id(&wrong_candidate_claim_fixture.selected_drain_intent_id);
    let mut wrong_candidate_claim = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut wrong_candidate_claim)
        .await
        .unwrap();
    let wrong_candidate_claim_error = call_pending_drain_execution(
        &mut *wrong_candidate_claim,
        &wrong_candidate_claim_fixture,
        "claim",
        "",
    )
    .await
    .unwrap_err();
    assert_sqlstate(&wrong_candidate_claim_error, "RX004");
    wrong_candidate_claim.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_claim
    );

    let mut wrong_source_claim_fixture = fixture.clone();
    wrong_source_claim_fixture.selected_source_state_digest = "9".repeat(64);
    let mut wrong_source_claim = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut wrong_source_claim)
        .await
        .unwrap();
    let wrong_source_claim_error = call_pending_drain_execution(
        &mut *wrong_source_claim,
        &wrong_source_claim_fixture,
        "claim",
        "",
    )
    .await
    .unwrap_err();
    assert_sqlstate(&wrong_source_claim_error, "RX004");
    wrong_source_claim.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_claim
    );

    let mut acknowledgement_fixture = fixture.clone();
    acknowledgement_fixture.minimum_database_now = database_now(&database.owner_pool).await;
    assert!(acknowledgement_fixture.minimum_database_now > claim.recorded_at);

    let mut wrong_prior_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut wrong_prior_transaction)
        .await
        .unwrap();
    let wrong_prior = call_pending_drain_execution(
        &mut *wrong_prior_transaction,
        &acknowledgement_fixture,
        "acknowledge",
        &"0".repeat(64),
    )
    .await
    .unwrap_err();
    assert_sqlstate(&wrong_prior, "RX003");
    wrong_prior_transaction.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_claim
    );

    let mut wrong_fixture = acknowledgement_fixture.clone();
    wrong_fixture.seal_key = vec![0x42; 16];
    let mut wrong_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut wrong_transaction)
        .await
        .unwrap();
    let wrong = call_pending_drain_execution(
        &mut *wrong_transaction,
        &wrong_fixture,
        "acknowledge",
        &claim.terminal_digest,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&wrong, "RX002");
    wrong_transaction.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_claim
    );

    let mut acknowledgement_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut acknowledgement_transaction)
        .await
        .unwrap();
    let acknowledgement = call_pending_drain_execution(
        &mut *acknowledgement_transaction,
        &acknowledgement_fixture,
        "acknowledge",
        &claim.terminal_digest,
    )
    .await
    .unwrap();
    acknowledgement_transaction.commit().await.unwrap();
    assert_eq!(acknowledgement.journal_outcome_name, "applied");
    assert_eq!(
        acknowledgement.terminal_outcome_name,
        "route_absent_acknowledged"
    );
    assert_eq!(acknowledgement.action_authority_revision, 3);
    assert_eq!(acknowledgement.selection_authority_revision, 2);
    assert_eq!(
        acknowledgement.minimum_database_now,
        acknowledgement_fixture.minimum_database_now
    );
    assert!(acknowledgement.recorded_at >= claim.recorded_at);
    assert!(acknowledgement.database_now >= acknowledgement.recorded_at);
    let after_acknowledgement = pending_drain_execution_state(&database.owner_pool).await;
    assert_eq!(after_acknowledgement.0, after_claim.0);
    assert_eq!(after_acknowledgement.1["intent_revision"], json!(3));
    assert_eq!(
        after_acknowledgement.1["intent_state"],
        json!("route_absent_acknowledged")
    );
    assert_eq!(after_acknowledgement.2, before.2);
    assert_eq!(after_acknowledgement.3, before.3);
    assert_eq!(after_acknowledgement.4, 2);

    let mut later_replay_fixture = acknowledgement_fixture.clone();
    later_replay_fixture.minimum_database_now = database_now(&database.owner_pool).await;
    assert!(later_replay_fixture.minimum_database_now > acknowledgement.recorded_at);
    let mut acknowledgement_replay = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut acknowledgement_replay)
        .await
        .unwrap();
    let replayed_acknowledgement = call_pending_drain_execution(
        &mut *acknowledgement_replay,
        &later_replay_fixture,
        "acknowledge",
        &claim.terminal_digest,
    )
    .await
    .unwrap();
    acknowledgement_replay.commit().await.unwrap();
    assert_eq!(replayed_acknowledgement.journal_outcome_name, "replayed");
    assert_eq!(
        replayed_acknowledgement.minimum_database_now,
        acknowledgement.minimum_database_now
    );
    assert!(
        replayed_acknowledgement.minimum_database_now < later_replay_fixture.minimum_database_now
    );
    assert!(replayed_acknowledgement.database_now >= replayed_acknowledgement.recorded_at);
    assert_eq!(
        replayed_acknowledgement.terminal_digest,
        acknowledgement.terminal_digest
    );
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_acknowledgement
    );

    let mut wrong_candidate_acknowledgement_fixture =
        later_replay_fixture.clone();
    wrong_candidate_acknowledgement_fixture.selected_drain_intent_id =
        "01010101010101010101010101010101".to_string();
    wrong_candidate_acknowledgement_fixture.seal_key = decode_pending_drain_intent_id(
        &wrong_candidate_acknowledgement_fixture.selected_drain_intent_id,
    );
    let mut wrong_candidate_acknowledgement =
        database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut wrong_candidate_acknowledgement)
        .await
        .unwrap();
    let wrong_candidate_acknowledgement_error = call_pending_drain_execution(
        &mut *wrong_candidate_acknowledgement,
        &wrong_candidate_acknowledgement_fixture,
        "acknowledge",
        &claim.terminal_digest,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&wrong_candidate_acknowledgement_error, "RX004");
    wrong_candidate_acknowledgement.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_acknowledgement
    );

    let mut wrong_source_acknowledgement_fixture =
        later_replay_fixture.clone();
    wrong_source_acknowledgement_fixture.selected_source_state_digest =
        "9".repeat(64);
    let mut wrong_source_acknowledgement =
        database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut wrong_source_acknowledgement)
        .await
        .unwrap();
    let wrong_source_acknowledgement_error = call_pending_drain_execution(
        &mut *wrong_source_acknowledgement,
        &wrong_source_acknowledgement_fixture,
        "acknowledge",
        &claim.terminal_digest,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&wrong_source_acknowledgement_error, "RX004");
    wrong_source_acknowledgement.rollback().await.unwrap();
    assert_eq!(
        pending_drain_execution_state(&database.owner_pool).await,
        after_acknowledgement
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_claim_rollback_leaves_no_partial_history() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
    let fixture =
        pending_drain_execution_fixture(&database, "defdefdefdefdefdefdefdefdefdefde").await;
    let before = pending_drain_execution_state(&database.owner_pool).await;
    let mut transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut transaction).await.unwrap();
    let applied = call_pending_drain_execution(&mut *transaction, &fixture, "claim", "")
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
async fn startup_pending_drain_history_guard_rejects_forged_gucs_and_extra_drift() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
    let before = pending_drain_execution_state(&database.owner_pool).await;
    let successor_controller = "recovery:06060606060606060606060606060606:2";
    let cases = [
        (
            "missing",
            false,
            "advance_history",
            DEPLOYMENT,
            1_i64,
            2_i64,
            successor_controller,
            false,
            false,
        ),
        (
            "wrong_action",
            true,
            "forged_history",
            DEPLOYMENT,
            1_i64,
            2_i64,
            successor_controller,
            false,
            false,
        ),
        (
            "wrong_deployment",
            true,
            "advance_history",
            "wrong-deployment",
            1_i64,
            2_i64,
            successor_controller,
            false,
            false,
        ),
        (
            "wrong_source",
            true,
            "advance_history",
            DEPLOYMENT,
            2_i64,
            2_i64,
            successor_controller,
            false,
            false,
        ),
        (
            "wrong_successor",
            true,
            "advance_history",
            DEPLOYMENT,
            1_i64,
            3_i64,
            successor_controller,
            false,
            false,
        ),
        (
            "wrong_controller",
            true,
            "advance_history",
            DEPLOYMENT,
            1_i64,
            2_i64,
            "forged-controller",
            false,
            false,
        ),
        (
            "extra_column",
            true,
            "advance_history",
            DEPLOYMENT,
            1_i64,
            2_i64,
            successor_controller,
            true,
            false,
        ),
        (
            "snapshot_drift",
            true,
            "advance_history",
            DEPLOYMENT,
            1_i64,
            2_i64,
            successor_controller,
            false,
            true,
        ),
    ];
    for (
        name,
        set_gucs,
        action,
        deployment_id,
        source_fence,
        successor_fence,
        configured_controller,
        extra_column,
        snapshot_drift,
    ) in cases
    {
        let mut transaction = database.owner_pool.begin().await.unwrap();
        sqlx::query("SET LOCAL statement_timeout = '5s'")
            .execute(&mut *transaction)
            .await
            .unwrap();
        if set_gucs {
            sqlx::query(
                "SELECT \
                    pg_catalog.set_config(\
                        'starring.runtime_pending_drain_deployment_action_v2', $1, TRUE\
                    ), \
                    pg_catalog.set_config(\
                        'starring.runtime_pending_drain_deployment_id_v2', $2, TRUE\
                    ), \
                    pg_catalog.set_config(\
                        'starring.runtime_pending_drain_source_fence_v2', $3, TRUE\
                    ), \
                    pg_catalog.set_config(\
                        'starring.runtime_pending_drain_successor_fence_v2', $4, TRUE\
                    ), \
                    pg_catalog.set_config(\
                        'starring.runtime_pending_drain_successor_controller_v2', $5, TRUE\
                    )",
            )
            .bind(action)
            .bind(deployment_id)
            .bind(source_fence.to_string())
            .bind(successor_fence.to_string())
            .bind(configured_controller)
            .execute(&mut *transaction)
            .await
            .unwrap();
        }
        sqlx::query("SELECT public.starring_runtime_mutation_clock()")
            .execute(&mut *transaction)
            .await
            .unwrap();
        let error = sqlx::query(
            "UPDATE public.runtime_deployments \
             SET snapshot = CASE WHEN $3 THEN \
                    pg_catalog.jsonb_set(\
                        pg_catalog.jsonb_set(\
                            snapshot, '{last_fencing_token}', '2'::JSONB, FALSE\
                        ), \
                        '{projection_padding}', '\"forged\"'::JSONB, TRUE\
                    ) \
                    ELSE pg_catalog.jsonb_set(\
                        snapshot, '{last_fencing_token}', '2'::JSONB, FALSE\
                    ) \
                 END, \
                 last_fencing_token = 2, \
                 last_controller_id = $2, \
                 updated_at = CASE WHEN $4 \
                    THEN updated_at + INTERVAL '1 microsecond' \
                    ELSE updated_at \
                 END \
             WHERE deployment_id = $1",
        )
        .bind(DEPLOYMENT)
        .bind(successor_controller)
        .bind(snapshot_drift)
        .bind(extra_column)
        .execute(&mut *transaction)
        .await
        .unwrap_err();
        assert_sqlstate(&error, "23514");
        transaction.rollback().await.unwrap();
        assert_eq!(
            pending_drain_execution_state(&database.owner_pool).await,
            before,
            "{name}"
        );
    }
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_concurrent_claim_has_one_apply_and_exact_replay() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
    let fixture =
        pending_drain_execution_fixture(&database, "05050505050505050505050505050505").await;
    let (first, second) = tokio::join!(
        committed_pending_drain_claim(&database.executor_pool, &fixture),
        committed_pending_drain_claim(&database.executor_pool, &fixture),
    );
    let mut applied = Vec::new();
    let mut replayed = Vec::new();
    let mut serialization_failure = false;
    for outcome in [first, second] {
        match outcome {
            Ok(row) if row.journal_outcome_name == "applied" => applied.push(row),
            Ok(row) if row.journal_outcome_name == "replayed" => replayed.push(row),
            Ok(row) => panic!(
                "unexpected pending drain race outcome {}",
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
            committed_pending_drain_claim(&database.executor_pool, &fixture)
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
    let state = pending_drain_execution_state(&database.owner_pool).await;
    assert_eq!(state.0["last_fencing_token"], json!(2));
    assert_eq!(state.1["intent_revision"], json!(2));
    assert_eq!(state.4, 1);
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_capabilities_are_executor_only() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;
    for identity in [
        "public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)",
        "public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)",
        "public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)",
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
    }
    let denied = sqlx::query(
        "SELECT * FROM starring_runtime_private_v2.\
         starring_runtime_pending_drain_candidate_v2()",
    )
    .fetch_all(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&denied, "42501");
    let table_denied = sqlx::query("SELECT * FROM public.runtime_drain_intents_v2")
        .fetch_all(&database.executor_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&table_denied, "42501");
    assert_cross_runtime_readiness(&mut database).await;
    cleanup(database).await;
    drop(server);
}
