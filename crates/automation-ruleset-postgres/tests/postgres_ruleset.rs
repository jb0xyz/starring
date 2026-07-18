use std::sync::Arc;

use automation_ruleset::{
    ExpectedActiveRuleSet, GuardedActivationOutcome, GuardedRuleSetActivation, PublishOutcome,
    PublishRuleSetRequest, RuleSetContentHash, RuleSetHashError, RuleSetHasher, RuleSetKey,
    RuleSetSchemaVersion, RuleSetStore, RuleSetStoreError, RuleSetVersion, RuleSetVersionId,
    RuleSetVersionIdentity,
};
use automation_ruleset_postgres::{PostgresRuleSetStore, MIGRATOR};
use automation_state::{
    ActionSpec, ActionTarget, InstanceRef, InteractionRule, InteractionRuleSet, RoleRef,
    TriggerSpec,
};
use discord_model::{GuildId, UserId};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

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
        .max_connections(24)
        .connect(&database_url())
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

async fn cleanup(pool: &PgPool, guild: GuildId) {
    let g = guild.to_string();
    for table in [
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "automation_ruleset_heads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE guild_id = $1"))
            .bind(&g)
            .execute(pool)
            .await
            .unwrap();
    }
}

fn definition(alias: &str) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::GrantRole {
                    role: RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias: alias.to_string(),
                    },
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: "done".to_string(),
                },
            ],
        }],
    }
}

fn request(guild: GuildId, key: &RuleSetKey, def: InteractionRuleSet) -> PublishRuleSetRequest {
    PublishRuleSetRequest {
        guild_id: guild,
        ruleset_key: key.clone(),
        definition: def,
        created_by: UserId(1),
    }
}

fn key() -> RuleSetKey {
    RuleSetKey::parse("studyroom").unwrap()
}

fn identity(version: &RuleSetVersion) -> RuleSetVersionIdentity {
    RuleSetVersionIdentity::from(version)
}

async fn head_next(pool: &PgPool, guild: GuildId, key: &RuleSetKey) -> i64 {
    sqlx::query_scalar(
        "SELECT next_version FROM automation_ruleset_heads WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(guild.to_string())
    .bind(key.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_same_content_creates_one_reuses_rest() {
    let pool = pool().await;
    let guild = GuildId(9_000_001);
    cleanup(&pool, guild).await;
    let store = Arc::new(PostgresRuleSetStore::new(pool.clone()));
    let barrier = Arc::new(tokio::sync::Barrier::new(20));

    let mut handles = Vec::new();
    for _ in 0..20 {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .publish(request(guild, &key(), definition("member_role")))
                .await
        }));
    }
    let mut created = 0;
    let mut reused = 0;
    for handle in handles {
        match handle.await.unwrap().unwrap() {
            PublishOutcome::Created(_) => created += 1,
            PublishOutcome::Reused(_) => reused += 1,
        }
    }
    assert_eq!(created, 1);
    assert_eq!(reused, 19);
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 1);
    assert_eq!(head_next(&pool, guild, &key()).await, 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_distinct_content_no_version_collision() {
    let pool = pool().await;
    let guild = GuildId(9_000_002);
    cleanup(&pool, guild).await;
    let store = Arc::new(PostgresRuleSetStore::new(pool.clone()));
    let barrier = Arc::new(tokio::sync::Barrier::new(10));

    let mut handles = Vec::new();
    for i in 0..10 {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .publish(request(guild, &key(), definition(&format!("alias_{i}"))))
                .await
        }));
    }
    let mut versions = Vec::new();
    for handle in handles {
        if let PublishOutcome::Created(v) = handle.await.unwrap().unwrap() {
            versions.push(v.version.get());
        }
    }
    versions.sort_unstable();
    versions.dedup();
    assert_eq!(versions.len(), 10);
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 10);
    cleanup(&pool, guild).await;
}

async fn drop_head_trigger(pool: &PgPool) {
    sqlx::query(
        "DROP TRIGGER IF EXISTS starring_test_fail_ruleset_head_update \
         ON automation_ruleset_heads",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS starring_test_fail_ruleset_head_update()")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn mid_transaction_failure_rolls_back_version_and_head() {
    let pool = pool().await;
    let guild = GuildId(9_000_003);
    cleanup(&pool, guild).await;
    drop_head_trigger(&pool).await;
    let store = PostgresRuleSetStore::new(pool.clone());

    store
        .publish(request(guild, &key(), definition("v1")))
        .await
        .unwrap();

    sqlx::query(
        "CREATE FUNCTION starring_test_fail_ruleset_head_update() RETURNS trigger AS $$ \
         BEGIN RAISE EXCEPTION 'forced head update failure'; END; $$ LANGUAGE plpgsql",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER starring_test_fail_ruleset_head_update \
         BEFORE UPDATE ON automation_ruleset_heads \
         FOR EACH ROW WHEN (NEW.guild_id = '{}') \
         EXECUTE FUNCTION starring_test_fail_ruleset_head_update()",
        guild
    ))
    .execute(&pool)
    .await
    .unwrap();

    let result = store
        .publish(request(guild, &key(), definition("v2")))
        .await;

    drop_head_trigger(&pool).await;

    assert!(matches!(result, Err(RuleSetStoreError::Backend(_))));
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 1);
    assert_eq!(head_next(&pool, guild, &key()).await, 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn version_overflow_boundary() {
    let pool = pool().await;
    let guild = GuildId(9_000_004);
    cleanup(&pool, guild).await;
    let store = PostgresRuleSetStore::new(pool.clone());

    store
        .publish(request(guild, &key(), definition("first")))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE automation_ruleset_heads SET next_version = 4294967295 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(guild.to_string())
    .bind(key().as_str())
    .execute(&pool)
    .await
    .unwrap();

    let created = store
        .publish(request(guild, &key(), definition("max")))
        .await
        .unwrap();
    match created {
        PublishOutcome::Created(v) => assert_eq!(v.version.get(), u32::MAX),
        PublishOutcome::Reused(_) => panic!("expected Created"),
    }
    assert_eq!(head_next(&pool, guild, &key()).await, 4_294_967_296);

    let overflow = store
        .publish(request(guild, &key(), definition("over")))
        .await;
    assert!(matches!(overflow, Err(RuleSetStoreError::VersionOverflow)));
    assert_eq!(head_next(&pool, guild, &key()).await, 4_294_967_296);
    cleanup(&pool, guild).await;
}

struct FixedHasher;

impl RuleSetHasher for FixedHasher {
    fn hash(
        &self,
        _schema_version: RuleSetSchemaVersion,
        _definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError> {
        Ok(RuleSetContentHash::parse_hex(&"cd".repeat(32)).unwrap())
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn hash_collision_leaves_store_unchanged() {
    let pool = pool().await;
    let guild = GuildId(9_000_005);
    cleanup(&pool, guild).await;
    let store = PostgresRuleSetStore::with_hasher(pool.clone(), FixedHasher);

    store
        .publish(request(guild, &key(), definition("a")))
        .await
        .unwrap();
    let err = store
        .publish(request(guild, &key(), definition("b")))
        .await
        .unwrap_err();
    assert_eq!(err, RuleSetStoreError::HashCollision);
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 1);
    assert_eq!(head_next(&pool, guild, &key()).await, 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn activation_integrity_and_rollback() {
    let pool = pool().await;
    let guild = GuildId(9_000_006);
    cleanup(&pool, guild).await;
    let store = PostgresRuleSetStore::new(pool.clone());
    let v1 = RuleSetVersionId::FIRST;
    let v2 = RuleSetVersionId::new(2).unwrap();

    assert_eq!(
        store.activate(guild, &key(), v1).await.unwrap_err(),
        RuleSetStoreError::VersionNotFound
    );
    assert!(store.active(guild, &key()).await.unwrap().is_none());

    store
        .publish(request(guild, &key(), definition("a")))
        .await
        .unwrap();
    store
        .publish(request(guild, &key(), definition("b")))
        .await
        .unwrap();

    store.activate(guild, &key(), v2).await.unwrap();
    assert_eq!(
        store.active(guild, &key()).await.unwrap().unwrap().version,
        v2
    );
    store.activate(guild, &key(), v2).await.unwrap();
    store.activate(guild, &key(), v1).await.unwrap();
    assert_eq!(
        store.active(guild, &key()).await.unwrap().unwrap().version,
        v1
    );
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn reconnect_durability() {
    let guild = GuildId(9_000_007);
    {
        let pool_a = pool().await;
        cleanup(&pool_a, guild).await;
        let store = PostgresRuleSetStore::new(pool_a.clone());
        store
            .publish(request(guild, &key(), definition("a")))
            .await
            .unwrap();
        store
            .activate(guild, &key(), RuleSetVersionId::FIRST)
            .await
            .unwrap();
        pool_a.close().await;
    }
    let pool_b = pool().await;
    let store = PostgresRuleSetStore::new(pool_b.clone());
    let versions = store.list_versions(guild, &key()).await.unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        store.active(guild, &key()).await.unwrap().unwrap().version,
        RuleSetVersionId::FIRST
    );
    cleanup(&pool_b, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn guarded_activation_preserves_exact_baseline_and_target_hash() {
    let pool = pool().await;
    let guild = GuildId(9_000_008);
    cleanup(&pool, guild).await;
    let store = PostgresRuleSetStore::new(pool.clone());
    let first = match store
        .publish(request(guild, &key(), definition("guarded_a")))
        .await
        .unwrap()
    {
        PublishOutcome::Created(version) => version,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    let second = match store
        .publish(request(guild, &key(), definition("guarded_b")))
        .await
        .unwrap()
    {
        PublishOutcome::Created(version) => version,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    let first_identity = identity(&first);
    let second_identity = identity(&second);

    assert!(matches!(
        store
            .activate_guarded(GuardedRuleSetActivation {
                guild_id: guild,
                ruleset_key: key(),
                target: first_identity.clone(),
                expected_active: ExpectedActiveRuleSet::Absent,
            })
            .await
            .unwrap(),
        GuardedActivationOutcome::Activated(_)
    ));
    assert!(matches!(
        store
            .activate_guarded(GuardedRuleSetActivation {
                guild_id: guild,
                ruleset_key: key(),
                target: first_identity.clone(),
                expected_active: ExpectedActiveRuleSet::Absent,
            })
            .await
            .unwrap(),
        GuardedActivationOutcome::AlreadyTarget(_)
    ));
    assert_eq!(
        store
            .activate_guarded(GuardedRuleSetActivation {
                guild_id: guild,
                ruleset_key: key(),
                target: second_identity.clone(),
                expected_active: ExpectedActiveRuleSet::Absent,
            })
            .await
            .unwrap(),
        GuardedActivationOutcome::BaselineMismatch {
            observed_active: Some(first_identity.clone())
        }
    );
    assert!(matches!(
        store
            .activate_guarded(GuardedRuleSetActivation {
                guild_id: guild,
                ruleset_key: key(),
                target: second_identity,
                expected_active: ExpectedActiveRuleSet::Exact {
                    identity: first_identity
                },
            })
            .await
            .unwrap(),
        GuardedActivationOutcome::Activated(_)
    ));
    assert_eq!(store.active(guild, &key()).await.unwrap().unwrap(), second);

    let mismatch = store
        .activate_guarded(GuardedRuleSetActivation {
            guild_id: guild,
            ruleset_key: key(),
            target: RuleSetVersionIdentity {
                version: first.version,
                content_hash: RuleSetContentHash::parse_hex(&"ef".repeat(32)).unwrap(),
            },
            expected_active: ExpectedActiveRuleSet::Exact {
                identity: identity(&second),
            },
        })
        .await;
    assert_eq!(mismatch.unwrap_err(), RuleSetStoreError::TargetHashMismatch);
    assert_eq!(store.active(guild, &key()).await.unwrap().unwrap(), second);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_absent_guarded_activations_have_one_winner() {
    let pool = pool().await;
    let guild = GuildId(9_000_009);
    cleanup(&pool, guild).await;
    let store = Arc::new(PostgresRuleSetStore::new(pool.clone()));
    let first = match store
        .publish(request(guild, &key(), definition("race_a")))
        .await
        .unwrap()
    {
        PublishOutcome::Created(version) => version,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    let second = match store
        .publish(request(guild, &key(), definition("race_b")))
        .await
        .unwrap()
    {
        PublishOutcome::Created(version) => version,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let first_store = store.clone();
    let first_barrier = barrier.clone();
    let first_task = tokio::spawn(async move {
        first_barrier.wait().await;
        first_store
            .activate_guarded(GuardedRuleSetActivation {
                guild_id: guild,
                ruleset_key: key(),
                target: identity(&first),
                expected_active: ExpectedActiveRuleSet::Absent,
            })
            .await
    });
    let second_store = store.clone();
    let second_barrier = barrier.clone();
    let second_task = tokio::spawn(async move {
        second_barrier.wait().await;
        second_store
            .activate_guarded(GuardedRuleSetActivation {
                guild_id: guild,
                ruleset_key: key(),
                target: identity(&second),
                expected_active: ExpectedActiveRuleSet::Absent,
            })
            .await
    });
    let outcomes = [
        first_task.await.unwrap().unwrap(),
        second_task.await.unwrap().unwrap(),
    ];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, GuardedActivationOutcome::Activated(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, GuardedActivationOutcome::BaselineMismatch { .. }))
            .count(),
        1
    );
    cleanup(&pool, guild).await;
}
