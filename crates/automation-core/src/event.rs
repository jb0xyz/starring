use std::collections::BTreeMap;

use automation_instance::{AutomationInstance, InstanceId, InstanceRuleSetVersion};
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
    InstanceAction {
        instance_id: InstanceId,
        action: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInstanceContext {
    pub instance: AutomationInstance,
    pub action: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningRuleSetIdentity {
    pub key: String,
    pub version: InstanceRuleSetVersion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeContext {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub ruleset_key: String,
    pub ruleset_version: InstanceRuleSetVersion,
    pub inputs: BTreeMap<String, String>,
    pub instance: Option<ResolvedInstanceContext>,
}

impl RuntimeContext {
    pub fn from_event(event: &RuntimeEvent, identity: &RunningRuleSetIdentity) -> Self {
        let inputs = match &event.kind {
            EventKind::ModalSubmit { inputs, .. } => inputs.clone(),
            EventKind::ButtonClick { .. } | EventKind::InstanceAction { .. } => BTreeMap::new(),
        };
        Self {
            guild_id: event.guild_id,
            actor: event.actor,
            ruleset_key: identity.key.clone(),
            ruleset_version: identity.version,
            inputs,
            instance: None,
        }
    }
}
