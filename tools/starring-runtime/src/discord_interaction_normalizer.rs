use std::fmt::{Debug, Display, Formatter};

use automation_runtime::{
    SharedGatewayInteractionApplicationIdV3, SharedGatewayInteractionEnvelopeErrorV3,
    SharedGatewayInteractionEnvelopeV3, SharedGatewayInteractionIdV3,
    SharedGatewayInteractionIdentityV3, SharedGatewayInteractionTokenV3, SharedGatewayModalInputV3,
};
use discord_model::{ChannelId, GuildId, UserId};
use paused_discord_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use paused_discord_model::application::interaction::modal::ModalInteractionComponent;
use paused_discord_model::application::interaction::{
    Interaction, InteractionData, InteractionType,
};
use paused_discord_model::channel::message::component::ComponentType;
use zeroize::Zeroize;

const MAX_RUNTIME_DISCORD_MODAL_DEPTH_V1: usize = 8;
const MAX_RUNTIME_DISCORD_MODAL_COMPONENTS_V1: usize = 32;

pub(crate) enum RuntimeDiscordInteractionNormalizationOutcomeV1 {
    Normalized(Box<SharedGatewayInteractionEnvelopeV3>),
    Ignored(RuntimeDiscordInteractionIgnoredV1),
    Rejected(RuntimeDiscordInteractionNormalizationErrorV1),
}

impl Debug for RuntimeDiscordInteractionNormalizationOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordInteractionNormalizationOutcomeV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordInteractionIgnoredV1 {
    DirectMessage,
    UnsupportedInteraction,
    UnsupportedMessageComponent,
    UnsupportedModalComponent,
}

impl RuntimeDiscordInteractionIgnoredV1 {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::DirectMessage => "runtime_discord_interaction_direct_message_ignored",
            Self::UnsupportedInteraction => "runtime_discord_interaction_kind_unsupported",
            Self::UnsupportedMessageComponent => "runtime_discord_message_component_unsupported",
            Self::UnsupportedModalComponent => "runtime_discord_modal_component_unsupported",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordInteractionNormalizationErrorV1 {
    MissingGuildIdentity,
    ConflictingGuildIdentity,
    MissingChannelIdentity,
    ConflictingChannelIdentity,
    MissingUserIdentity,
    ConflictingUserIdentity,
    InteractionDataShape,
    ModalStructure,
    Envelope(SharedGatewayInteractionEnvelopeErrorV3),
}

impl RuntimeDiscordInteractionNormalizationErrorV1 {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::MissingGuildIdentity => "runtime_discord_interaction_guild_identity_missing",
            Self::ConflictingGuildIdentity => {
                "runtime_discord_interaction_guild_identity_conflicting"
            }
            Self::MissingChannelIdentity => "runtime_discord_interaction_channel_identity_missing",
            Self::ConflictingChannelIdentity => {
                "runtime_discord_interaction_channel_identity_conflicting"
            }
            Self::MissingUserIdentity => "runtime_discord_interaction_user_identity_missing",
            Self::ConflictingUserIdentity => {
                "runtime_discord_interaction_user_identity_conflicting"
            }
            Self::InteractionDataShape => "runtime_discord_interaction_data_shape_invalid",
            Self::ModalStructure => "runtime_discord_modal_structure_invalid",
            Self::Envelope(error) => error.code(),
        }
    }
}

impl Debug for RuntimeDiscordInteractionNormalizationErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordInteractionNormalizationErrorV1(<redacted>)")
    }
}

impl Display for RuntimeDiscordInteractionNormalizationErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for RuntimeDiscordInteractionNormalizationErrorV1 {}

impl From<SharedGatewayInteractionEnvelopeErrorV3>
    for RuntimeDiscordInteractionNormalizationErrorV1
{
    fn from(error: SharedGatewayInteractionEnvelopeErrorV3) -> Self {
        Self::Envelope(error)
    }
}

enum RuntimeDiscordInteractionNormalizationDecisionV1 {
    Normalized(SharedGatewayInteractionEnvelopeV3),
    Ignored(RuntimeDiscordInteractionIgnoredV1),
}

pub(crate) struct ZeroizingPinnedDiscordInteractionV1(Interaction);

pub(crate) fn pin_runtime_discord_interaction_v1(
    interaction: Interaction,
) -> Box<ZeroizingPinnedDiscordInteractionV1> {
    Box::new(ZeroizingPinnedDiscordInteractionV1(interaction))
}

impl Drop for ZeroizingPinnedDiscordInteractionV1 {
    fn drop(&mut self) {
        zeroize_pinned_discord_interaction_v1(&mut self.0);
    }
}

pub(crate) fn normalize_runtime_discord_interaction_v1(
    interaction: Interaction,
) -> RuntimeDiscordInteractionNormalizationOutcomeV1 {
    normalize_pinned_runtime_discord_interaction_v1(pin_runtime_discord_interaction_v1(interaction))
}

pub(crate) fn normalize_pinned_runtime_discord_interaction_v1(
    mut interaction: Box<ZeroizingPinnedDiscordInteractionV1>,
) -> RuntimeDiscordInteractionNormalizationOutcomeV1 {
    match normalize_guarded_runtime_discord_interaction_v1(&mut interaction.0) {
        Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Normalized(envelope)) => {
            RuntimeDiscordInteractionNormalizationOutcomeV1::Normalized(Box::new(envelope))
        }
        Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Ignored(reason)) => {
            RuntimeDiscordInteractionNormalizationOutcomeV1::Ignored(reason)
        }
        Err(error) => RuntimeDiscordInteractionNormalizationOutcomeV1::Rejected(error),
    }
}

fn normalize_guarded_runtime_discord_interaction_v1(
    interaction: &mut Interaction,
) -> Result<
    RuntimeDiscordInteractionNormalizationDecisionV1,
    RuntimeDiscordInteractionNormalizationErrorV1,
> {
    match interaction.kind {
        InteractionType::MessageComponent => {
            let Some(InteractionData::MessageComponent(data)) = interaction.data.as_ref() else {
                return Err(RuntimeDiscordInteractionNormalizationErrorV1::InteractionDataShape);
            };
            if data.component_type != ComponentType::Button {
                return Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Ignored(
                    RuntimeDiscordInteractionIgnoredV1::UnsupportedMessageComponent,
                ));
            }
        }
        InteractionType::ModalSubmit => {
            if !matches!(
                interaction.data.as_ref(),
                Some(InteractionData::ModalSubmit(_))
            ) {
                return Err(RuntimeDiscordInteractionNormalizationErrorV1::InteractionDataShape);
            }
        }
        _ => {
            return Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Ignored(
                RuntimeDiscordInteractionIgnoredV1::UnsupportedInteraction,
            ));
        }
    }

    let Some(guild_id) = resolve_runtime_discord_guild_id_v1(interaction)? else {
        return Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Ignored(
            RuntimeDiscordInteractionIgnoredV1::DirectMessage,
        ));
    };
    let channel_id = resolve_runtime_discord_channel_id_v1(interaction)?;
    let user_id = resolve_runtime_discord_user_id_v1(interaction)?;
    let identity = SharedGatewayInteractionIdentityV3::new(
        GuildId(guild_id),
        ChannelId(channel_id),
        UserId(user_id),
        SharedGatewayInteractionApplicationIdV3::new(interaction.application_id.get())?,
        SharedGatewayInteractionIdV3::new(interaction.id.get())?,
    )?;

    match interaction.kind {
        InteractionType::MessageComponent => {
            let custom_id = match interaction.data.as_ref() {
                Some(InteractionData::MessageComponent(data)) => data.custom_id.clone(),
                _ => {
                    return Err(
                        RuntimeDiscordInteractionNormalizationErrorV1::InteractionDataShape,
                    );
                }
            };
            let token =
                SharedGatewayInteractionTokenV3::new(std::mem::take(&mut interaction.token))?;
            let envelope = SharedGatewayInteractionEnvelopeV3::message_component_v3(
                identity,
                custom_id,
                interaction.locale.take(),
                token,
            )?;
            Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Normalized(envelope))
        }
        InteractionType::ModalSubmit => {
            let (custom_id, inputs) = match interaction.data.as_mut() {
                Some(InteractionData::ModalSubmit(data)) => {
                    let mut inputs = Vec::new();
                    let mut component_count = 0;
                    let supported = collect_runtime_discord_modal_inputs_v1(
                        &mut data.components,
                        &mut inputs,
                        0,
                        &mut component_count,
                    )?;
                    if !supported {
                        return Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Ignored(
                            RuntimeDiscordInteractionIgnoredV1::UnsupportedModalComponent,
                        ));
                    }
                    (data.custom_id.clone(), inputs)
                }
                _ => {
                    return Err(
                        RuntimeDiscordInteractionNormalizationErrorV1::InteractionDataShape,
                    );
                }
            };
            let token =
                SharedGatewayInteractionTokenV3::new(std::mem::take(&mut interaction.token))?;
            let envelope = SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
                identity,
                custom_id,
                inputs,
                interaction.locale.take(),
                token,
            )?;
            Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Normalized(envelope))
        }
        _ => Ok(RuntimeDiscordInteractionNormalizationDecisionV1::Ignored(
            RuntimeDiscordInteractionIgnoredV1::UnsupportedInteraction,
        )),
    }
}

fn resolve_runtime_discord_guild_id_v1(
    interaction: &Interaction,
) -> Result<Option<u64>, RuntimeDiscordInteractionNormalizationErrorV1> {
    let direct = interaction.guild_id.map(|id| id.get());
    let partial = interaction
        .guild
        .as_ref()
        .and_then(|guild| guild.id)
        .map(|id| id.get());
    if matches!((direct, partial), (Some(left), Some(right)) if left != right) {
        return Err(RuntimeDiscordInteractionNormalizationErrorV1::ConflictingGuildIdentity);
    }
    let resolved = direct.or(partial);
    if resolved.is_none() && (interaction.member.is_some() || interaction.guild.is_some()) {
        return Err(RuntimeDiscordInteractionNormalizationErrorV1::MissingGuildIdentity);
    }
    Ok(resolved)
}

fn resolve_runtime_discord_channel_id_v1(
    interaction: &Interaction,
) -> Result<u64, RuntimeDiscordInteractionNormalizationErrorV1> {
    let direct = interaction.channel.as_ref().map(|channel| channel.id.get());
    #[allow(deprecated)]
    let legacy = interaction.channel_id.map(|id| id.get());
    if matches!((direct, legacy), (Some(left), Some(right)) if left != right) {
        return Err(RuntimeDiscordInteractionNormalizationErrorV1::ConflictingChannelIdentity);
    }
    direct
        .or(legacy)
        .ok_or(RuntimeDiscordInteractionNormalizationErrorV1::MissingChannelIdentity)
}

fn resolve_runtime_discord_user_id_v1(
    interaction: &Interaction,
) -> Result<u64, RuntimeDiscordInteractionNormalizationErrorV1> {
    let member = interaction
        .member
        .as_ref()
        .and_then(|member| member.user.as_ref())
        .map(|user| user.id.get());
    let direct = interaction.user.as_ref().map(|user| user.id.get());
    if matches!((member, direct), (Some(left), Some(right)) if left != right) {
        return Err(RuntimeDiscordInteractionNormalizationErrorV1::ConflictingUserIdentity);
    }
    member
        .or(direct)
        .ok_or(RuntimeDiscordInteractionNormalizationErrorV1::MissingUserIdentity)
}

fn collect_runtime_discord_modal_inputs_v1(
    components: &mut [ModalInteractionComponent],
    inputs: &mut Vec<SharedGatewayModalInputV3>,
    depth: usize,
    component_count: &mut usize,
) -> Result<bool, RuntimeDiscordInteractionNormalizationErrorV1> {
    if depth > MAX_RUNTIME_DISCORD_MODAL_DEPTH_V1 {
        return Err(RuntimeDiscordInteractionNormalizationErrorV1::ModalStructure);
    }
    for component in components {
        *component_count = component_count
            .checked_add(1)
            .ok_or(RuntimeDiscordInteractionNormalizationErrorV1::ModalStructure)?;
        if *component_count > MAX_RUNTIME_DISCORD_MODAL_COMPONENTS_V1 {
            return Err(RuntimeDiscordInteractionNormalizationErrorV1::ModalStructure);
        }
        match component {
            ModalInteractionComponent::TextInput(input) => {
                inputs.push(SharedGatewayModalInputV3::new(
                    input.id,
                    input.custom_id.clone(),
                    std::mem::take(&mut input.value),
                )?);
            }
            ModalInteractionComponent::ActionRow(row) => {
                if !collect_runtime_discord_modal_inputs_v1(
                    &mut row.components,
                    inputs,
                    depth + 1,
                    component_count,
                )? {
                    return Ok(false);
                }
            }
            ModalInteractionComponent::Label(label) => {
                if !collect_runtime_discord_modal_inputs_v1(
                    std::slice::from_mut(label.component.as_mut()),
                    inputs,
                    depth + 1,
                    component_count,
                )? {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn zeroize_pinned_discord_interaction_v1(interaction: &mut Interaction) {
    interaction.token.zeroize();
    match interaction.data.as_mut() {
        Some(InteractionData::ApplicationCommand(data)) => {
            zeroize_pinned_discord_command_options_v1(&mut data.options);
        }
        Some(InteractionData::MessageComponent(data)) => {
            for value in &mut data.values {
                value.zeroize();
            }
        }
        Some(InteractionData::ModalSubmit(data)) => {
            zeroize_pinned_discord_modal_components_v1(&mut data.components);
        }
        _ => {}
    }
}

fn zeroize_pinned_discord_command_options_v1(options: &mut Vec<CommandDataOption>) {
    let mut ancestors = Vec::new();
    let mut current = std::mem::take(options);
    'tree: loop {
        while let Some(mut option) = current.pop() {
            if let Some(nested) =
                take_pinned_discord_command_children_after_zeroize_v1(&mut option.value)
            {
                if !nested.is_empty() {
                    ancestors.push(current);
                    current = nested;
                    continue 'tree;
                }
            }
        }
        let Some(parent) = ancestors.pop() else {
            break;
        };
        current = parent;
    }
}

fn take_pinned_discord_command_children_after_zeroize_v1(
    value: &mut CommandOptionValue,
) -> Option<Vec<CommandDataOption>> {
    match value {
        CommandOptionValue::Focused(value, _) | CommandOptionValue::String(value) => {
            value.zeroize();
            None
        }
        CommandOptionValue::SubCommand(options) | CommandOptionValue::SubCommandGroup(options) => {
            Some(std::mem::take(options))
        }
        _ => None,
    }
}

fn zeroize_pinned_discord_modal_components_v1(components: &mut [ModalInteractionComponent]) {
    for component in components {
        match component {
            ModalInteractionComponent::TextInput(input) => input.value.zeroize(),
            ModalInteractionComponent::ActionRow(row) => {
                zeroize_pinned_discord_modal_components_v1(&mut row.components);
            }
            ModalInteractionComponent::Label(label) => {
                zeroize_pinned_discord_modal_components_v1(std::slice::from_mut(
                    label.component.as_mut(),
                ));
            }
            ModalInteractionComponent::StringSelect(select) => {
                for value in &mut select.values {
                    value.zeroize();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use automation_runtime::{
        SharedGatewayInteractionEnvelopeErrorV3, SharedGatewayInteractionKindV3,
        MAX_SHARED_GATEWAY_CUSTOM_ID_BYTES_V3, MAX_SHARED_GATEWAY_MODAL_INPUT_VALUE_BYTES_V3,
    };
    use paused_discord_model::application::command::{CommandOptionType, CommandType};
    use paused_discord_model::application::interaction::application_command::CommandData;
    use paused_discord_model::application::interaction::message_component::MessageComponentInteractionData;
    use paused_discord_model::application::interaction::modal::{
        ModalInteractionActionRow, ModalInteractionData, ModalInteractionLabel,
        ModalInteractionTextDisplay, ModalInteractionTextInput,
    };
    use paused_discord_model::id::marker::{
        ApplicationMarker, ChannelMarker, CommandMarker, GuildMarker, InteractionMarker, UserMarker,
    };
    use paused_discord_model::id::Id;
    use paused_discord_model::oauth::ApplicationIntegrationMap;
    use paused_discord_model::user::User;

    use super::*;

    fn user(id: u64) -> User {
        User {
            accent_color: None,
            avatar: None,
            avatar_decoration: None,
            avatar_decoration_data: None,
            banner: None,
            bot: false,
            discriminator: 0,
            email: None,
            flags: None,
            global_name: None,
            id: Id::<UserMarker>::new(id),
            locale: None,
            mfa_enabled: None,
            name: String::new(),
            premium_type: None,
            primary_guild: None,
            public_flags: None,
            system: None,
            verified: None,
        }
    }

    #[allow(deprecated)]
    fn interaction(
        kind: InteractionType,
        data: Option<InteractionData>,
        guild_id: Option<u64>,
    ) -> Interaction {
        Interaction {
            app_permissions: None,
            application_id: Id::<ApplicationMarker>::new(41),
            authorizing_integration_owners: ApplicationIntegrationMap {
                guild: None,
                user: None,
            },
            channel: None,
            channel_id: Some(Id::<ChannelMarker>::new(43)),
            context: None,
            data,
            entitlements: Vec::new(),
            guild: None,
            guild_id: guild_id.map(Id::<GuildMarker>::new),
            guild_locale: None,
            id: Id::<InteractionMarker>::new(47),
            kind,
            locale: Some("ko".to_string()),
            member: None,
            message: None,
            token: "interaction-token-secret".to_string(),
            user: Some(user(53)),
        }
    }

    fn button(component_type: ComponentType, custom_id: String) -> Interaction {
        interaction(
            InteractionType::MessageComponent,
            Some(InteractionData::MessageComponent(Box::new(
                MessageComponentInteractionData {
                    custom_id,
                    component_type,
                    resolved: None,
                    values: Vec::new(),
                },
            ))),
            Some(42),
        )
    }

    fn text_input(id: i32, custom_id: &str, value: &str) -> ModalInteractionComponent {
        ModalInteractionComponent::TextInput(ModalInteractionTextInput {
            custom_id: custom_id.to_string(),
            id,
            value: value.to_string(),
        })
    }

    fn nested_modal(
        first_custom_id: &str,
        second_custom_id: &str,
        first_value: &str,
    ) -> Interaction {
        interaction(
            InteractionType::ModalSubmit,
            Some(InteractionData::ModalSubmit(Box::new(
                ModalInteractionData {
                    components: vec![ModalInteractionComponent::ActionRow(
                        ModalInteractionActionRow {
                            id: 1,
                            components: vec![
                                ModalInteractionComponent::Label(ModalInteractionLabel {
                                    id: 2,
                                    component: Box::new(text_input(
                                        3,
                                        first_custom_id,
                                        first_value,
                                    )),
                                }),
                                text_input(4, second_custom_id, "second-secret"),
                            ],
                        },
                    )],
                    custom_id: "submit_room".to_string(),
                    resolved: None,
                },
            ))),
            Some(42),
        )
    }

    fn application_command(options: Vec<CommandDataOption>) -> Interaction {
        interaction(
            InteractionType::ApplicationCommand,
            Some(InteractionData::ApplicationCommand(Box::new(CommandData {
                guild_id: Some(Id::<GuildMarker>::new(42)),
                id: Id::<CommandMarker>::new(59),
                name: "ignored".to_string(),
                kind: CommandType::ChatInput,
                options,
                resolved: None,
                target_id: None,
            }))),
            Some(42),
        )
    }

    fn deeply_nested_command_options(depth: usize) -> Vec<CommandDataOption> {
        let mut value = CommandOptionValue::String("deep-command-secret".to_string());
        for index in 0..depth {
            let option = CommandDataOption {
                name: format!("level_{index}"),
                value,
            };
            value = if index % 2 == 0 {
                CommandOptionValue::SubCommand(vec![option])
            } else {
                CommandOptionValue::SubCommandGroup(vec![option])
            };
        }
        vec![CommandDataOption {
            name: "root".to_string(),
            value,
        }]
    }

    #[test]
    fn exact_button_identity_and_payload_are_normalized() {
        let outcome =
            normalize_runtime_discord_interaction_v1(button(ComponentType::Button, "join".into()));
        let RuntimeDiscordInteractionNormalizationOutcomeV1::Normalized(envelope) = outcome else {
            panic!("expected normalized interaction");
        };
        let identity = envelope.identity_v3();
        assert_eq!(identity.guild_id(), GuildId(42));
        assert_eq!(identity.channel_id(), ChannelId(43));
        assert_eq!(identity.user_id(), UserId(53));
        assert_eq!(identity.application_id().get(), 41);
        assert_eq!(identity.interaction_id().get(), 47);
        assert_eq!(
            envelope.kind_v3(),
            SharedGatewayInteractionKindV3::MessageComponent
        );
        assert_eq!(envelope.custom_id_v3(), "join");
        assert_eq!(envelope.locale_v3(), Some("ko"));
    }

    #[test]
    fn nested_action_row_and_label_modal_inputs_are_normalized() {
        let outcome = normalize_runtime_discord_interaction_v1(nested_modal(
            "room_name",
            "topic",
            "first-secret",
        ));
        let RuntimeDiscordInteractionNormalizationOutcomeV1::Normalized(envelope) = outcome else {
            panic!("expected normalized interaction");
        };
        assert_eq!(
            envelope.kind_v3(),
            SharedGatewayInteractionKindV3::ModalSubmit
        );
        assert_eq!(envelope.custom_id_v3(), "submit_room");
    }

    #[test]
    fn nested_duplicate_modal_inputs_prove_recursive_collection() {
        let outcome = normalize_runtime_discord_interaction_v1(nested_modal(
            "room_name",
            "room_name",
            "first-secret",
        ));
        let RuntimeDiscordInteractionNormalizationOutcomeV1::Rejected(error) = outcome else {
            panic!("expected rejected interaction");
        };
        assert_eq!(
            error,
            RuntimeDiscordInteractionNormalizationErrorV1::Envelope(
                SharedGatewayInteractionEnvelopeErrorV3::DuplicateModalInput
            )
        );
    }

    #[test]
    fn direct_messages_and_unsupported_components_are_ignored() {
        let mut direct_message = button(ComponentType::Button, "join".into());
        direct_message.guild_id = None;
        let direct_message = normalize_runtime_discord_interaction_v1(direct_message);
        let RuntimeDiscordInteractionNormalizationOutcomeV1::Ignored(reason) = direct_message
        else {
            panic!("expected ignored interaction");
        };
        assert_eq!(reason, RuntimeDiscordInteractionIgnoredV1::DirectMessage);
        assert_eq!(
            reason.code(),
            "runtime_discord_interaction_direct_message_ignored"
        );
        assert!(matches!(
            normalize_runtime_discord_interaction_v1(button(
                ComponentType::TextSelectMenu,
                "join".into()
            )),
            RuntimeDiscordInteractionNormalizationOutcomeV1::Ignored(
                RuntimeDiscordInteractionIgnoredV1::UnsupportedMessageComponent
            )
        ));
        assert!(matches!(
            normalize_runtime_discord_interaction_v1(interaction(
                InteractionType::Ping,
                None,
                Some(42)
            )),
            RuntimeDiscordInteractionNormalizationOutcomeV1::Ignored(
                RuntimeDiscordInteractionIgnoredV1::UnsupportedInteraction
            )
        ));
        let unsupported_modal = interaction(
            InteractionType::ModalSubmit,
            Some(InteractionData::ModalSubmit(Box::new(
                ModalInteractionData {
                    components: vec![ModalInteractionComponent::TextDisplay(
                        ModalInteractionTextDisplay { id: 1 },
                    )],
                    custom_id: "submit_room".to_string(),
                    resolved: None,
                },
            ))),
            Some(42),
        );
        assert!(matches!(
            normalize_runtime_discord_interaction_v1(unsupported_modal),
            RuntimeDiscordInteractionNormalizationOutcomeV1::Ignored(
                RuntimeDiscordInteractionIgnoredV1::UnsupportedModalComponent
            )
        ));
    }

    #[test]
    fn malformed_and_bounded_payloads_have_stable_rejections() {
        let outcome = normalize_runtime_discord_interaction_v1(button(
            ComponentType::Button,
            "x".repeat(MAX_SHARED_GATEWAY_CUSTOM_ID_BYTES_V3 + 1),
        ));
        let RuntimeDiscordInteractionNormalizationOutcomeV1::Rejected(error) = outcome else {
            panic!("expected rejected interaction");
        };
        assert_eq!(error.code(), "shared_gateway_interaction_custom_id_invalid");

        let outcome = normalize_runtime_discord_interaction_v1(nested_modal(
            "room_name",
            "topic",
            &"x".repeat(MAX_SHARED_GATEWAY_MODAL_INPUT_VALUE_BYTES_V3 + 1),
        ));
        let RuntimeDiscordInteractionNormalizationOutcomeV1::Rejected(error) = outcome else {
            panic!("expected rejected interaction");
        };
        assert_eq!(error.code(), "shared_gateway_modal_input_invalid");
    }

    #[test]
    fn debug_output_is_redacted_for_success_and_rejection() {
        let success = normalize_runtime_discord_interaction_v1(button(
            ComponentType::Button,
            "secret-id".into(),
        ));
        let success_debug = format!("{success:?}");
        assert!(!success_debug.contains("interaction-token-secret"));
        assert!(!success_debug.contains("secret-id"));

        let rejection = normalize_runtime_discord_interaction_v1(button(
            ComponentType::Button,
            "x".repeat(MAX_SHARED_GATEWAY_CUSTOM_ID_BYTES_V3 + 1),
        ));
        let rejection_debug = format!("{rejection:?}");
        assert!(!rejection_debug.contains("interaction-token-secret"));
        assert!(!rejection_debug.contains(&"x".repeat(32)));
    }

    #[test]
    fn zeroization_seam_clears_token_and_nested_modal_values() {
        let mut interaction = nested_modal("room_name", "topic", "first-secret");
        zeroize_pinned_discord_interaction_v1(&mut interaction);
        assert!(interaction.token.is_empty());
        let Some(InteractionData::ModalSubmit(data)) = interaction.data.as_ref() else {
            panic!("expected modal data");
        };
        let ModalInteractionComponent::ActionRow(row) = &data.components[0] else {
            panic!("expected action row");
        };
        let ModalInteractionComponent::Label(label) = &row.components[0] else {
            panic!("expected label");
        };
        let ModalInteractionComponent::TextInput(first) = label.component.as_ref() else {
            panic!("expected first input");
        };
        let ModalInteractionComponent::TextInput(second) = &row.components[1] else {
            panic!("expected second input");
        };
        assert!(first.value.is_empty());
        assert!(second.value.is_empty());
    }

    #[test]
    fn ignored_command_values_are_zeroized_without_recursive_drop() {
        let mut string = CommandOptionValue::String("string-secret".to_string());
        assert!(take_pinned_discord_command_children_after_zeroize_v1(&mut string).is_none());
        let CommandOptionValue::String(string) = string else {
            panic!("expected string option");
        };
        assert!(string.is_empty());

        let mut focused =
            CommandOptionValue::Focused("focused-secret".to_string(), CommandOptionType::String);
        assert!(take_pinned_discord_command_children_after_zeroize_v1(&mut focused).is_none());
        let CommandOptionValue::Focused(focused, _) = focused else {
            panic!("expected focused option");
        };
        assert!(focused.is_empty());

        let outcome = normalize_runtime_discord_interaction_v1(application_command(
            deeply_nested_command_options(4_096),
        ));
        assert!(matches!(
            outcome,
            RuntimeDiscordInteractionNormalizationOutcomeV1::Ignored(
                RuntimeDiscordInteractionIgnoredV1::UnsupportedInteraction
            )
        ));
    }
}
