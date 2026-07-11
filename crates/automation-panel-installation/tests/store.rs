use automation_panel_installation::{
    InMemoryPanelInstallationStore, PanelInstallation, PanelInstallationKey, PanelInstallationStore,
};
use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use discord_model::{ChannelId, GuildId, MessageId};
use futures::executor::block_on;

fn key(guild_id: GuildId, panel_key: &str) -> PanelInstallationKey {
    PanelInstallationKey {
        guild_id,
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        panel_key: panel_key.to_string(),
    }
}

fn installation(
    guild_id: GuildId,
    panel_key: &str,
    version: u32,
    message: u64,
) -> PanelInstallation {
    PanelInstallation {
        guild_id,
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        panel_key: panel_key.to_string(),
        installed_version: RuleSetVersionId::new(version).unwrap(),
        channel_id: ChannelId(10),
        message_id: MessageId(message),
        spec_hash: "a".repeat(64),
    }
}

#[test]
fn upsert_roundtrips_and_replaces_logical_key() {
    let store = InMemoryPanelInstallationStore::new();
    block_on(store.upsert(installation(GuildId(7), "panel", 1, 100))).unwrap();
    block_on(store.upsert(installation(GuildId(7), "panel", 2, 101))).unwrap();
    let stored = block_on(store.get(&key(GuildId(7), "panel")))
        .unwrap()
        .unwrap();
    assert_eq!(stored.installed_version, RuleSetVersionId::new(2).unwrap());
    assert_eq!(stored.message_id, MessageId(101));
}

#[test]
fn logical_key_isolates_guild_and_panel() {
    let store = InMemoryPanelInstallationStore::new();
    block_on(store.upsert(installation(GuildId(7), "a", 1, 100))).unwrap();
    block_on(store.upsert(installation(GuildId(8), "a", 1, 101))).unwrap();
    block_on(store.upsert(installation(GuildId(7), "b", 1, 102))).unwrap();
    assert_eq!(
        block_on(store.get(&key(GuildId(7), "a")))
            .unwrap()
            .unwrap()
            .message_id,
        MessageId(100)
    );
    assert_eq!(
        block_on(store.get(&key(GuildId(8), "a")))
            .unwrap()
            .unwrap()
            .message_id,
        MessageId(101)
    );
    assert_eq!(
        block_on(store.get(&key(GuildId(7), "b")))
            .unwrap()
            .unwrap()
            .message_id,
        MessageId(102)
    );
}
