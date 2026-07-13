use std::collections::BTreeSet;

use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ChannelRef, InstanceRef, InstanceResourceRefs,
    ModalFieldStyle, OverwriteTargetSpec, RoleRef, TriggerSpec,
};
use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use crate::draft::{Draft, DraftSummary};
use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::tools::ToolDefinition;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnIntent {
    Build,
    Modify,
    Brainstorm,
    Inspect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestedOutcome {
    Discussion,
    DraftUpdate,
    ValidatedPreview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SimulationProfile {
    None,
    StudyRoom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    GrantRole,
    RespondEphemeral,
    OpenModal,
    CreateChannel,
    CreateRole,
    UpsertOverwrite,
    PostPanel,
    DeferEphemeral,
    EditResponse,
    RegisterInstance,
    TeardownInstance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeButtonRoute {
    Static { key: String },
    InstanceAction { action: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScopeModalFieldStyle {
    Short,
    Paragraph,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeModalField {
    pub key: String,
    pub label: String,
    pub style: ScopeModalFieldStyle,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeTrigger {
    ButtonClick { component: String },
    ModalSubmit { modal: String },
    InstanceAction { action: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeInstanceRef {
    Event,
    Created { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeResourceRef {
    Created { name: String },
    Existing { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeRoleRef {
    Created {
        name: String,
    },
    Existing {
        name: String,
    },
    Instance {
        instance: ScopeInstanceRef,
        alias: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScopeActionTarget {
    Actor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeOverwriteTarget {
    Everyone,
    Role { role: ScopeRoleRef },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScopePermission {
    CreateInstantInvite,
    KickMembers,
    BanMembers,
    Administrator,
    ManageChannels,
    ManageGuild,
    AddReactions,
    ViewChannel,
    SendMessages,
    ManageMessages,
    EmbedLinks,
    AttachFiles,
    ReadMessageHistory,
    MentionEveryone,
    Connect,
    Speak,
    ManageRoles,
    ModerateMembers,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopePostPanelButtonRoute {
    Static {
        key: String,
    },
    InstanceAction {
        instance: ScopeInstanceRef,
        action: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopePostPanelButton {
    pub label: String,
    pub route: ScopePostPanelButtonRoute,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeManifestEntry {
    pub alias: String,
    pub created: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeInstanceResources {
    pub roles: Vec<ScopeManifestEntry>,
    pub channels: Vec<ScopeManifestEntry>,
    pub messages: Vec<ScopeManifestEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeAction {
    GrantRole {
        role: ScopeRoleRef,
        target: ScopeActionTarget,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal {
        modal: String,
    },
    CreateChannel {
        key: String,
        name: String,
    },
    CreateRole {
        key: String,
        name: String,
    },
    UpsertOverwrite {
        channel: ScopeResourceRef,
        target: ScopeOverwriteTarget,
        allow: Vec<ScopePermission>,
        deny: Vec<ScopePermission>,
    },
    PostPanel {
        key: String,
        channel: ScopeResourceRef,
        content: String,
        buttons: Vec<ScopePostPanelButton>,
    },
    DeferEphemeral,
    EditResponse {
        content: String,
    },
    RegisterInstance {
        key: String,
        instance_kind: String,
        resources: ScopeInstanceResources,
    },
    TeardownInstance {
        instance: ScopeInstanceRef,
    },
}

impl ScopeAction {
    fn kind(&self) -> ActionKind {
        match self {
            Self::GrantRole { .. } => ActionKind::GrantRole,
            Self::RespondEphemeral { .. } => ActionKind::RespondEphemeral,
            Self::OpenModal { .. } => ActionKind::OpenModal,
            Self::CreateChannel { .. } => ActionKind::CreateChannel,
            Self::CreateRole { .. } => ActionKind::CreateRole,
            Self::UpsertOverwrite { .. } => ActionKind::UpsertOverwrite,
            Self::PostPanel { .. } => ActionKind::PostPanel,
            Self::DeferEphemeral => ActionKind::DeferEphemeral,
            Self::EditResponse { .. } => ActionKind::EditResponse,
            Self::RegisterInstance { .. } => ActionKind::RegisterInstance,
            Self::TeardownInstance { .. } => ActionKind::TeardownInstance,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeRequirement {
    Panel {
        id: String,
        key: String,
        channel: String,
        content: String,
    },
    Button {
        id: String,
        panel_key: String,
        label: String,
        route: ScopeButtonRoute,
    },
    Modal {
        id: String,
        key: String,
        title: String,
        fields: Vec<ScopeModalField>,
    },
    Rule {
        id: String,
        key: String,
        trigger: ScopeTrigger,
    },
    Action {
        id: String,
        rule_key: String,
        action: ScopeAction,
        minimum: usize,
    },
    NoUnresolvedReferences {
        id: String,
    },
}

impl ScopeRequirement {
    pub fn id(&self) -> &str {
        match self {
            Self::Panel { id, .. }
            | Self::Button { id, .. }
            | Self::Modal { id, .. }
            | Self::Rule { id, .. }
            | Self::Action { id, .. }
            | Self::NoUnresolvedReferences { id } => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockingDecision {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TurnVerification {
    pub validate: bool,
    pub simulation: SimulationProfile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TurnBrief {
    pub intent: TurnIntent,
    pub objective: String,
    pub requested_outcome: RequestedOutcome,
    pub requirements: Vec<ScopeRequirement>,
    pub assumptions: Vec<String>,
    pub blocking_decisions: Vec<BlockingDecision>,
    pub verification: TurnVerification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FinishTurnKind {
    NeedsInput,
    Progressed,
    #[serde(alias = "success")]
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinishTurn {
    pub kind: FinishTurnKind,
    pub message: String,
    #[serde(default)]
    pub question: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScopeCheck {
    pub ok: bool,
    pub satisfied: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftPreview {
    pub revision: u64,
    pub draft: DraftSummary,
    pub ruleset: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptivePhase {
    Assess,
    Build,
    Verify,
    Simulate,
    Preview,
    Reply,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveTurnState {
    pub phase: AdaptivePhase,
    pub brief: Option<TurnBrief>,
    pub scoped_revision: Option<u64>,
    pub previewed_revision: Option<u64>,
}

impl Default for AdaptiveTurnState {
    fn default() -> Self {
        Self {
            phase: AdaptivePhase::Assess,
            brief: None,
            scoped_revision: None,
            previewed_revision: None,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetTurnBriefInput {
    intent: TurnIntent,
    objective: String,
    requested_outcome: RequestedOutcome,
    assumptions: Vec<String>,
    validate: bool,
}

pub fn control_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        definition::<SetTurnBriefInput>(
            "set_turn_brief",
            "Classify the current user turn with a concise objective and verification plan",
        ),
        definition::<EmptyInput>(
            "check_turn_scope",
            "Check the current Draft against the active turn requirements",
        ),
        definition::<EmptyInput>(
            "render_preview",
            "Render the current validated Draft for human review",
        ),
        definition::<FinishTurn>(
            "finish_turn",
            "Finish the human turn with kind needs_input, progressed, or ready; include question only for needs_input",
        ),
    ]
}

pub fn parse_turn_brief(arguments: &str) -> Result<TurnBrief, StructuredError> {
    parse::<SetTurnBriefInput>("set_turn_brief", arguments).map(|input| TurnBrief {
        intent: input.intent,
        objective: input.objective,
        requested_outcome: input.requested_outcome,
        requirements: Vec::new(),
        assumptions: input.assumptions,
        blocking_decisions: Vec::new(),
        verification: TurnVerification {
            validate: input.validate,
            simulation: SimulationProfile::None,
        },
    })
}

pub fn parse_finish_turn(arguments: &str) -> Result<FinishTurn, StructuredError> {
    parse("finish_turn", arguments)
}

pub fn parse_empty_control(name: &str, arguments: &str) -> Result<(), StructuredError> {
    parse::<EmptyInput>(name, arguments).map(|_| ())
}

pub fn check_scope(draft: &Draft, brief: &TurnBrief) -> ScopeCheck {
    let mut satisfied = Vec::new();
    let mut missing = Vec::new();
    for requirement in &brief.requirements {
        let complete = requirement_satisfied(draft, requirement);
        if complete {
            satisfied.push(requirement.id().to_string());
        } else {
            missing.push(requirement.id().to_string());
        }
    }
    ScopeCheck {
        ok: missing.is_empty(),
        satisfied,
        missing,
    }
}

pub fn render_preview(draft: &Draft) -> DraftPreview {
    DraftPreview {
        revision: draft.draft_revision,
        draft: draft.summary(),
        ruleset: serde_json::to_value(&draft.ruleset).unwrap_or(Value::Null),
    }
}

pub fn required_mutation_tools(brief: &TurnBrief) -> BTreeSet<String> {
    let mut tools = BTreeSet::new();
    for requirement in &brief.requirements {
        match requirement {
            ScopeRequirement::Panel { .. } => insert(&mut tools, "add_panel"),
            ScopeRequirement::Button { .. } => insert(&mut tools, "add_button"),
            ScopeRequirement::Modal { .. } => insert(&mut tools, "add_modal"),
            ScopeRequirement::Rule { .. } => insert(&mut tools, "begin_rule"),
            ScopeRequirement::Action { action, .. } => {
                insert(&mut tools, action_tool(action.kind()));
            }
            ScopeRequirement::NoUnresolvedReferences { .. } => {}
        }
    }
    if brief.intent == TurnIntent::Modify {
        for name in [
            "update_panel",
            "remove_panel",
            "update_modal",
            "remove_modal",
            "update_button",
            "remove_button",
            "update_rule",
            "remove_rule",
            "update_action",
            "remove_action",
        ] {
            insert(&mut tools, name);
        }
    }
    tools
}

fn requirement_satisfied(draft: &Draft, requirement: &ScopeRequirement) -> bool {
    match requirement {
        ScopeRequirement::Panel {
            key,
            channel,
            content,
            ..
        } => draft.ruleset.panels.iter().any(|panel| {
            &panel.key == key && panel.content == *content && panel.channel.0 == *channel
        }),
        ScopeRequirement::Button {
            panel_key,
            label,
            route,
            ..
        } => draft
            .ruleset
            .panels
            .iter()
            .find(|panel| &panel.key == panel_key)
            .is_some_and(|panel| {
                panel
                    .buttons
                    .iter()
                    .any(|button| button.label == *label && button_route_matches(button, route))
            }),
        ScopeRequirement::Modal {
            key, title, fields, ..
        } => draft.ruleset.modals.iter().any(|modal| {
            &modal.key == key
                && modal.title == *title
                && modal.fields.len() == fields.len()
                && modal
                    .fields
                    .iter()
                    .zip(fields)
                    .all(|(field, required)| modal_field_matches(field, required))
        }),
        ScopeRequirement::Rule { key, trigger, .. } => draft
            .ruleset
            .rules
            .iter()
            .any(|rule| &rule.key == key && trigger_matches(&rule.trigger, trigger)),
        ScopeRequirement::Action {
            rule_key,
            action,
            minimum,
            ..
        } => draft
            .ruleset
            .rules
            .iter()
            .find(|rule| &rule.key == rule_key)
            .is_some_and(|rule| {
                rule.actions
                    .iter()
                    .filter(|candidate| action_matches(candidate, action))
                    .count()
                    >= *minimum
            }),
        ScopeRequirement::NoUnresolvedReferences { .. } => {
            draft.summary().unresolved_references.is_empty()
        }
    }
}

fn action_matches(action: &ActionSpec, required: &ScopeAction) -> bool {
    match (action, required) {
        (
            ActionSpec::GrantRole { role, target },
            ScopeAction::GrantRole {
                role: expected_role,
                target: expected_target,
            },
        ) => {
            role_ref_matches(role, expected_role) && action_target_matches(target, *expected_target)
        }
        (
            ActionSpec::UpsertOverwrite {
                channel,
                target,
                allow,
                deny,
            },
            ScopeAction::UpsertOverwrite {
                channel: expected_channel,
                target: expected_target,
                allow: expected_allow,
                deny: expected_deny,
            },
        ) => {
            resource_ref_matches(channel, expected_channel)
                && overwrite_target_matches(target, expected_target)
                && allow.bits() == permission_bits(expected_allow)
                && deny.bits() == permission_bits(expected_deny)
        }
        (ActionSpec::DeferEphemeral, ScopeAction::DeferEphemeral) => true,
        (
            ActionSpec::TeardownInstance { instance },
            ScopeAction::TeardownInstance { instance: expected },
        ) => instance_ref_matches(instance, expected),
        (
            ActionSpec::RespondEphemeral { content },
            ScopeAction::RespondEphemeral { content: expected },
        )
        | (ActionSpec::EditResponse { content }, ScopeAction::EditResponse { content: expected }) => {
            content == expected
        }
        (ActionSpec::OpenModal { modal }, ScopeAction::OpenModal { modal: expected }) => {
            modal == expected
        }
        (
            ActionSpec::CreateChannel { key, name },
            ScopeAction::CreateChannel {
                key: expected_key,
                name: expected_name,
            },
        )
        | (
            ActionSpec::CreateRole { key, name },
            ScopeAction::CreateRole {
                key: expected_key,
                name: expected_name,
            },
        ) => key == expected_key && name == expected_name,
        (
            ActionSpec::PostPanel {
                key,
                channel,
                content,
                buttons,
            },
            ScopeAction::PostPanel {
                key: expected_key,
                channel: expected_channel,
                content: expected_content,
                buttons: expected_buttons,
            },
        ) => {
            key == expected_key
                && resource_ref_matches(channel, expected_channel)
                && content == expected_content
                && buttons.len() == expected_buttons.len()
                && buttons
                    .iter()
                    .zip(expected_buttons)
                    .all(|(button, expected)| post_panel_button_matches(button, expected))
        }
        (
            ActionSpec::RegisterInstance {
                key,
                kind,
                resources,
            },
            ScopeAction::RegisterInstance {
                key: expected_key,
                instance_kind,
                resources: expected_resources,
            },
        ) => {
            key == expected_key
                && kind.0 == *instance_kind
                && instance_resources_match(resources, expected_resources)
        }
        _ => false,
    }
}

fn action_target_matches(target: &ActionTarget, expected: ScopeActionTarget) -> bool {
    matches!(
        (target, expected),
        (ActionTarget::Actor, ScopeActionTarget::Actor)
    )
}

fn resource_ref_matches(reference: &ChannelRef, expected: &ScopeResourceRef) -> bool {
    match (reference, expected) {
        (ChannelRef::Created(reference), ScopeResourceRef::Created { name }) => {
            reference.created == *name
        }
        (ChannelRef::Existing(reference), ScopeResourceRef::Existing { name }) => {
            reference.0 == *name
        }
        _ => false,
    }
}

fn role_ref_matches(reference: &RoleRef, expected: &ScopeRoleRef) -> bool {
    match (reference, expected) {
        (RoleRef::Created(reference), ScopeRoleRef::Created { name }) => reference.created == *name,
        (RoleRef::Existing(reference), ScopeRoleRef::Existing { name }) => reference.0 == *name,
        (
            RoleRef::Instance { instance, alias },
            ScopeRoleRef::Instance {
                instance: expected_instance,
                alias: expected_alias,
            },
        ) => instance_ref_matches(instance, expected_instance) && alias == expected_alias,
        _ => false,
    }
}

fn instance_ref_matches(reference: &InstanceRef, expected: &ScopeInstanceRef) -> bool {
    match (reference, expected) {
        (InstanceRef::Event, ScopeInstanceRef::Event) => true,
        (InstanceRef::Created(reference), ScopeInstanceRef::Created { name }) => {
            reference.created == *name
        }
        _ => false,
    }
}

fn overwrite_target_matches(target: &OverwriteTargetSpec, expected: &ScopeOverwriteTarget) -> bool {
    match (target, expected) {
        (OverwriteTargetSpec::Everyone, ScopeOverwriteTarget::Everyone) => true,
        (OverwriteTargetSpec::Role(role), ScopeOverwriteTarget::Role { role: expected }) => {
            role_ref_matches(role, expected)
        }
        _ => false,
    }
}

fn permission_bits(permissions: &[ScopePermission]) -> u64 {
    permissions.iter().fold(0, |bits, permission| {
        bits | match permission {
            ScopePermission::CreateInstantInvite => 1 << 0,
            ScopePermission::KickMembers => 1 << 1,
            ScopePermission::BanMembers => 1 << 2,
            ScopePermission::Administrator => 1 << 3,
            ScopePermission::ManageChannels => 1 << 4,
            ScopePermission::ManageGuild => 1 << 5,
            ScopePermission::AddReactions => 1 << 6,
            ScopePermission::ViewChannel => 1 << 10,
            ScopePermission::SendMessages => 1 << 11,
            ScopePermission::ManageMessages => 1 << 13,
            ScopePermission::EmbedLinks => 1 << 14,
            ScopePermission::AttachFiles => 1 << 15,
            ScopePermission::ReadMessageHistory => 1 << 16,
            ScopePermission::MentionEveryone => 1 << 17,
            ScopePermission::Connect => 1 << 20,
            ScopePermission::Speak => 1 << 21,
            ScopePermission::ManageRoles => 1 << 28,
            ScopePermission::ModerateMembers => 1 << 40,
        }
    })
}

fn post_panel_button_matches(
    button: &automation_state::ButtonSpec,
    expected: &ScopePostPanelButton,
) -> bool {
    if button.label != expected.label {
        return false;
    }
    match (&button.route, &expected.route) {
        (ButtonRoute::Static { key }, ScopePostPanelButtonRoute::Static { key: expected }) => {
            key == expected
        }
        (
            ButtonRoute::InstanceAction { instance, action },
            ScopePostPanelButtonRoute::InstanceAction {
                instance: expected_instance,
                action: expected_action,
            },
        ) => instance_ref_matches(instance, expected_instance) && action == expected_action,
        _ => false,
    }
}

fn instance_resources_match(
    resources: &InstanceResourceRefs,
    expected: &ScopeInstanceResources,
) -> bool {
    manifest_matches(&resources.roles, &expected.roles)
        && manifest_matches(&resources.channels, &expected.channels)
        && manifest_matches(&resources.messages, &expected.messages)
}

fn manifest_matches(
    resources: &std::collections::BTreeMap<String, automation_state::CreatedRef>,
    expected: &[ScopeManifestEntry],
) -> bool {
    resources.len() == expected.len()
        && expected.iter().all(|entry| {
            resources
                .get(&entry.alias)
                .is_some_and(|reference| reference.created == entry.created)
        })
}

fn modal_field_matches(
    field: &automation_state::ModalFieldSpec,
    required: &ScopeModalField,
) -> bool {
    let style = match field.style {
        ModalFieldStyle::Short => ScopeModalFieldStyle::Short,
        ModalFieldStyle::Paragraph => ScopeModalFieldStyle::Paragraph,
    };
    field.key == required.key
        && field.label == required.label
        && style == required.style
        && field.required == required.required
}

fn trigger_matches(trigger: &TriggerSpec, required: &ScopeTrigger) -> bool {
    match (trigger, required) {
        (
            TriggerSpec::ButtonClick { component },
            ScopeTrigger::ButtonClick {
                component: expected,
            },
        ) => component == expected,
        (TriggerSpec::ModalSubmit { modal }, ScopeTrigger::ModalSubmit { modal: expected }) => {
            modal == expected
        }
        (
            TriggerSpec::InstanceAction { action },
            ScopeTrigger::InstanceAction { action: expected },
        ) => action == expected,
        _ => false,
    }
}

fn button_route_matches(
    button: &automation_state::ButtonSpec,
    required: &ScopeButtonRoute,
) -> bool {
    match (&button.route, required) {
        (ButtonRoute::Static { key }, ScopeButtonRoute::Static { key: expected }) => {
            key == expected
        }
        (
            ButtonRoute::InstanceAction { action, .. },
            ScopeButtonRoute::InstanceAction { action: expected },
        ) => action == expected,
        _ => false,
    }
}

fn action_tool(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::CreateChannel | ActionKind::CreateRole => "add_resource_action",
        ActionKind::GrantRole => "add_grant_role_action",
        ActionKind::UpsertOverwrite => "add_upsert_overwrite_action",
        ActionKind::OpenModal
        | ActionKind::DeferEphemeral
        | ActionKind::EditResponse
        | ActionKind::RespondEphemeral => "add_interaction_action",
        ActionKind::PostPanel => "add_post_panel_action",
        ActionKind::RegisterInstance => "set_register_instance",
        ActionKind::TeardownInstance => "add_interaction_action",
    }
}

fn insert(tools: &mut BTreeSet<String>, name: &str) {
    tools.insert(name.to_string());
}

fn definition<T: JsonSchema>(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({})),
    }
}

fn parse<T: DeserializeOwned + JsonSchema>(
    name: &str,
    arguments: &str,
) -> Result<T, StructuredError> {
    serde_json::from_str(arguments).map_err(|error| {
        let parameters = control_tool_definitions()
            .into_iter()
            .find(|definition| definition.name == name)
            .map(|definition| definition.parameters)
            .unwrap_or_else(|| json!({}));
        translate_tool_arguments_error(name, &error, &parameters)
    })
}

#[cfg(test)]
mod tests {
    use automation_state::{
        InteractionRule, InteractionRuleSet, ModalSpec, PanelSpec, TriggerSpec,
    };

    use super::*;

    fn brief(requirements: Vec<ScopeRequirement>) -> TurnBrief {
        TurnBrief {
            intent: TurnIntent::Build,
            objective: "test".to_string(),
            requested_outcome: RequestedOutcome::ValidatedPreview,
            requirements,
            assumptions: Vec::new(),
            blocking_decisions: Vec::new(),
            verification: TurnVerification {
                validate: true,
                simulation: SimulationProfile::None,
            },
        }
    }

    #[test]
    fn scope_checks_structure_actions_and_unresolved_references() {
        let mut draft = Draft::new();
        draft.ruleset = InteractionRuleSet {
            version: 1,
            panels: vec![PanelSpec {
                key: "panel".to_string(),
                channel: serde_json::from_value(Value::String("study_hub".to_string())).unwrap(),
                content: "content".to_string(),
                buttons: Vec::new(),
            }],
            modals: vec![ModalSpec {
                key: "modal".to_string(),
                title: "Modal".to_string(),
                fields: Vec::new(),
            }],
            rules: vec![InteractionRule {
                key: "rule".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: "modal".to_string(),
                },
                actions: vec![ActionSpec::CreateRole {
                    key: "role".to_string(),
                    name: "Role".to_string(),
                }],
            }],
        };
        let result = check_scope(
            &draft,
            &brief(vec![
                ScopeRequirement::Panel {
                    id: "panel".to_string(),
                    key: "panel".to_string(),
                    channel: "study_hub".to_string(),
                    content: "content".to_string(),
                },
                ScopeRequirement::Action {
                    id: "role".to_string(),
                    rule_key: "rule".to_string(),
                    action: ScopeAction::CreateRole {
                        key: "role".to_string(),
                        name: "Role".to_string(),
                    },
                    minimum: 1,
                },
                ScopeRequirement::NoUnresolvedReferences {
                    id: "refs".to_string(),
                },
            ]),
        );
        assert!(result.ok);
        assert_eq!(result.satisfied, ["panel", "role", "refs"]);
    }

    #[test]
    fn scope_matches_permission_sensitive_actions_exactly() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version": 1,
            "panels": [],
            "modals": [],
            "rules": [{
                "key": "room",
                "trigger": {"type": "instance_action", "action": "join"},
                "actions": [
                    {
                        "type": "grant_role",
                        "role": {"created": "member_role"},
                        "target": "actor"
                    },
                    {
                        "type": "upsert_overwrite",
                        "channel": {"created": "room_channel"},
                        "target": {"role": {"created": "member_role"}},
                        "allow": "3072",
                        "deny": "0"
                    },
                    {
                        "type": "post_panel",
                        "key": "welcome_panel",
                        "channel": {"created": "room_channel"},
                        "content": "Welcome",
                        "buttons": [
                            {"label": "Help", "route": {"static": {"key": "help"}}},
                            {"label": "Join", "route": {"instance_action": {"instance": {"created": "study_instance"}, "action": "join"}}}
                        ]
                    },
                    {
                        "type": "register_instance",
                        "key": "study_instance",
                        "kind": "study_room",
                        "resources": {
                            "roles": {"member": {"created": "member_role"}},
                            "channels": {"room": {"created": "room_channel"}},
                            "messages": {"welcome": {"created": "welcome_panel"}}
                        }
                    },
                    {"type": "teardown_instance", "instance": "event"}
                ]
            }]
        }))
        .unwrap();

        let exact = [
            ScopeAction::GrantRole {
                role: ScopeRoleRef::Created {
                    name: "member_role".to_string(),
                },
                target: ScopeActionTarget::Actor,
            },
            ScopeAction::UpsertOverwrite {
                channel: ScopeResourceRef::Created {
                    name: "room_channel".to_string(),
                },
                target: ScopeOverwriteTarget::Role {
                    role: ScopeRoleRef::Created {
                        name: "member_role".to_string(),
                    },
                },
                allow: vec![ScopePermission::ViewChannel, ScopePermission::SendMessages],
                deny: Vec::new(),
            },
            ScopeAction::PostPanel {
                key: "welcome_panel".to_string(),
                channel: ScopeResourceRef::Created {
                    name: "room_channel".to_string(),
                },
                content: "Welcome".to_string(),
                buttons: vec![
                    ScopePostPanelButton {
                        label: "Help".to_string(),
                        route: ScopePostPanelButtonRoute::Static {
                            key: "help".to_string(),
                        },
                    },
                    ScopePostPanelButton {
                        label: "Join".to_string(),
                        route: ScopePostPanelButtonRoute::InstanceAction {
                            instance: ScopeInstanceRef::Created {
                                name: "study_instance".to_string(),
                            },
                            action: "join".to_string(),
                        },
                    },
                ],
            },
            ScopeAction::RegisterInstance {
                key: "study_instance".to_string(),
                instance_kind: "study_room".to_string(),
                resources: ScopeInstanceResources {
                    roles: vec![ScopeManifestEntry {
                        alias: "member".to_string(),
                        created: "member_role".to_string(),
                    }],
                    channels: vec![ScopeManifestEntry {
                        alias: "room".to_string(),
                        created: "room_channel".to_string(),
                    }],
                    messages: vec![ScopeManifestEntry {
                        alias: "welcome".to_string(),
                        created: "welcome_panel".to_string(),
                    }],
                },
            },
            ScopeAction::TeardownInstance {
                instance: ScopeInstanceRef::Event,
            },
        ];
        let requirements = exact
            .iter()
            .enumerate()
            .map(|(index, action)| ScopeRequirement::Action {
                id: format!("action-{index}"),
                rule_key: "room".to_string(),
                action: action.clone(),
                minimum: 1,
            })
            .collect();
        assert!(check_scope(&draft, &brief(requirements)).ok);

        let mismatches = vec![
            ScopeAction::GrantRole {
                role: ScopeRoleRef::Created {
                    name: "other_role".to_string(),
                },
                target: ScopeActionTarget::Actor,
            },
            ScopeAction::UpsertOverwrite {
                channel: ScopeResourceRef::Created {
                    name: "room_channel".to_string(),
                },
                target: ScopeOverwriteTarget::Role {
                    role: ScopeRoleRef::Created {
                        name: "member_role".to_string(),
                    },
                },
                allow: vec![ScopePermission::ViewChannel],
                deny: Vec::new(),
            },
            ScopeAction::PostPanel {
                key: "welcome_panel".to_string(),
                channel: ScopeResourceRef::Existing {
                    name: "study_hub".to_string(),
                },
                content: "Welcome".to_string(),
                buttons: vec![
                    ScopePostPanelButton {
                        label: "Help".to_string(),
                        route: ScopePostPanelButtonRoute::Static {
                            key: "help".to_string(),
                        },
                    },
                    ScopePostPanelButton {
                        label: "Join".to_string(),
                        route: ScopePostPanelButtonRoute::InstanceAction {
                            instance: ScopeInstanceRef::Created {
                                name: "wrong_instance".to_string(),
                            },
                            action: "join".to_string(),
                        },
                    },
                ],
            },
            ScopeAction::RegisterInstance {
                key: "study_instance".to_string(),
                instance_kind: "study_room".to_string(),
                resources: ScopeInstanceResources {
                    roles: vec![ScopeManifestEntry {
                        alias: "member".to_string(),
                        created: "wrong_role".to_string(),
                    }],
                    channels: vec![ScopeManifestEntry {
                        alias: "room".to_string(),
                        created: "room_channel".to_string(),
                    }],
                    messages: vec![ScopeManifestEntry {
                        alias: "welcome".to_string(),
                        created: "welcome_panel".to_string(),
                    }],
                },
            },
            ScopeAction::TeardownInstance {
                instance: ScopeInstanceRef::Created {
                    name: "study_instance".to_string(),
                },
            },
        ];
        for (index, action) in mismatches.into_iter().enumerate() {
            let result = check_scope(
                &draft,
                &brief(vec![ScopeRequirement::Action {
                    id: format!("mismatch-{index}"),
                    rule_key: "room".to_string(),
                    action,
                    minimum: 1,
                }]),
            );
            assert!(!result.ok, "mismatch {index} passed scope");
        }
    }

    #[test]
    fn turn_brief_rejects_legacy_requirement_and_verification_shapes() {
        let requirements = r#"{"intent":"build","objective":"room","requested_outcome":"draft_update","requirements":[],"assumptions":[],"validate":false}"#;
        assert!(parse_turn_brief(requirements).is_err());

        let verification = r#"{"intent":"build","objective":"room","requested_outcome":"draft_update","assumptions":[],"verification":{"validate":false}}"#;
        assert!(parse_turn_brief(verification).is_err());
    }

    #[test]
    fn control_tool_schemas_parse_strict_inputs() {
        let parsed = parse_turn_brief(
            r#"{"intent":"brainstorm","objective":"game","requested_outcome":"discussion","assumptions":[],"validate":false}"#,
        )
        .unwrap();
        assert_eq!(parsed.intent, TurnIntent::Brainstorm);
        assert!(parsed.requirements.is_empty());
        assert!(parsed.blocking_decisions.is_empty());
        assert!(!parsed.verification.validate);
        assert_eq!(parsed.verification.simulation, SimulationProfile::None);
        assert!(parse_turn_brief(
            r#"{"intent":"brainstorm","objective":"game","requested_outcome":"discussion","assumptions":[],"validate":false,"simulation":"study_room"}"#
        )
        .is_err());
        let schema = control_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "set_turn_brief")
            .unwrap()
            .parameters;
        assert!(schema.pointer("/properties/validate").is_some());
        assert!(schema.pointer("/properties/simulation").is_none());
        assert!(parse_finish_turn(
            r#"{"kind":"needs_input","message":"Choose a genre","question":"Which genre?"}"#
        )
        .is_ok());
        assert_eq!(
            parse_finish_turn(r#"{"kind":"ready","message":"Ready"}"#)
                .unwrap()
                .kind,
            FinishTurnKind::Ready
        );
        assert_eq!(
            parse_finish_turn(r#"{"kind":"success","message":"Ready"}"#)
                .unwrap()
                .kind,
            FinishTurnKind::Ready
        );
        assert!(parse_finish_turn(r#"{"kind":"ready","message":"Ready","changes":[]}"#).is_err());
        assert!(parse_finish_turn(r#"{"kind":"done","message":"Ready"}"#).is_err());
        let finish_schema = control_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "finish_turn")
            .unwrap()
            .parameters;
        let properties = finish_schema["properties"].as_object().unwrap();
        assert_eq!(
            properties.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "kind".to_string(),
                "message".to_string(),
                "question".to_string()
            ])
        );
        assert_eq!(finish_schema["required"], json!(["kind", "message"]));
        assert_eq!(
            finish_schema["$defs"]["FinishTurnKind"]["enum"],
            json!(["needs_input", "progressed", "ready"])
        );
        assert_eq!(control_tool_definitions().len(), 4);
    }
}
