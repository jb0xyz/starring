use std::collections::BTreeMap;

use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef, InstanceKind,
    InstanceRef, InstanceResourceRefs, ModalFieldSpec, ModalFieldStyle, ModalInputPolicy,
    ModalSpec, OverwriteTargetSpec, PanelSpec, RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::Permissions;
use serde::{Deserialize, Serialize};

pub const AUTOMATION_SPEC_SCHEMA_VERSION_V1: u16 = 1;
pub const AUTOMATION_SPEC_KIND_V1: &str = "starring.automation-spec.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSpecV1 {
    pub schema_version: u16,
    pub kind: String,
    pub key: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub panels: Vec<DeclaredPanelV1>,
    #[serde(default)]
    pub modals: Vec<ModalDefinitionV1>,
    pub workflows: Vec<WorkflowSpecV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredPanelV1 {
    pub id: String,
    pub channel: String,
    pub content: String,
    #[serde(default)]
    pub buttons: Vec<DeclaredButtonV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredButtonV1 {
    pub label: String,
    pub trigger_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalDefinitionV1 {
    pub id: String,
    pub title: String,
    pub fields: Vec<ModalFieldDefinitionV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalFieldDefinitionV1 {
    pub id: String,
    pub label: String,
    pub style: ModalFieldStyleV1,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u16>,
    #[serde(default, skip_serializing_if = "ModalInputPolicyV1::is_preserve")]
    pub input_policy: ModalInputPolicyV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalFieldStyleV1 {
    Short,
    Paragraph,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalInputPolicyV1 {
    #[default]
    Preserve,
    TrimUnicodeWhitespace,
}

impl ModalInputPolicyV1 {
    fn is_preserve(&self) -> bool {
        matches!(self, Self::Preserve)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpecV1 {
    pub id: String,
    pub trigger: TriggerV1,
    #[serde(default)]
    pub condition: ConditionExprV1,
    pub actions: Vec<ActionNodeV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionNodeV1 {
    pub id: String,
    pub action: ActionV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerV1 {
    ButtonClick { trigger_id: String },
    ModalSubmit { modal_id: String },
    InstanceAction { action_id: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConditionExprV1 {
    #[default]
    Always,
    InputNonEmpty {
        input_id: String,
    },
    InputEquals {
        input_id: String,
        value: String,
    },
    All {
        conditions: Vec<ConditionExprV1>,
    },
    Any {
        conditions: Vec<ConditionExprV1>,
    },
    Not {
        condition: Box<ConditionExprV1>,
    },
}

impl ConditionExprV1 {
    pub fn is_unconditional(&self) -> bool {
        matches!(self, Self::Always)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionV1 {
    GrantRole {
        role: RoleReferenceV1,
        target: ActionTargetV1,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal {
        modal_id: String,
    },
    CreateChannel {
        output: String,
        name: String,
    },
    CreateRole {
        output: String,
        name: String,
    },
    UpsertOverwrite {
        channel: ChannelReferenceV1,
        target: OverwriteTargetV1,
        #[serde(default)]
        allow: Vec<DiscordPermissionV1>,
        #[serde(default)]
        deny: Vec<DiscordPermissionV1>,
    },
    PostPanel {
        output: String,
        channel: ChannelReferenceV1,
        content: String,
        #[serde(default)]
        buttons: Vec<ActionButtonV1>,
    },
    DeferEphemeral,
    EditResponse {
        content: String,
    },
    RegisterInstance {
        output: String,
        instance_kind: String,
        resources: InstanceResourcesV1,
    },
    TeardownInstance {
        instance: InstanceReferenceV1,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionTargetV1 {
    Actor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoleReferenceV1 {
    Existing {
        binding: String,
    },
    Created {
        output: String,
    },
    Instance {
        instance: InstanceReferenceV1,
        alias: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChannelReferenceV1 {
    Existing { binding: String },
    Created { output: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstanceReferenceV1 {
    Event,
    Created { output: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OverwriteTargetV1 {
    Everyone,
    Role { role: RoleReferenceV1 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionButtonV1 {
    pub label: String,
    pub route: ActionButtonRouteV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionButtonRouteV1 {
    Static {
        trigger_id: String,
    },
    InstanceAction {
        instance: InstanceReferenceV1,
        action_id: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceResourcesV1 {
    #[serde(default)]
    pub roles: BTreeMap<String, CreatedResourceReferenceV1>,
    #[serde(default)]
    pub channels: BTreeMap<String, CreatedResourceReferenceV1>,
    #[serde(default)]
    pub messages: BTreeMap<String, CreatedResourceReferenceV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatedResourceReferenceV1 {
    pub output: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordPermissionV1 {
    AddReactions,
    ViewChannel,
    SendMessages,
    EmbedLinks,
    AttachFiles,
    ReadMessageHistory,
    Connect,
    Speak,
}

impl DiscordPermissionV1 {
    pub(crate) fn runtime(self) -> Permissions {
        match self {
            Self::AddReactions => Permissions::ADD_REACTIONS,
            Self::ViewChannel => Permissions::VIEW_CHANNEL,
            Self::SendMessages => Permissions::SEND_MESSAGES,
            Self::EmbedLinks => Permissions::EMBED_LINKS,
            Self::AttachFiles => Permissions::ATTACH_FILES,
            Self::ReadMessageHistory => Permissions::READ_MESSAGE_HISTORY,
            Self::Connect => Permissions::CONNECT,
            Self::Speak => Permissions::SPEAK,
        }
    }
}

pub(crate) fn lower_panel(panel: &DeclaredPanelV1) -> PanelSpec {
    PanelSpec {
        key: panel.id.clone(),
        channel: ResourceKey(panel.channel.clone()),
        content: panel.content.clone(),
        buttons: panel
            .buttons
            .iter()
            .map(|button| ButtonSpec {
                label: button.label.clone(),
                route: ButtonRoute::Static {
                    key: button.trigger_id.clone(),
                },
            })
            .collect(),
    }
}

pub(crate) fn lower_modal(modal: &ModalDefinitionV1) -> ModalSpec {
    ModalSpec {
        key: modal.id.clone(),
        title: modal.title.clone(),
        fields: modal
            .fields
            .iter()
            .map(|field| ModalFieldSpec {
                key: field.id.clone(),
                label: field.label.clone(),
                style: match field.style {
                    ModalFieldStyleV1::Short => ModalFieldStyle::Short,
                    ModalFieldStyleV1::Paragraph => ModalFieldStyle::Paragraph,
                },
                required: field.required,
                min_length: field.min_length,
                max_length: field.max_length,
                input_policy: match field.input_policy {
                    ModalInputPolicyV1::Preserve => ModalInputPolicy::Preserve,
                    ModalInputPolicyV1::TrimUnicodeWhitespace => {
                        ModalInputPolicy::TrimUnicodeWhitespace
                    }
                },
            })
            .collect(),
    }
}

pub(crate) fn lower_trigger(trigger: &TriggerV1) -> TriggerSpec {
    match trigger {
        TriggerV1::ButtonClick { trigger_id } => TriggerSpec::ButtonClick {
            component: trigger_id.clone(),
        },
        TriggerV1::ModalSubmit { modal_id } => TriggerSpec::ModalSubmit {
            modal: modal_id.clone(),
        },
        TriggerV1::InstanceAction { action_id } => TriggerSpec::InstanceAction {
            action: action_id.clone(),
        },
    }
}

pub(crate) fn lower_action(action: &ActionV1) -> ActionSpec {
    match action {
        ActionV1::GrantRole { role, target } => ActionSpec::GrantRole {
            role: lower_role_reference(role),
            target: match target {
                ActionTargetV1::Actor => ActionTarget::Actor,
            },
        },
        ActionV1::RespondEphemeral { content } => ActionSpec::RespondEphemeral {
            content: content.clone(),
        },
        ActionV1::OpenModal { modal_id } => ActionSpec::OpenModal {
            modal: modal_id.clone(),
        },
        ActionV1::CreateChannel { output, name } => ActionSpec::CreateChannel {
            key: output.clone(),
            name: name.clone(),
        },
        ActionV1::CreateRole { output, name } => ActionSpec::CreateRole {
            key: output.clone(),
            name: name.clone(),
        },
        ActionV1::UpsertOverwrite {
            channel,
            target,
            allow,
            deny,
        } => ActionSpec::UpsertOverwrite {
            channel: lower_channel_reference(channel),
            target: match target {
                OverwriteTargetV1::Everyone => OverwriteTargetSpec::Everyone,
                OverwriteTargetV1::Role { role } => {
                    OverwriteTargetSpec::Role(lower_role_reference(role))
                }
            },
            allow: permission_set(allow),
            deny: permission_set(deny),
        },
        ActionV1::PostPanel {
            output,
            channel,
            content,
            buttons,
        } => ActionSpec::PostPanel {
            key: output.clone(),
            channel: lower_channel_reference(channel),
            content: content.clone(),
            buttons: buttons
                .iter()
                .map(|button| ButtonSpec {
                    label: button.label.clone(),
                    route: match &button.route {
                        ActionButtonRouteV1::Static { trigger_id } => ButtonRoute::Static {
                            key: trigger_id.clone(),
                        },
                        ActionButtonRouteV1::InstanceAction {
                            instance,
                            action_id,
                        } => ButtonRoute::InstanceAction {
                            instance: lower_instance_reference(instance),
                            action: action_id.clone(),
                        },
                    },
                })
                .collect(),
        },
        ActionV1::DeferEphemeral => ActionSpec::DeferEphemeral,
        ActionV1::EditResponse { content } => ActionSpec::EditResponse {
            content: content.clone(),
        },
        ActionV1::RegisterInstance {
            output,
            instance_kind,
            resources,
        } => ActionSpec::RegisterInstance {
            key: output.clone(),
            kind: InstanceKind(instance_kind.clone()),
            resources: InstanceResourceRefs {
                roles: lower_created_resource_map(&resources.roles),
                channels: lower_created_resource_map(&resources.channels),
                messages: lower_created_resource_map(&resources.messages),
            },
        },
        ActionV1::TeardownInstance { instance } => ActionSpec::TeardownInstance {
            instance: lower_instance_reference(instance),
        },
    }
}

fn lower_role_reference(reference: &RoleReferenceV1) -> RoleRef {
    match reference {
        RoleReferenceV1::Existing { binding } => RoleRef::Existing(ResourceKey(binding.clone())),
        RoleReferenceV1::Created { output } => RoleRef::Created(CreatedRef {
            created: output.clone(),
        }),
        RoleReferenceV1::Instance { instance, alias } => RoleRef::Instance {
            instance: lower_instance_reference(instance),
            alias: alias.clone(),
        },
    }
}

fn lower_channel_reference(reference: &ChannelReferenceV1) -> ChannelRef {
    match reference {
        ChannelReferenceV1::Existing { binding } => {
            ChannelRef::Existing(ResourceKey(binding.clone()))
        }
        ChannelReferenceV1::Created { output } => ChannelRef::Created(CreatedRef {
            created: output.clone(),
        }),
    }
}

fn lower_instance_reference(reference: &InstanceReferenceV1) -> InstanceRef {
    match reference {
        InstanceReferenceV1::Event => InstanceRef::Event,
        InstanceReferenceV1::Created { output } => InstanceRef::Created(CreatedRef {
            created: output.clone(),
        }),
    }
}

fn lower_created_resource_map(
    references: &BTreeMap<String, CreatedResourceReferenceV1>,
) -> BTreeMap<String, CreatedRef> {
    references
        .iter()
        .map(|(alias, reference)| {
            (
                alias.clone(),
                CreatedRef {
                    created: reference.output.clone(),
                },
            )
        })
        .collect()
}

fn permission_set(permissions: &[DiscordPermissionV1]) -> Permissions {
    permissions
        .iter()
        .copied()
        .fold(Permissions::empty(), |set, permission| {
            set | permission.runtime()
        })
}
