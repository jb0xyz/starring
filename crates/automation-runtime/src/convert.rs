use std::collections::BTreeMap;

use automation_core::{EventKind, RuntimeEvent};
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
            let parsed = custom_id::decode(&data.custom_id).ok()?;
            if parsed.kind != ComponentKind::Button {
                return None;
            }
            if !matches_context(&parsed, ruleset_key, guild_id) {
                return None;
            }
            Some(RuntimeEvent {
                guild_id,
                actor,
                kind: EventKind::ButtonClick {
                    component: parsed.key,
                },
            })
        }
        Some(InteractionData::ModalSubmit(data)) => {
            let parsed = custom_id::decode(&data.custom_id).ok()?;
            if parsed.kind != ComponentKind::Modal {
                return None;
            }
            if !matches_context(&parsed, ruleset_key, guild_id) {
                return None;
            }
            Some(RuntimeEvent {
                guild_id,
                actor,
                kind: EventKind::ModalSubmit {
                    modal: parsed.key,
                    inputs: collect_inputs(data),
                },
            })
        }
        _ => None,
    }
}

fn matches_context(parsed: &ParsedCustomId, ruleset_key: &str, guild_id: GuildId) -> bool {
    parsed.ruleset_key == ruleset_key && parsed.guild_id == guild_id
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

    fn parsed(ruleset: &str, guild: u64) -> ParsedCustomId {
        ParsedCustomId {
            guild_id: GuildId(guild),
            ruleset_key: ruleset.to_string(),
            kind: ComponentKind::Button,
            key: "study_help".to_string(),
        }
    }

    #[test]
    fn matches_same_context() {
        assert!(matches_context(
            &parsed("studyroom_demo", 7),
            "studyroom_demo",
            GuildId(7)
        ));
    }

    #[test]
    fn rejects_ruleset_mismatch() {
        assert!(!matches_context(
            &parsed("other_demo", 7),
            "studyroom_demo",
            GuildId(7)
        ));
    }

    #[test]
    fn rejects_guild_mismatch() {
        assert!(!matches_context(
            &parsed("studyroom_demo", 9),
            "studyroom_demo",
            GuildId(7)
        ));
    }
}
