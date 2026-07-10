use automation_core::{AdapterError, InteractionResponder, ModalPresentation};
use automation_state::ModalFieldStyle;
use discord_model::GuildId;
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{Component, Label, TextInput, TextInputStyle};
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ApplicationMarker, InteractionMarker};
use twilight_model::id::Id;

use crate::custom_id;
use crate::error::classify_error;

pub struct TwilightInteractionResponder<'a> {
    http: &'a Client,
    application_id: Id<ApplicationMarker>,
    interaction_id: Id<InteractionMarker>,
    interaction_token: String,
    guild_id: GuildId,
    ruleset_key: String,
}

impl<'a> TwilightInteractionResponder<'a> {
    pub fn from_interaction(
        http: &'a Client,
        interaction: &Interaction,
        ruleset_key: &str,
    ) -> Self {
        Self {
            http,
            application_id: interaction.application_id,
            interaction_id: interaction.id,
            interaction_token: interaction.token.clone(),
            guild_id: GuildId(interaction.guild_id.map_or(0, |guild| guild.get())),
            ruleset_key: ruleset_key.to_string(),
        }
    }

    async fn send(&self, response: &InteractionResponse) -> Result<(), AdapterError> {
        self.http
            .interaction(self.application_id)
            .create_response(self.interaction_id, &self.interaction_token, response)
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }
}

impl InteractionResponder for TwilightInteractionResponder<'_> {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError> {
        let response = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(InteractionResponseData {
                content: Some(content),
                flags: Some(MessageFlags::EPHEMERAL),
                ..Default::default()
            }),
        };
        self.send(&response).await
    }

    #[allow(deprecated)]
    async fn open_modal(&self, modal: &ModalPresentation) -> Result<(), AdapterError> {
        let components: Vec<Component> = modal
            .fields
            .iter()
            .map(|field| {
                Component::Label(Label {
                    id: None,
                    label: field.label.clone(),
                    description: None,
                    component: Box::new(Component::TextInput(TextInput {
                        id: None,
                        custom_id: field.key.clone(),
                        label: None,
                        max_length: None,
                        min_length: None,
                        placeholder: None,
                        required: Some(field.required),
                        style: text_input_style(field.style),
                        value: None,
                    })),
                })
            })
            .collect();
        let response = InteractionResponse {
            kind: InteractionResponseType::Modal,
            data: Some(InteractionResponseData {
                custom_id: Some(custom_id::encode_modal(
                    self.guild_id,
                    &self.ruleset_key,
                    &modal.key,
                )),
                title: Some(modal.title.clone()),
                components: Some(components),
                ..Default::default()
            }),
        };
        self.send(&response).await
    }
}

fn text_input_style(style: ModalFieldStyle) -> TextInputStyle {
    match style {
        ModalFieldStyle::Short => TextInputStyle::Short,
        ModalFieldStyle::Paragraph => TextInputStyle::Paragraph,
    }
}
