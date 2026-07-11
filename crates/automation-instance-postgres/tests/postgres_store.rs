use std::collections::BTreeMap;

use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceResources, InstanceRuleSetVersion,
    InstanceStatus, InstanceStore, InstanceStoreError,
};
use automation_instance_postgres::{PostgresInstanceStore, MIGRATOR};
use discord_model::{GuildId, RoleId, UserId};
use sqlx::PgPool;

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
    assert_eq!(
        store_b.list_by_guild(GuildId(990001)).await.unwrap().len(),
        1
    );
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
