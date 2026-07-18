use automation_ruleset::{
    content_hash, ExpectedActiveRuleSet, GuardedActivationOutcome, GuardedRuleSetActivation,
    InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetContentHash,
    RuleSetHashError, RuleSetHasher, RuleSetKey, RuleSetSchemaVersion, RuleSetStore,
    RuleSetStoreError, RuleSetVersionId, RuleSetVersionIdentity, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_state::{
    ActionSpec, ActionTarget, InstanceRef, InteractionRule, InteractionRuleSet, RoleRef,
    TriggerSpec,
};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;

fn ruleset(content: &str) -> InteractionRuleSet {
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
                        alias: "member_role".to_string(),
                    },
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: content.to_string(),
                },
            ],
        }],
    }
}

fn req(guild: u64, key: &str, def: InteractionRuleSet) -> PublishRuleSetRequest {
    PublishRuleSetRequest {
        guild_id: GuildId(guild),
        ruleset_key: RuleSetKey::parse(key).unwrap(),
        definition: def,
        created_by: UserId(1),
    }
}

fn key(k: &str) -> RuleSetKey {
    RuleSetKey::parse(k).unwrap()
}

#[test]
fn first_publish_creates_v1_reuse_and_change() {
    let store = InMemoryRuleSetStore::default();
    let a = block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    let v1 = match a {
        PublishOutcome::Created(v) => v,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    assert_eq!(v1.version, RuleSetVersionId::FIRST);

    let again = block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    assert!(matches!(again, PublishOutcome::Reused(ref v) if v.version == RuleSetVersionId::FIRST));
    assert_eq!(
        block_on(store.list_versions(GuildId(7), &key("studyroom")))
            .unwrap()
            .len(),
        1
    );

    let changed = block_on(store.publish(req(7, "studyroom", ruleset("b")))).unwrap();
    assert!(matches!(changed, PublishOutcome::Created(ref v) if v.version.get() == 2));
}

#[test]
fn guild_and_key_isolation() {
    let store = InMemoryRuleSetStore::default();
    for (g, k) in [(7, "studyroom"), (8, "studyroom"), (7, "ticket")] {
        let out = block_on(store.publish(req(g, k, ruleset("x")))).unwrap();
        assert!(
            matches!(out, PublishOutcome::Created(ref v) if v.version == RuleSetVersionId::FIRST)
        );
    }
}

#[test]
fn publish_does_not_change_activation() {
    let store = InMemoryRuleSetStore::default();
    block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    assert!(block_on(store.active(GuildId(7), &key("studyroom")))
        .unwrap()
        .is_none());
}

#[test]
fn activate_missing_then_activate_and_rollback() {
    let store = InMemoryRuleSetStore::default();
    assert_eq!(
        block_on(store.activate(GuildId(7), &key("studyroom"), RuleSetVersionId::FIRST))
            .unwrap_err(),
        RuleSetStoreError::VersionNotFound
    );
    block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    block_on(store.publish(req(7, "studyroom", ruleset("b")))).unwrap();
    let v1 = RuleSetVersionId::FIRST;
    let v2 = RuleSetVersionId::new(2).unwrap();

    let act = block_on(store.activate(GuildId(7), &key("studyroom"), v2)).unwrap();
    assert_eq!(act.active_version, v2);
    assert_eq!(
        block_on(store.active(GuildId(7), &key("studyroom")))
            .unwrap()
            .unwrap()
            .version,
        v2
    );
    block_on(store.activate(GuildId(7), &key("studyroom"), v1)).unwrap();
    assert_eq!(
        block_on(store.active(GuildId(7), &key("studyroom")))
            .unwrap()
            .unwrap()
            .version,
        v1
    );
}

#[test]
fn guarded_activation_is_idempotent_and_never_overwrites_a_drifted_baseline() {
    let store = InMemoryRuleSetStore::default();
    let first = match block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap() {
        PublishOutcome::Created(version) => version,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    let second = match block_on(store.publish(req(7, "studyroom", ruleset("b")))).unwrap() {
        PublishOutcome::Created(version) => version,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    let first_identity = RuleSetVersionIdentity::from(&first);
    let second_identity = RuleSetVersionIdentity::from(&second);
    let activate_first = GuardedRuleSetActivation {
        guild_id: GuildId(7),
        ruleset_key: key("studyroom"),
        target: first_identity.clone(),
        expected_active: ExpectedActiveRuleSet::Absent,
    };
    assert!(matches!(
        block_on(store.activate_guarded(activate_first.clone())).unwrap(),
        GuardedActivationOutcome::Activated(_)
    ));
    assert!(matches!(
        block_on(store.activate_guarded(activate_first)).unwrap(),
        GuardedActivationOutcome::AlreadyTarget(_)
    ));

    let drifted = block_on(store.activate_guarded(GuardedRuleSetActivation {
        guild_id: GuildId(7),
        ruleset_key: key("studyroom"),
        target: second_identity.clone(),
        expected_active: ExpectedActiveRuleSet::Absent,
    }))
    .unwrap();
    assert_eq!(
        drifted,
        GuardedActivationOutcome::BaselineMismatch {
            observed_active: Some(first_identity.clone())
        }
    );
    assert_eq!(
        block_on(store.active(GuildId(7), &key("studyroom")))
            .unwrap()
            .unwrap()
            .version,
        first.version
    );

    assert!(matches!(
        block_on(store.activate_guarded(GuardedRuleSetActivation {
            guild_id: GuildId(7),
            ruleset_key: key("studyroom"),
            target: second_identity,
            expected_active: ExpectedActiveRuleSet::Exact {
                identity: first_identity
            },
        }))
        .unwrap(),
        GuardedActivationOutcome::Activated(_)
    ));
    assert_eq!(
        block_on(store.active(GuildId(7), &key("studyroom")))
            .unwrap()
            .unwrap()
            .version,
        second.version
    );
}

#[test]
fn guarded_activation_binds_the_exact_target_hash() {
    let store = InMemoryRuleSetStore::default();
    let target = match block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap() {
        PublishOutcome::Created(version) => version,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    let result = block_on(store.activate_guarded(GuardedRuleSetActivation {
        guild_id: GuildId(7),
        ruleset_key: key("studyroom"),
        target: RuleSetVersionIdentity {
            version: target.version,
            content_hash: RuleSetContentHash::parse_hex(&"ff".repeat(32)).unwrap(),
        },
        expected_active: ExpectedActiveRuleSet::Absent,
    }));

    assert_eq!(result.unwrap_err(), RuleSetStoreError::TargetHashMismatch);
    assert!(block_on(store.active(GuildId(7), &key("studyroom")))
        .unwrap()
        .is_none());
}

#[test]
fn invalid_definition_rejected_before_version() {
    let store = InMemoryRuleSetStore::default();
    let mut bad = ruleset("a");
    bad.rules[0].key = String::new();
    bad.rules.push(InteractionRule {
        key: String::new(),
        trigger: TriggerSpec::InstanceAction {
            action: "join".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: "y".to_string(),
        }],
    });
    let err = block_on(store.publish(req(7, "studyroom", bad))).unwrap_err();
    assert!(matches!(err, RuleSetStoreError::InvalidDefinition(_)));
    assert!(block_on(store.list_versions(GuildId(7), &key("studyroom")))
        .unwrap()
        .is_empty());
}

struct FixedHasher;

impl RuleSetHasher for FixedHasher {
    fn hash(
        &self,
        _schema_version: RuleSetSchemaVersion,
        _definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError> {
        Ok(RuleSetContentHash::parse_hex(&"ab".repeat(32)).unwrap())
    }
}

#[test]
fn same_hash_different_definition_is_collision() {
    let store = InMemoryRuleSetStore::new(FixedHasher);
    block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    let err = block_on(store.publish(req(7, "studyroom", ruleset("b")))).unwrap_err();
    assert_eq!(err, RuleSetStoreError::HashCollision);
}

#[test]
fn returned_artifact_clone_does_not_mutate_store() {
    let store = InMemoryRuleSetStore::default();
    let out = block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    let mut v = match out {
        PublishOutcome::Created(v) => v,
        PublishOutcome::Reused(v) => v,
    };
    v.created_by = UserId(999);
    let stored =
        block_on(store.get_version(GuildId(7), &key("studyroom"), RuleSetVersionId::FIRST))
            .unwrap()
            .unwrap();
    assert_eq!(stored.created_by, UserId(1));
}

#[test]
fn schema_version_hash_relative_check() {
    let a = content_hash(RuleSetSchemaVersion::new(1).unwrap(), &ruleset("a")).unwrap();
    let b = content_hash(RuleSetSchemaVersion::new(2).unwrap(), &ruleset("a")).unwrap();
    assert_ne!(a, b);
    let _ = CURRENT_RULESET_SCHEMA_VERSION;
}
