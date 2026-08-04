use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};

use automation_core::preflight::{
    ActionEntryIdV1, ActionPlanSnapshotV1, PreflightChannelRefV1, PreflightInstanceRefV1,
    PreflightOverwriteTargetV1, PreflightRoleRefV1, PreflightedActionPlanV1, PreparedPlanActionV1,
};
use automation_core::{CreateChannelSpec, CreateRoleSpec};
use automation_runtime_interaction::{
    InteractionActionPlanDigestV1, InteractionEffectActionDigestV1,
    InteractionEffectActionIdentityV1, InteractionEffectActionIndexV1,
    InteractionEffectChannelIdV1, InteractionEffectExpectedPostimageDigestV1,
    InteractionEffectGuildIdV1, InteractionEffectIdentityErrorV1, InteractionEffectInputDigestV1,
    InteractionEffectInstanceStateV1, InteractionEffectKindV1, InteractionEffectOutputClassV1,
    InteractionEffectPayloadDigestV1, InteractionEffectPermissionStateV1,
    InteractionEffectPermissionValueV1, InteractionEffectPlanDefinitionV1,
    InteractionEffectPlannedChannelReferenceV1, InteractionEffectPlannedDependencyV1,
    InteractionEffectPlannedInstanceTargetV1, InteractionEffectPlannedOverwriteTargetV1,
    InteractionEffectPlannedPermissionTargetV1, InteractionEffectPlannedPreimageV1,
    InteractionEffectPlannedRecoveryInputV1, InteractionEffectPlannedRoleMembershipTargetV1,
    InteractionEffectPlannedRoleReferenceV1, InteractionEffectPlannedTargetV1,
    InteractionEffectRoleIdV1, InteractionEffectUserIdV1, InteractionPreflightSnapshotDigestV1,
    InteractionReceiptIdentityV1,
};

use crate::action_plan_preflight_certificate::InteractionActionPreflightCertificateV1;
use crate::action_plan_wire_preflight::ActionPlanWirePreflightV1;
use crate::discord_effect_postimage::{
    expected_created_channel_postimage_digest_v1, expected_created_role_postimage_digest_v1,
    instance_postimage_digest_v1, overwrite_postimage_digest_v1, panel_postimage_digest_v1,
    response_postimage_digest_v1, role_membership_postimage_digest_v1,
};

const ACTION_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.action.v1\0";
const INPUT_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.input.v1\0";
const PAYLOAD_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.payload.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InteractionEffectPlanBuildErrorV1 {
    #[error("interaction effect plan identity is invalid")]
    Identity,
    #[error("interaction effect plan snapshot is invalid")]
    Snapshot,
    #[error("interaction effect plan references an unavailable producer")]
    Producer,
    #[error("interaction effect plan response ordering is invalid")]
    ResponseOrdering,
    #[error("interaction effect plan irreversible action ordering is invalid")]
    IrreversibleOrdering,
    #[error("interaction effect plan teardown manifest is unavailable")]
    TeardownManifest,
}

impl From<InteractionEffectIdentityErrorV1> for InteractionEffectPlanBuildErrorV1 {
    fn from(_: InteractionEffectIdentityErrorV1) -> Self {
        Self::Identity
    }
}

#[derive(Clone)]
pub(crate) struct InteractionEffectExecutionPlanEntryV1 {
    action_entry: ActionEntryIdV1,
    action: PreparedPlanActionV1,
    definition: InteractionEffectPlanDefinitionV1,
    expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
}

impl InteractionEffectExecutionPlanEntryV1 {
    pub(crate) fn action_entry(&self) -> ActionEntryIdV1 {
        self.action_entry
    }

    pub(crate) fn action(&self) -> &PreparedPlanActionV1 {
        &self.action
    }

    pub(crate) fn definition(&self) -> &InteractionEffectPlanDefinitionV1 {
        &self.definition
    }

    pub(crate) fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
    }
}

impl Debug for InteractionEffectExecutionPlanEntryV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectExecutionPlanEntryV1(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct InteractionEffectExecutionPlanV1 {
    snapshot_digest: InteractionPreflightSnapshotDigestV1,
    entries: Vec<InteractionEffectExecutionPlanEntryV1>,
}

impl InteractionEffectExecutionPlanV1 {
    pub(crate) fn entries(&self) -> &[InteractionEffectExecutionPlanEntryV1] {
        &self.entries
    }

    pub(crate) fn snapshot_digest(&self) -> &InteractionPreflightSnapshotDigestV1 {
        &self.snapshot_digest
    }
}

impl Debug for InteractionEffectExecutionPlanV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionEffectExecutionPlanV1")
            .field("effect_count", &self.entries.len())
            .finish()
    }
}

#[derive(Clone)]
struct ProducerEffectV1 {
    action_index: InteractionEffectActionIndexV1,
    planned_identity_digest:
        automation_runtime_interaction::InteractionEffectPlannedIdentityDigestV1,
    output_class: InteractionEffectOutputClassV1,
}

impl ProducerEffectV1 {
    fn dependency(&self) -> InteractionEffectPlannedDependencyV1 {
        InteractionEffectPlannedDependencyV1::new(
            self.action_index,
            self.planned_identity_digest.clone(),
            self.output_class,
        )
    }
}

pub(crate) fn build_interaction_effect_execution_plan_v1(
    plan: &PreflightedActionPlanV1,
    snapshot: &ActionPlanSnapshotV1,
    wire: &ActionPlanWirePreflightV1,
    certificate: &InteractionActionPreflightCertificateV1,
    action_plan_digest: &InteractionActionPlanDigestV1,
    receipt_identity: InteractionReceiptIdentityV1,
) -> Result<InteractionEffectExecutionPlanV1, InteractionEffectPlanBuildErrorV1> {
    if snapshot.guild_id != plan.context().guild_id
        || &snapshot.identity != plan.snapshot_identity()
        || certificate.action_plan_digest() != action_plan_digest
    {
        return Err(InteractionEffectPlanBuildErrorV1::Snapshot);
    }
    let mut entries = Vec::new();
    let mut producers = BTreeMap::new();
    let mut response_tail_seen = false;
    let mut irreversible_effect_seen = false;
    for action in plan.actions() {
        if matches!(
            action,
            PreparedPlanActionV1::RespondEphemeral { .. } | PreparedPlanActionV1::OpenModal { .. }
        ) {
            response_tail_seen = true;
            continue;
        }
        let Some(kind) = effect_kind_v1(action) else {
            continue;
        };
        if response_tail_seen {
            return Err(InteractionEffectPlanBuildErrorV1::ResponseOrdering);
        }
        irreversible_effect_seen =
            advance_irreversible_boundary_v1(irreversible_effect_seen, kind)?;
        if kind == InteractionEffectKindV1::EditResponse {
            response_tail_seen = true;
        }
        let action_index = InteractionEffectActionIndexV1::new(
            u16::try_from(entries.len())
                .map_err(|_| InteractionEffectPlanBuildErrorV1::Identity)?,
        )?;
        let mut dependencies = action_dependencies_v1(action, &producers)?;
        if let Some(previous) = entries.last().map(
            |entry: &InteractionEffectExecutionPlanEntryV1| ProducerEffectV1 {
                action_index: entry.definition.action().action_index(),
                planned_identity_digest: entry.definition.planned_identity_digest(),
                output_class: entry.definition.output_class(),
            },
        ) {
            insert_dependency_v1(&mut dependencies, previous.dependency())?;
        }
        dependencies.sort_by_key(InteractionEffectPlannedDependencyV1::action_index);
        let entry = action.entry();
        let (action_digest, input_digest) =
            action_identity_digests_v1(plan.digest_material_v1(), entry, action_index, kind);
        let action_identity = InteractionEffectActionIdentityV1::new(
            receipt_identity,
            action_plan_digest.clone(),
            certificate.digest().clone(),
            action_index,
            kind,
            action_digest,
            input_digest,
        );
        let recovery_input =
            planned_recovery_input_v1(action, plan, snapshot, wire, &producers, receipt_identity)?;
        let expected_postimage_digest = expected_postimage_digest_v1(action, plan, wire)?;
        let definition =
            InteractionEffectPlanDefinitionV1::new(action_identity, recovery_input, dependencies)?;
        let producer = ProducerEffectV1 {
            action_index,
            planned_identity_digest: definition.planned_identity_digest(),
            output_class: definition.output_class(),
        };
        if producers.insert(entry, producer).is_some() {
            return Err(InteractionEffectPlanBuildErrorV1::Producer);
        }
        entries.push(InteractionEffectExecutionPlanEntryV1 {
            action_entry: entry,
            action: action.clone(),
            definition,
            expected_postimage_digest,
        });
    }
    Ok(InteractionEffectExecutionPlanV1 {
        snapshot_digest: certificate.snapshot_digest().clone(),
        entries,
    })
}

fn advance_irreversible_boundary_v1(
    irreversible_effect_seen: bool,
    kind: InteractionEffectKindV1,
) -> Result<bool, InteractionEffectPlanBuildErrorV1> {
    if irreversible_effect_seen && kind != InteractionEffectKindV1::EditResponse {
        return Err(InteractionEffectPlanBuildErrorV1::IrreversibleOrdering);
    }
    Ok(irreversible_effect_seen || kind == InteractionEffectKindV1::TeardownInstance)
}

fn effect_kind_v1(action: &PreparedPlanActionV1) -> Option<InteractionEffectKindV1> {
    match action {
        PreparedPlanActionV1::CreateRole { .. } => Some(InteractionEffectKindV1::CreateRole),
        PreparedPlanActionV1::CreateChannel { .. } => Some(InteractionEffectKindV1::CreateChannel),
        PreparedPlanActionV1::GrantRole { .. } => Some(InteractionEffectKindV1::GrantRole),
        PreparedPlanActionV1::UpsertOverwrite { .. } => {
            Some(InteractionEffectKindV1::UpsertOverwrite)
        }
        PreparedPlanActionV1::PostPanel { .. } => Some(InteractionEffectKindV1::PostPanel),
        PreparedPlanActionV1::RegisterInstance { .. } => {
            Some(InteractionEffectKindV1::RegisterInstance)
        }
        PreparedPlanActionV1::TeardownInstance { .. } => {
            Some(InteractionEffectKindV1::TeardownInstance)
        }
        PreparedPlanActionV1::EditResponse { .. } => Some(InteractionEffectKindV1::EditResponse),
        PreparedPlanActionV1::RespondEphemeral { .. }
        | PreparedPlanActionV1::OpenModal { .. }
        | PreparedPlanActionV1::DeferEphemeral { .. } => None,
    }
}

fn action_dependencies_v1(
    action: &PreparedPlanActionV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
) -> Result<Vec<InteractionEffectPlannedDependencyV1>, InteractionEffectPlanBuildErrorV1> {
    let mut dependencies = Vec::new();
    match action {
        PreparedPlanActionV1::GrantRole { role, .. } => {
            insert_role_dependency_v1(&mut dependencies, role, producers)?;
        }
        PreparedPlanActionV1::UpsertOverwrite {
            channel, target, ..
        } => {
            insert_channel_dependency_v1(&mut dependencies, channel, producers)?;
            if let PreflightOverwriteTargetV1::Role(role) = target {
                insert_role_dependency_v1(&mut dependencies, role, producers)?;
            }
        }
        PreparedPlanActionV1::PostPanel { channel, .. } => {
            insert_channel_dependency_v1(&mut dependencies, channel, producers)?;
        }
        PreparedPlanActionV1::RegisterInstance { resources, .. } => {
            for reference in resources.roles.values() {
                insert_producer_dependency_v1(
                    &mut dependencies,
                    reference.producer(),
                    InteractionEffectOutputClassV1::CreatedRole,
                    producers,
                )?;
            }
            for reference in resources.channels.values() {
                insert_producer_dependency_v1(
                    &mut dependencies,
                    reference.producer(),
                    InteractionEffectOutputClassV1::CreatedChannel,
                    producers,
                )?;
            }
            for reference in resources.messages.values() {
                insert_producer_dependency_v1(
                    &mut dependencies,
                    reference.producer(),
                    InteractionEffectOutputClassV1::PostedMessage,
                    producers,
                )?;
            }
        }
        PreparedPlanActionV1::TeardownInstance {
            instance: PreflightInstanceRefV1::Registered(reference),
            ..
        } => {
            insert_producer_dependency_v1(
                &mut dependencies,
                reference.producer(),
                InteractionEffectOutputClassV1::InstanceState,
                producers,
            )?;
        }
        PreparedPlanActionV1::CreateRole { .. }
        | PreparedPlanActionV1::CreateChannel { .. }
        | PreparedPlanActionV1::TeardownInstance { .. }
        | PreparedPlanActionV1::EditResponse { .. }
        | PreparedPlanActionV1::RespondEphemeral { .. }
        | PreparedPlanActionV1::OpenModal { .. }
        | PreparedPlanActionV1::DeferEphemeral { .. } => {}
    }
    Ok(dependencies)
}

fn insert_role_dependency_v1(
    dependencies: &mut Vec<InteractionEffectPlannedDependencyV1>,
    role: &PreflightRoleRefV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
) -> Result<(), InteractionEffectPlanBuildErrorV1> {
    if let PreflightRoleRefV1::Produced(reference) = role {
        insert_producer_dependency_v1(
            dependencies,
            reference.producer(),
            InteractionEffectOutputClassV1::CreatedRole,
            producers,
        )?;
    }
    Ok(())
}

fn insert_channel_dependency_v1(
    dependencies: &mut Vec<InteractionEffectPlannedDependencyV1>,
    channel: &PreflightChannelRefV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
) -> Result<(), InteractionEffectPlanBuildErrorV1> {
    if let PreflightChannelRefV1::Produced(reference) = channel {
        insert_producer_dependency_v1(
            dependencies,
            reference.producer(),
            InteractionEffectOutputClassV1::CreatedChannel,
            producers,
        )?;
    }
    Ok(())
}

fn insert_producer_dependency_v1(
    dependencies: &mut Vec<InteractionEffectPlannedDependencyV1>,
    producer: ActionEntryIdV1,
    output_class: InteractionEffectOutputClassV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
) -> Result<(), InteractionEffectPlanBuildErrorV1> {
    let producer = producers
        .get(&producer)
        .ok_or(InteractionEffectPlanBuildErrorV1::Producer)?;
    if producer.output_class != output_class {
        return Err(InteractionEffectPlanBuildErrorV1::Producer);
    }
    insert_dependency_v1(dependencies, producer.dependency())
}

fn insert_dependency_v1(
    dependencies: &mut Vec<InteractionEffectPlannedDependencyV1>,
    dependency: InteractionEffectPlannedDependencyV1,
) -> Result<(), InteractionEffectPlanBuildErrorV1> {
    if let Some(existing) = dependencies
        .iter()
        .find(|existing| existing.action_index() == dependency.action_index())
    {
        if existing != &dependency {
            return Err(InteractionEffectPlanBuildErrorV1::Producer);
        }
        return Ok(());
    }
    dependencies.push(dependency);
    Ok(())
}

fn planned_recovery_input_v1(
    action: &PreparedPlanActionV1,
    plan: &PreflightedActionPlanV1,
    snapshot: &ActionPlanSnapshotV1,
    wire: &ActionPlanWirePreflightV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
    receipt_identity: InteractionReceiptIdentityV1,
) -> Result<InteractionEffectPlannedRecoveryInputV1, InteractionEffectPlanBuildErrorV1> {
    let guild_id = InteractionEffectGuildIdV1::try_from(plan.context().guild_id)?;
    let (target, preimage) = match action {
        PreparedPlanActionV1::CreateRole { .. } => (
            InteractionEffectPlannedTargetV1::CreateRole { guild_id },
            InteractionEffectPlannedPreimageV1::None,
        ),
        PreparedPlanActionV1::CreateChannel { .. } => (
            InteractionEffectPlannedTargetV1::CreateChannel { guild_id },
            InteractionEffectPlannedPreimageV1::None,
        ),
        PreparedPlanActionV1::GrantRole { role, target, .. } => {
            let role = planned_role_reference_v1(role, producers)?;
            let membership = InteractionEffectPlannedRoleMembershipTargetV1::new(
                guild_id,
                InteractionEffectUserIdV1::try_from(*target)?,
                role.clone(),
            );
            let present = role_preimage_v1(snapshot, *target, role)?;
            (
                InteractionEffectPlannedTargetV1::GrantRole {
                    target: membership.clone(),
                },
                InteractionEffectPlannedPreimageV1::RoleMembership {
                    target: membership,
                    present,
                },
            )
        }
        PreparedPlanActionV1::UpsertOverwrite {
            channel,
            target,
            allow,
            deny,
            ..
        } => {
            let channel = planned_channel_reference_v1(channel, producers)?;
            let overwrite = planned_overwrite_target_v1(target, guild_id, producers)?;
            let permission_target = InteractionEffectPlannedPermissionTargetV1::new(
                guild_id,
                channel.clone(),
                overwrite.clone(),
            );
            let desired = InteractionEffectPermissionValueV1::new(allow.bits(), deny.bits())?;
            let before = overwrite_preimage_v1(snapshot, &channel, &overwrite)?;
            (
                InteractionEffectPlannedTargetV1::UpsertOverwrite {
                    target: permission_target.clone(),
                    desired,
                },
                InteractionEffectPlannedPreimageV1::PermissionOverwrite {
                    target: permission_target,
                    before,
                },
            )
        }
        PreparedPlanActionV1::PostPanel { entry, channel, .. } => (
            InteractionEffectPlannedTargetV1::PostPanel {
                guild_id,
                channel: planned_channel_reference_v1(channel, producers)?,
                payload_digest: payload_digest_v1(plan.digest_material_v1(), *entry, b"post_panel"),
            },
            InteractionEffectPlannedPreimageV1::None,
        ),
        PreparedPlanActionV1::RegisterInstance {
            entry, id, kind, ..
        } => {
            let target = InteractionEffectPlannedInstanceTargetV1::new(guild_id, id.clone());
            (
                InteractionEffectPlannedTargetV1::RegisterInstance {
                    target: target.clone(),
                    kind: kind.clone(),
                    manifest_digest: payload_digest_v1(
                        plan.digest_material_v1(),
                        *entry,
                        b"instance_manifest",
                    ),
                },
                InteractionEffectPlannedPreimageV1::InstanceRegistration {
                    target,
                    before: InteractionEffectInstanceStateV1::Absent,
                },
            )
        }
        PreparedPlanActionV1::TeardownInstance {
            entry,
            instance: PreflightInstanceRefV1::Existing(instance_id),
        } => {
            let manifest = wire
                .teardown_manifest_for_entry_v1(*entry)
                .filter(|manifest| manifest.instance_id() == instance_id)
                .ok_or(InteractionEffectPlanBuildErrorV1::TeardownManifest)?;
            let target =
                InteractionEffectPlannedInstanceTargetV1::new(guild_id, instance_id.clone());
            let manifest_digest = InteractionEffectPayloadDigestV1::from_canonical_bytes(
                manifest.canonical_manifest_bytes_v1(),
            );
            (
                InteractionEffectPlannedTargetV1::TeardownInstance {
                    target: target.clone(),
                },
                InteractionEffectPlannedPreimageV1::InstanceRegistration {
                    target,
                    before: InteractionEffectInstanceStateV1::Present { manifest_digest },
                },
            )
        }
        PreparedPlanActionV1::TeardownInstance {
            instance: PreflightInstanceRefV1::Registered(_),
            ..
        } => return Err(InteractionEffectPlanBuildErrorV1::TeardownManifest),
        PreparedPlanActionV1::EditResponse { entry, .. } => (
            InteractionEffectPlannedTargetV1::EditResponse {
                receipt_identity,
                payload_digest: payload_digest_v1(
                    plan.digest_material_v1(),
                    *entry,
                    b"edit_response",
                ),
            },
            InteractionEffectPlannedPreimageV1::None,
        ),
        PreparedPlanActionV1::RespondEphemeral { .. }
        | PreparedPlanActionV1::OpenModal { .. }
        | PreparedPlanActionV1::DeferEphemeral { .. } => {
            return Err(InteractionEffectPlanBuildErrorV1::ResponseOrdering);
        }
    };
    InteractionEffectPlannedRecoveryInputV1::new(target, preimage).map_err(Into::into)
}

fn expected_postimage_digest_v1(
    action: &PreparedPlanActionV1,
    plan: &PreflightedActionPlanV1,
    wire: &ActionPlanWirePreflightV1,
) -> Result<InteractionEffectExpectedPostimageDigestV1, InteractionEffectPlanBuildErrorV1> {
    match action {
        PreparedPlanActionV1::CreateRole { name, .. } => {
            Ok(expected_created_role_postimage_digest_v1(&CreateRoleSpec {
                name: name.clone(),
            }))
        }
        PreparedPlanActionV1::CreateChannel { name, .. } => Ok(
            expected_created_channel_postimage_digest_v1(&CreateChannelSpec { name: name.clone() }),
        ),
        PreparedPlanActionV1::GrantRole { .. } => Ok(role_membership_postimage_digest_v1(true)),
        PreparedPlanActionV1::UpsertOverwrite { allow, deny, .. } => {
            Ok(overwrite_postimage_digest_v1(*allow, *deny))
        }
        PreparedPlanActionV1::PostPanel {
            entry,
            content,
            buttons,
            ..
        } => {
            let exact = wire
                .panel_buttons_for_entry_v1(*entry)
                .filter(|exact| exact.len() == buttons.len())
                .ok_or(InteractionEffectPlanBuildErrorV1::Identity)?;
            let mut postimage_buttons = Vec::with_capacity(buttons.len());
            for (index, button) in buttons.iter().enumerate() {
                let exact = exact
                    .get(index)
                    .filter(|exact| usize::from(exact.index) == index)
                    .ok_or(InteractionEffectPlanBuildErrorV1::Identity)?;
                postimage_buttons
                    .push((button.label.clone(), exact.custom_id.as_str().to_string()));
            }
            Ok(panel_postimage_digest_v1(content, &postimage_buttons))
        }
        PreparedPlanActionV1::RegisterInstance { entry, .. } => {
            let payload = payload_digest_v1(
                plan.digest_material_v1(),
                *entry,
                b"planned_instance_postimage",
            );
            Ok(
                InteractionEffectExpectedPostimageDigestV1::from_canonical_bytes(
                    payload.as_str().as_bytes(),
                ),
            )
        }
        PreparedPlanActionV1::TeardownInstance { entry, .. } => {
            let manifest = wire
                .teardown_manifest_for_entry_v1(*entry)
                .ok_or(InteractionEffectPlanBuildErrorV1::TeardownManifest)?;
            Ok(instance_postimage_digest_v1(
                manifest.digest().as_str(),
                false,
            ))
        }
        PreparedPlanActionV1::EditResponse { content, .. } => {
            Ok(response_postimage_digest_v1(content))
        }
        PreparedPlanActionV1::RespondEphemeral { .. }
        | PreparedPlanActionV1::OpenModal { .. }
        | PreparedPlanActionV1::DeferEphemeral { .. } => {
            Err(InteractionEffectPlanBuildErrorV1::ResponseOrdering)
        }
    }
}

fn planned_role_reference_v1(
    role: &PreflightRoleRefV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
) -> Result<InteractionEffectPlannedRoleReferenceV1, InteractionEffectPlanBuildErrorV1> {
    match role {
        PreflightRoleRefV1::Existing(role_id) | PreflightRoleRefV1::Instance(role_id) => {
            Ok(InteractionEffectPlannedRoleReferenceV1::Existing(
                InteractionEffectRoleIdV1::try_from(*role_id)?,
            ))
        }
        PreflightRoleRefV1::Produced(reference) => {
            let producer = producers
                .get(&reference.producer())
                .filter(|producer| {
                    producer.output_class == InteractionEffectOutputClassV1::CreatedRole
                })
                .ok_or(InteractionEffectPlanBuildErrorV1::Producer)?;
            InteractionEffectPlannedRoleReferenceV1::produced(producer.dependency())
                .map_err(Into::into)
        }
    }
}

fn planned_channel_reference_v1(
    channel: &PreflightChannelRefV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
) -> Result<InteractionEffectPlannedChannelReferenceV1, InteractionEffectPlanBuildErrorV1> {
    match channel {
        PreflightChannelRefV1::Existing(channel_id) => {
            Ok(InteractionEffectPlannedChannelReferenceV1::Existing(
                InteractionEffectChannelIdV1::try_from(*channel_id)?,
            ))
        }
        PreflightChannelRefV1::Produced(reference) => {
            let producer = producers
                .get(&reference.producer())
                .filter(|producer| {
                    producer.output_class == InteractionEffectOutputClassV1::CreatedChannel
                })
                .ok_or(InteractionEffectPlanBuildErrorV1::Producer)?;
            InteractionEffectPlannedChannelReferenceV1::produced(producer.dependency())
                .map_err(Into::into)
        }
    }
}

fn planned_overwrite_target_v1(
    target: &PreflightOverwriteTargetV1,
    guild_id: InteractionEffectGuildIdV1,
    producers: &BTreeMap<ActionEntryIdV1, ProducerEffectV1>,
) -> Result<InteractionEffectPlannedOverwriteTargetV1, InteractionEffectPlanBuildErrorV1> {
    match target {
        PreflightOverwriteTargetV1::Everyone => {
            Ok(InteractionEffectPlannedOverwriteTargetV1::Role(
                InteractionEffectPlannedRoleReferenceV1::Existing(InteractionEffectRoleIdV1::new(
                    guild_id.get(),
                )?),
            ))
        }
        PreflightOverwriteTargetV1::Role(role) => {
            Ok(InteractionEffectPlannedOverwriteTargetV1::Role(
                planned_role_reference_v1(role, producers)?,
            ))
        }
    }
}

fn role_preimage_v1(
    snapshot: &ActionPlanSnapshotV1,
    target: discord_model::UserId,
    role: InteractionEffectPlannedRoleReferenceV1,
) -> Result<bool, InteractionEffectPlanBuildErrorV1> {
    let InteractionEffectPlannedRoleReferenceV1::Existing(role_id) = role else {
        return Ok(false);
    };
    let member = snapshot
        .actor_member
        .as_ref()
        .filter(|member| member.user_id == target)
        .ok_or(InteractionEffectPlanBuildErrorV1::Snapshot)?;
    Ok(member
        .roles
        .iter()
        .any(|candidate| candidate.0 == role_id.get()))
}

fn overwrite_preimage_v1(
    snapshot: &ActionPlanSnapshotV1,
    channel: &InteractionEffectPlannedChannelReferenceV1,
    target: &InteractionEffectPlannedOverwriteTargetV1,
) -> Result<InteractionEffectPermissionStateV1, InteractionEffectPlanBuildErrorV1> {
    let InteractionEffectPlannedChannelReferenceV1::Existing(channel_id) = channel else {
        return Ok(InteractionEffectPermissionStateV1::Absent);
    };
    let InteractionEffectPlannedOverwriteTargetV1::Role(
        InteractionEffectPlannedRoleReferenceV1::Existing(role_id),
    ) = target
    else {
        return Ok(InteractionEffectPermissionStateV1::Absent);
    };
    let channel = snapshot
        .channels
        .as_ref()
        .and_then(|channels| {
            channels
                .iter()
                .find(|channel| channel.id.0 == channel_id.get())
        })
        .ok_or(InteractionEffectPlanBuildErrorV1::Snapshot)?;
    let overwrite = channel.overwrites.iter().find(|overwrite| {
        matches!(
            overwrite.target,
            discord_model::OverwriteTarget::Role(candidate) if candidate.0 == role_id.get()
        )
    });
    match overwrite {
        Some(overwrite) => Ok(InteractionEffectPermissionStateV1::Present(
            InteractionEffectPermissionValueV1::new(overwrite.allow.bits(), overwrite.deny.bits())?,
        )),
        None => Ok(InteractionEffectPermissionStateV1::Absent),
    }
}

fn action_identity_digests_v1(
    plan_material: &[u8],
    entry: ActionEntryIdV1,
    action_index: InteractionEffectActionIndexV1,
    kind: InteractionEffectKindV1,
) -> (
    InteractionEffectActionDigestV1,
    InteractionEffectInputDigestV1,
) {
    let mut action = CanonicalEffectFrameV1::new(ACTION_DIGEST_DOMAIN_V1);
    action.u16(entry.ordinal());
    action.u16(action_index.get());
    action.bytes(kind.code().as_bytes());
    let mut input = CanonicalEffectFrameV1::new(INPUT_DIGEST_DOMAIN_V1);
    input.u16(entry.ordinal());
    input.bytes(kind.code().as_bytes());
    input.bytes(plan_material);
    (
        InteractionEffectActionDigestV1::from_canonical_bytes(&action.finish()),
        InteractionEffectInputDigestV1::from_canonical_bytes(&input.finish()),
    )
}

fn payload_digest_v1(
    plan_material: &[u8],
    entry: ActionEntryIdV1,
    payload_kind: &[u8],
) -> InteractionEffectPayloadDigestV1 {
    let mut frame = CanonicalEffectFrameV1::new(PAYLOAD_DIGEST_DOMAIN_V1);
    frame.u16(entry.ordinal());
    frame.bytes(payload_kind);
    frame.bytes(plan_material);
    InteractionEffectPayloadDigestV1::from_canonical_bytes(&frame.finish())
}

struct CanonicalEffectFrameV1 {
    bytes: Vec<u8>,
}

impl CanonicalEffectFrameV1 {
    fn new(domain: &[u8]) -> Self {
        let mut frame = Self {
            bytes: Vec::with_capacity(domain.len() + 128),
        };
        frame.bytes(domain);
        frame
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_be_bytes());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(
            &u64::try_from(value.len())
                .expect("bounded canonical effect field length fits u64")
                .to_be_bytes(),
        );
        self.bytes.extend_from_slice(value);
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teardown_is_the_final_mutable_effect() {
        let boundary =
            advance_irreversible_boundary_v1(false, InteractionEffectKindV1::TeardownInstance)
                .unwrap();
        assert!(boundary);
        assert_eq!(
            advance_irreversible_boundary_v1(boundary, InteractionEffectKindV1::EditResponse),
            Ok(true)
        );
        for kind in [
            InteractionEffectKindV1::CreateRole,
            InteractionEffectKindV1::CreateChannel,
            InteractionEffectKindV1::GrantRole,
            InteractionEffectKindV1::UpsertOverwrite,
            InteractionEffectKindV1::PostPanel,
            InteractionEffectKindV1::RegisterInstance,
            InteractionEffectKindV1::TeardownInstance,
        ] {
            assert_eq!(
                advance_irreversible_boundary_v1(boundary, kind),
                Err(InteractionEffectPlanBuildErrorV1::IrreversibleOrdering)
            );
        }
    }
}
