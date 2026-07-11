use std::collections::BTreeMap;

use automation_core::{EventKind, RuntimeEvent};
use automation_instance::InstanceId;
use discord_model::{GuildId, UserId};
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};
use twilight_model::application::interaction::{Interaction, InteractionData};

use crate::custom_id::{self, ComponentKind, ParsedCustomId};

pub fn interaction_to_event(interaction: &Interaction, ruleset_key: &str) -> Option<RuntimeEvent> {
    let guild_id = GuildId(interaction.guild_id?.get());
    let actor = actor_id(interaction)?;
    match &interaction.data {
        Some(InteractionData::MessageComponent(data)) => {
            message_component_event(guild_id, actor, &data.custom_id, ruleset_key)
        }
        Some(InteractionData::ModalSubmit(data)) => {
            let ParsedCustomId::Component {
                guild_id: parsed_guild,
                ruleset_key: parsed_ruleset,
                kind: ComponentKind::Modal,
                key,
            } = custom_id::decode(&data.custom_id).ok()?
            else {
                return None;
            };
            if !matches_context(&parsed_ruleset, parsed_guild, ruleset_key, guild_id) {
                return None;
            }
            Some(RuntimeEvent {
                guild_id,
                actor,
                kind: EventKind::ModalSubmit {
                    modal: key,
                    inputs: collect_inputs(data),
                },
            })
        }
        _ => None,
    }
}

fn message_component_event(
    guild_id: GuildId,
    actor: UserId,
    custom_id: &str,
    ruleset_key: &str,
) -> Option<RuntimeEvent> {
    match custom_id::decode(custom_id).ok()? {
        ParsedCustomId::Component {
            guild_id: parsed_guild,
            ruleset_key: parsed_ruleset,
            kind: ComponentKind::Button,
            key,
        } if matches_context(&parsed_ruleset, parsed_guild, ruleset_key, guild_id) => {
            Some(RuntimeEvent {
                guild_id,
                actor,
                kind: EventKind::ButtonClick { component: key },
            })
        }
        ParsedCustomId::InstanceAction {
            instance_id,
            action,
        } => Some(RuntimeEvent {
            guild_id,
            actor,
            kind: EventKind::InstanceAction {
                instance_id: InstanceId::parse(&instance_id).ok()?,
                action,
            },
        }),
        _ => None,
    }
}

fn matches_context(
    parsed_ruleset: &str,
    parsed_guild: GuildId,
    ruleset_key: &str,
    guild_id: GuildId,
) -> bool {
    parsed_ruleset == ruleset_key && parsed_guild == guild_id
}

fn collect_inputs(data: &ModalInteractionData) -> BTreeMap<String, String> {
    let mut inputs = BTreeMap::new();
    for component in &data.components {
        collect_text_inputs(component, &mut inputs);
    }
    inputs
}

fn collect_text_inputs(component: &ModalInteractionComponent, out: &mut BTreeMap<String, String>) {
    match component {
        ModalInteractionComponent::TextInput(input) => {
            out.insert(input.custom_id.clone(), input.value.clone());
        }
        ModalInteractionComponent::ActionRow(row) => {
            for inner in &row.components {
                collect_text_inputs(inner, out);
            }
        }
        ModalInteractionComponent::Label(label) => {
            collect_text_inputs(&label.component, out);
        }
        _ => {}
    }
}

fn actor_id(interaction: &Interaction) -> Option<UserId> {
    interaction
        .member
        .as_ref()
        .and_then(|member| member.user.as_ref())
        .or(interaction.user.as_ref())
        .map(|user| UserId(user.id.get()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_same_context() {
        assert!(matches_context(
            "studyroom_demo",
            GuildId(7),
            "studyroom_demo",
            GuildId(7)
        ));
    }

    #[test]
    fn rejects_ruleset_mismatch() {
        assert!(!matches_context(
            "other_demo",
            GuildId(7),
            "studyroom_demo",
            GuildId(7)
        ));
    }

    #[test]
    fn rejects_guild_mismatch() {
        assert!(!matches_context(
            "studyroom_demo",
            GuildId(9),
            "studyroom_demo",
            GuildId(7)
        ));
    }

    #[test]
    fn instance_action_converts_without_ruleset_guard() {
        assert_eq!(
            message_component_event(
                GuildId(7),
                UserId(42),
                "starring:i:room_001:join",
                "other_ruleset",
            ),
            Some(RuntimeEvent {
                guild_id: GuildId(7),
                actor: UserId(42),
                kind: EventKind::InstanceAction {
                    instance_id: automation_instance::InstanceId::parse("room_001").unwrap(),
                    action: "join".to_string(),
                },
            })
        );
    }

    #[test]
    fn invalid_instance_id_is_rejected() {
        assert!(message_component_event(
            GuildId(7),
            UserId(42),
            "starring:i:bad id:join",
            "studyroom_demo",
        )
        .is_none());
    }

    #[test]
    fn static_button_still_requires_context() {
        assert!(message_component_event(
            GuildId(7),
            UserId(42),
            "starring:7:other:button:help",
            "studyroom_demo",
        )
        .is_none());
    }
}
