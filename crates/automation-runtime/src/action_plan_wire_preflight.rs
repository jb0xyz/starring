use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter, Write};

use automation_core::preflight::{
    ActionEntryIdV1, PreflightButtonRouteV1, PreflightInstanceRefV1, PreparedActionPlanV1,
    PreparedPlanActionV1,
};
use automation_instance::{AutomationInstance, InstanceId, InstanceResources, InstanceStatus};
use automation_runtime_interaction::InteractionInstanceManifestDigestV1;
use discord_model::{ChannelId, MessageId, RoleId};
use sha2::{Digest, Sha256};

use crate::custom_id::{
    decode, encode_button, encode_instance_action, encode_modal, ComponentKind, ParsedCustomId,
};

const MAX_CUSTOM_ID_BYTES: usize = 100;
const MAX_WIRE_DIGEST_MATERIAL_BYTES: usize = 1_048_576;
const MAX_MANIFEST_CANONICAL_BYTES: usize = 524_288;
const MAX_MANIFEST_ROLES: usize = 250;
const MAX_MANIFEST_CHANNELS: usize = 500;
const MAX_MANIFEST_MESSAGES: usize = 1_000;
const MAX_RESOURCE_ALIAS_BYTES: usize = 32;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExactCustomIdV1(String);

impl ExactCustomIdV1 {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ExactCustomIdV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ExactCustomIdV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExactPostPanelButtonV1 {
    pub(crate) index: u8,
    pub(crate) custom_id: ExactCustomIdV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExactManifestRoleV1 {
    pub(crate) alias: String,
    pub(crate) id: RoleId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExactManifestChannelV1 {
    pub(crate) alias: String,
    pub(crate) id: ChannelId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExactManifestMessageV1 {
    pub(crate) alias: String,
    pub(crate) channel: ChannelId,
    pub(crate) id: MessageId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExactTeardownDeleteV1 {
    Message(ExactManifestMessageV1),
    Channel(ExactManifestChannelV1),
    Role(ExactManifestRoleV1),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExactPinnedInstanceManifestV1 {
    instance_id: InstanceId,
    digest: InteractionInstanceManifestDigestV1,
    roles: Vec<ExactManifestRoleV1>,
    channels: Vec<ExactManifestChannelV1>,
    messages: Vec<ExactManifestMessageV1>,
    reverse_delete_order: Vec<ExactTeardownDeleteV1>,
    canonical_manifest: Vec<u8>,
}

impl ExactPinnedInstanceManifestV1 {
    pub(crate) fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub(crate) fn digest(&self) -> &InteractionInstanceManifestDigestV1 {
        &self.digest
    }

    pub(crate) fn reverse_delete_order(&self) -> &[ExactTeardownDeleteV1] {
        &self.reverse_delete_order
    }

    pub(crate) fn canonical_manifest_bytes_v1(&self) -> &[u8] {
        &self.canonical_manifest
    }
}

impl Debug for ExactPinnedInstanceManifestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExactPinnedInstanceManifestV1")
            .field("instance_id", &"<redacted>")
            .field("digest", &"<redacted>")
            .field("role_count", &self.roles.len())
            .field("channel_count", &self.channels.len())
            .field("message_count", &self.messages.len())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ActionPlanWireBindingV1 {
    OpenModal {
        entry: ActionEntryIdV1,
        custom_id: ExactCustomIdV1,
    },
    PostPanel {
        entry: ActionEntryIdV1,
        buttons: Vec<ExactPostPanelButtonV1>,
    },
    TeardownInstance {
        entry: ActionEntryIdV1,
        manifest: ExactPinnedInstanceManifestV1,
    },
}

impl ActionPlanWireBindingV1 {
    pub(crate) fn entry(&self) -> ActionEntryIdV1 {
        match self {
            Self::OpenModal { entry, .. }
            | Self::PostPanel { entry, .. }
            | Self::TeardownInstance { entry, .. } => *entry,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ActionPlanWirePreflightV1 {
    bindings: Vec<ActionPlanWireBindingV1>,
    wire_digest_material: Vec<u8>,
}

impl ActionPlanWirePreflightV1 {
    pub(crate) fn wire_digest_material_v1(&self) -> &[u8] {
        &self.wire_digest_material
    }

    pub(crate) fn teardown_manifests(
        &self,
    ) -> impl Iterator<Item = &ExactPinnedInstanceManifestV1> {
        self.bindings.iter().filter_map(|binding| match binding {
            ActionPlanWireBindingV1::TeardownInstance { manifest, .. } => Some(manifest),
            ActionPlanWireBindingV1::OpenModal { .. }
            | ActionPlanWireBindingV1::PostPanel { .. } => None,
        })
    }
}

impl Debug for ActionPlanWirePreflightV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActionPlanWirePreflightV1")
            .field("binding_count", &self.bindings.len())
            .field("wire_digest_material", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ActionPlanWirePreflightErrorV1 {
    #[error("an exact Discord custom id exceeds its bound")]
    CustomIdBound,
    #[error("an exact Discord custom id does not round trip")]
    CustomIdShape,
    #[error("a panel contains duplicate exact Discord custom ids")]
    DuplicatePanelCustomId,
    #[error("the pinned teardown instance context is unavailable")]
    TeardownContextMissing,
    #[error("the pinned teardown instance identity changed")]
    TeardownInstanceDrift,
    #[error("a same-plan teardown cannot resolve Discord generated ids before mutation")]
    DynamicTeardownManifest,
    #[error("the pinned teardown instance is not active")]
    TeardownInstanceInactive,
    #[error("the pinned teardown manifest is invalid")]
    InvalidTeardownManifest,
    #[error("the pinned teardown manifest exceeds its bound")]
    TeardownManifestBound,
    #[error("the exact wire digest projection exceeds its bound")]
    WireDigestMaterialBound,
}

pub(crate) fn preflight_action_plan_wire_v1(
    prepared: &PreparedActionPlanV1,
) -> Result<ActionPlanWirePreflightV1, ActionPlanWirePreflightErrorV1> {
    let mut bindings = Vec::new();
    let mut teardown_instances = BTreeSet::new();
    for action in prepared.actions() {
        match action {
            PreparedPlanActionV1::OpenModal { entry, modal } => {
                let encoded = encode_modal(
                    prepared.context().guild_id,
                    &prepared.context().ruleset_key,
                    &modal.key,
                );
                let expected = ParsedCustomId::Component {
                    guild_id: prepared.context().guild_id,
                    ruleset_key: prepared.context().ruleset_key.clone(),
                    kind: ComponentKind::Modal,
                    key: modal.key.clone(),
                };
                bindings.push(ActionPlanWireBindingV1::OpenModal {
                    entry: *entry,
                    custom_id: exact_custom_id(encoded, expected)?,
                });
            }
            PreparedPlanActionV1::PostPanel { entry, buttons, .. } => {
                let mut exact = Vec::with_capacity(buttons.len());
                let mut unique = BTreeSet::new();
                for (index, button) in buttons.iter().enumerate() {
                    let (encoded, expected) = match &button.route {
                        PreflightButtonRouteV1::Static { key } => {
                            let encoded = encode_button(
                                prepared.context().guild_id,
                                &prepared.context().ruleset_key,
                                key,
                            );
                            let expected = ParsedCustomId::Component {
                                guild_id: prepared.context().guild_id,
                                ruleset_key: prepared.context().ruleset_key.clone(),
                                kind: ComponentKind::Button,
                                key: key.clone(),
                            };
                            (encoded, expected)
                        }
                        PreflightButtonRouteV1::InstanceAction {
                            instance_id,
                            action,
                            ..
                        } => {
                            let encoded = encode_instance_action(instance_id.as_str(), action)
                                .map_err(|_| ActionPlanWirePreflightErrorV1::CustomIdBound)?;
                            let expected = ParsedCustomId::InstanceAction {
                                instance_id: instance_id.as_str().to_string(),
                                action: action.clone(),
                            };
                            (encoded, expected)
                        }
                    };
                    let custom_id = exact_custom_id(encoded, expected)?;
                    if !unique.insert(custom_id.0.clone()) {
                        return Err(ActionPlanWirePreflightErrorV1::DuplicatePanelCustomId);
                    }
                    exact.push(ExactPostPanelButtonV1 {
                        index: index as u8,
                        custom_id,
                    });
                }
                bindings.push(ActionPlanWireBindingV1::PostPanel {
                    entry: *entry,
                    buttons: exact,
                });
            }
            PreparedPlanActionV1::TeardownInstance { entry, instance } => {
                let PreflightInstanceRefV1::Existing(instance_id) = instance else {
                    return Err(ActionPlanWirePreflightErrorV1::DynamicTeardownManifest);
                };
                if !teardown_instances.insert(instance_id.clone()) {
                    return Err(ActionPlanWirePreflightErrorV1::TeardownInstanceDrift);
                }
                let pinned = prepared
                    .context()
                    .instance
                    .as_ref()
                    .ok_or(ActionPlanWirePreflightErrorV1::TeardownContextMissing)?;
                if &pinned.instance.id != instance_id
                    || pinned.instance.guild_id != prepared.context().guild_id
                {
                    return Err(ActionPlanWirePreflightErrorV1::TeardownInstanceDrift);
                }
                if pinned.instance.status != InstanceStatus::Active {
                    return Err(ActionPlanWirePreflightErrorV1::TeardownInstanceInactive);
                }
                bindings.push(ActionPlanWireBindingV1::TeardownInstance {
                    entry: *entry,
                    manifest: exact_manifest(&pinned.instance)?,
                });
            }
            PreparedPlanActionV1::GrantRole { .. }
            | PreparedPlanActionV1::RespondEphemeral { .. }
            | PreparedPlanActionV1::CreateChannel { .. }
            | PreparedPlanActionV1::CreateRole { .. }
            | PreparedPlanActionV1::UpsertOverwrite { .. }
            | PreparedPlanActionV1::DeferEphemeral { .. }
            | PreparedPlanActionV1::EditResponse { .. }
            | PreparedPlanActionV1::RegisterInstance { .. } => {}
        }
    }
    let wire_digest_material = canonical_wire_material(&bindings)?;
    Ok(ActionPlanWirePreflightV1 {
        bindings,
        wire_digest_material,
    })
}

fn exact_custom_id(
    encoded: String,
    expected: ParsedCustomId,
) -> Result<ExactCustomIdV1, ActionPlanWirePreflightErrorV1> {
    if encoded.is_empty() || encoded.len() > MAX_CUSTOM_ID_BYTES {
        return Err(ActionPlanWirePreflightErrorV1::CustomIdBound);
    }
    if decode(&encoded).ok().as_ref() != Some(&expected) {
        return Err(ActionPlanWirePreflightErrorV1::CustomIdShape);
    }
    Ok(ExactCustomIdV1(encoded))
}

fn exact_manifest(
    instance: &AutomationInstance,
) -> Result<ExactPinnedInstanceManifestV1, ActionPlanWirePreflightErrorV1> {
    validate_manifest(instance.guild_id, &instance.resources)?;
    let roles = instance
        .resources
        .roles
        .iter()
        .map(|(alias, id)| ExactManifestRoleV1 {
            alias: alias.clone(),
            id: *id,
        })
        .collect::<Vec<_>>();
    let channels = instance
        .resources
        .channels
        .iter()
        .map(|(alias, id)| ExactManifestChannelV1 {
            alias: alias.clone(),
            id: *id,
        })
        .collect::<Vec<_>>();
    let messages = instance
        .resources
        .messages
        .iter()
        .map(|(alias, message)| ExactManifestMessageV1 {
            alias: alias.clone(),
            channel: message.channel,
            id: message.id,
        })
        .collect::<Vec<_>>();
    let reverse_delete_order = messages
        .iter()
        .cloned()
        .map(ExactTeardownDeleteV1::Message)
        .chain(channels.iter().cloned().map(ExactTeardownDeleteV1::Channel))
        .chain(roles.iter().cloned().map(ExactTeardownDeleteV1::Role))
        .collect();
    let canonical_manifest = canonical_manifest_json(&instance.resources)?;
    let digest =
        InteractionInstanceManifestDigestV1::parse(lower_hex(Sha256::digest(&canonical_manifest)))
            .map_err(|_| ActionPlanWirePreflightErrorV1::InvalidTeardownManifest)?;
    Ok(ExactPinnedInstanceManifestV1 {
        instance_id: instance.id.clone(),
        digest,
        roles,
        channels,
        messages,
        reverse_delete_order,
        canonical_manifest,
    })
}

fn validate_manifest(
    guild_id: discord_model::GuildId,
    resources: &InstanceResources,
) -> Result<(), ActionPlanWirePreflightErrorV1> {
    if resources.roles.len() > MAX_MANIFEST_ROLES
        || resources.channels.len() > MAX_MANIFEST_CHANNELS
        || resources.messages.len() > MAX_MANIFEST_MESSAGES
        || resources.roles.is_empty()
            && resources.channels.is_empty()
            && resources.messages.is_empty()
        || resources
            .roles
            .keys()
            .chain(resources.channels.keys())
            .chain(resources.messages.keys())
            .any(|alias| !valid_alias(alias))
        || resources
            .roles
            .values()
            .any(|id| id.0 == 0 || *id == RoleId(guild_id.0))
        || resources.channels.values().any(|id| id.0 == 0)
        || resources
            .messages
            .values()
            .any(|message| message.channel.0 == 0 || message.id.0 == 0)
    {
        return Err(ActionPlanWirePreflightErrorV1::InvalidTeardownManifest);
    }
    let role_ids = resources.roles.values().copied().collect::<BTreeSet<_>>();
    let channel_ids = resources
        .channels
        .values()
        .copied()
        .collect::<BTreeSet<_>>();
    let message_ids = resources
        .messages
        .values()
        .map(|message| (message.channel, message.id))
        .collect::<BTreeSet<_>>();
    if role_ids.len() != resources.roles.len()
        || channel_ids.len() != resources.channels.len()
        || message_ids.len() != resources.messages.len()
    {
        return Err(ActionPlanWirePreflightErrorV1::InvalidTeardownManifest);
    }
    Ok(())
}

fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= MAX_RESOURCE_ALIAS_BYTES
        && alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn canonical_manifest_json(
    resources: &InstanceResources,
) -> Result<Vec<u8>, ActionPlanWirePreflightErrorV1> {
    let mut canonical = String::new();
    canonical.push_str("{\"channels\":{");
    for (index, (alias, id)) in resources.channels.iter().enumerate() {
        if index > 0 {
            canonical.push(',');
        }
        write!(&mut canonical, "\"{alias}\":\"{}\"", id.0)
            .map_err(|_| ActionPlanWirePreflightErrorV1::InvalidTeardownManifest)?;
    }
    canonical.push_str("},\"messages\":{");
    for (index, (alias, message)) in resources.messages.iter().enumerate() {
        if index > 0 {
            canonical.push(',');
        }
        write!(
            &mut canonical,
            "\"{alias}\":{{\"channel\":\"{}\",\"id\":\"{}\"}}",
            message.channel.0, message.id.0
        )
        .map_err(|_| ActionPlanWirePreflightErrorV1::InvalidTeardownManifest)?;
    }
    canonical.push_str("},\"roles\":{");
    for (index, (alias, id)) in resources.roles.iter().enumerate() {
        if index > 0 {
            canonical.push(',');
        }
        write!(&mut canonical, "\"{alias}\":\"{}\"", id.0)
            .map_err(|_| ActionPlanWirePreflightErrorV1::InvalidTeardownManifest)?;
    }
    canonical.push_str("}}");
    if canonical.len() > MAX_MANIFEST_CANONICAL_BYTES {
        return Err(ActionPlanWirePreflightErrorV1::TeardownManifestBound);
    }
    Ok(canonical.into_bytes())
}

struct CanonicalFrameV1 {
    bytes: Vec<u8>,
}

impl CanonicalFrameV1 {
    fn new(domain: &[u8]) -> Self {
        let mut frame = Self { bytes: Vec::new() };
        frame.field(0, domain);
        frame
    }

    fn field(&mut self, tag: u16, value: &[u8]) {
        self.bytes.extend_from_slice(&tag.to_be_bytes());
        self.bytes
            .extend_from_slice(&(value.len() as u32).to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

fn canonical_wire_material(
    bindings: &[ActionPlanWireBindingV1],
) -> Result<Vec<u8>, ActionPlanWirePreflightErrorV1> {
    let mut outer = CanonicalFrameV1::new(b"starring.runtime.action_plan_wire_preflight.v1\0");
    for binding in bindings {
        let mut action = CanonicalFrameV1::new(b"wire_action.v1\0");
        action.field(1, &binding.entry().ordinal().to_be_bytes());
        match binding {
            ActionPlanWireBindingV1::OpenModal { custom_id, .. } => {
                action.field(2, b"open_modal");
                action.field(3, custom_id.as_str().as_bytes());
            }
            ActionPlanWireBindingV1::PostPanel { buttons, .. } => {
                action.field(2, b"post_panel");
                for button in buttons {
                    let mut encoded = CanonicalFrameV1::new(b"panel_button.v1\0");
                    encoded.field(1, &[button.index]);
                    encoded.field(2, button.custom_id.as_str().as_bytes());
                    action.field(4, &encoded.finish());
                }
            }
            ActionPlanWireBindingV1::TeardownInstance { manifest, .. } => {
                action.field(2, b"teardown_instance");
                action.field(5, manifest.instance_id().as_str().as_bytes());
                action.field(6, manifest.digest().as_str().as_bytes());
                action.field(7, manifest.canonical_manifest_bytes_v1());
                for deletion in manifest.reverse_delete_order() {
                    action.field(8, &canonical_delete(deletion));
                }
            }
        }
        outer.field(1, &action.finish());
        if outer.bytes.len() > MAX_WIRE_DIGEST_MATERIAL_BYTES {
            return Err(ActionPlanWirePreflightErrorV1::WireDigestMaterialBound);
        }
    }
    Ok(outer.finish())
}

fn canonical_delete(deletion: &ExactTeardownDeleteV1) -> Vec<u8> {
    let mut frame = CanonicalFrameV1::new(b"teardown_delete.v1\0");
    match deletion {
        ExactTeardownDeleteV1::Message(message) => {
            frame.field(1, b"message");
            frame.field(2, message.alias.as_bytes());
            frame.field(3, &message.channel.0.to_be_bytes());
            frame.field(4, &message.id.0.to_be_bytes());
        }
        ExactTeardownDeleteV1::Channel(channel) => {
            frame.field(1, b"channel");
            frame.field(2, channel.alias.as_bytes());
            frame.field(3, &channel.id.0.to_be_bytes());
        }
        ExactTeardownDeleteV1::Role(role) => {
            frame.field(1, b"role");
            frame.field(2, role.alias.as_bytes());
            frame.field(3, &role.id.0.to_be_bytes());
        }
    }
    frame.finish()
}

fn lower_hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use automation_core::event::ResolvedInstanceContext;
    use automation_core::plan::{ActionPlan, ModalPresentation, PlannedAction, PlannedChannel};
    use automation_core::preflight::prepare_action_plan_v1;
    use automation_core::RuntimeContext;
    use automation_instance::{
        AutomationInstance, InstanceId, InstanceKind, InstanceMessageRef, InstanceResources,
        InstanceRuleSetVersion, SequenceInstanceIdGenerator,
    };
    use automation_state::{
        ButtonRoute, ButtonSpec, CreatedRef, InstanceRef, InstanceResourceRefs, ModalFieldSpec,
        ModalFieldStyle, ModalInputPolicy,
    };
    use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};

    use super::*;

    fn created(key: &str) -> CreatedRef {
        CreatedRef {
            created: key.to_string(),
        }
    }

    fn context() -> RuntimeContext {
        RuntimeContext {
            guild_id: GuildId(7),
            actor: UserId(42),
            ruleset_key: "studyroom".to_string(),
            ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
            inputs: BTreeMap::new(),
            instance: None,
        }
    }

    fn modal() -> ModalPresentation {
        ModalPresentation {
            key: "room_modal".to_string(),
            title: "Room".to_string(),
            fields: vec![ModalFieldSpec {
                key: "name".to_string(),
                label: "Name".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
                min_length: None,
                max_length: None,
                input_policy: ModalInputPolicy::Preserve,
            }],
        }
    }

    fn wire_plan() -> ActionPlan {
        ActionPlan {
            steps: vec![
                PlannedAction::OpenModal(modal()),
                PlannedAction::CreateChannel {
                    key: "room".to_string(),
                    name: "room".to_string(),
                },
                PlannedAction::PostPanel {
                    key: "panel".to_string(),
                    channel: PlannedChannel::Created("room".to_string()),
                    content: "panel".to_string(),
                    buttons: vec![
                        ButtonSpec {
                            label: "Help".to_string(),
                            route: ButtonRoute::Static {
                                key: "help".to_string(),
                            },
                        },
                        ButtonSpec {
                            label: "Join".to_string(),
                            route: ButtonRoute::InstanceAction {
                                instance: InstanceRef::Created(created("instance")),
                                action: "join".to_string(),
                            },
                        },
                    ],
                },
                PlannedAction::RegisterInstance {
                    key: "instance".to_string(),
                    kind: InstanceKind("study_room".to_string()),
                    resources: InstanceResourceRefs {
                        roles: BTreeMap::new(),
                        channels: BTreeMap::from([("room".to_string(), created("room"))]),
                        messages: BTreeMap::from([("panel".to_string(), created("panel"))]),
                    },
                },
            ],
        }
    }

    #[test]
    fn exact_component_ids_are_precomputed_and_deterministic() {
        let first_prepared = prepare_action_plan_v1(
            &context(),
            &wire_plan(),
            &SequenceInstanceIdGenerator::new("room", 1),
        )
        .unwrap();
        let second_prepared = prepare_action_plan_v1(
            &context(),
            &wire_plan(),
            &SequenceInstanceIdGenerator::new("room", 1),
        )
        .unwrap();
        let first = preflight_action_plan_wire_v1(&first_prepared).unwrap();
        let second = preflight_action_plan_wire_v1(&second_prepared).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.bindings.len(), 2);
        match &first.bindings[0] {
            ActionPlanWireBindingV1::OpenModal { custom_id, .. } => {
                assert_eq!(custom_id.as_str(), "starring:7:studyroom:modal:room_modal")
            }
            other => panic!("unexpected binding: {other:?}"),
        }
        match &first.bindings[1] {
            ActionPlanWireBindingV1::PostPanel { buttons, .. } => {
                assert_eq!(
                    buttons[0].custom_id.as_str(),
                    "starring:7:studyroom:button:help"
                );
                assert_eq!(buttons[1].custom_id.as_str(), "starring:i:room_001:join");
            }
            other => panic!("unexpected binding: {other:?}"),
        }
        assert!(!first.wire_digest_material_v1().is_empty());
    }

    #[test]
    fn static_component_over_hundred_bytes_fails_closed() {
        let mut long_context = context();
        long_context.ruleset_key = "r".repeat(90);
        let prepared = prepare_action_plan_v1(
            &long_context,
            &ActionPlan {
                steps: vec![PlannedAction::OpenModal(modal())],
            },
            &SequenceInstanceIdGenerator::new("room", 1),
        )
        .unwrap();

        assert_eq!(
            preflight_action_plan_wire_v1(&prepared).unwrap_err(),
            ActionPlanWirePreflightErrorV1::CustomIdBound
        );
    }

    #[test]
    fn component_delimiter_in_semantic_key_fails_roundtrip() {
        let mut bad_modal = modal();
        bad_modal.key = "room:modal".to_string();
        let prepared = prepare_action_plan_v1(
            &context(),
            &ActionPlan {
                steps: vec![PlannedAction::OpenModal(bad_modal)],
            },
            &SequenceInstanceIdGenerator::new("room", 1),
        )
        .unwrap();

        assert_eq!(
            preflight_action_plan_wire_v1(&prepared).unwrap_err(),
            ActionPlanWirePreflightErrorV1::CustomIdShape
        );
    }

    fn pinned_context() -> RuntimeContext {
        let resources = InstanceResources {
            roles: BTreeMap::from([
                ("member".to_string(), RoleId(81)),
                ("owner".to_string(), RoleId(82)),
            ]),
            channels: BTreeMap::from([
                ("room".to_string(), ChannelId(71)),
                ("voice".to_string(), ChannelId(72)),
            ]),
            messages: BTreeMap::from([
                (
                    "hub".to_string(),
                    InstanceMessageRef {
                        channel: ChannelId(99),
                        id: MessageId(61),
                    },
                ),
                (
                    "panel".to_string(),
                    InstanceMessageRef {
                        channel: ChannelId(71),
                        id: MessageId(62),
                    },
                ),
            ]),
        };
        RuntimeContext {
            instance: Some(ResolvedInstanceContext {
                instance: AutomationInstance {
                    id: InstanceId::parse("room_001").unwrap(),
                    guild_id: GuildId(7),
                    ruleset_key: "studyroom".to_string(),
                    ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
                    kind: InstanceKind("study_room".to_string()),
                    created_by: UserId(42),
                    resources,
                    status: InstanceStatus::Active,
                },
                action: "close".to_string(),
            }),
            ..context()
        }
    }

    #[test]
    fn teardown_binds_sql_canonical_manifest_and_reverse_dependency_order() {
        let prepared = prepare_action_plan_v1(
            &pinned_context(),
            &ActionPlan {
                steps: vec![PlannedAction::TeardownInstance {
                    instance: InstanceRef::Event,
                }],
            },
            &SequenceInstanceIdGenerator::new("unused", 1),
        )
        .unwrap();
        let wire = preflight_action_plan_wire_v1(&prepared).unwrap();
        let manifest = wire.teardown_manifests().next().unwrap();

        assert_eq!(manifest.instance_id().as_str(), "room_001");
        assert_eq!(
            std::str::from_utf8(manifest.canonical_manifest_bytes_v1()).unwrap(),
            "{\"channels\":{\"room\":\"71\",\"voice\":\"72\"},\"messages\":{\"hub\":{\"channel\":\"99\",\"id\":\"61\"},\"panel\":{\"channel\":\"71\",\"id\":\"62\"}},\"roles\":{\"member\":\"81\",\"owner\":\"82\"}}"
        );
        assert_eq!(
            manifest.digest().as_str(),
            "c108608e8200e7e76ab6cb8bef5383e6cf5fbecd8bd84bcdf6bb80e78e525015"
        );
        assert_eq!(manifest.reverse_delete_order().len(), 6);
        assert!(matches!(
            &manifest.reverse_delete_order()[0],
            ExactTeardownDeleteV1::Message(message) if message.alias == "hub"
        ));
        assert!(matches!(
            &manifest.reverse_delete_order()[2],
            ExactTeardownDeleteV1::Channel(channel) if channel.alias == "room"
        ));
        assert!(matches!(
            &manifest.reverse_delete_order()[4],
            ExactTeardownDeleteV1::Role(role) if role.alias == "member"
        ));
        assert_eq!(manifest.roles.len(), 2);
        assert_eq!(manifest.channels.len(), 2);
        assert_eq!(manifest.messages.len(), 2);
    }

    #[test]
    fn same_plan_teardown_rejects_unresolved_discord_ids() {
        let prepared = prepare_action_plan_v1(
            &context(),
            &ActionPlan {
                steps: vec![
                    PlannedAction::RegisterInstance {
                        key: "instance".to_string(),
                        kind: InstanceKind("study_room".to_string()),
                        resources: InstanceResourceRefs::default(),
                    },
                    PlannedAction::TeardownInstance {
                        instance: InstanceRef::Created(created("instance")),
                    },
                ],
            },
            &SequenceInstanceIdGenerator::new("room", 1),
        )
        .unwrap();

        assert_eq!(
            preflight_action_plan_wire_v1(&prepared).unwrap_err(),
            ActionPlanWirePreflightErrorV1::DynamicTeardownManifest
        );
    }
}
