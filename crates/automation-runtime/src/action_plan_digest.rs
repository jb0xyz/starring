use std::collections::BTreeMap;

use automation_core::{
    ActionPlan, ModalPresentation, PlannedAction, PlannedChannel, PlannedOverwriteTarget,
    PlannedRole, RuntimeContext,
};
use automation_instance::{AutomationInstance, InstanceResources, InstanceStatus};
use automation_runtime_interaction::{
    InteractionActionPlanDigestBuilderErrorV1, InteractionActionPlanDigestBuilderV1,
    InteractionActionPlanDigestV1, InteractionRequestDigestV1, InteractionRouteBindingV1,
};
use automation_state::{
    ButtonRoute, ButtonSpec, CreatedRef, InstanceRef, InstanceResourceRefs, ModalFieldSpec,
    ModalFieldStyle, ModalInputPolicy,
};

const ACTION_PAYLOAD_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.action.v1\0";
const CONTEXT_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.context.v1\0";
const RESOLVED_INSTANCE_CONTEXT_DOMAIN_V1: &[u8] =
    b"starring.runtime.action_plan_projection.resolved_instance_context.v1\0";
const INSTANCE_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.instance.v1\0";
const INSTANCE_RESOURCES_DOMAIN_V1: &[u8] =
    b"starring.runtime.action_plan_projection.instance_resources.v1\0";
const INSTANCE_RESOURCE_REFS_DOMAIN_V1: &[u8] =
    b"starring.runtime.action_plan_projection.instance_resource_refs.v1\0";
const MAP_ENTRY_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.map_entry.v1\0";
const MODAL_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.modal.v1\0";
const MODAL_FIELD_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.modal_field.v1\0";
const BUTTON_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.button.v1\0";
const BUTTON_ROUTE_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.button_route.v1\0";
const PLANNED_ROLE_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.planned_role.v1\0";
const PLANNED_CHANNEL_DOMAIN_V1: &[u8] =
    b"starring.runtime.action_plan_projection.planned_channel.v1\0";
const OVERWRITE_TARGET_DOMAIN_V1: &[u8] =
    b"starring.runtime.action_plan_projection.overwrite_target.v1\0";
const INSTANCE_REF_DOMAIN_V1: &[u8] = b"starring.runtime.action_plan_projection.instance_ref.v1\0";
const PROJECTION_VERSION_V1: u16 = 1;

pub fn build_interaction_action_plan_digest_v1(
    route: &InteractionRouteBindingV1,
    request_digest: &InteractionRequestDigestV1,
    context: &RuntimeContext,
    plan: &ActionPlan,
    leading_defer_ephemeral: bool,
) -> Result<InteractionActionPlanDigestV1, InteractionActionPlanDigestBuilderErrorV1> {
    let mut builder =
        InteractionActionPlanDigestBuilderV1::new(route, request_digest, leading_defer_ephemeral);
    for (index, action) in plan.steps.iter().enumerate() {
        let (kind, discriminant) = action_identity_v1(action);
        let payload = action_payload_v1(action, discriminant, (index == 0).then_some(context));
        builder.push_action(kind, &payload)?;
    }
    builder.finish()
}

fn action_identity_v1(action: &PlannedAction) -> (&'static str, u8) {
    match action {
        PlannedAction::GrantRole { .. } => ("grant_role", 1),
        PlannedAction::RespondEphemeral { .. } => ("respond_ephemeral", 2),
        PlannedAction::OpenModal(_) => ("open_modal", 3),
        PlannedAction::CreateChannel { .. } => ("create_channel", 4),
        PlannedAction::CreateRole { .. } => ("create_role", 5),
        PlannedAction::UpsertOverwrite { .. } => ("upsert_overwrite", 6),
        PlannedAction::PostPanel { .. } => ("post_panel", 7),
        PlannedAction::DeferEphemeral => ("defer_ephemeral", 8),
        PlannedAction::EditResponse { .. } => ("edit_response", 9),
        PlannedAction::RegisterInstance { .. } => ("register_instance", 10),
        PlannedAction::TeardownInstance { .. } => ("teardown_instance", 11),
    }
}

fn action_payload_v1(
    action: &PlannedAction,
    discriminant: u8,
    context: Option<&RuntimeContext>,
) -> Vec<u8> {
    let mut payload = CanonicalFrameV1::new(ACTION_PAYLOAD_DOMAIN_V1);
    payload.u8(3, u8::from(context.is_some()));
    if let Some(context) = context {
        payload.nested(4, encode_runtime_context_v1(context));
    }
    payload.u8(5, discriminant);
    match action {
        PlannedAction::GrantRole { role, target } => {
            payload.nested(10, encode_planned_role_v1(role));
            payload.u64(11, target.0);
        }
        PlannedAction::RespondEphemeral { content } => payload.text(10, content),
        PlannedAction::OpenModal(modal) => payload.nested(10, encode_modal_v1(modal)),
        PlannedAction::CreateChannel { key, name } | PlannedAction::CreateRole { key, name } => {
            payload.text(10, key);
            payload.text(11, name);
        }
        PlannedAction::UpsertOverwrite {
            channel,
            target,
            allow,
            deny,
        } => {
            payload.nested(10, encode_planned_channel_v1(channel));
            payload.nested(11, encode_overwrite_target_v1(target));
            payload.u64(12, allow.bits());
            payload.u64(13, deny.bits());
        }
        PlannedAction::PostPanel {
            key,
            channel,
            content,
            buttons,
        } => {
            payload.text(10, key);
            payload.nested(11, encode_planned_channel_v1(channel));
            payload.text(12, content);
            payload.u64(13, usize_to_u64_v1(buttons.len()));
            for button in buttons {
                payload.nested(14, encode_button_v1(button));
            }
        }
        PlannedAction::DeferEphemeral => {}
        PlannedAction::EditResponse { content } => payload.text(10, content),
        PlannedAction::RegisterInstance {
            key,
            kind,
            resources,
        } => {
            payload.text(10, key);
            payload.text(11, &kind.0);
            payload.nested(12, encode_instance_resource_refs_v1(resources));
        }
        PlannedAction::TeardownInstance { instance } => {
            payload.nested(10, encode_instance_ref_v1(instance));
        }
    }
    payload.finish()
}

fn encode_runtime_context_v1(context: &RuntimeContext) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(CONTEXT_DOMAIN_V1);
    frame.u64(3, context.guild_id.0);
    frame.u64(4, context.actor.0);
    frame.text(5, &context.ruleset_key);
    frame.u32(6, context.ruleset_version.get());
    frame.u64(7, usize_to_u64_v1(context.inputs.len()));
    for (key, value) in &context.inputs {
        frame.nested(8, encode_text_map_entry_v1(key, value));
    }
    frame.u8(9, u8::from(context.instance.is_some()));
    if let Some(resolved) = &context.instance {
        let mut nested = CanonicalFrameV1::new(RESOLVED_INSTANCE_CONTEXT_DOMAIN_V1);
        nested.nested(3, encode_instance_v1(&resolved.instance));
        nested.text(4, &resolved.action);
        frame.nested(10, nested);
    }
    frame
}

fn encode_instance_v1(instance: &AutomationInstance) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(INSTANCE_DOMAIN_V1);
    frame.text(3, instance.id.as_str());
    frame.u64(4, instance.guild_id.0);
    frame.text(5, &instance.ruleset_key);
    frame.u32(6, instance.ruleset_version.get());
    frame.text(7, &instance.kind.0);
    frame.u64(8, instance.created_by.0);
    frame.nested(9, encode_instance_resources_v1(&instance.resources));
    frame.u8(10, instance_status_discriminant_v1(instance.status));
    frame
}

fn instance_status_discriminant_v1(status: InstanceStatus) -> u8 {
    match status {
        InstanceStatus::Active => 1,
        InstanceStatus::Deleting => 2,
        InstanceStatus::Disabled => 3,
        InstanceStatus::Deleted => 4,
    }
}

fn encode_instance_resources_v1(resources: &InstanceResources) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(INSTANCE_RESOURCES_DOMAIN_V1);
    frame.u64(3, usize_to_u64_v1(resources.roles.len()));
    for (alias, role_id) in &resources.roles {
        frame.nested(4, encode_u64_map_entry_v1(alias, role_id.0));
    }
    frame.u64(5, usize_to_u64_v1(resources.channels.len()));
    for (alias, channel_id) in &resources.channels {
        frame.nested(6, encode_u64_map_entry_v1(alias, channel_id.0));
    }
    frame.u64(7, usize_to_u64_v1(resources.messages.len()));
    for (alias, message) in &resources.messages {
        let mut entry = CanonicalFrameV1::new(MAP_ENTRY_DOMAIN_V1);
        entry.text(3, alias);
        entry.u64(4, message.channel.0);
        entry.u64(5, message.id.0);
        frame.nested(8, entry);
    }
    frame
}

fn encode_instance_resource_refs_v1(resources: &InstanceResourceRefs) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(INSTANCE_RESOURCE_REFS_DOMAIN_V1);
    encode_created_ref_map_v1(&mut frame, 3, 4, &resources.roles);
    encode_created_ref_map_v1(&mut frame, 5, 6, &resources.channels);
    encode_created_ref_map_v1(&mut frame, 7, 8, &resources.messages);
    frame
}

fn encode_created_ref_map_v1(
    frame: &mut CanonicalFrameV1,
    count_tag: u16,
    entry_tag: u16,
    values: &BTreeMap<String, CreatedRef>,
) {
    frame.u64(count_tag, usize_to_u64_v1(values.len()));
    for (alias, created) in values {
        frame.nested(entry_tag, encode_text_map_entry_v1(alias, &created.created));
    }
}

fn encode_modal_v1(modal: &ModalPresentation) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(MODAL_DOMAIN_V1);
    frame.text(3, &modal.key);
    frame.text(4, &modal.title);
    frame.u64(5, usize_to_u64_v1(modal.fields.len()));
    for field in &modal.fields {
        frame.nested(6, encode_modal_field_v1(field));
    }
    frame
}

fn encode_modal_field_v1(field: &ModalFieldSpec) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(MODAL_FIELD_DOMAIN_V1);
    frame.text(3, &field.key);
    frame.text(4, &field.label);
    frame.u8(
        5,
        match field.style {
            ModalFieldStyle::Short => 1,
            ModalFieldStyle::Paragraph => 2,
        },
    );
    frame.u8(6, u8::from(field.required));
    frame.optional_u16(7, 8, field.min_length);
    frame.optional_u16(9, 10, field.max_length);
    frame.u8(
        11,
        match field.input_policy {
            ModalInputPolicy::Preserve => 1,
            ModalInputPolicy::TrimUnicodeWhitespace => 2,
        },
    );
    frame
}

fn encode_button_v1(button: &ButtonSpec) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(BUTTON_DOMAIN_V1);
    frame.text(3, &button.label);
    frame.nested(4, encode_button_route_v1(&button.route));
    frame
}

fn encode_button_route_v1(route: &ButtonRoute) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(BUTTON_ROUTE_DOMAIN_V1);
    match route {
        ButtonRoute::Static { key } => {
            frame.u8(3, 1);
            frame.text(4, key);
        }
        ButtonRoute::InstanceAction { instance, action } => {
            frame.u8(3, 2);
            frame.nested(4, encode_instance_ref_v1(instance));
            frame.text(5, action);
        }
    }
    frame
}

fn encode_planned_role_v1(role: &PlannedRole) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(PLANNED_ROLE_DOMAIN_V1);
    match role {
        PlannedRole::Resolved(role_id) => {
            frame.u8(3, 1);
            frame.u64(4, role_id.0);
        }
        PlannedRole::Created(key) => {
            frame.u8(3, 2);
            frame.text(4, key);
        }
        PlannedRole::Instance { alias } => {
            frame.u8(3, 3);
            frame.text(4, alias);
        }
    }
    frame
}

fn encode_planned_channel_v1(channel: &PlannedChannel) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(PLANNED_CHANNEL_DOMAIN_V1);
    match channel {
        PlannedChannel::Resolved(channel_id) => {
            frame.u8(3, 1);
            frame.u64(4, channel_id.0);
        }
        PlannedChannel::Created(key) => {
            frame.u8(3, 2);
            frame.text(4, key);
        }
    }
    frame
}

fn encode_overwrite_target_v1(target: &PlannedOverwriteTarget) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(OVERWRITE_TARGET_DOMAIN_V1);
    match target {
        PlannedOverwriteTarget::Everyone => frame.u8(3, 1),
        PlannedOverwriteTarget::Role(role) => {
            frame.u8(3, 2);
            frame.nested(4, encode_planned_role_v1(role));
        }
    }
    frame
}

fn encode_instance_ref_v1(instance: &InstanceRef) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(INSTANCE_REF_DOMAIN_V1);
    match instance {
        InstanceRef::Event => frame.u8(3, 1),
        InstanceRef::Created(created) => {
            frame.u8(3, 2);
            frame.text(4, &created.created);
        }
    }
    frame
}

fn encode_text_map_entry_v1(key: &str, value: &str) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(MAP_ENTRY_DOMAIN_V1);
    frame.text(3, key);
    frame.text(4, value);
    frame
}

fn encode_u64_map_entry_v1(key: &str, value: u64) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(MAP_ENTRY_DOMAIN_V1);
    frame.text(3, key);
    frame.u64(4, value);
    frame
}

fn usize_to_u64_v1(value: usize) -> u64 {
    u64::try_from(value).expect("collection length fits the canonical u64 frame")
}

struct CanonicalFrameV1 {
    bytes: Vec<u8>,
}

impl CanonicalFrameV1 {
    fn new(domain: &[u8]) -> Self {
        let mut frame = Self {
            bytes: Vec::with_capacity(256),
        };
        frame.bytes(1, domain);
        frame.u16(2, PROJECTION_VERSION_V1);
        frame
    }

    fn bytes(&mut self, tag: u16, value: &[u8]) {
        self.bytes.extend_from_slice(&tag.to_be_bytes());
        self.bytes
            .extend_from_slice(&usize_to_u64_v1(value.len()).to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn text(&mut self, tag: u16, value: &str) {
        self.bytes(tag, value.as_bytes());
    }

    fn u8(&mut self, tag: u16, value: u8) {
        self.bytes(tag, &[value]);
    }

    fn u16(&mut self, tag: u16, value: u16) {
        self.bytes(tag, &value.to_be_bytes());
    }

    fn u32(&mut self, tag: u16, value: u32) {
        self.bytes(tag, &value.to_be_bytes());
    }

    fn u64(&mut self, tag: u16, value: u64) {
        self.bytes(tag, &value.to_be_bytes());
    }

    fn optional_u16(&mut self, discriminant_tag: u16, value_tag: u16, value: Option<u16>) {
        self.u8(discriminant_tag, u8::from(value.is_some()));
        if let Some(value) = value {
            self.u16(value_tag, value);
        }
    }

    fn nested(&mut self, tag: u16, value: CanonicalFrameV1) {
        self.bytes(tag, &value.finish());
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests;
