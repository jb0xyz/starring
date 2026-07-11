use std::collections::BTreeMap;

use automation_ruleset::{
    PublishOutcome, PublishRuleSetRequest, RuleSetKey, RuleSetStore, RuleSetVersionId,
};
use automation_ruleset_postgres::{PostgresRuleSetStore, MIGRATOR};
use automation_ruleset_readiness::{activate_if_ready, ActivationError, GuildCapabilities};
use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
use discord_model::{GuildId, Permissions, UserId};
use resource_resolution::ResourceBindingMap;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

const GUILD: GuildId = GuildId(9_000_104);

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
    for table in [
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "automation_ruleset_heads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE guild_id = $1"))
            .bind(GUILD.to_string())
            .execute(pool)
            .await
            .unwrap();
    }
}

fn key() -> RuleSetKey {
    RuleSetKey::parse("studyroom_activate").unwrap()
}

fn ready_rule() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "test".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::EditResponse {
                    content: "done".to_string(),
                },
            ],
        }],
    }
}

fn not_ready_rule() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "test".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::CreateRole {
                    key: "role".to_string(),
                    name: "n".to_string(),
                },
                ActionSpec::EditResponse {
                    content: "done".to_string(),
                },
            ],
        }],
    }
}

async fn publish(store: &PostgresRuleSetStore, definition: InteractionRuleSet) -> RuleSetVersionId {
    let outcome = store
        .publish(PublishRuleSetRequest {
            guild_id: GUILD,
            ruleset_key: key(),
            definition,
            created_by: UserId(1),
        })
        .await
        .unwrap();
    match outcome {
        PublishOutcome::Created(version) | PublishOutcome::Reused(version) => version.version,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn rejected_activation_keeps_active_pointer_and_published_artifact() {
    let pool = pool().await;
    cleanup(&pool).await;
    let store = PostgresRuleSetStore::new(pool.clone());
    let v1 = publish(&store, ready_rule()).await;
    let v2 = publish(&store, not_ready_rule()).await;
    let bindings = ResourceBindingMap::default();
    let roles = BTreeMap::new();
    let admin = GuildCapabilities {
        base_permissions: Permissions::ADMINISTRATOR,
    };
    let no_manage_roles = GuildCapabilities {
        base_permissions: Permissions::SEND_MESSAGES,
    };

    let activated = activate_if_ready(&store, GUILD, &key(), v1, &bindings, &admin, &roles)
        .await
        .unwrap();
    assert_eq!(activated.activation.active_version, v1);

    let rejected = activate_if_ready(
        &store,
        GUILD,
        &key(),
        v2,
        &bindings,
        &no_manage_roles,
        &roles,
    )
    .await
    .unwrap_err();
    assert!(matches!(rejected, ActivationError::NotReady(_)));
    assert_eq!(
        store.active(GUILD, &key()).await.unwrap().unwrap().version,
        v1
    );
    assert!(store
        .get_version(GUILD, &key(), v2)
        .await
        .unwrap()
        .is_some());
    cleanup(&pool).await;
}
