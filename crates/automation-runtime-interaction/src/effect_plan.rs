use std::collections::{BTreeMap, BTreeSet};

use automation_instance::{InstanceId, InstanceKind};

use crate::effect::{
    InteractionEffectActionIdentityV1, InteractionEffectChannelIdV1,
    InteractionEffectCompensationClassV1, InteractionEffectCorrelationClassV1,
    InteractionEffectDefinitionV1, InteractionEffectDependencyResolutionV1,
    InteractionEffectDependencyV1, InteractionEffectGuildIdV1, InteractionEffectIdentityErrorV1,
    InteractionEffectInstanceStateV1, InteractionEffectInstanceTargetV1, InteractionEffectKindV1,
    InteractionEffectObservedOutputV1, InteractionEffectOutputClassV1,
    InteractionEffectOverwriteTargetV1, InteractionEffectPermissionStateV1,
    InteractionEffectPermissionTargetV1, InteractionEffectPermissionValueV1,
    InteractionEffectPlannedDependencyV1, InteractionEffectPreimageV1,
    InteractionEffectRecoveryScopeV1, InteractionEffectRoleIdV1,
    InteractionEffectRoleMembershipTargetV1, InteractionEffectTargetV1, InteractionEffectUserIdV1,
    MAX_INTERACTION_EFFECT_DEPENDENCIES_V1,
};
use crate::effect_digest::{
    InteractionEffectOpaqueIdentityDigestV1, InteractionEffectPayloadDigestV1,
    InteractionEffectPlannedIdentityDigestV1,
};
use crate::InteractionReceiptIdentityV1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectPlannedRoleReferenceV1 {
    Existing(InteractionEffectRoleIdV1),
    Produced(InteractionEffectPlannedDependencyV1),
}

impl InteractionEffectPlannedRoleReferenceV1 {
    pub fn produced(
        dependency: InteractionEffectPlannedDependencyV1,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if dependency.output_class() != InteractionEffectOutputClassV1::CreatedRole {
            return Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch);
        }
        Ok(Self::Produced(dependency))
    }

    pub fn dependency(&self) -> Option<&InteractionEffectPlannedDependencyV1> {
        match self {
            Self::Existing(_) => None,
            Self::Produced(dependency) => Some(dependency),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectPlannedChannelReferenceV1 {
    Existing(InteractionEffectChannelIdV1),
    Produced(InteractionEffectPlannedDependencyV1),
}

impl InteractionEffectPlannedChannelReferenceV1 {
    pub fn produced(
        dependency: InteractionEffectPlannedDependencyV1,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if dependency.output_class() != InteractionEffectOutputClassV1::CreatedChannel {
            return Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch);
        }
        Ok(Self::Produced(dependency))
    }

    pub fn dependency(&self) -> Option<&InteractionEffectPlannedDependencyV1> {
        match self {
            Self::Existing(_) => None,
            Self::Produced(dependency) => Some(dependency),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectPlannedOverwriteTargetV1 {
    Role(InteractionEffectPlannedRoleReferenceV1),
    Member(InteractionEffectUserIdV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectPlannedRoleMembershipTargetV1 {
    guild_id: InteractionEffectGuildIdV1,
    user_id: InteractionEffectUserIdV1,
    role: InteractionEffectPlannedRoleReferenceV1,
}

impl InteractionEffectPlannedRoleMembershipTargetV1 {
    pub fn new(
        guild_id: InteractionEffectGuildIdV1,
        user_id: InteractionEffectUserIdV1,
        role: InteractionEffectPlannedRoleReferenceV1,
    ) -> Self {
        Self {
            guild_id,
            user_id,
            role,
        }
    }

    pub fn guild_id(&self) -> InteractionEffectGuildIdV1 {
        self.guild_id
    }

    pub fn user_id(&self) -> InteractionEffectUserIdV1 {
        self.user_id
    }

    pub fn role(&self) -> &InteractionEffectPlannedRoleReferenceV1 {
        &self.role
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectPlannedPermissionTargetV1 {
    guild_id: InteractionEffectGuildIdV1,
    channel: InteractionEffectPlannedChannelReferenceV1,
    target: InteractionEffectPlannedOverwriteTargetV1,
}

impl InteractionEffectPlannedPermissionTargetV1 {
    pub fn new(
        guild_id: InteractionEffectGuildIdV1,
        channel: InteractionEffectPlannedChannelReferenceV1,
        target: InteractionEffectPlannedOverwriteTargetV1,
    ) -> Self {
        Self {
            guild_id,
            channel,
            target,
        }
    }

    pub fn guild_id(&self) -> InteractionEffectGuildIdV1 {
        self.guild_id
    }

    pub fn channel(&self) -> &InteractionEffectPlannedChannelReferenceV1 {
        &self.channel
    }

    pub fn target(&self) -> &InteractionEffectPlannedOverwriteTargetV1 {
        &self.target
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectPlannedInstanceTargetV1 {
    guild_id: InteractionEffectGuildIdV1,
    instance_id: InstanceId,
    instance_identity_digest: InteractionEffectOpaqueIdentityDigestV1,
}

impl InteractionEffectPlannedInstanceTargetV1 {
    pub fn new(guild_id: InteractionEffectGuildIdV1, instance_id: InstanceId) -> Self {
        let mut canonical = Vec::with_capacity(instance_id.as_str().len() + 48);
        canonical.extend_from_slice(b"starring.runtime.interaction.instance.v1\0");
        canonical.extend_from_slice(instance_id.as_str().as_bytes());
        Self {
            guild_id,
            instance_id,
            instance_identity_digest: InteractionEffectOpaqueIdentityDigestV1::from_canonical_bytes(
                &canonical,
            ),
        }
    }

    pub fn guild_id(&self) -> InteractionEffectGuildIdV1 {
        self.guild_id
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn instance_identity_digest(&self) -> &InteractionEffectOpaqueIdentityDigestV1 {
        &self.instance_identity_digest
    }

    fn exact(&self) -> InteractionEffectInstanceTargetV1 {
        InteractionEffectInstanceTargetV1::new(self.guild_id, self.instance_identity_digest.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectPlannedTargetV1 {
    CreateRole {
        guild_id: InteractionEffectGuildIdV1,
    },
    CreateChannel {
        guild_id: InteractionEffectGuildIdV1,
    },
    GrantRole {
        target: InteractionEffectPlannedRoleMembershipTargetV1,
    },
    UpsertOverwrite {
        target: InteractionEffectPlannedPermissionTargetV1,
        desired: InteractionEffectPermissionValueV1,
    },
    PostPanel {
        guild_id: InteractionEffectGuildIdV1,
        channel: InteractionEffectPlannedChannelReferenceV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
    RegisterInstance {
        target: InteractionEffectPlannedInstanceTargetV1,
        kind: InstanceKind,
        manifest_digest: InteractionEffectPayloadDigestV1,
    },
    TeardownInstance {
        target: InteractionEffectPlannedInstanceTargetV1,
    },
    EditResponse {
        receipt_identity: InteractionReceiptIdentityV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
}

impl InteractionEffectPlannedTargetV1 {
    pub fn kind(&self) -> InteractionEffectKindV1 {
        match self {
            Self::CreateRole { .. } => InteractionEffectKindV1::CreateRole,
            Self::CreateChannel { .. } => InteractionEffectKindV1::CreateChannel,
            Self::GrantRole { .. } => InteractionEffectKindV1::GrantRole,
            Self::UpsertOverwrite { .. } => InteractionEffectKindV1::UpsertOverwrite,
            Self::PostPanel { .. } => InteractionEffectKindV1::PostPanel,
            Self::RegisterInstance { .. } => InteractionEffectKindV1::RegisterInstance,
            Self::TeardownInstance { .. } => InteractionEffectKindV1::TeardownInstance,
            Self::EditResponse { .. } => InteractionEffectKindV1::EditResponse,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectPlannedPreimageV1 {
    None,
    RoleMembership {
        target: InteractionEffectPlannedRoleMembershipTargetV1,
        present: bool,
    },
    PermissionOverwrite {
        target: InteractionEffectPlannedPermissionTargetV1,
        before: InteractionEffectPermissionStateV1,
    },
    InstanceRegistration {
        target: InteractionEffectPlannedInstanceTargetV1,
        before: InteractionEffectInstanceStateV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectPlannedRecoveryInputV1 {
    target: InteractionEffectPlannedTargetV1,
    preimage: InteractionEffectPlannedPreimageV1,
}

impl InteractionEffectPlannedRecoveryInputV1 {
    pub fn new(
        target: InteractionEffectPlannedTargetV1,
        preimage: InteractionEffectPlannedPreimageV1,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        validate_planned_preimage_v1(&target, &preimage)?;
        Ok(Self { target, preimage })
    }

    pub fn target(&self) -> &InteractionEffectPlannedTargetV1 {
        &self.target
    }

    pub fn preimage(&self) -> &InteractionEffectPlannedPreimageV1 {
        &self.preimage
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectResolvedInputV1 {
    target: InteractionEffectTargetV1,
    preimage: InteractionEffectPreimageV1,
}

impl InteractionEffectResolvedInputV1 {
    pub fn target(&self) -> &InteractionEffectTargetV1 {
        &self.target
    }

    pub fn preimage(&self) -> &InteractionEffectPreimageV1 {
        &self.preimage
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectPlanDefinitionV1 {
    action: InteractionEffectActionIdentityV1,
    recovery_input: InteractionEffectPlannedRecoveryInputV1,
    dependencies: Vec<InteractionEffectPlannedDependencyV1>,
}

impl InteractionEffectPlanDefinitionV1 {
    pub fn new(
        action: InteractionEffectActionIdentityV1,
        recovery_input: InteractionEffectPlannedRecoveryInputV1,
        dependencies: Vec<InteractionEffectPlannedDependencyV1>,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if action.kind() != recovery_input.target().kind() {
            return Err(InteractionEffectIdentityErrorV1::TargetKind);
        }
        validate_dependency_order_v1(action.action_index(), &dependencies)?;
        validate_symbolic_dependencies_v1(&recovery_input, &dependencies)?;
        Ok(Self {
            action,
            recovery_input,
            dependencies,
        })
    }

    pub fn action(&self) -> &InteractionEffectActionIdentityV1 {
        &self.action
    }

    pub fn recovery_input(&self) -> &InteractionEffectPlannedRecoveryInputV1 {
        &self.recovery_input
    }

    pub fn dependencies(&self) -> &[InteractionEffectPlannedDependencyV1] {
        &self.dependencies
    }

    pub fn output_class(&self) -> InteractionEffectOutputClassV1 {
        output_class_v1(self.action.kind())
    }

    pub fn correlation_class(&self) -> InteractionEffectCorrelationClassV1 {
        correlation_class_v1(self.action.kind())
    }

    pub fn compensation_class(&self) -> InteractionEffectCompensationClassV1 {
        compensation_class_v1(self.action.kind())
    }

    pub fn recovery_scope(&self) -> InteractionEffectRecoveryScopeV1 {
        match self.action.kind() {
            InteractionEffectKindV1::EditResponse => InteractionEffectRecoveryScopeV1::ResponseTail,
            InteractionEffectKindV1::CreateRole
            | InteractionEffectKindV1::CreateChannel
            | InteractionEffectKindV1::GrantRole
            | InteractionEffectKindV1::UpsertOverwrite
            | InteractionEffectKindV1::PostPanel
            | InteractionEffectKindV1::RegisterInstance
            | InteractionEffectKindV1::TeardownInstance => {
                InteractionEffectRecoveryScopeV1::MutableProvisioning
            }
        }
    }

    pub fn planned_identity_digest(&self) -> InteractionEffectPlannedIdentityDigestV1 {
        crate::effect_digest::build_interaction_effect_planned_identity_digest_v1(self)
    }

    pub fn materialize(
        &self,
        resolutions: Vec<InteractionEffectDependencyResolutionV1>,
    ) -> Result<InteractionEffectMaterializedPlanV1, InteractionEffectIdentityErrorV1> {
        if resolutions.len() != self.dependencies.len() {
            return Err(InteractionEffectIdentityErrorV1::DependencyResolutionCount);
        }
        if self
            .dependencies
            .iter()
            .zip(&resolutions)
            .any(|(planned, resolution)| planned != resolution.planned())
        {
            return Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch);
        }
        let resolved = resolutions
            .iter()
            .map(|resolution| (resolution.planned().action_index(), resolution))
            .collect::<BTreeMap<_, _>>();
        if resolved.len() != resolutions.len() {
            return Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch);
        }
        let target = resolve_target_v1(self.recovery_input.target(), &resolved)?;
        let preimage = resolve_preimage_v1(self.recovery_input.preimage(), &resolved)?;
        let resolved_input = InteractionEffectResolvedInputV1 {
            target: target.clone(),
            preimage: preimage.clone(),
        };
        let dependencies = resolutions
            .into_iter()
            .map(InteractionEffectDependencyV1::from_resolution)
            .collect();
        let planned_identity_digest = self.planned_identity_digest();
        let definition = InteractionEffectDefinitionV1::from_materialized(
            self.action.clone(),
            target,
            preimage,
            dependencies,
            planned_identity_digest.clone(),
        )?;
        Ok(InteractionEffectMaterializedPlanV1 {
            definition,
            planned_recovery_input: self.recovery_input.clone(),
            resolved_input,
            planned_identity_digest,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectMaterializedPlanV1 {
    definition: InteractionEffectDefinitionV1,
    planned_recovery_input: InteractionEffectPlannedRecoveryInputV1,
    resolved_input: InteractionEffectResolvedInputV1,
    planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
}

impl InteractionEffectMaterializedPlanV1 {
    pub fn definition(&self) -> &InteractionEffectDefinitionV1 {
        &self.definition
    }

    pub fn resolved_input(&self) -> &InteractionEffectResolvedInputV1 {
        &self.resolved_input
    }

    pub fn planned_recovery_input(&self) -> &InteractionEffectPlannedRecoveryInputV1 {
        &self.planned_recovery_input
    }

    pub fn planned_identity_digest(&self) -> &InteractionEffectPlannedIdentityDigestV1 {
        &self.planned_identity_digest
    }

    pub fn into_definition(self) -> InteractionEffectDefinitionV1 {
        self.definition
    }
}

fn validate_dependency_order_v1(
    consumer: crate::InteractionEffectActionIndexV1,
    dependencies: &[InteractionEffectPlannedDependencyV1],
) -> Result<(), InteractionEffectIdentityErrorV1> {
    if dependencies.len() > MAX_INTERACTION_EFFECT_DEPENDENCIES_V1 {
        return Err(InteractionEffectIdentityErrorV1::DependencyCount);
    }
    let mut previous = None;
    for dependency in dependencies {
        if dependency.action_index() >= consumer {
            return Err(InteractionEffectIdentityErrorV1::DependencyNotPrior);
        }
        if previous.is_some_and(|index| index >= dependency.action_index()) {
            return Err(InteractionEffectIdentityErrorV1::DependencyOrder);
        }
        previous = Some(dependency.action_index());
    }
    Ok(())
}

fn validate_symbolic_dependencies_v1(
    input: &InteractionEffectPlannedRecoveryInputV1,
    dependencies: &[InteractionEffectPlannedDependencyV1],
) -> Result<(), InteractionEffectIdentityErrorV1> {
    let declared = dependencies
        .iter()
        .map(InteractionEffectPlannedDependencyV1::action_index)
        .collect::<BTreeSet<_>>();
    let mut required = Vec::new();
    collect_target_dependencies_v1(input.target(), &mut required);
    collect_preimage_dependencies_v1(input.preimage(), &mut required);
    if required
        .iter()
        .any(|dependency| !declared.contains(&dependency.action_index()))
    {
        return Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch);
    }
    for dependency in required {
        let declared = dependencies
            .iter()
            .find(|candidate| candidate.action_index() == dependency.action_index())
            .ok_or(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch)?;
        if declared != dependency {
            return Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch);
        }
    }
    Ok(())
}

fn collect_target_dependencies_v1<'a>(
    target: &'a InteractionEffectPlannedTargetV1,
    dependencies: &mut Vec<&'a InteractionEffectPlannedDependencyV1>,
) {
    match target {
        InteractionEffectPlannedTargetV1::GrantRole { target } => {
            collect_role_reference_v1(target.role(), dependencies);
        }
        InteractionEffectPlannedTargetV1::UpsertOverwrite { target, .. } => {
            collect_channel_reference_v1(target.channel(), dependencies);
            if let InteractionEffectPlannedOverwriteTargetV1::Role(role) = target.target() {
                collect_role_reference_v1(role, dependencies);
            }
        }
        InteractionEffectPlannedTargetV1::PostPanel { channel, .. } => {
            collect_channel_reference_v1(channel, dependencies);
        }
        InteractionEffectPlannedTargetV1::CreateRole { .. }
        | InteractionEffectPlannedTargetV1::CreateChannel { .. }
        | InteractionEffectPlannedTargetV1::RegisterInstance { .. }
        | InteractionEffectPlannedTargetV1::TeardownInstance { .. }
        | InteractionEffectPlannedTargetV1::EditResponse { .. } => {}
    }
}

fn collect_preimage_dependencies_v1<'a>(
    preimage: &'a InteractionEffectPlannedPreimageV1,
    dependencies: &mut Vec<&'a InteractionEffectPlannedDependencyV1>,
) {
    match preimage {
        InteractionEffectPlannedPreimageV1::RoleMembership { target, .. } => {
            collect_role_reference_v1(target.role(), dependencies);
        }
        InteractionEffectPlannedPreimageV1::PermissionOverwrite { target, .. } => {
            collect_channel_reference_v1(target.channel(), dependencies);
            if let InteractionEffectPlannedOverwriteTargetV1::Role(role) = target.target() {
                collect_role_reference_v1(role, dependencies);
            }
        }
        InteractionEffectPlannedPreimageV1::None
        | InteractionEffectPlannedPreimageV1::InstanceRegistration { .. } => {}
    }
}

fn collect_role_reference_v1<'a>(
    reference: &'a InteractionEffectPlannedRoleReferenceV1,
    dependencies: &mut Vec<&'a InteractionEffectPlannedDependencyV1>,
) {
    if let Some(dependency) = reference.dependency() {
        dependencies.push(dependency);
    }
}

fn collect_channel_reference_v1<'a>(
    reference: &'a InteractionEffectPlannedChannelReferenceV1,
    dependencies: &mut Vec<&'a InteractionEffectPlannedDependencyV1>,
) {
    if let Some(dependency) = reference.dependency() {
        dependencies.push(dependency);
    }
}

fn resolve_target_v1(
    planned: &InteractionEffectPlannedTargetV1,
    resolutions: &BTreeMap<
        crate::InteractionEffectActionIndexV1,
        &InteractionEffectDependencyResolutionV1,
    >,
) -> Result<InteractionEffectTargetV1, InteractionEffectIdentityErrorV1> {
    match planned {
        InteractionEffectPlannedTargetV1::CreateRole { guild_id } => {
            Ok(InteractionEffectTargetV1::CreateRole {
                guild_id: *guild_id,
            })
        }
        InteractionEffectPlannedTargetV1::CreateChannel { guild_id } => {
            Ok(InteractionEffectTargetV1::CreateChannel {
                guild_id: *guild_id,
            })
        }
        InteractionEffectPlannedTargetV1::GrantRole { target } => {
            let role_id = resolve_role_reference_v1(target.guild_id(), target.role(), resolutions)?;
            Ok(InteractionEffectTargetV1::GrantRole {
                target: InteractionEffectRoleMembershipTargetV1::new(
                    target.guild_id(),
                    target.user_id(),
                    role_id,
                ),
            })
        }
        InteractionEffectPlannedTargetV1::UpsertOverwrite { target, desired } => {
            let channel_id =
                resolve_channel_reference_v1(target.guild_id(), target.channel(), resolutions)?;
            let overwrite =
                resolve_overwrite_target_v1(target.guild_id(), target.target(), resolutions)?;
            Ok(InteractionEffectTargetV1::UpsertOverwrite {
                target: InteractionEffectPermissionTargetV1::new(
                    target.guild_id(),
                    channel_id,
                    overwrite,
                ),
                desired: *desired,
            })
        }
        InteractionEffectPlannedTargetV1::PostPanel {
            guild_id,
            channel,
            payload_digest,
        } => Ok(InteractionEffectTargetV1::PostPanel {
            guild_id: *guild_id,
            channel_id: resolve_channel_reference_v1(*guild_id, channel, resolutions)?,
            payload_digest: payload_digest.clone(),
        }),
        InteractionEffectPlannedTargetV1::RegisterInstance {
            target,
            kind,
            manifest_digest,
        } => Ok(InteractionEffectTargetV1::RegisterInstance {
            target: target.exact(),
            kind: kind.clone(),
            manifest_digest: manifest_digest.clone(),
        }),
        InteractionEffectPlannedTargetV1::TeardownInstance { target } => {
            Ok(InteractionEffectTargetV1::TeardownInstance {
                target: target.exact(),
            })
        }
        InteractionEffectPlannedTargetV1::EditResponse {
            receipt_identity,
            payload_digest,
        } => Ok(InteractionEffectTargetV1::EditResponse {
            receipt_identity: *receipt_identity,
            payload_digest: payload_digest.clone(),
        }),
    }
}

fn resolve_preimage_v1(
    planned: &InteractionEffectPlannedPreimageV1,
    resolutions: &BTreeMap<
        crate::InteractionEffectActionIndexV1,
        &InteractionEffectDependencyResolutionV1,
    >,
) -> Result<InteractionEffectPreimageV1, InteractionEffectIdentityErrorV1> {
    match planned {
        InteractionEffectPlannedPreimageV1::None => Ok(InteractionEffectPreimageV1::None),
        InteractionEffectPlannedPreimageV1::RoleMembership { target, present } => {
            let role_id = resolve_role_reference_v1(target.guild_id(), target.role(), resolutions)?;
            Ok(InteractionEffectPreimageV1::RoleMembership {
                target: InteractionEffectRoleMembershipTargetV1::new(
                    target.guild_id(),
                    target.user_id(),
                    role_id,
                ),
                present: *present,
            })
        }
        InteractionEffectPlannedPreimageV1::PermissionOverwrite { target, before } => {
            let channel_id =
                resolve_channel_reference_v1(target.guild_id(), target.channel(), resolutions)?;
            let overwrite =
                resolve_overwrite_target_v1(target.guild_id(), target.target(), resolutions)?;
            Ok(InteractionEffectPreimageV1::PermissionOverwrite {
                target: InteractionEffectPermissionTargetV1::new(
                    target.guild_id(),
                    channel_id,
                    overwrite,
                ),
                before: *before,
            })
        }
        InteractionEffectPlannedPreimageV1::InstanceRegistration { target, before } => {
            Ok(InteractionEffectPreimageV1::InstanceRegistration {
                target: target.exact(),
                before: before.clone(),
            })
        }
    }
}

fn resolve_role_reference_v1(
    expected_guild_id: InteractionEffectGuildIdV1,
    reference: &InteractionEffectPlannedRoleReferenceV1,
    resolutions: &BTreeMap<
        crate::InteractionEffectActionIndexV1,
        &InteractionEffectDependencyResolutionV1,
    >,
) -> Result<InteractionEffectRoleIdV1, InteractionEffectIdentityErrorV1> {
    match reference {
        InteractionEffectPlannedRoleReferenceV1::Existing(role_id) => Ok(*role_id),
        InteractionEffectPlannedRoleReferenceV1::Produced(dependency) => {
            let resolution = resolutions
                .get(&dependency.action_index())
                .ok_or(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch)?;
            match resolution.output() {
                InteractionEffectObservedOutputV1::CreatedRole { guild_id, role_id }
                    if *guild_id == expected_guild_id =>
                {
                    Ok(*role_id)
                }
                _ => Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch),
            }
        }
    }
}

fn resolve_channel_reference_v1(
    expected_guild_id: InteractionEffectGuildIdV1,
    reference: &InteractionEffectPlannedChannelReferenceV1,
    resolutions: &BTreeMap<
        crate::InteractionEffectActionIndexV1,
        &InteractionEffectDependencyResolutionV1,
    >,
) -> Result<InteractionEffectChannelIdV1, InteractionEffectIdentityErrorV1> {
    match reference {
        InteractionEffectPlannedChannelReferenceV1::Existing(channel_id) => Ok(*channel_id),
        InteractionEffectPlannedChannelReferenceV1::Produced(dependency) => {
            let resolution = resolutions
                .get(&dependency.action_index())
                .ok_or(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch)?;
            match resolution.output() {
                InteractionEffectObservedOutputV1::CreatedChannel {
                    guild_id,
                    channel_id,
                } if *guild_id == expected_guild_id => Ok(*channel_id),
                _ => Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch),
            }
        }
    }
}

fn resolve_overwrite_target_v1(
    expected_guild_id: InteractionEffectGuildIdV1,
    target: &InteractionEffectPlannedOverwriteTargetV1,
    resolutions: &BTreeMap<
        crate::InteractionEffectActionIndexV1,
        &InteractionEffectDependencyResolutionV1,
    >,
) -> Result<InteractionEffectOverwriteTargetV1, InteractionEffectIdentityErrorV1> {
    match target {
        InteractionEffectPlannedOverwriteTargetV1::Role(role) => {
            Ok(InteractionEffectOverwriteTargetV1::Role(
                resolve_role_reference_v1(expected_guild_id, role, resolutions)?,
            ))
        }
        InteractionEffectPlannedOverwriteTargetV1::Member(user_id) => {
            Ok(InteractionEffectOverwriteTargetV1::Member(*user_id))
        }
    }
}

fn validate_planned_preimage_v1(
    target: &InteractionEffectPlannedTargetV1,
    preimage: &InteractionEffectPlannedPreimageV1,
) -> Result<(), InteractionEffectIdentityErrorV1> {
    let valid = match (target, preimage) {
        (
            InteractionEffectPlannedTargetV1::CreateRole { .. }
            | InteractionEffectPlannedTargetV1::CreateChannel { .. }
            | InteractionEffectPlannedTargetV1::PostPanel { .. }
            | InteractionEffectPlannedTargetV1::EditResponse { .. },
            InteractionEffectPlannedPreimageV1::None,
        ) => true,
        (
            InteractionEffectPlannedTargetV1::GrantRole { target },
            InteractionEffectPlannedPreimageV1::RoleMembership {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        (
            InteractionEffectPlannedTargetV1::UpsertOverwrite { target, .. },
            InteractionEffectPlannedPreimageV1::PermissionOverwrite {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        (
            InteractionEffectPlannedTargetV1::RegisterInstance { target, .. }
            | InteractionEffectPlannedTargetV1::TeardownInstance { target },
            InteractionEffectPlannedPreimageV1::InstanceRegistration {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(InteractionEffectIdentityErrorV1::Preimage)
    }
}

fn output_class_v1(kind: InteractionEffectKindV1) -> InteractionEffectOutputClassV1 {
    match kind {
        InteractionEffectKindV1::CreateRole => InteractionEffectOutputClassV1::CreatedRole,
        InteractionEffectKindV1::CreateChannel => InteractionEffectOutputClassV1::CreatedChannel,
        InteractionEffectKindV1::GrantRole => InteractionEffectOutputClassV1::RoleMembership,
        InteractionEffectKindV1::UpsertOverwrite => {
            InteractionEffectOutputClassV1::PermissionOverwrite
        }
        InteractionEffectKindV1::PostPanel => InteractionEffectOutputClassV1::PostedMessage,
        InteractionEffectKindV1::RegisterInstance | InteractionEffectKindV1::TeardownInstance => {
            InteractionEffectOutputClassV1::InstanceState
        }
        InteractionEffectKindV1::EditResponse => InteractionEffectOutputClassV1::OriginalResponse,
    }
}

fn correlation_class_v1(kind: InteractionEffectKindV1) -> InteractionEffectCorrelationClassV1 {
    match kind {
        InteractionEffectKindV1::CreateRole
        | InteractionEffectKindV1::CreateChannel
        | InteractionEffectKindV1::GrantRole
        | InteractionEffectKindV1::UpsertOverwrite => {
            InteractionEffectCorrelationClassV1::AuditLogReason
        }
        InteractionEffectKindV1::PostPanel => InteractionEffectCorrelationClassV1::Unsupported,
        InteractionEffectKindV1::RegisterInstance | InteractionEffectKindV1::TeardownInstance => {
            InteractionEffectCorrelationClassV1::InternalIdempotencyKey
        }
        InteractionEffectKindV1::EditResponse => {
            InteractionEffectCorrelationClassV1::InteractionReceipt
        }
    }
}

fn compensation_class_v1(kind: InteractionEffectKindV1) -> InteractionEffectCompensationClassV1 {
    match kind {
        InteractionEffectKindV1::CreateRole => {
            InteractionEffectCompensationClassV1::DeleteCreatedRole
        }
        InteractionEffectKindV1::CreateChannel => {
            InteractionEffectCompensationClassV1::DeleteCreatedChannel
        }
        InteractionEffectKindV1::GrantRole => {
            InteractionEffectCompensationClassV1::RestoreRoleMembership
        }
        InteractionEffectKindV1::UpsertOverwrite => {
            InteractionEffectCompensationClassV1::RestorePermissionOverwrite
        }
        InteractionEffectKindV1::PostPanel => {
            InteractionEffectCompensationClassV1::DeletePostedMessage
        }
        InteractionEffectKindV1::RegisterInstance => {
            InteractionEffectCompensationClassV1::RestoreInstanceRegistration
        }
        InteractionEffectKindV1::TeardownInstance | InteractionEffectKindV1::EditResponse => {
            InteractionEffectCompensationClassV1::NotCompensable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        build_interaction_effect_planned_correlation_v1, DiscordApplicationIdV1,
        DiscordInteractionIdV1, InteractionActionPlanDigestV1, InteractionEffectActionDigestV1,
        InteractionEffectInputDigestV1, InteractionEffectPayloadDigestV1,
        InteractionPreflightCertificateDigestV1,
    };

    fn hex(value: char) -> String {
        value.to_string().repeat(64)
    }

    fn receipt() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(10).unwrap(),
            DiscordInteractionIdV1::new(20).unwrap(),
        )
    }

    fn action(index: u16, kind: InteractionEffectKindV1) -> InteractionEffectActionIdentityV1 {
        InteractionEffectActionIdentityV1::new(
            receipt(),
            InteractionActionPlanDigestV1::parse(hex('a')).unwrap(),
            InteractionPreflightCertificateDigestV1::parse(hex('b')).unwrap(),
            crate::InteractionEffectActionIndexV1::new(index).unwrap(),
            kind,
            InteractionEffectActionDigestV1::parse(hex('c')).unwrap(),
            InteractionEffectInputDigestV1::parse(hex('d')).unwrap(),
        )
    }

    fn create_role_plan() -> InteractionEffectPlanDefinitionV1 {
        InteractionEffectPlanDefinitionV1::new(
            action(0, InteractionEffectKindV1::CreateRole),
            InteractionEffectPlannedRecoveryInputV1::new(
                InteractionEffectPlannedTargetV1::CreateRole {
                    guild_id: InteractionEffectGuildIdV1::new(30).unwrap(),
                },
                InteractionEffectPlannedPreimageV1::None,
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap()
    }

    fn grant_role_plan(
        dependency: InteractionEffectPlannedDependencyV1,
    ) -> InteractionEffectPlanDefinitionV1 {
        let role = InteractionEffectPlannedRoleReferenceV1::produced(dependency.clone()).unwrap();
        let target = InteractionEffectPlannedRoleMembershipTargetV1::new(
            InteractionEffectGuildIdV1::new(30).unwrap(),
            InteractionEffectUserIdV1::new(40).unwrap(),
            role,
        );
        InteractionEffectPlanDefinitionV1::new(
            action(1, InteractionEffectKindV1::GrantRole),
            InteractionEffectPlannedRecoveryInputV1::new(
                InteractionEffectPlannedTargetV1::GrantRole {
                    target: target.clone(),
                },
                InteractionEffectPlannedPreimageV1::RoleMembership {
                    target,
                    present: false,
                },
            )
            .unwrap(),
            vec![dependency],
        )
        .unwrap()
    }

    #[test]
    fn generated_role_reference_materializes_only_from_exact_producer_output() {
        let producer = create_role_plan();
        let dependency = InteractionEffectPlannedDependencyV1::new(
            crate::InteractionEffectActionIndexV1::new(0).unwrap(),
            producer.planned_identity_digest(),
            InteractionEffectOutputClassV1::CreatedRole,
        );
        let consumer = grant_role_plan(dependency.clone());
        assert_eq!(
            consumer.materialize(Vec::new()),
            Err(InteractionEffectIdentityErrorV1::DependencyResolutionCount)
        );
        let output = InteractionEffectObservedOutputV1::CreatedRole {
            guild_id: InteractionEffectGuildIdV1::new(30).unwrap(),
            role_id: InteractionEffectRoleIdV1::new(50).unwrap(),
        };
        let resolution = InteractionEffectDependencyResolutionV1::new(dependency, output).unwrap();
        let materialized = consumer.materialize(vec![resolution]).unwrap();
        let InteractionEffectTargetV1::GrantRole { target } = materialized.definition().target()
        else {
            panic!("grant role target expected");
        };
        assert_eq!(target.role_id().get(), 50);
        assert_eq!(
            materialized.planned_identity_digest(),
            &consumer.planned_identity_digest()
        );
    }

    #[test]
    fn generated_reference_rejects_wrong_output_class_and_guild() {
        let producer = create_role_plan();
        let dependency = InteractionEffectPlannedDependencyV1::new(
            crate::InteractionEffectActionIndexV1::new(0).unwrap(),
            producer.planned_identity_digest(),
            InteractionEffectOutputClassV1::CreatedRole,
        );
        assert_eq!(
            InteractionEffectDependencyResolutionV1::new(
                dependency.clone(),
                InteractionEffectObservedOutputV1::CreatedChannel {
                    guild_id: InteractionEffectGuildIdV1::new(30).unwrap(),
                    channel_id: InteractionEffectChannelIdV1::new(50).unwrap(),
                },
            ),
            Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch)
        );
        let consumer = grant_role_plan(dependency.clone());
        let wrong_guild = InteractionEffectDependencyResolutionV1::new(
            dependency,
            InteractionEffectObservedOutputV1::CreatedRole {
                guild_id: InteractionEffectGuildIdV1::new(31).unwrap(),
                role_id: InteractionEffectRoleIdV1::new(50).unwrap(),
            },
        )
        .unwrap();
        assert_eq!(
            consumer.materialize(vec![wrong_guild]),
            Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch)
        );
    }

    #[test]
    fn post_panel_uses_unsupported_correlation_until_exact_readback_exists() {
        let plan = InteractionEffectPlanDefinitionV1::new(
            action(0, InteractionEffectKindV1::PostPanel),
            InteractionEffectPlannedRecoveryInputV1::new(
                InteractionEffectPlannedTargetV1::PostPanel {
                    guild_id: InteractionEffectGuildIdV1::new(30).unwrap(),
                    channel: InteractionEffectPlannedChannelReferenceV1::Existing(
                        InteractionEffectChannelIdV1::new(40).unwrap(),
                    ),
                    payload_digest: InteractionEffectPayloadDigestV1::parse(hex('e')).unwrap(),
                },
                InteractionEffectPlannedPreimageV1::None,
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let correlation = build_interaction_effect_planned_correlation_v1(&plan);
        assert_eq!(
            correlation.class(),
            InteractionEffectCorrelationClassV1::Unsupported
        );
        assert_eq!(correlation.message_nonce(), None);
    }
}
