use std::time::{SystemTime, UNIX_EPOCH};

use automation_panel_installation::strict::{
    StrictPanelCleanupIntentV1, StrictPanelCleanupKindV1, StrictPanelInstallationStore,
    StrictPanelMessageRefV1, StrictPanelOperationJournal, StrictPanelOperationKeyV1,
    StrictPanelOperationStateV1, StrictPanelOperationV1, MAX_STRICT_PANEL_RECORDS_PER_SLOT,
};
use automation_panel_installation::{
    PanelInstallation, PanelInstallationKey, PanelInstallationStore,
};
use automation_panel_installation_postgres::{PostgresPanelInstallationStore, MIGRATOR};
use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use discord_model::{ChannelId, GuildId, MessageId};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::{Connection, PgPool};

const GUILD: GuildId = GuildId(9_000_105);

fn database_options() -> PgConnectOptions {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL must be set for ignored postgres tests");
    let options = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let database = options
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        database.starts_with("starring_")
            && database.split('_').any(|segment| segment == "test")
            && database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to create outside the strict Starring test database namespace"
    );
    options
}

struct TestDatabase {
    name: String,
    administrator: PgConnection,
    connect_options: PgConnectOptions,
    pool: PgPool,
}

async fn test_database(label: &str) -> TestDatabase {
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("starring_panel_test_{label}_{suffix}");
    assert!(name.len() <= 63);
    let base = database_options();
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .expect("connect administrator");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .expect("create isolated test database");
    let connect_options = base.database(&name);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(connect_options.clone())
        .await
        .expect("connect");
    if let Err(error) = MIGRATOR.run(&pool).await {
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
            .execute(&mut administrator)
            .await
            .expect("drop failed migration database");
        panic!("migration failed: {error}");
    }
    TestDatabase {
        name,
        administrator,
        connect_options,
        pool,
    }
}

async fn drop_test_database(database: TestDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .expect("drop isolated test database");
}

async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM public.strict_panel_operation_journal WHERE guild_id = $1")
        .bind(GUILD.to_string())
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.ruleset_panel_installations WHERE guild_id = $1")
        .bind(GUILD.to_string())
        .execute(pool)
        .await
        .unwrap();
}

fn key() -> PanelInstallationKey {
    PanelInstallationKey {
        guild_id: GUILD,
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        panel_key: "entry".to_string(),
    }
}

fn installation(version: u32, channel: u64, message: u64, hash: char) -> PanelInstallation {
    installation_for("entry", version, channel, message, hash)
}

fn installation_for(
    panel_key: &str,
    version: u32,
    channel: u64,
    message: u64,
    hash: char,
) -> PanelInstallation {
    PanelInstallation {
        guild_id: GUILD,
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        panel_key: panel_key.to_string(),
        installed_version: RuleSetVersionId::new(version).unwrap(),
        channel_id: ChannelId(channel),
        message_id: MessageId(message),
        spec_hash: hash.to_string().repeat(64),
    }
}

fn operation_key(panel_key: &str) -> StrictPanelOperationKeyV1 {
    StrictPanelOperationKeyV1 {
        guild_id: GUILD,
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        panel_key: panel_key.to_string(),
    }
}

fn cleanup_operation(
    panel_key: &str,
    channel_id: u64,
    message_id: u64,
    kind: StrictPanelCleanupKindV1,
) -> StrictPanelOperationV1 {
    StrictPanelOperationV1 {
        key: operation_key(panel_key),
        state: StrictPanelOperationStateV1::CleanupPending {
            intent: StrictPanelCleanupIntentV1 {
                message: StrictPanelMessageRefV1 {
                    channel_id: ChannelId(channel_id),
                    message_id: MessageId(message_id),
                },
                kind,
                remove_installation: matches!(kind, StrictPanelCleanupKindV1::Removed),
            },
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn upsert_conflict_updates_in_place() {
    let database = test_database("upsert").await;
    let pool = database.pool.clone();
    cleanup(&pool).await;
    let store = PostgresPanelInstallationStore::new(pool.clone());
    store.upsert(installation(1, 10, 100, 'a')).await.unwrap();
    store.upsert(installation(2, 20, 200, 'b')).await.unwrap();
    assert_eq!(
        store.get(&key()).await.unwrap(),
        Some(installation(2, 20, 200, 'b'))
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM public.ruleset_panel_installations WHERE guild_id = $1 AND ruleset_key = $2 AND panel_key = $3",
    )
    .bind(GUILD.to_string())
    .bind("studyroom")
    .bind("entry")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    cleanup(&pool).await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn reconnect_preserves_installation() {
    let database = test_database("installation_reconnect").await;
    cleanup(&database.pool).await;
    let store = PostgresPanelInstallationStore::new(database.pool.clone());
    store.upsert(installation(1, 10, 100, 'a')).await.unwrap();
    database.pool.close().await;
    let second_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(database.connect_options.clone())
        .await
        .unwrap();
    let store = PostgresPanelInstallationStore::new(second_pool.clone());
    assert_eq!(
        store.get(&key()).await.unwrap(),
        Some(installation(1, 10, 100, 'a'))
    );
    cleanup(&second_pool).await;
    second_pool.close().await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn strict_slot_list_is_ordered_bounded_and_remove_is_idempotent() {
    let database = test_database("strict_slot").await;
    let pool = database.pool.clone();
    cleanup(&pool).await;
    let store = PostgresPanelInstallationStore::new(pool.clone());
    store
        .upsert(installation_for("zeta", 1, 10, 100, 'a'))
        .await
        .unwrap();
    store
        .upsert(installation_for("alpha", 2, 20, 200, 'b'))
        .await
        .unwrap();
    let ruleset_key = RuleSetKey::parse("studyroom").unwrap();
    let listed = StrictPanelInstallationStore::list_slot(&store, GUILD, &ruleset_key)
        .await
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|installation| installation.panel_key.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "zeta"]
    );
    let alpha = PanelInstallationKey {
        guild_id: GUILD,
        ruleset_key: ruleset_key.clone(),
        panel_key: "alpha".to_string(),
    };
    StrictPanelInstallationStore::remove(&store, &alpha)
        .await
        .unwrap();
    StrictPanelInstallationStore::remove(&store, &alpha)
        .await
        .unwrap();
    let listed = StrictPanelInstallationStore::list_slot(&store, GUILD, &ruleset_key)
        .await
        .unwrap();
    assert_eq!(listed, vec![installation_for("zeta", 1, 10, 100, 'a')]);
    cleanup(&pool).await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn strict_journal_roundtrips_replaces_and_removes_idempotently() {
    let database = test_database("journal_roundtrip").await;
    let pool = database.pool.clone();
    cleanup(&pool).await;
    let store = PostgresPanelInstallationStore::new(pool.clone());
    let first = cleanup_operation("zeta", 10, 100, StrictPanelCleanupKindV1::Removed);
    let second = cleanup_operation("alpha", 20, 200, StrictPanelCleanupKindV1::Orphan);
    StrictPanelOperationJournal::put(&store, first.clone())
        .await
        .unwrap();
    StrictPanelOperationJournal::put(&store, second.clone())
        .await
        .unwrap();
    let ruleset_key = RuleSetKey::parse("studyroom").unwrap();
    assert_eq!(
        StrictPanelOperationJournal::list_slot(&store, GUILD, &ruleset_key)
            .await
            .unwrap(),
        vec![second.clone(), first.clone()]
    );
    let replaced = cleanup_operation("alpha", 30, 300, StrictPanelCleanupKindV1::PayloadReplaced);
    StrictPanelOperationJournal::put(&store, replaced.clone())
        .await
        .unwrap();
    let listed = StrictPanelOperationJournal::list_slot(&store, GUILD, &ruleset_key)
        .await
        .unwrap();
    assert_eq!(listed[0], replaced);
    StrictPanelOperationJournal::remove(&store, &operation_key("alpha"))
        .await
        .unwrap();
    StrictPanelOperationJournal::remove(&store, &operation_key("alpha"))
        .await
        .unwrap();
    assert_eq!(
        StrictPanelOperationJournal::list_slot(&store, GUILD, &ruleset_key)
            .await
            .unwrap(),
        vec![first]
    );
    cleanup(&pool).await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn reconnect_preserves_strict_journal() {
    let expected = cleanup_operation("entry", 10, 100, StrictPanelCleanupKindV1::Removed);
    let database = test_database("journal_reconnect").await;
    cleanup(&database.pool).await;
    let store = PostgresPanelInstallationStore::new(database.pool.clone());
    StrictPanelOperationJournal::put(&store, expected.clone())
        .await
        .unwrap();
    database.pool.close().await;
    let second_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(database.connect_options.clone())
        .await
        .unwrap();
    let store = PostgresPanelInstallationStore::new(second_pool.clone());
    assert_eq!(
        StrictPanelOperationJournal::list_slot(
            &store,
            GUILD,
            &RuleSetKey::parse("studyroom").unwrap(),
        )
        .await
        .unwrap(),
        vec![expected]
    );
    cleanup(&second_pool).await;
    second_pool.close().await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn database_rejects_mismatched_journal_identity() {
    let database = test_database("journal_mismatch").await;
    let pool = database.pool.clone();
    cleanup(&pool).await;
    let operation = cleanup_operation("actual", 10, 100, StrictPanelCleanupKindV1::Removed);
    let result = sqlx::query(
        "INSERT INTO public.strict_panel_operation_journal \
         (record_format_version, guild_id, ruleset_key, panel_key, state_tag, operation_payload) \
         VALUES (1, $1, $2, $3, $4, $5)",
    )
    .bind(GUILD.to_string())
    .bind("studyroom")
    .bind("mismatch")
    .bind("cleanup_pending")
    .bind(sqlx::types::Json(&operation))
    .execute(&pool)
    .await;
    assert!(result.is_err());
    cleanup(&pool).await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn oversized_slots_fail_closed_at_the_read_boundary() {
    let database = test_database("slot_capacity").await;
    let pool = database.pool.clone();
    cleanup(&pool).await;
    sqlx::query(
        "INSERT INTO public.ruleset_panel_installations \
         (guild_id, ruleset_key, panel_key, installed_version, channel_id, message_id, spec_hash) \
         SELECT $1, $2, 'panel_' || lpad(sequence::TEXT, 3, '0'), 1, '10', \
         (1000 + sequence)::TEXT, repeat('a', 64) \
         FROM generate_series(1, 257) AS sequence",
    )
    .bind(GUILD.to_string())
    .bind("studyroom")
    .execute(&pool)
    .await
    .unwrap();
    let store = PostgresPanelInstallationStore::new(pool.clone());
    let ruleset_key = RuleSetKey::parse("studyroom").unwrap();
    assert!(
        StrictPanelInstallationStore::list_slot(&store, GUILD, &ruleset_key)
            .await
            .is_err()
    );
    let template = cleanup_operation("template", 10, 100, StrictPanelCleanupKindV1::Removed);
    sqlx::query(
        "INSERT INTO public.strict_panel_operation_journal \
         (record_format_version, guild_id, ruleset_key, panel_key, state_tag, operation_payload) \
         SELECT 1, $1, $2, generated.panel_key, 'cleanup_pending', \
         jsonb_set($3::JSONB, '{key,panel_key}', to_jsonb(generated.panel_key)) \
         FROM (SELECT 'panel_' || lpad(sequence::TEXT, 3, '0') AS panel_key \
               FROM generate_series(1, 257) AS sequence) AS generated",
    )
    .bind(GUILD.to_string())
    .bind("studyroom")
    .bind(sqlx::types::Json(&template))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        StrictPanelOperationJournal::list_slot(&store, GUILD, &ruleset_key)
            .await
            .is_err()
    );
    cleanup(&pool).await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn public_write_paths_reject_the_first_over_capacity_record() {
    let database = test_database("write_capacity").await;
    let pool = database.pool.clone();
    cleanup(&pool).await;
    sqlx::query(
        "INSERT INTO public.ruleset_panel_installations \
         (guild_id, ruleset_key, panel_key, installed_version, channel_id, message_id, spec_hash) \
         SELECT $1, $2, 'panel_' || lpad(sequence::TEXT, 3, '0'), 1, '10', \
         (1000 + sequence)::TEXT, repeat('a', 64) \
         FROM generate_series(1, $3) AS sequence",
    )
    .bind(GUILD.to_string())
    .bind("studyroom")
    .bind(MAX_STRICT_PANEL_RECORDS_PER_SLOT as i64)
    .execute(&pool)
    .await
    .unwrap();
    let store = PostgresPanelInstallationStore::new(pool.clone());
    store
        .upsert(installation_for("panel_001", 2, 20, 200, 'b'))
        .await
        .unwrap();
    assert!(store
        .upsert(installation_for("panel_257", 1, 20, 257, 'b'))
        .await
        .is_err());
    let template = cleanup_operation("template", 10, 100, StrictPanelCleanupKindV1::Removed);
    sqlx::query(
        "INSERT INTO public.strict_panel_operation_journal \
         (record_format_version, guild_id, ruleset_key, panel_key, state_tag, operation_payload) \
         SELECT 1, $1, $2, generated.panel_key, 'cleanup_pending', \
         jsonb_set($3::JSONB, '{key,panel_key}', to_jsonb(generated.panel_key)) \
         FROM (SELECT 'panel_' || lpad(sequence::TEXT, 3, '0') AS panel_key \
               FROM generate_series(1, $4) AS sequence) AS generated",
    )
    .bind(GUILD.to_string())
    .bind("studyroom")
    .bind(sqlx::types::Json(&template))
    .bind(MAX_STRICT_PANEL_RECORDS_PER_SLOT as i64)
    .execute(&pool)
    .await
    .unwrap();
    StrictPanelOperationJournal::put(
        &store,
        cleanup_operation("panel_001", 20, 200, StrictPanelCleanupKindV1::Removed),
    )
    .await
    .unwrap();
    assert!(StrictPanelOperationJournal::put(
        &store,
        cleanup_operation("panel_257", 20, 257, StrictPanelCleanupKindV1::Removed),
    )
    .await
    .is_err());
    cleanup(&pool).await;
    drop_test_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn database_rejects_missing_and_null_journal_envelope_paths() {
    let database = test_database("journal_null_paths").await;
    let pool = database.pool.clone();
    cleanup(&pool).await;
    let missing = serde_json::json!({"padding": "x".repeat(64)});
    let missing_result = sqlx::query(
        "INSERT INTO public.strict_panel_operation_journal \
         (record_format_version, guild_id, ruleset_key, panel_key, state_tag, operation_payload) \
         VALUES (1, $1, 'studyroom', 'entry', 'cleanup_pending', $2)",
    )
    .bind(GUILD.to_string())
    .bind(sqlx::types::Json(missing))
    .execute(&pool)
    .await;
    assert!(missing_result.is_err());
    let null_paths = serde_json::json!({
        "key": {"guild_id": null, "ruleset_key": null, "panel_key": null},
        "state": {"state": null},
        "padding": "x".repeat(64)
    });
    let null_result = sqlx::query(
        "INSERT INTO public.strict_panel_operation_journal \
         (record_format_version, guild_id, ruleset_key, panel_key, state_tag, operation_payload) \
         VALUES (1, $1, 'studyroom', 'entry', 'cleanup_pending', $2)",
    )
    .bind(GUILD.to_string())
    .bind(sqlx::types::Json(null_paths))
    .execute(&pool)
    .await;
    assert!(null_result.is_err());
    cleanup(&pool).await;
    drop_test_database(database).await;
}
