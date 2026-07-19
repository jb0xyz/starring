use std::collections::{BTreeMap, BTreeSet};

use automation_core::{
    AutomationServices, EventKind, HandleOutcome, MockInteractionResponder, MockMutationAdapter,
    MutationCall, ResponderCall, RuntimeEvent,
};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
    InstanceRuleSetVersion, InstanceStatus, InstanceStore, SequenceInstanceIdGenerator,
};
use automation_ruleset::{
    PublishOutcome, PublishRuleSetRequest, RuleSetKey, RuleSetStore, RuleSetVersionId,
};
use automation_ruleset_dispatch::{
    dispatch_instance_action, GuildRoleSnapshot, GuildRoleSnapshotProvider, SnapshotError,
};
use automation_ruleset_postgres::{PostgresRuleSetStore, MIGRATOR};
use automation_state::{
    ActionSpec, ActionTarget, InstanceRef, InteractionRule, InteractionRuleSet, RoleRef,
    TriggerSpec,
};
use discord_model::{GuildId, Permissions, RoleId, UserId};
use resource_resolution::ResourceBindingMap;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

const GUILD: GuildId = GuildId(9_000_103);
const ACTOR: UserId = UserId(42);
const MEMBER_ROLE: RoleId = RoleId(500);

struct StubSnapshot;

impl GuildRoleSnapshotProvider for StubSnapshot {
    async fn snapshot(&self, _guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        let mut roles = BTreeMap::new();
        roles.insert(RoleId(GUILD.0), Permissions::ADMINISTRATOR);
        Ok(GuildRoleSnapshot {
            roles,
            bot_role_ids: BTreeSet::new(),
        })
    }
}

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
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE automation_ruleset_versions \
         DISABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    for table in [
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "automation_ruleset_heads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE guild_id = $1"))
            .bind(GUILD.to_string())
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    sqlx::query(
        "ALTER TABLE automation_ruleset_versions \
         ENABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn key() -> RuleSetKey {
    RuleSetKey::parse("studyroom_demo").unwrap()
}

fn join_rule(tag: &str) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "join".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::GrantRole {
                    role: RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias: "member_role".to_string(),
                    },
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: format!("joined {tag}"),
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

fn instance(version: RuleSetVersionId) -> AutomationInstance {
    let mut resources = InstanceResources::default();
    resources
        .roles
        .insert("member_role".to_string(), MEMBER_ROLE);
    AutomationInstance {
        id: InstanceId::parse("room_a").unwrap(),
        guild_id: GUILD,
        ruleset_key: key().to_string(),
        ruleset_version: InstanceRuleSetVersion::new(version.get()).unwrap(),
        kind: InstanceKind("study_room".to_string()),
        created_by: ACTOR,
        resources,
        status: InstanceStatus::Active,
    }
}

fn join_event() -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GUILD,
        actor: ACTOR,
        kind: EventKind::InstanceAction {
            instance_id: InstanceId::parse("room_a").unwrap(),
            action: "join".to_string(),
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn pinned_v1_dispatch_uses_postgres_version_while_v2_is_active() {
    let pool = pool().await;
    cleanup(&pool).await;
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let v1 = publish(&rulesets, join_rule("v1")).await;
    let v2 = publish(&rulesets, join_rule("v2")).await;
    rulesets.activate(GUILD, &key(), v2).await.unwrap();

    let instances = InMemoryInstanceStore::new();
    instances.register(instance(v1)).await.unwrap();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instance_ids = SequenceInstanceIdGenerator::new("room", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &instance_ids,
        teardown: &automation_core::MockInstanceTeardownService::new(),
    };
    let event = join_event();
    let outcome = dispatch_instance_action(
        &event,
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &rulesets,
        &StubSnapshot,
        &ResourceBindingMap::default(),
        &services,
        "failed",
    )
    .await
    .unwrap();

    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::GrantRole {
            guild: GUILD,
            member: ACTOR,
            role: MEMBER_ROLE,
        }]
    );
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "joined v1".to_string(),
            },
        ]
    );
    assert_eq!(
        rulesets
            .active(GUILD, &key())
            .await
            .unwrap()
            .unwrap()
            .version,
        v2
    );
    cleanup(&pool).await;
}
