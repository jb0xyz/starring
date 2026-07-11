use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use discord_model::{ChannelId, GuildId, MessageId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelInstallationKey {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub panel_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelInstallation {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub panel_key: String,
    pub installed_version: RuleSetVersionId,
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub spec_hash: String,
}
