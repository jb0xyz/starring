use automation_core::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    PostPanelButtonSpec, PostPanelSpec, ResolvedButtonRoute,
};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::guild::Permissions as TwilightPermissions;
use twilight_model::http::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::id::Id;

use crate::custom_id::{encode_button, encode_instance_action};
use crate::error::{classify_body_error, classify_error};

pub struct TwilightMutationAdapter<'a> {
    pub(crate) http: &'a Client,
    pub(crate) ruleset_key: String,
}

impl<'a> TwilightMutationAdapter<'a> {
    pub fn new(http: &'a Client, ruleset_key: String) -> Self {
        Self { http, ruleset_key }
    }
}

fn to_twilight_permissions(permissions: Permissions) -> TwilightPermissions {
    TwilightPermissions::from_bits_retain(permissions.bits())
}

pub(crate) fn to_permission_overwrite(
    target: OverwriteTarget,
    allow: Permissions,
    deny: Permissions,
) -> PermissionOverwrite {
    let (raw_id, kind) = match target {
        OverwriteTarget::Role(role) => (role.0, PermissionOverwriteType::Role),
        OverwriteTarget::Member(user) => (user.0, PermissionOverwriteType::Member),
    };
    PermissionOverwrite {
        allow: Some(to_twilight_permissions(allow)),
        deny: Some(to_twilight_permissions(deny)),
        id: Id::new(raw_id),
        kind,
    }
}

pub(crate) fn to_button_component(
    guild: GuildId,
    ruleset_key: &str,
    button: &PostPanelButtonSpec,
) -> Result<Component, AdapterError> {
    let custom_id = match &button.route {
        ResolvedButtonRoute::Static { key } => encode_button(guild, ruleset_key, key),
        ResolvedButtonRoute::InstanceAction {
            instance_id,
            action,
        } => encode_instance_action(instance_id.as_str(), action).map_err(|error| {
            AdapterError::new(
                AdapterErrorKind::BadRequest,
                format!("custom_id error: {error:?}"),
            )
        })?,
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
}

impl DiscordMutationAdapter for TwilightMutationAdapter<'_> {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError> {
        self.http
            .add_guild_member_role(Id::new(guild.0), Id::new(member.0), Id::new(role.0))
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }

    async fn create_role(
        &self,
        guild: GuildId,
        spec: CreateRoleSpec,
    ) -> Result<RoleId, AdapterError> {
        let role = self
            .http
            .create_role(Id::new(guild.0))
            .name(&spec.name)
            .permissions(TwilightPermissions::empty())
            .await
            .map_err(|error| classify_error(&error))?
            .model()
            .await
            .map_err(|error| classify_body_error(&error))?;
        Ok(RoleId(role.id.get()))
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        let channel = self
            .http
            .create_guild_channel(Id::new(guild.0), &spec.name)
            .await
            .map_err(|error| classify_error(&error))?
            .model()
            .await
            .map_err(|error| classify_body_error(&error))?;
        Ok(ChannelId(channel.id.get()))
    }

    async fn upsert_overwrite(
        &self,
        _guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        let overwrite = to_permission_overwrite(target, allow, deny);
        self.http
            .update_channel_permission(Id::new(channel.0), &overwrite)
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }

    async fn post_panel(
        &self,
        guild: GuildId,
        channel: ChannelId,
        spec: PostPanelSpec,
    ) -> Result<MessageId, AdapterError> {
        let buttons: Vec<Component> = spec
            .buttons
            .iter()
            .map(|button| to_button_component(guild, &self.ruleset_key, button))
            .collect::<Result<Vec<Component>, AdapterError>>()?;
        let components = [Component::ActionRow(ActionRow {
            id: None,
            components: buttons,
        })];
        let message = self
            .http
            .create_message(Id::new(channel.0))
            .content(&spec.content)
            .components(&components)
            .await
            .map_err(|error| classify_error(&error))?
            .model()
            .await
            .map_err(|error| classify_body_error(&error))?;
        Ok(MessageId(message.id.get()))
    }
}
