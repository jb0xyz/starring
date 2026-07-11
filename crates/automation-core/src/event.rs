use std::collections::BTreeMap;

use discord_model::{GuildId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    ButtonClick {
        component: String,
    },
    ModalSubmit {
        modal: String,
        inputs: BTreeMap<String, String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeContext {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub ruleset_key: String,
    pub inputs: BTreeMap<String, String>,
}

impl RuntimeContext {
    pub fn from_event(event: &RuntimeEvent, ruleset_key: &str) -> Self {
        let inputs = match &event.kind {
            EventKind::ModalSubmit { inputs, .. } => inputs.clone(),
            EventKind::ButtonClick { .. } => BTreeMap::new(),
        };
        Self {
            guild_id: event.guild_id,
            actor: event.actor,
            ruleset_key: ruleset_key.to_string(),
            inputs,
        }
    }
}
