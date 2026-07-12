use std::collections::BTreeMap;

use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceResources, InstanceRuleSetVersion,
    InstanceStatus, InstanceStore, InstanceStoreError,
};
use automation_instance_postgres::{PostgresInstanceStore, MIGRATOR};
use discord_model::{GuildId, RoleId, UserId};
use sqlx::{Connection, PgConnection, PgPool};

const INITIAL_MIGRATION: &str =
    include_str!("../../../migrations/202607110001_create_automation_instances.sql");
const VERSION_MIGRATION: &str =
    include_str!("../../../migrations/202607120001_add_instance_ruleset_version.sql");

fn require_test_db() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored postgres integration tests");
    assert!(
        url.contains("test"),
        "refusing non-test DB: url must contain 'test'"
    );
    url
}

fn instance(guild: u64, id: &str) -> AutomationInstance {
    let mut roles = BTreeMap::new();
    roles.insert("member_role".to_string(), RoleId(900_100));
    AutomationInstance {
        id: InstanceId::parse(id).unwrap(),
        guild_id: GuildId(guild),
        ruleset_key: "studyroom_demo".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(7).unwrap(),
        kind: InstanceKind("study_room".to_string()),
        created_by: UserId(3),
        resources: InstanceResources {
            roles,
            channels: BTreeMap::new(),
            messages: BTreeMap::new(),
        },
        status: InstanceStatus::Active,
    }
}

async fn cleanup(pool: &PgPool, guild: u64) {
    let _ = sqlx::query("DELETE FROM automation_instances WHERE guild_id = $1")
        .bind(guild.to_string())
        .execute(pool)
        .await;
}

async fn legacy_connection(suffix: &str) -> (PgConnection, String) {
    let url = require_test_db();
    let mut connection = PgConnection::connect(&url).await.unwrap();
    let schema = format!("instance_version_{}_{}", std::process::id(), suffix);
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query(&format!("SET search_path TO {schema}"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::raw_sql(INITIAL_MIGRATION)
        .execute(&mut connection)
        .await
        .unwrap();
    (connection, schema)
}

async fn drop_schema(connection: &mut PgConnection, schema: &str) {
    sqlx::query("SET search_path TO public")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(connection)
        .await
        .unwrap();
}

async fn insert_legacy(connection: &mut PgConnection, status: &str) {
    sqlx::query(
        "INSERT INTO automation_instances \
         (guild_id, instance_id, ruleset_key, kind, created_by, status, resources) \
         VALUES ('7', 'legacy_room', 'studyroom_demo', 'study_room', '3', $1, '{}'::jsonb)",
    )
    .bind(status)
    .execute(connection)
    .await
    .unwrap();
}

async fn assert_non_deleted_migration_fails(status: &str, suffix: &str) {
    let (mut connection, schema) = legacy_connection(suffix).await;
    insert_legacy(&mut connection, status).await;
    let error = sqlx::raw_sql(VERSION_MIGRATION)
        .execute(&mut connection)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("non-deleted legacy automation instances require an explicit ruleset version"));
    drop_schema(&mut connection, &schema).await;
}

#[tokio::test]
#[ignore]
async fn register_reconnect_get_durability() {
    let url = require_test_db();
    let pool_a = PgPool::connect(&url).await.unwrap();
    MIGRATOR.run(&pool_a).await.unwrap();
    cleanup(&pool_a, 990001).await;
    let store_a = PostgresInstanceStore::new(pool_a.clone());
    let value = instance(990001, "durable_room");
    store_a.register(value.clone()).await.unwrap();
    let persisted_version: i64 = sqlx::query_scalar(
        "SELECT ruleset_version FROM automation_instances \
         WHERE guild_id = $1 AND instance_id = $2",
    )
    .bind("990001")
    .bind("durable_room")
    .fetch_one(&pool_a)
    .await
    .unwrap();
    assert_eq!(persisted_version, 7);
    assert_eq!(
        store_a.register(value.clone()).await.unwrap_err(),
        InstanceStoreError::DuplicateInstance
    );
    pool_a.close().await;

    let pool_b = PgPool::connect(&url).await.unwrap();
    let store_b = PostgresInstanceStore::new(pool_b.clone());
    let id = InstanceId::parse("durable_room").unwrap();
    assert_eq!(
        store_b.get(GuildId(990001), &id).await.unwrap(),
        Some(value)
    );
    let listed = store_b.list_by_guild(GuildId(990001)).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].ruleset_version.get(), 7);
    store_b
        .update_status(GuildId(990001), &id, InstanceStatus::Disabled)
        .await
        .unwrap();
    assert_eq!(
        store_b
            .get(GuildId(990001), &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Disabled
    );
    cleanup(&pool_b, 990001).await;
    pool_b.close().await;
}

#[tokio::test]
#[ignore]
async fn guild_isolation_and_missing() {
    let url = require_test_db();
    let pool = PgPool::connect(&url).await.unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    cleanup(&pool, 990002).await;
    cleanup(&pool, 990003).await;
    let store = PostgresInstanceStore::new(pool.clone());
    store.register(instance(990002, "room1")).await.unwrap();
    store.register(instance(990003, "room1")).await.unwrap();
    let id = InstanceId::parse("room1").unwrap();
    assert_eq!(
        store
            .get(GuildId(990002), &id)
            .await
            .unwrap()
            .unwrap()
            .guild_id,
        GuildId(990002)
    );
    assert!(store.get(GuildId(990099), &id).await.unwrap().is_none());
    assert_eq!(
        store
            .update_status(GuildId(990099), &id, InstanceStatus::Deleted)
            .await
            .unwrap_err(),
        InstanceStoreError::NotFound
    );
    cleanup(&pool, 990002).await;
    cleanup(&pool, 990003).await;
    pool.close().await;
}

#[tokio::test]
#[ignore]
async fn deleted_legacy_row_backfills_version_one() {
    let (mut connection, schema) = legacy_connection("deleted").await;
    insert_legacy(&mut connection, "deleted").await;
    sqlx::raw_sql(VERSION_MIGRATION)
        .execute(&mut connection)
        .await
        .unwrap();
    let version: i64 = sqlx::query_scalar("SELECT ruleset_version FROM automation_instances")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(version, 1);
    drop_schema(&mut connection, &schema).await;
}

#[tokio::test]
#[ignore]
async fn active_legacy_row_blocks_migration() {
    assert_non_deleted_migration_fails("active", "active").await;
}

#[tokio::test]
#[ignore]
async fn disabled_legacy_row_blocks_migration() {
    assert_non_deleted_migration_fails("disabled", "disabled").await;
}

#[tokio::test]
#[ignore]
async fn teardown_status_cas_list_and_reconnect() {
    let url = require_test_db();
    let pool_a = PgPool::connect(&url).await.unwrap();
    MIGRATOR.run(&pool_a).await.unwrap();
    cleanup(&pool_a, 990004).await;
    let store_a = PostgresInstanceStore::new(pool_a.clone());
    let id = InstanceId::parse("teardown_room").unwrap();
    store_a
        .register(instance(990004, "teardown_room"))
        .await
        .unwrap();
    store_a
        .transition_to_deleting(GuildId(990004), &id)
        .await
        .unwrap();
    assert_eq!(
        store_a
            .transition_to_deleting(GuildId(990004), &id)
            .await
            .unwrap_err(),
        InstanceStoreError::NotFound
    );
    assert_eq!(
        store_a.list_deleting(GuildId(990004)).await.unwrap().len(),
        1
    );
    pool_a.close().await;

    let pool_b = PgPool::connect(&url).await.unwrap();
    let store_b = PostgresInstanceStore::new(pool_b.clone());
    assert_eq!(
        store_b
            .get(GuildId(990004), &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleting
    );
    store_b.mark_deleted(GuildId(990004), &id).await.unwrap();
    assert!(store_b
        .list_deleting(GuildId(990004))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store_b
            .get(GuildId(990004), &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleted
    );
    cleanup(&pool_b, 990004).await;
    pool_b.close().await;
}
