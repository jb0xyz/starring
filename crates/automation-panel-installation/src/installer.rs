use std::fmt;

use automation_state::PanelSpec;
use discord_model::{ChannelId, GuildId, MessageId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelPresence {
    Present,
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelEditOutcome {
    Updated,
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallerError(String);

impl InstallerError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstallerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for InstallerError {}

#[allow(async_fn_in_trait)]
pub trait PanelInstaller {
    async fn fetch_message(
        &self,
        channel: ChannelId,
        message: MessageId,
    ) -> Result<PanelPresence, InstallerError>;

    async fn post_message(
        &self,
        channel: ChannelId,
        guild: GuildId,
        ruleset_key: &str,
        spec: &PanelSpec,
    ) -> Result<MessageId, InstallerError>;

    async fn edit_message(
        &self,
        channel: ChannelId,
        message: MessageId,
        guild: GuildId,
        ruleset_key: &str,
        spec: &PanelSpec,
    ) -> Result<PanelEditOutcome, InstallerError>;
}
