use automation_core::AdapterErrorKind;
use automation_panel_installation::{
    InstallerError, PanelEditOutcome, PanelInstaller, PanelPresence,
};
use automation_state::{ButtonRoute, PanelSpec};
use discord_model::{ChannelId, GuildId, MessageId};
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::id::Id;

use crate::custom_id::encode_button;
use crate::error::{classify_body_error, classify_error};

pub struct TwilightPanelInstaller<'a> {
    http: &'a Client,
}

impl<'a> TwilightPanelInstaller<'a> {
    pub fn new(http: &'a Client) -> Self {
        Self { http }
    }
}

fn build_components(
    guild_id: GuildId,
    ruleset_key: &str,
    spec: &PanelSpec,
) -> Result<Vec<Component>, InstallerError> {
    let buttons = spec
        .buttons
        .iter()
        .map(|button| {
            let custom_id = match &button.route {
                ButtonRoute::Static { key } => encode_button(guild_id, ruleset_key, key),
                ButtonRoute::InstanceAction { .. } => {
                    return Err(InstallerError::new(
                        "instance action routes cannot be installed without an instance",
                    ));
                }
            };
            Ok(Component::Button(Button {
                id: None,
                custom_id: Some(custom_id),
                disabled: false,
                emoji: None,
                label: Some(button.label.clone()),
                style: ButtonStyle::Primary,
                url: None,
                sku_id: None,
            }))
        })
        .collect::<Result<Vec<_>, InstallerError>>()?;
    if buttons.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![Component::ActionRow(ActionRow {
        id: None,
        components: buttons,
    })])
}

fn request_error(error: twilight_http::Error) -> Result<(), InstallerError> {
    let classified = classify_error(&error);
    if classified.kind == AdapterErrorKind::NotFound {
        return Ok(());
    }
    Err(InstallerError::new(classified.message))
}

impl PanelInstaller for TwilightPanelInstaller<'_> {
    async fn fetch_message(
        &self,
        channel: ChannelId,
        message: MessageId,
    ) -> Result<PanelPresence, InstallerError> {
        match self
            .http
            .message(Id::new(channel.0), Id::new(message.0))
            .await
        {
            Ok(_) => Ok(PanelPresence::Present),
            Err(error) => match request_error(error) {
                Ok(()) => Ok(PanelPresence::Gone),
                Err(error) => Err(error),
            },
        }
    }

    async fn post_message(
        &self,
        channel: ChannelId,
        guild: GuildId,
        ruleset_key: &str,
        spec: &PanelSpec,
    ) -> Result<MessageId, InstallerError> {
        let components = build_components(guild, ruleset_key, spec)?;
        let message = self
            .http
            .create_message(Id::new(channel.0))
            .content(&spec.content)
            .components(&components)
            .await
            .map_err(|error| InstallerError::new(classify_error(&error).message))?
            .model()
            .await
            .map_err(|error| InstallerError::new(classify_body_error(&error).message))?;
        Ok(MessageId(message.id.get()))
    }

    async fn edit_message(
        &self,
        channel: ChannelId,
        message: MessageId,
        guild: GuildId,
        ruleset_key: &str,
        spec: &PanelSpec,
    ) -> Result<PanelEditOutcome, InstallerError> {
        let components = build_components(guild, ruleset_key, spec)?;
        match self
            .http
            .update_message(Id::new(channel.0), Id::new(message.0))
            .content(Some(spec.content.as_str()))
            .components(Some(&components))
            .await
        {
            Ok(_) => Ok(PanelEditOutcome::Updated),
            Err(error) => match request_error(error) {
                Ok(()) => Ok(PanelEditOutcome::Gone),
                Err(error) => Err(error),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_state::{ButtonRoute, ButtonSpec, InstanceRef, PanelSpec};
    use desired_state::ResourceKey;
    use twilight_model::channel::message::component::Component;

    fn panel(route: ButtonRoute) -> PanelSpec {
        PanelSpec {
            key: "entry".to_string(),
            channel: ResourceKey("hub".to_string()),
            content: "Panel".to_string(),
            buttons: vec![ButtonSpec {
                label: "Join".to_string(),
                route,
            }],
        }
    }

    #[test]
    fn static_button_uses_existing_custom_id_codec() {
        let components = build_components(
            GuildId(7),
            "studyroom",
            &panel(ButtonRoute::Static {
                key: "join".to_string(),
            }),
        )
        .unwrap();
        let Component::ActionRow(row) = &components[0] else {
            panic!("expected action row");
        };
        let Component::Button(button) = &row.components[0] else {
            panic!("expected button");
        };
        assert_eq!(
            button.custom_id.as_deref(),
            Some("starring:7:studyroom:button:join")
        );
        assert_eq!(button.label.as_deref(), Some("Join"));
    }

    #[test]
    fn instance_action_route_is_rejected() {
        let error = build_components(
            GuildId(7),
            "studyroom",
            &panel(ButtonRoute::InstanceAction {
                instance: InstanceRef::Event,
                action: "join".to_string(),
            }),
        )
        .unwrap_err();
        assert!(error.message().contains("instance action"));
    }
}
