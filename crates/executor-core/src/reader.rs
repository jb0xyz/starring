use discord_model::{GuildId, GuildState};

use crate::adapter::AdapterError;

#[allow(async_fn_in_trait)]
pub trait GuildStateReader {
    async fn read_guild_state(&self, guild_id: GuildId) -> Result<GuildState, AdapterError>;
}
