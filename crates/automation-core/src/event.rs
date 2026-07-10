use discord_model::{GuildId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    ButtonClick { component: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeContext {
    pub guild_id: GuildId,
    pub actor: UserId,
}

impl RuntimeContext {
    pub fn from_event(event: &RuntimeEvent) -> Self {
        Self {
            guild_id: event.guild_id,
            actor: event.actor,
        }
    }
}
