use std::collections::BTreeMap;

use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceIdError, InstanceKind,
    InstanceMessageRef, InstanceResources, InstanceRuleSetVersion, InstanceStatus, InstanceStore,
    InstanceStoreError,
};
use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};
use futures::executor::block_on;

fn instance(guild: u64, id: &str) -> AutomationInstance {
    let mut roles = BTreeMap::new();
    roles.insert("member_role".to_string(), RoleId(100));
    let mut messages = BTreeMap::new();
    messages.insert(
        "welcome_panel".to_string(),
        InstanceMessageRef {
            channel: ChannelId(200),
            id: MessageId(300),
        },
    );
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
            messages,
        },
        status: InstanceStatus::Active,
    }
}

#[test]
fn instance_id_parse_validation() {
    assert!(InstanceId::parse("study_room_1").is_ok());
    assert!(InstanceId::parse("room-1").is_ok());
    assert_eq!(InstanceId::parse("").unwrap_err(), InstanceIdError::Empty);
    assert_eq!(
        InstanceId::parse(&"a".repeat(33)).unwrap_err(),
        InstanceIdError::TooLong
    );
    assert_eq!(
        InstanceId::parse("room 1").unwrap_err(),
        InstanceIdError::InvalidChar
    );
    assert_eq!(
        InstanceId::parse("room/1").unwrap_err(),
        InstanceIdError::InvalidChar
    );
    assert_eq!(
        InstanceId::parse("한글방").unwrap_err(),
        InstanceIdError::InvalidChar
    );
}

#[test]
fn bad_instance_id_json_rejected() {
    assert!(serde_json::from_str::<InstanceId>(r#""room 1""#).is_err());
    assert!(serde_json::from_str::<InstanceId>(r#""room:1""#).is_err());
    assert!(serde_json::from_str::<InstanceId>(r#""study_room_1""#).is_ok());
}

#[test]
fn automation_instance_serde_roundtrip() {
    let value = instance(7, "study_room_1");
    let json = serde_json::to_string(&value).unwrap();
    let back: AutomationInstance = serde_json::from_str(&json).unwrap();
    assert_eq!(value, back);
    let mut object = serde_json::to_value(&value).unwrap();
    assert_eq!(object["ruleset_version"], 7);
    object.as_object_mut().unwrap().remove("ruleset_version");
    assert!(serde_json::from_value::<AutomationInstance>(object).is_err());
    assert_eq!(
        serde_json::to_value(&value).unwrap()["resources"]["messages"]["welcome_panel"],
        serde_json::json!({"channel": "200", "id": "300"})
    );
}

#[test]
fn deleting_status_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&InstanceStatus::Deleting).unwrap(),
        r#""deleting""#
    );
    assert_eq!(
        serde_json::from_str::<InstanceStatus>(r#""deleting""#).unwrap(),
        InstanceStatus::Deleting
    );
}

#[test]
fn register_then_get() {
    let store = InMemoryInstanceStore::new();
    let value = instance(7, "room1");
    block_on(store.register(value.clone())).unwrap();
    assert_eq!(
        block_on(store.get(GuildId(7), &InstanceId::parse("room1").unwrap())).unwrap(),
        Some(value)
    );
    assert_eq!(
        block_on(store.get(GuildId(7), &InstanceId::parse("room1").unwrap()))
            .unwrap()
            .unwrap()
            .ruleset_version,
        InstanceRuleSetVersion::new(7).unwrap()
    );
}

#[test]
fn duplicate_register_fails() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(7, "room1"))).unwrap();
    assert_eq!(
        block_on(store.register(instance(7, "room1"))).unwrap_err(),
        InstanceStoreError::DuplicateInstance
    );
}

#[test]
fn same_id_allowed_across_guilds() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(1, "room1"))).unwrap();
    block_on(store.register(instance(2, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    assert_eq!(
        block_on(store.get(GuildId(1), &id))
            .unwrap()
            .unwrap()
            .guild_id,
        GuildId(1)
    );
    assert_eq!(
        block_on(store.get(GuildId(2), &id))
            .unwrap()
            .unwrap()
            .guild_id,
        GuildId(2)
    );
}

#[test]
fn guild_isolation() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(1, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    assert!(block_on(store.get(GuildId(2), &id)).unwrap().is_none());
    assert!(block_on(store.list_by_guild(GuildId(2)))
        .unwrap()
        .is_empty());
    assert_eq!(block_on(store.list_by_guild(GuildId(1))).unwrap().len(), 1);
}

#[test]
fn list_by_guild_deterministic() {
    let store = InMemoryInstanceStore::new();
    for id in ["c", "a", "b"] {
        block_on(store.register(instance(7, id))).unwrap();
    }
    let ids: Vec<String> = block_on(store.list_by_guild(GuildId(7)))
        .unwrap()
        .into_iter()
        .map(|value| value.id.as_str().to_string())
        .collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}

#[test]
fn update_status_works() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(7, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    block_on(store.update_status(GuildId(7), &id, InstanceStatus::Disabled)).unwrap();
    assert_eq!(
        block_on(store.get(GuildId(7), &id))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Disabled
    );
}

#[test]
fn update_status_missing_not_found() {
    let store = InMemoryInstanceStore::new();
    let id = InstanceId::parse("ghost").unwrap();
    assert_eq!(
        block_on(store.update_status(GuildId(7), &id, InstanceStatus::Disabled)).unwrap_err(),
        InstanceStoreError::NotFound
    );
}

#[test]
fn transition_to_deleting_is_active_only_cas() {
    let store = InMemoryInstanceStore::new();
    let id = InstanceId::parse("room1").unwrap();
    block_on(store.register(instance(7, "room1"))).unwrap();

    block_on(store.transition_to_deleting(GuildId(7), &id)).unwrap();
    assert_eq!(
        block_on(store.get(GuildId(7), &id))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleting
    );
    assert_eq!(
        block_on(store.transition_to_deleting(GuildId(7), &id)).unwrap_err(),
        InstanceStoreError::NotFound
    );
}

#[test]
fn mark_deleted_requires_deleting() {
    let store = InMemoryInstanceStore::new();
    let id = InstanceId::parse("room1").unwrap();
    block_on(store.register(instance(7, "room1"))).unwrap();

    assert_eq!(
        block_on(store.mark_deleted(GuildId(7), &id)).unwrap_err(),
        InstanceStoreError::NotFound
    );
    block_on(store.transition_to_deleting(GuildId(7), &id)).unwrap();
    block_on(store.mark_deleted(GuildId(7), &id)).unwrap();
    assert_eq!(
        block_on(store.get(GuildId(7), &id))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleted
    );
    assert_eq!(
        block_on(store.mark_deleted(GuildId(7), &id)).unwrap_err(),
        InstanceStoreError::NotFound
    );
}

#[test]
fn list_deleting_filters_and_orders() {
    let store = InMemoryInstanceStore::new();
    for id in ["c", "a", "b"] {
        block_on(store.register(instance(7, id))).unwrap();
    }
    block_on(store.transition_to_deleting(GuildId(7), &InstanceId::parse("c").unwrap())).unwrap();
    block_on(store.transition_to_deleting(GuildId(7), &InstanceId::parse("a").unwrap())).unwrap();

    let ids: Vec<String> = block_on(store.list_deleting(GuildId(7)))
        .unwrap()
        .into_iter()
        .map(|instance| instance.id.as_str().to_string())
        .collect();
    assert_eq!(ids, vec!["a", "c"]);
}

#[test]
fn returned_clone_does_not_mutate_store() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(7, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    let mut fetched = block_on(store.get(GuildId(7), &id)).unwrap().unwrap();
    fetched.status = InstanceStatus::Deleted;
    fetched.resources.roles.clear();
    let again = block_on(store.get(GuildId(7), &id)).unwrap().unwrap();
    assert_eq!(again.status, InstanceStatus::Active);
    assert_eq!(again.resources.roles.len(), 1);
}
