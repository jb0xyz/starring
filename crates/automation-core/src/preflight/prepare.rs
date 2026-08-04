use std::collections::{BTreeMap, BTreeSet};

use crate::event::RuntimeContext;
use crate::plan::{ActionPlan, PlannedAction, PlannedChannel, PlannedOverwriteTarget, PlannedRole};
use crate::template::{SanitizeContext, TemplateString};
use automation_instance::{InstanceId, InstanceIdGenerator};
use automation_state::{ButtonRoute, InstanceRef, InstanceResourceRefs};

use super::types::{
    bounded_key, validate_buttons_shape, validate_modal_shape, ActionEntryIdV1,
    ActionInputDependencyV1, ActionPlanPreflightErrorV1, ActionPlanSnapshotRequestV1,
    CreatedChannelOutputRefV1, CreatedInstanceOutputRefV1, CreatedMessageOutputRefV1,
    CreatedRoleOutputRefV1, FreshObservationV1, PreflightButtonRouteV1, PreflightButtonSpecV1,
    PreflightChannelRefV1, PreflightInstanceRefV1, PreflightInstanceResourceRefsV1,
    PreflightOverwriteTargetV1, PreflightRoleRefV1, PreparedActionPlanV1, PreparedPlanActionV1,
    ProducerOutputKindV1, MAX_PREFLIGHT_ACTIONS_V1, MAX_PREFLIGHT_DIGEST_MATERIAL_BYTES_V1,
};

#[derive(Clone, Copy)]
struct ProducerV1 {
    entry: ActionEntryIdV1,
    kind: ProducerOutputKindV1,
}

struct PreparationV1<'a> {
    context: &'a RuntimeContext,
    producers: BTreeMap<String, ProducerV1>,
    preallocated_instances: BTreeMap<ActionEntryIdV1, InstanceId>,
    snapshot_request: ActionPlanSnapshotRequestV1,
    dependencies: BTreeMap<ActionEntryIdV1, BTreeSet<ActionInputDependencyV1>>,
}

pub fn prepare_action_plan_v1<G>(
    context: &RuntimeContext,
    plan: &ActionPlan,
    instance_ids: &G,
) -> Result<PreparedActionPlanV1, ActionPlanPreflightErrorV1>
where
    G: InstanceIdGenerator,
{
    validate_context(context)?;
    if plan.steps.len() > MAX_PREFLIGHT_ACTIONS_V1 {
        return Err(ActionPlanPreflightErrorV1::TooManyActions {
            count: plan.steps.len(),
            limit: MAX_PREFLIGHT_ACTIONS_V1,
        });
    }
    let producers = index_producers(plan)?;
    let preallocated_instances = preallocate_instances(&producers, instance_ids)?;
    let mut preparation = PreparationV1 {
        context,
        producers,
        preallocated_instances,
        snapshot_request: ActionPlanSnapshotRequestV1::new(context.guild_id, context.actor),
        dependencies: BTreeMap::new(),
    };
    let mut actions = Vec::with_capacity(plan.steps.len());
    for (index, step) in plan.steps.iter().enumerate() {
        let entry = ActionEntryIdV1::from_index(index)?;
        preparation
            .dependencies
            .entry(entry)
            .or_default()
            .insert(ActionInputDependencyV1::Static);
        actions.push(preparation.prepare_action(entry, step)?);
    }
    let digest_material = canonical_digest_material_v1(
        context,
        &actions,
        &preparation.snapshot_request,
        &preparation.dependencies,
    )?;
    Ok(PreparedActionPlanV1 {
        context: context.clone(),
        actions,
        snapshot_request: preparation.snapshot_request,
        dependencies: preparation.dependencies,
        digest_material,
    })
}

fn validate_context(context: &RuntimeContext) -> Result<(), ActionPlanPreflightErrorV1> {
    if context.guild_id.0 == 0
        || context.actor.0 == 0
        || !bounded_key(&context.ruleset_key)
        || context
            .instance
            .as_ref()
            .is_some_and(|resolved| resolved.instance.guild_id != context.guild_id)
    {
        return Err(ActionPlanPreflightErrorV1::InvalidContext);
    }
    Ok(())
}

fn index_producers(
    plan: &ActionPlan,
) -> Result<BTreeMap<String, ProducerV1>, ActionPlanPreflightErrorV1> {
    let mut producers = BTreeMap::new();
    for (index, action) in plan.steps.iter().enumerate() {
        let entry = ActionEntryIdV1::from_index(index)?;
        let produced = match action {
            PlannedAction::CreateRole { key, .. } => Some((key, ProducerOutputKindV1::Role)),
            PlannedAction::CreateChannel { key, .. } => Some((key, ProducerOutputKindV1::Channel)),
            PlannedAction::PostPanel { key, .. } => Some((key, ProducerOutputKindV1::Message)),
            PlannedAction::RegisterInstance { key, .. } => {
                Some((key, ProducerOutputKindV1::Instance))
            }
            PlannedAction::GrantRole { .. }
            | PlannedAction::RespondEphemeral { .. }
            | PlannedAction::OpenModal(_)
            | PlannedAction::UpsertOverwrite { .. }
            | PlannedAction::DeferEphemeral
            | PlannedAction::EditResponse { .. }
            | PlannedAction::TeardownInstance { .. } => None,
        };
        if let Some((key, kind)) = produced {
            if !bounded_key(key) {
                return Err(ActionPlanPreflightErrorV1::InvalidKey { entry });
            }
            if let Some(first) = producers.insert(key.clone(), ProducerV1 { entry, kind }) {
                return Err(ActionPlanPreflightErrorV1::DuplicateProducerKey {
                    key: key.clone(),
                    first: first.entry,
                    duplicate: entry,
                });
            }
        }
    }
    Ok(producers)
}

fn preallocate_instances<G>(
    producers: &BTreeMap<String, ProducerV1>,
    instance_ids: &G,
) -> Result<BTreeMap<ActionEntryIdV1, InstanceId>, ActionPlanPreflightErrorV1>
where
    G: InstanceIdGenerator,
{
    let mut instance_entries = producers
        .values()
        .filter(|producer| producer.kind == ProducerOutputKindV1::Instance)
        .map(|producer| producer.entry)
        .collect::<Vec<_>>();
    instance_entries.sort_unstable();
    let mut ids = BTreeMap::new();
    let mut unique = BTreeSet::new();
    for entry in instance_entries {
        let id = instance_ids
            .generate()
            .map_err(|error| ActionPlanPreflightErrorV1::InstanceIdGeneration { entry, error })?;
        if !unique.insert(id.clone()) {
            return Err(ActionPlanPreflightErrorV1::DuplicatePreallocatedInstanceId { entry });
        }
        ids.insert(entry, id);
    }
    Ok(ids)
}

impl PreparationV1<'_> {
    fn prepare_action(
        &mut self,
        entry: ActionEntryIdV1,
        action: &PlannedAction,
    ) -> Result<PreparedPlanActionV1, ActionPlanPreflightErrorV1> {
        match action {
            PlannedAction::GrantRole { role, target } => {
                if *target != self.context.actor {
                    return Err(ActionPlanPreflightErrorV1::UnsupportedGrantTarget { entry });
                }
                self.observe_role_state(entry);
                self.observe_actor(entry);
                Ok(PreparedPlanActionV1::GrantRole {
                    entry,
                    role: self.resolve_role(entry, role)?,
                    target: *target,
                })
            }
            PlannedAction::RespondEphemeral { content } => {
                Ok(PreparedPlanActionV1::RespondEphemeral {
                    entry,
                    content: self.render(
                        entry,
                        content,
                        SanitizeContext::EphemeralMessageContent,
                    )?,
                })
            }
            PlannedAction::OpenModal(modal) => {
                validate_modal_shape(entry, modal)?;
                Ok(PreparedPlanActionV1::OpenModal {
                    entry,
                    modal: modal.clone(),
                })
            }
            PlannedAction::CreateChannel { key, name } => {
                self.observe_channel_state(entry);
                Ok(PreparedPlanActionV1::CreateChannel {
                    entry,
                    output: CreatedChannelOutputRefV1::new(entry),
                    key: key.clone(),
                    name: self.render(entry, name, SanitizeContext::ChannelName)?,
                })
            }
            PlannedAction::CreateRole { key, name } => {
                self.observe_role_state(entry);
                Ok(PreparedPlanActionV1::CreateRole {
                    entry,
                    output: CreatedRoleOutputRefV1::new(entry),
                    key: key.clone(),
                    name: self.render(entry, name, SanitizeContext::RoleName)?,
                })
            }
            PlannedAction::UpsertOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                if allow.intersects(*deny) || (allow.is_empty() && deny.is_empty()) {
                    return Err(ActionPlanPreflightErrorV1::ConflictingOverwrite { entry });
                }
                self.observe_role_state(entry);
                self.observe_channel_state(entry);
                let channel = self.resolve_channel(entry, channel)?;
                let target = match target {
                    PlannedOverwriteTarget::Everyone => PreflightOverwriteTargetV1::Everyone,
                    PlannedOverwriteTarget::Role(role) => {
                        PreflightOverwriteTargetV1::Role(self.resolve_role(entry, role)?)
                    }
                };
                Ok(PreparedPlanActionV1::UpsertOverwrite {
                    entry,
                    channel,
                    target,
                    allow: *allow,
                    deny: *deny,
                })
            }
            PlannedAction::PostPanel {
                key,
                channel,
                content,
                buttons,
            } => {
                validate_buttons_shape(entry, buttons)?;
                self.observe_role_state(entry);
                self.observe_channel_state(entry);
                Ok(PreparedPlanActionV1::PostPanel {
                    entry,
                    output: CreatedMessageOutputRefV1::new(entry),
                    key: key.clone(),
                    channel: self.resolve_channel(entry, channel)?,
                    content: self.render(
                        entry,
                        content,
                        SanitizeContext::EphemeralMessageContent,
                    )?,
                    buttons: buttons
                        .iter()
                        .map(|button| self.resolve_button(entry, button))
                        .collect::<Result<Vec<_>, _>>()?,
                })
            }
            PlannedAction::DeferEphemeral => Ok(PreparedPlanActionV1::DeferEphemeral { entry }),
            PlannedAction::EditResponse { content } => Ok(PreparedPlanActionV1::EditResponse {
                entry,
                content: self.render(entry, content, SanitizeContext::EphemeralMessageContent)?,
            }),
            PlannedAction::RegisterInstance {
                key,
                kind,
                resources,
            } => {
                if !bounded_key(&kind.0) {
                    return Err(ActionPlanPreflightErrorV1::InvalidKey { entry });
                }
                let id = self
                    .preallocated_instances
                    .get(&entry)
                    .cloned()
                    .ok_or(ActionPlanPreflightErrorV1::ExecutionInvariant { entry })?;
                Ok(PreparedPlanActionV1::RegisterInstance {
                    entry,
                    output: CreatedInstanceOutputRefV1::new(entry),
                    key: key.clone(),
                    id,
                    kind: kind.clone(),
                    resources: self.resolve_manifest(entry, resources)?,
                })
            }
            PlannedAction::TeardownInstance { instance } => {
                self.observe_role_state(entry);
                self.observe_channel_state(entry);
                Ok(PreparedPlanActionV1::TeardownInstance {
                    entry,
                    instance: self.resolve_teardown_instance(entry, instance)?,
                })
            }
        }
    }

    fn render(
        &self,
        entry: ActionEntryIdV1,
        source: &str,
        sanitize: SanitizeContext,
    ) -> Result<String, ActionPlanPreflightErrorV1> {
        TemplateString::parse(source)
            .and_then(|template| template.render(&self.context.inputs, sanitize))
            .map_err(|error| ActionPlanPreflightErrorV1::Template { entry, error })
    }

    fn resolve_role(
        &mut self,
        entry: ActionEntryIdV1,
        role: &PlannedRole,
    ) -> Result<PreflightRoleRefV1, ActionPlanPreflightErrorV1> {
        match role {
            PlannedRole::Resolved(role_id) => {
                if role_id.0 == 0 {
                    return Err(ActionPlanPreflightErrorV1::RequiredRoleMissing(*role_id));
                }
                self.snapshot_request.require_role(*role_id);
                Ok(PreflightRoleRefV1::Existing(*role_id))
            }
            PlannedRole::Created(key) => {
                let producer =
                    self.resolve_producer(entry, key, ProducerOutputKindV1::Role, true)?;
                Ok(PreflightRoleRefV1::Produced(CreatedRoleOutputRefV1::new(
                    producer.entry,
                )))
            }
            PlannedRole::Instance { alias } => {
                if !bounded_key(alias) {
                    return Err(ActionPlanPreflightErrorV1::InvalidKey { entry });
                }
                let resolved = self
                    .context
                    .instance
                    .as_ref()
                    .ok_or(ActionPlanPreflightErrorV1::InstanceContextMissing { entry })?;
                if resolved.instance.guild_id != self.context.guild_id {
                    return Err(ActionPlanPreflightErrorV1::InstanceContextGuildMismatch { entry });
                }
                let role_id = resolved
                    .instance
                    .resources
                    .roles
                    .get(alias)
                    .copied()
                    .ok_or_else(|| ActionPlanPreflightErrorV1::InstanceResourceMissing {
                        entry,
                        alias: alias.clone(),
                    })?;
                self.snapshot_request.require_role(role_id);
                Ok(PreflightRoleRefV1::Instance(role_id))
            }
        }
    }

    fn resolve_channel(
        &mut self,
        entry: ActionEntryIdV1,
        channel: &PlannedChannel,
    ) -> Result<PreflightChannelRefV1, ActionPlanPreflightErrorV1> {
        match channel {
            PlannedChannel::Resolved(channel_id) => {
                if channel_id.0 == 0 {
                    return Err(ActionPlanPreflightErrorV1::RequiredChannelMissing(
                        *channel_id,
                    ));
                }
                self.snapshot_request.require_channel(*channel_id);
                Ok(PreflightChannelRefV1::Existing(*channel_id))
            }
            PlannedChannel::Created(key) => {
                let producer =
                    self.resolve_producer(entry, key, ProducerOutputKindV1::Channel, true)?;
                Ok(PreflightChannelRefV1::Produced(
                    CreatedChannelOutputRefV1::new(producer.entry),
                ))
            }
        }
    }

    fn resolve_button(
        &mut self,
        entry: ActionEntryIdV1,
        button: &automation_state::ButtonSpec,
    ) -> Result<PreflightButtonSpecV1, ActionPlanPreflightErrorV1> {
        let route = match &button.route {
            ButtonRoute::Static { key } => PreflightButtonRouteV1::Static { key: key.clone() },
            ButtonRoute::InstanceAction { instance, action } => match instance {
                InstanceRef::Event => {
                    let resolved = self
                        .context
                        .instance
                        .as_ref()
                        .ok_or(ActionPlanPreflightErrorV1::InstanceContextMissing { entry })?;
                    if resolved.instance.guild_id != self.context.guild_id {
                        return Err(ActionPlanPreflightErrorV1::InstanceContextGuildMismatch {
                            entry,
                        });
                    }
                    PreflightButtonRouteV1::InstanceAction {
                        instance_id: resolved.instance.id.clone(),
                        producer: None,
                        action: action.clone(),
                    }
                }
                InstanceRef::Created(created) => {
                    let producer = self.resolve_producer(
                        entry,
                        &created.created,
                        ProducerOutputKindV1::Instance,
                        false,
                    )?;
                    let instance_id = self
                        .preallocated_instances
                        .get(&producer.entry)
                        .cloned()
                        .ok_or(ActionPlanPreflightErrorV1::ExecutionInvariant { entry })?;
                    PreflightButtonRouteV1::InstanceAction {
                        instance_id,
                        producer: Some(CreatedInstanceOutputRefV1::new(producer.entry)),
                        action: action.clone(),
                    }
                }
            },
        };
        Ok(PreflightButtonSpecV1 {
            label: button.label.clone(),
            route,
        })
    }

    fn resolve_manifest(
        &mut self,
        entry: ActionEntryIdV1,
        resources: &InstanceResourceRefs,
    ) -> Result<PreflightInstanceResourceRefsV1, ActionPlanPreflightErrorV1> {
        let mut resolved = PreflightInstanceResourceRefsV1::default();
        for (alias, created) in &resources.roles {
            if !bounded_key(alias) {
                return Err(ActionPlanPreflightErrorV1::InvalidKey { entry });
            }
            let producer =
                self.resolve_producer(entry, &created.created, ProducerOutputKindV1::Role, true)?;
            resolved
                .roles
                .insert(alias.clone(), CreatedRoleOutputRefV1::new(producer.entry));
        }
        for (alias, created) in &resources.channels {
            if !bounded_key(alias) {
                return Err(ActionPlanPreflightErrorV1::InvalidKey { entry });
            }
            let producer = self.resolve_producer(
                entry,
                &created.created,
                ProducerOutputKindV1::Channel,
                true,
            )?;
            resolved.channels.insert(
                alias.clone(),
                CreatedChannelOutputRefV1::new(producer.entry),
            );
        }
        for (alias, created) in &resources.messages {
            if !bounded_key(alias) {
                return Err(ActionPlanPreflightErrorV1::InvalidKey { entry });
            }
            let producer = self.resolve_producer(
                entry,
                &created.created,
                ProducerOutputKindV1::Message,
                true,
            )?;
            resolved.messages.insert(
                alias.clone(),
                CreatedMessageOutputRefV1::new(producer.entry),
            );
        }
        Ok(resolved)
    }

    fn resolve_teardown_instance(
        &mut self,
        entry: ActionEntryIdV1,
        instance: &InstanceRef,
    ) -> Result<PreflightInstanceRefV1, ActionPlanPreflightErrorV1> {
        match instance {
            InstanceRef::Event => {
                let resolved = self
                    .context
                    .instance
                    .as_ref()
                    .ok_or(ActionPlanPreflightErrorV1::InstanceContextMissing { entry })?;
                if resolved.instance.guild_id != self.context.guild_id {
                    return Err(ActionPlanPreflightErrorV1::InstanceContextGuildMismatch { entry });
                }
                for role_id in resolved.instance.resources.roles.values() {
                    self.snapshot_request.require_role(*role_id);
                }
                for channel_id in resolved.instance.resources.channels.values() {
                    self.snapshot_request.require_channel(*channel_id);
                }
                for message in resolved.instance.resources.messages.values() {
                    self.snapshot_request.require_channel(message.channel);
                }
                Ok(PreflightInstanceRefV1::Existing(
                    resolved.instance.id.clone(),
                ))
            }
            InstanceRef::Created(created) => {
                let producer = self.resolve_producer(
                    entry,
                    &created.created,
                    ProducerOutputKindV1::Instance,
                    true,
                )?;
                Ok(PreflightInstanceRefV1::Registered(
                    CreatedInstanceOutputRefV1::new(producer.entry),
                ))
            }
        }
    }

    fn resolve_producer(
        &mut self,
        entry: ActionEntryIdV1,
        key: &str,
        expected: ProducerOutputKindV1,
        must_be_prior: bool,
    ) -> Result<ProducerV1, ActionPlanPreflightErrorV1> {
        let producer = self.producers.get(key).copied().ok_or_else(|| {
            ActionPlanPreflightErrorV1::UnknownProducer {
                entry,
                key: key.to_string(),
                expected,
            }
        })?;
        if producer.kind != expected {
            return Err(ActionPlanPreflightErrorV1::ProducerTypeMismatch {
                entry,
                key: key.to_string(),
                expected,
                actual: producer.kind,
            });
        }
        if must_be_prior {
            if producer.entry >= entry {
                return Err(ActionPlanPreflightErrorV1::ProducerNotPrior {
                    entry,
                    producer: producer.entry,
                });
            }
            self.dependencies
                .entry(entry)
                .or_default()
                .insert(ActionInputDependencyV1::PriorEffect(producer.entry));
        }
        Ok(producer)
    }

    fn observe_role_state(&mut self, entry: ActionEntryIdV1) {
        self.observe(entry, FreshObservationV1::GuildRoles);
        self.observe(entry, FreshObservationV1::BotMember);
    }

    fn observe_channel_state(&mut self, entry: ActionEntryIdV1) {
        self.observe(entry, FreshObservationV1::GuildChannels);
        self.observe(entry, FreshObservationV1::GuildRoles);
        self.observe(entry, FreshObservationV1::BotMember);
    }

    fn observe_actor(&mut self, entry: ActionEntryIdV1) {
        self.observe(entry, FreshObservationV1::ActorMember);
    }

    fn observe(&mut self, entry: ActionEntryIdV1, observation: FreshObservationV1) {
        self.snapshot_request.observe(observation);
        self.dependencies
            .entry(entry)
            .or_default()
            .insert(ActionInputDependencyV1::FreshObservation(observation));
    }
}

struct CanonicalBytesV1 {
    bytes: Vec<u8>,
}

impl CanonicalBytesV1 {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn entry(&mut self, entry: ActionEntryIdV1) {
        self.u16(entry.ordinal());
    }
}

fn canonical_digest_material_v1(
    context: &RuntimeContext,
    actions: &[PreparedPlanActionV1],
    request: &ActionPlanSnapshotRequestV1,
    dependencies: &BTreeMap<ActionEntryIdV1, BTreeSet<ActionInputDependencyV1>>,
) -> Result<Vec<u8>, ActionPlanPreflightErrorV1> {
    let mut out = CanonicalBytesV1::new();
    out.string("automation-core-action-preflight-v1");
    out.u64(context.guild_id.0);
    out.u64(context.actor.0);
    out.string(&context.ruleset_key);
    out.u64(u64::from(context.ruleset_version.get()));
    encode_snapshot_request(&mut out, request);
    out.u16(dependencies.len() as u16);
    for (entry, values) in dependencies {
        out.entry(*entry);
        out.u16(values.len() as u16);
        for value in values {
            match value {
                ActionInputDependencyV1::Static => out.byte(0),
                ActionInputDependencyV1::PriorEffect(producer) => {
                    out.byte(1);
                    out.entry(*producer);
                }
                ActionInputDependencyV1::FreshObservation(observation) => {
                    out.byte(2);
                    out.byte(observation_tag(*observation));
                }
            }
        }
    }
    out.u16(actions.len() as u16);
    for action in actions {
        encode_action(&mut out, action);
    }
    if out.bytes.len() > MAX_PREFLIGHT_DIGEST_MATERIAL_BYTES_V1 {
        return Err(ActionPlanPreflightErrorV1::DigestMaterialTooLarge {
            size: out.bytes.len(),
            limit: MAX_PREFLIGHT_DIGEST_MATERIAL_BYTES_V1,
        });
    }
    Ok(out.bytes)
}

fn encode_snapshot_request(out: &mut CanonicalBytesV1, request: &ActionPlanSnapshotRequestV1) {
    out.u64(request.guild_id().0);
    out.u64(request.actor().0);
    out.u16(request.observations().len() as u16);
    for observation in request.observations() {
        out.byte(observation_tag(*observation));
    }
    out.u16(request.existing_roles().len() as u16);
    for role_id in request.existing_roles() {
        out.u64(role_id.0);
    }
    out.u16(request.existing_channels().len() as u16);
    for channel_id in request.existing_channels() {
        out.u64(channel_id.0);
    }
}

fn observation_tag(observation: FreshObservationV1) -> u8 {
    match observation {
        FreshObservationV1::GuildRoles => 0,
        FreshObservationV1::GuildChannels => 1,
        FreshObservationV1::BotMember => 2,
        FreshObservationV1::ActorMember => 3,
    }
}

fn encode_action(out: &mut CanonicalBytesV1, action: &PreparedPlanActionV1) {
    match action {
        PreparedPlanActionV1::GrantRole {
            entry,
            role,
            target,
        } => {
            out.byte(0);
            out.entry(*entry);
            encode_role_ref(out, role);
            out.u64(target.0);
        }
        PreparedPlanActionV1::RespondEphemeral { entry, content } => {
            out.byte(1);
            out.entry(*entry);
            out.string(content);
        }
        PreparedPlanActionV1::OpenModal { entry, modal } => {
            out.byte(2);
            out.entry(*entry);
            out.string(&modal.key);
            out.string(&modal.title);
            out.u16(modal.fields.len() as u16);
            for field in &modal.fields {
                out.string(&field.key);
                out.string(&field.label);
                out.byte(match field.style {
                    automation_state::ModalFieldStyle::Short => 0,
                    automation_state::ModalFieldStyle::Paragraph => 1,
                });
                out.bool(field.required);
                encode_optional_u16(out, field.min_length);
                encode_optional_u16(out, field.max_length);
                out.byte(match field.input_policy {
                    automation_state::ModalInputPolicy::Preserve => 0,
                    automation_state::ModalInputPolicy::TrimUnicodeWhitespace => 1,
                });
            }
        }
        PreparedPlanActionV1::CreateChannel {
            entry,
            output,
            key,
            name,
        } => {
            out.byte(3);
            out.entry(*entry);
            out.entry(output.producer());
            out.string(key);
            out.string(name);
        }
        PreparedPlanActionV1::CreateRole {
            entry,
            output,
            key,
            name,
        } => {
            out.byte(4);
            out.entry(*entry);
            out.entry(output.producer());
            out.string(key);
            out.string(name);
        }
        PreparedPlanActionV1::UpsertOverwrite {
            entry,
            channel,
            target,
            allow,
            deny,
        } => {
            out.byte(5);
            out.entry(*entry);
            encode_channel_ref(out, channel);
            match target {
                PreflightOverwriteTargetV1::Everyone => out.byte(0),
                PreflightOverwriteTargetV1::Role(role) => {
                    out.byte(1);
                    encode_role_ref(out, role);
                }
            }
            out.u64(allow.bits());
            out.u64(deny.bits());
        }
        PreparedPlanActionV1::PostPanel {
            entry,
            output,
            key,
            channel,
            content,
            buttons,
        } => {
            out.byte(6);
            out.entry(*entry);
            out.entry(output.producer());
            out.string(key);
            encode_channel_ref(out, channel);
            out.string(content);
            out.u16(buttons.len() as u16);
            for button in buttons {
                out.string(&button.label);
                match &button.route {
                    PreflightButtonRouteV1::Static { key } => {
                        out.byte(0);
                        out.string(key);
                    }
                    PreflightButtonRouteV1::InstanceAction {
                        instance_id,
                        producer,
                        action,
                    } => {
                        out.byte(1);
                        out.string(instance_id.as_str());
                        match producer {
                            Some(producer) => {
                                out.bool(true);
                                out.entry(producer.producer());
                            }
                            None => out.bool(false),
                        }
                        out.string(action);
                    }
                }
            }
        }
        PreparedPlanActionV1::DeferEphemeral { entry } => {
            out.byte(7);
            out.entry(*entry);
        }
        PreparedPlanActionV1::EditResponse { entry, content } => {
            out.byte(8);
            out.entry(*entry);
            out.string(content);
        }
        PreparedPlanActionV1::RegisterInstance {
            entry,
            output,
            key,
            id,
            kind,
            resources,
        } => {
            out.byte(9);
            out.entry(*entry);
            out.entry(output.producer());
            out.string(key);
            out.string(id.as_str());
            out.string(&kind.0);
            out.u16(resources.roles.len() as u16);
            for (alias, reference) in &resources.roles {
                out.string(alias);
                out.entry(reference.producer());
            }
            out.u16(resources.channels.len() as u16);
            for (alias, reference) in &resources.channels {
                out.string(alias);
                out.entry(reference.producer());
            }
            out.u16(resources.messages.len() as u16);
            for (alias, reference) in &resources.messages {
                out.string(alias);
                out.entry(reference.producer());
            }
        }
        PreparedPlanActionV1::TeardownInstance { entry, instance } => {
            out.byte(10);
            out.entry(*entry);
            match instance {
                PreflightInstanceRefV1::Existing(instance_id) => {
                    out.byte(0);
                    out.string(instance_id.as_str());
                }
                PreflightInstanceRefV1::Registered(reference) => {
                    out.byte(1);
                    out.entry(reference.producer());
                }
            }
        }
    }
}

fn encode_role_ref(out: &mut CanonicalBytesV1, reference: &PreflightRoleRefV1) {
    match reference {
        PreflightRoleRefV1::Existing(role_id) => {
            out.byte(0);
            out.u64(role_id.0);
        }
        PreflightRoleRefV1::Instance(role_id) => {
            out.byte(1);
            out.u64(role_id.0);
        }
        PreflightRoleRefV1::Produced(reference) => {
            out.byte(2);
            out.entry(reference.producer());
        }
    }
}

fn encode_channel_ref(out: &mut CanonicalBytesV1, reference: &PreflightChannelRefV1) {
    match reference {
        PreflightChannelRefV1::Existing(channel_id) => {
            out.byte(0);
            out.u64(channel_id.0);
        }
        PreflightChannelRefV1::Produced(reference) => {
            out.byte(1);
            out.entry(reference.producer());
        }
    }
}

fn encode_optional_u16(out: &mut CanonicalBytesV1, value: Option<u16>) {
    match value {
        Some(value) => {
            out.bool(true);
            out.u16(value);
        }
        None => out.bool(false),
    }
}
