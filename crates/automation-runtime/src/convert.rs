use automation_core::{EventKind, RuntimeEvent};
use discord_model::{GuildId, UserId};
use twilight_model::application::interaction::{Interaction, InteractionData};

use crate::custom_id;

pub fn interaction_to_event(interaction: &Interaction) -> Option<RuntimeEvent> {
    let data = match &interaction.data {
        Some(InteractionData::MessageComponent(data)) => data,
        _ => return None,
    };
    let parsed = custom_id::decode(&data.custom_id).ok()?;
    let guild = interaction.guild_id?;
    let actor = actor_id(interaction)?;
    Some(RuntimeEvent {
        guild_id: GuildId(guild.get()),
        actor,
        kind: EventKind::ButtonClick {
            component: parsed.button_key,
        },
    })
}

fn actor_id(interaction: &Interaction) -> Option<UserId> {
    interaction
        .member
        .as_ref()
        .and_then(|member| member.user.as_ref())
        .or(interaction.user.as_ref())
        .map(|user| UserId(user.id.get()))
}
