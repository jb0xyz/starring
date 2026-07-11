use automation_panel_installation::{
    PanelInstallation, PanelInstallationKey, PanelInstallationStore,
};
use automation_panel_installation_postgres::{PostgresPanelInstallationStore, MIGRATOR};
use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use discord_model::{ChannelId, GuildId, MessageId};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

const GUILD: GuildId = GuildId(9_000_105);

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL must be set for ignored postgres tests");
    assert!(
        url.contains("test"),
        "refusing to run against a database whose name does not contain 'test'"
    );
    url
}

async fn pool() -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url())
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM ruleset_panel_installations WHERE guild_id = $1")
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
    PanelInstallation {
        guild_id: GUILD,
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        panel_key: "entry".to_string(),
        installed_version: RuleSetVersionId::new(version).unwrap(),
        channel_id: ChannelId(channel),
        message_id: MessageId(message),
        spec_hash: hash.to_string().repeat(64),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn upsert_conflict_updates_in_place() {
    let pool = pool().await;
    cleanup(&pool).await;
    let store = PostgresPanelInstallationStore::new(pool.clone());
    store.upsert(installation(1, 10, 100, 'a')).await.unwrap();
    store.upsert(installation(2, 20, 200, 'b')).await.unwrap();
    assert_eq!(
        store.get(&key()).await.unwrap(),
        Some(installation(2, 20, 200, 'b'))
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ruleset_panel_installations WHERE guild_id = $1 AND ruleset_key = $2 AND panel_key = $3",
    )
    .bind(GUILD.to_string())
    .bind("studyroom")
    .bind("entry")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    cleanup(&pool).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn reconnect_preserves_installation() {
    {
        let first_pool = pool().await;
        cleanup(&first_pool).await;
        let store = PostgresPanelInstallationStore::new(first_pool.clone());
        store.upsert(installation(1, 10, 100, 'a')).await.unwrap();
        first_pool.close().await;
    }
    let second_pool = pool().await;
    let store = PostgresPanelInstallationStore::new(second_pool.clone());
    assert_eq!(
        store.get(&key()).await.unwrap(),
        Some(installation(1, 10, 100, 'a'))
    );
    cleanup(&second_pool).await;
}
