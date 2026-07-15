use std::collections::BTreeMap;

use automation_core::{
    CreatedResource, MutationCall, PostPanelButtonSpec, ResolvedButtonRoute, ResponderCall,
    RunningRuleSetIdentity, SanitizeContext,
};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceStatus, InstanceStore,
};
use automation_state::{ActionSpec, ButtonRoute, TriggerSpec};
use resource_resolution::ResourceBindingMap;
use serde_json::json;

use crate::draft::Draft;
use crate::errors::StructuredError;

use super::super::compile::CompiledIntentV2;
use super::super::model::{ResolvedCloseControlV1, ResolvedManagedPrivateRoomV1};
use super::support::{
    bound_hub_channel, identity_error, instance_manifest_error, render_pattern, trace_error,
    CREATOR_ID, GUILD_ID, HUB_PANEL_ALIAS, MEMBER_ROLE_ALIAS, ROOM_CHANNEL_ALIAS, VIEW_CHANNEL_BIT,
    WELCOME_PANEL_ALIAS,
};
use super::RecipeKeys;

pub(super) fn assert_submit_mutations(
    calls: &[MutationCall],
    room: &ResolvedManagedPrivateRoomV1,
    keys: &RecipeKeys,
    bindings: &ResourceBindingMap,
    instance_id: &InstanceId,
    inputs: &BTreeMap<String, String>,
) -> Result<(), StructuredError> {
    let [MutationCall::CreateRole {
        name: role_name, ..
    }, MutationCall::CreateChannel {
        name: channel_name, ..
    }, MutationCall::UpsertOverwrite {
        channel: denied_channel,
        target: everyone_target,
        allow: everyone_allow,
        deny: everyone_deny,
        ..
    }, MutationCall::UpsertOverwrite {
        channel: allowed_channel,
        target: member_target,
        allow: member_allow,
        deny: member_deny,
        ..
    }, MutationCall::GrantRole {
        member: creator,
        role: granted_role,
        ..
    }, MutationCall::PostPanel {
        channel: welcome_channel,
        content: welcome_content,
        buttons: welcome_buttons,
        ..
    }, MutationCall::PostPanel {
        channel: hub_channel,
        content: hub_content,
        buttons: hub_buttons,
        ..
    }] = calls
    else {
        return Err(trace_error(
            "submit",
            "The submit trace did not perform the exact seven recipe mutations in order",
            "Restore role, channel, privacy, creator grant, welcome panel, and hub panel actions",
        ));
    };
    let expected_role_name = render_pattern(
        &room.naming.member_role_name.value,
        inputs,
        SanitizeContext::RoleName,
        "submit.member_role",
    )?;
    let expected_channel_name = render_pattern(
        &room.naming.channel_name.value,
        inputs,
        SanitizeContext::ChannelName,
        "submit.room_channel",
    )?;
    if role_name != &expected_role_name || channel_name != &expected_channel_name {
        return Err(trace_error(
            "submit",
            "Created role or channel names differ from the normalized naming intent",
            "Compile both names directly from their normalized room-name patterns",
        ));
    }
    if denied_channel != allowed_channel
        || !everyone_allow.is_empty()
        || everyone_deny.bits() != VIEW_CHANNEL_BIT
        || member_allow.bits() != VIEW_CHANNEL_BIT
        || !member_deny.is_empty()
    {
        return Err(trace_error(
            "submit",
            "The created room does not have the exact private visibility overwrites",
            "Deny view_channel for everyone and allow it for the created member role",
        ));
    }
    let everyone_json = serde_json::to_value(everyone_target).ok();
    if everyone_json != Some(json!({"type":"role","id":GUILD_ID})) {
        return Err(trace_error(
            "submit",
            "The private deny overwrite does not target the guild everyone role",
            "Use the deterministic everyone overwrite target",
        ));
    }
    let granted_role_id = granted_role.to_string();
    let member_target_json = serde_json::to_value(member_target).ok();
    if member_target_json != Some(json!({"type":"role","id":granted_role_id}))
        || creator.to_string() != CREATOR_ID
    {
        return Err(trace_error(
            "submit",
            "The creator grant or member overwrite did not use the same created member role",
            "Use one created member role for privacy and grant it to the submitting actor",
        ));
    }
    if welcome_channel != denied_channel {
        return Err(trace_error(
            "submit",
            "The welcome panel was not posted in the created private channel",
            "Post the welcome panel to the compiled room_channel reference",
        ));
    }
    let bound_hub = bound_hub_channel(room, bindings)?;
    if hub_channel.to_string() != bound_hub {
        return Err(trace_error(
            "submit",
            "The discovery panel was not posted to the normalized hub binding",
            "Resolve the hub channel from the compilation manifest binding",
        ));
    }
    let expected_welcome = render_pattern(
        &room.copy.welcome_content.value,
        inputs,
        SanitizeContext::EphemeralMessageContent,
        "submit.welcome_panel",
    )?;
    let expected_hub = render_pattern(
        &room.copy.hub_announcement.value,
        inputs,
        SanitizeContext::EphemeralMessageContent,
        "submit.hub_panel",
    )?;
    if welcome_content != &expected_welcome || hub_content != &expected_hub {
        return Err(trace_error(
            "submit",
            "Posted panel content differs from the normalized copy intent",
            "Compile welcome and discovery copy directly from normalized patterns",
        ));
    }
    let mut expected_welcome_buttons = vec![PostPanelButtonSpec {
        label: room.controls.help.label.value.clone(),
        route: ResolvedButtonRoute::Static {
            key: keys.help_button.clone(),
        },
    }];
    if let (ResolvedCloseControlV1::AnyMember { label, .. }, Some(close)) =
        (&room.controls.close, &keys.close)
    {
        expected_welcome_buttons.push(PostPanelButtonSpec {
            label: label.value.clone(),
            route: ResolvedButtonRoute::InstanceAction {
                instance_id: instance_id.clone(),
                action: close.action.clone(),
            },
        });
    }
    let expected_hub_buttons = vec![PostPanelButtonSpec {
        label: room.controls.join.label.value.clone(),
        route: ResolvedButtonRoute::InstanceAction {
            instance_id: instance_id.clone(),
            action: keys.join_action.clone(),
        },
    }];
    if welcome_buttons != &expected_welcome_buttons || hub_buttons != &expected_hub_buttons {
        return Err(trace_error(
            "submit",
            "Posted panel controls differ from the compiled recipe routes",
            "Restore exact help, join, and optional close routes from the manifest",
        ));
    }
    Ok(())
}

pub(super) fn assert_created_resources(
    created: &[CreatedResource],
    keys: &RecipeKeys,
    instance_id: &InstanceId,
) -> Result<(), StructuredError> {
    let [CreatedResource::Role { key: role, .. }, CreatedResource::Channel { key: channel, .. }, CreatedResource::Message { key: welcome, .. }, CreatedResource::Message { key: hub, .. }, CreatedResource::Instance {
        key: instance, id, ..
    }] = created
    else {
        return Err(trace_error(
            "submit",
            "The submit trace did not create the exact recipe resource set",
            "Create and register one role, channel, two panels, and one instance",
        ));
    };
    if role != &keys.member_role
        || channel != &keys.room_channel
        || welcome != &keys.welcome_panel
        || hub != &keys.hub_panel
        || instance != &keys.instance
        || id != instance_id
    {
        return Err(trace_error(
            "submit",
            "Created resources do not match the compilation manifest ownership keys",
            "Use only the generated object keys recorded by the recipe compiler",
        ));
    }
    Ok(())
}

pub(super) async fn load_and_verify_instance(
    instances: &InMemoryInstanceStore,
    identity: &RunningRuleSetIdentity,
    room: &ResolvedManagedPrivateRoomV1,
    bindings: &ResourceBindingMap,
    instance_id: &InstanceId,
    created: &[CreatedResource],
) -> Result<AutomationInstance, StructuredError> {
    let guild_id = GUILD_ID.parse().map_err(|_| identity_error())?;
    let instance = instances
        .get(guild_id, instance_id)
        .await
        .map_err(|_| instance_manifest_error())?
        .ok_or_else(instance_manifest_error)?;
    let [CreatedResource::Role {
        id: created_role, ..
    }, CreatedResource::Channel {
        id: created_channel,
        ..
    }, CreatedResource::Message {
        channel: welcome_channel,
        id: welcome_message,
        ..
    }, CreatedResource::Message {
        channel: hub_channel,
        id: hub_message,
        ..
    }, CreatedResource::Instance { .. }] = created
    else {
        return Err(instance_manifest_error());
    };
    let hub = bound_hub_channel(room, bindings)?;
    let valid_resources = instance.resources.roles.len() == 1
        && instance.resources.channels.len() == 1
        && instance.resources.messages.len() == 2
        && instance.resources.roles.get(MEMBER_ROLE_ALIAS) == Some(created_role)
        && instance.resources.channels.get(ROOM_CHANNEL_ALIAS) == Some(created_channel)
        && instance
            .resources
            .messages
            .get(WELCOME_PANEL_ALIAS)
            .is_some_and(|message| {
                message.channel == *welcome_channel
                    && message.channel == *created_channel
                    && message.id == *welcome_message
            })
        && instance
            .resources
            .messages
            .get(HUB_PANEL_ALIAS)
            .is_some_and(|message| {
                message.channel == *hub_channel
                    && message.channel.to_string() == hub
                    && message.id == *hub_message
            });
    if instance.id != *instance_id
        || instance.ruleset_key != identity.key
        || instance.ruleset_version != identity.version
        || instance.kind.0 != "study_room"
        || instance.created_by.to_string() != CREATOR_ID
        || instance.status != InstanceStatus::Active
        || !valid_resources
    {
        return Err(instance_manifest_error());
    }
    Ok(instance)
}

pub(super) fn prove_close_disabled(
    candidate: &Draft,
    compiled: &CompiledIntentV2,
) -> Result<(), StructuredError> {
    let close_action = format!("{}__close", compiled.manifest.feature_id);
    let close_rule = format!("{}__close_room", compiled.manifest.feature_id);
    let top_level_route = candidate.ruleset.panels.iter().any(|panel| {
        panel.buttons.iter().any(|button| {
            matches!(
                &button.route,
                ButtonRoute::InstanceAction { action, .. } if action == &close_action
            )
        })
    });
    let dynamic_route = candidate.ruleset.rules.iter().any(|rule| {
        rule.actions.iter().any(|action| match action {
            ActionSpec::PostPanel { buttons, .. } => buttons.iter().any(|button| {
                matches!(
                    &button.route,
                    ButtonRoute::InstanceAction { action, .. } if action == &close_action
                )
            }),
            _ => false,
        })
    });
    let handler = candidate.ruleset.rules.iter().any(|rule| {
        rule.key == close_rule
            || matches!(
                &rule.trigger,
                TriggerSpec::InstanceAction { action } if action == &close_action
            )
    });
    if top_level_route || dynamic_route || handler {
        return Err(trace_error(
            "close",
            "Disabled close policy still renders or handles the generated close action",
            "Remove both the close route and its instance-action handler",
        ));
    }
    Ok(())
}

pub(super) fn expect_responses(
    trace: &str,
    actual: &[ResponderCall],
    expected: &[ResponderCall],
) -> Result<(), StructuredError> {
    if actual != expected {
        return Err(trace_error(
            trace,
            "The interaction response sequence differs from the normalized recipe",
            "Restore the exact response lifecycle and normalized response copy",
        ));
    }
    Ok(())
}
