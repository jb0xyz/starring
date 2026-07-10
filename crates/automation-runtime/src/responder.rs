use automation_core::{AdapterError, InteractionResponder};
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ApplicationMarker, InteractionMarker};
use twilight_model::id::Id;

use crate::error::classify_error;

pub struct TwilightInteractionResponder<'a> {
    http: &'a Client,
    application_id: Id<ApplicationMarker>,
    interaction_id: Id<InteractionMarker>,
    interaction_token: String,
}

impl<'a> TwilightInteractionResponder<'a> {
    pub fn from_interaction(http: &'a Client, interaction: &Interaction) -> Self {
        Self {
            http,
            application_id: interaction.application_id,
            interaction_id: interaction.id,
            interaction_token: interaction.token.clone(),
        }
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
        self.http
            .interaction(self.application_id)
            .create_response(self.interaction_id, &self.interaction_token, &response)
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }
}
