use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use discord_model::{
    Channel, ChannelId, ChannelType, Member, OverwriteTarget, Permissions, Role, RoleId, UserId,
};

use super::types::{
    ActionEntryIdV1, ActionPlanPreflightErrorV1, ActionPlanSnapshotV1, FreshObservationV1,
    PreflightChannelRefV1, PreflightInstanceRefV1, PreflightOverwriteTargetV1, PreflightRoleRefV1,
    PreflightedActionPlanV1, PreparedActionPlanV1, PreparedPlanActionV1,
};

const MAX_GUILD_ROLES: usize = 250;
const MAX_GUILD_CHANNELS: usize = 500;
const MAX_CHANNEL_OVERWRITES: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SimulatedOverwriteTargetV1 {
    Everyone,
    Role(RoleId),
    Member(UserId),
    ProducedRole(ActionEntryIdV1),
}

#[derive(Clone)]
struct SimulatedOverwriteV1 {
    target: SimulatedOverwriteTargetV1,
    allow: Permissions,
    deny: Permissions,
}

#[derive(Clone)]
struct SimulatedChannelV1 {
    channel_type: ChannelType,
    overwrites: Vec<SimulatedOverwriteV1>,
}

struct SnapshotStateV1 {
    roles: BTreeMap<RoleId, Role>,
    channels: BTreeMap<ChannelId, SimulatedChannelV1>,
    bot_member: Option<Member>,
    actor_member: Option<Member>,
    created_roles: BTreeSet<ActionEntryIdV1>,
    created_channels: BTreeMap<ActionEntryIdV1, SimulatedChannelV1>,
    created_messages: BTreeSet<ActionEntryIdV1>,
    role_count: usize,
    channel_count: usize,
    guild_id: discord_model::GuildId,
}

pub fn preflight_action_plan_v1(
    prepared: PreparedActionPlanV1,
    snapshot: ActionPlanSnapshotV1,
) -> Result<PreflightedActionPlanV1, ActionPlanPreflightErrorV1> {
    if snapshot.guild_id != prepared.snapshot_request.guild_id() {
        return Err(ActionPlanPreflightErrorV1::SnapshotGuildMismatch);
    }
    let identity = snapshot.identity.clone();
    let mut state = SnapshotStateV1::new(&prepared, snapshot)?;
    state.validate_required_resources(&prepared)?;
    for action in &prepared.actions {
        state.validate_action(action, &prepared)?;
    }
    Ok(PreflightedActionPlanV1 {
        prepared,
        snapshot_identity: identity,
    })
}

impl SnapshotStateV1 {
    fn new(
        prepared: &PreparedActionPlanV1,
        snapshot: ActionPlanSnapshotV1,
    ) -> Result<Self, ActionPlanPreflightErrorV1> {
        let request = prepared.snapshot_request();
        require_evidence(
            request
                .observations()
                .contains(&FreshObservationV1::GuildRoles),
            snapshot.roles.is_some(),
            FreshObservationV1::GuildRoles,
        )?;
        require_evidence(
            request
                .observations()
                .contains(&FreshObservationV1::GuildChannels),
            snapshot.channels.is_some(),
            FreshObservationV1::GuildChannels,
        )?;
        require_evidence(
            request
                .observations()
                .contains(&FreshObservationV1::BotMember),
            snapshot.bot_member.is_some(),
            FreshObservationV1::BotMember,
        )?;
        require_evidence(
            request
                .observations()
                .contains(&FreshObservationV1::ActorMember),
            snapshot.actor_member.is_some(),
            FreshObservationV1::ActorMember,
        )?;
        let roles = canonical_roles(snapshot.guild_id, snapshot.roles)?;
        let channels = canonical_channels(snapshot.guild_id, &roles, snapshot.channels)?;
        let bot_member = canonical_member(snapshot.bot_member, &roles)?;
        let actor_member = canonical_member(snapshot.actor_member, &roles)?;
        if actor_member
            .as_ref()
            .is_some_and(|member| member.user_id != request.actor())
        {
            return Err(ActionPlanPreflightErrorV1::UnexpectedSnapshotActor);
        }
        let role_count = roles.len();
        let channel_count = channels.len();
        Ok(Self {
            roles,
            channels,
            bot_member,
            actor_member,
            created_roles: BTreeSet::new(),
            created_channels: BTreeMap::new(),
            created_messages: BTreeSet::new(),
            role_count,
            channel_count,
            guild_id: snapshot.guild_id,
        })
    }

    fn validate_required_resources(
        &self,
        prepared: &PreparedActionPlanV1,
    ) -> Result<(), ActionPlanPreflightErrorV1> {
        for role_id in prepared.snapshot_request().existing_roles() {
            if !self.roles.contains_key(role_id) {
                return Err(ActionPlanPreflightErrorV1::RequiredRoleMissing(*role_id));
            }
        }
        for channel_id in prepared.snapshot_request().existing_channels() {
            if !self.channels.contains_key(channel_id) {
                return Err(ActionPlanPreflightErrorV1::RequiredChannelMissing(
                    *channel_id,
                ));
            }
        }
        Ok(())
    }

    fn validate_action(
        &mut self,
        action: &PreparedPlanActionV1,
        prepared: &PreparedActionPlanV1,
    ) -> Result<(), ActionPlanPreflightErrorV1> {
        match action {
            PreparedPlanActionV1::GrantRole { entry, role, .. } => {
                self.require_actor(*entry)?;
                self.require_guild_permission(*entry, Permissions::MANAGE_ROLES)?;
                self.require_manageable_role(*entry, role)?;
            }
            PreparedPlanActionV1::CreateChannel { entry, output, .. } => {
                self.require_guild_permission(*entry, Permissions::MANAGE_CHANNELS)?;
                if self.channel_count >= MAX_GUILD_CHANNELS {
                    return Err(ActionPlanPreflightErrorV1::ChannelLimitExceeded);
                }
                self.channel_count += 1;
                self.created_channels.insert(
                    output.producer(),
                    SimulatedChannelV1 {
                        channel_type: ChannelType::Text,
                        overwrites: Vec::new(),
                    },
                );
            }
            PreparedPlanActionV1::CreateRole { entry, output, .. } => {
                self.require_guild_permission(*entry, Permissions::MANAGE_ROLES)?;
                if self.role_count >= MAX_GUILD_ROLES {
                    return Err(ActionPlanPreflightErrorV1::RoleLimitExceeded);
                }
                if self
                    .highest_bot_role()
                    .is_none_or(|role| role.position <= 0)
                {
                    return Err(ActionPlanPreflightErrorV1::TargetRoleOutranksBot {
                        entry: *entry,
                    });
                }
                self.role_count += 1;
                self.created_roles.insert(output.producer());
            }
            PreparedPlanActionV1::UpsertOverwrite {
                entry,
                channel,
                target,
                allow,
                deny,
            } => {
                self.require_channel_permission(*entry, channel, Permissions::MANAGE_ROLES)?;
                if let PreflightOverwriteTargetV1::Role(role) = target {
                    self.require_manageable_role(*entry, role)?;
                }
                let simulated_target = self.simulated_target(target);
                let simulated_channel = self.channel_mut(*entry, channel)?;
                if let Some(overwrite) = simulated_channel
                    .overwrites
                    .iter_mut()
                    .find(|overwrite| overwrite.target == simulated_target)
                {
                    overwrite.allow = *allow;
                    overwrite.deny = *deny;
                } else {
                    if simulated_channel.overwrites.len() >= MAX_CHANNEL_OVERWRITES {
                        return Err(ActionPlanPreflightErrorV1::OverwriteLimitExceeded {
                            entry: *entry,
                        });
                    }
                    simulated_channel.overwrites.push(SimulatedOverwriteV1 {
                        target: simulated_target,
                        allow: *allow,
                        deny: *deny,
                    });
                }
            }
            PreparedPlanActionV1::PostPanel {
                entry,
                output,
                channel,
                ..
            } => {
                let simulated_channel = self.channel(*entry, channel)?;
                if simulated_channel.channel_type != ChannelType::Text {
                    return Err(ActionPlanPreflightErrorV1::ChannelCannotReceiveMessages {
                        entry: *entry,
                    });
                }
                let permissions = self.effective_channel_permissions(simulated_channel)?;
                let required = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
                if !permissions.contains(required) {
                    return Err(ActionPlanPreflightErrorV1::BotPermissionMissing {
                        entry: *entry,
                        permission: required,
                    });
                }
                self.created_messages.insert(output.producer());
            }
            PreparedPlanActionV1::RegisterInstance {
                entry, resources, ..
            } => {
                for reference in resources.roles.values() {
                    if !self.created_roles.contains(&reference.producer()) {
                        return Err(ActionPlanPreflightErrorV1::ExecutionInvariant {
                            entry: *entry,
                        });
                    }
                }
                for reference in resources.channels.values() {
                    if !self.created_channels.contains_key(&reference.producer()) {
                        return Err(ActionPlanPreflightErrorV1::ExecutionInvariant {
                            entry: *entry,
                        });
                    }
                }
                for reference in resources.messages.values() {
                    if !self.created_messages.contains(&reference.producer()) {
                        return Err(ActionPlanPreflightErrorV1::ExecutionInvariant {
                            entry: *entry,
                        });
                    }
                }
            }
            PreparedPlanActionV1::TeardownInstance { entry, instance } => {
                self.require_guild_permission(*entry, Permissions::MANAGE_CHANNELS)?;
                self.require_guild_permission(*entry, Permissions::MANAGE_ROLES)?;
                if let PreflightInstanceRefV1::Existing(instance_id) = instance {
                    if let Some(resolved) = prepared.context().instance.as_ref() {
                        if &resolved.instance.id == instance_id {
                            for role_id in resolved.instance.resources.roles.values() {
                                self.require_manageable_role(
                                    *entry,
                                    &PreflightRoleRefV1::Instance(*role_id),
                                )?;
                            }
                        }
                    }
                }
            }
            PreparedPlanActionV1::RespondEphemeral { .. }
            | PreparedPlanActionV1::OpenModal { .. }
            | PreparedPlanActionV1::DeferEphemeral { .. }
            | PreparedPlanActionV1::EditResponse { .. } => {}
        }
        Ok(())
    }

    fn require_actor(&self, entry: ActionEntryIdV1) -> Result<(), ActionPlanPreflightErrorV1> {
        if self.actor_member.is_none() {
            return Err(ActionPlanPreflightErrorV1::ExecutionInvariant { entry });
        }
        Ok(())
    }

    fn require_guild_permission(
        &self,
        entry: ActionEntryIdV1,
        required: Permissions,
    ) -> Result<(), ActionPlanPreflightErrorV1> {
        let permissions = self.guild_permissions()?;
        if permissions.contains(Permissions::ADMINISTRATOR) || permissions.contains(required) {
            Ok(())
        } else {
            Err(ActionPlanPreflightErrorV1::BotPermissionMissing {
                entry,
                permission: required,
            })
        }
    }

    fn require_channel_permission(
        &self,
        entry: ActionEntryIdV1,
        channel: &PreflightChannelRefV1,
        required: Permissions,
    ) -> Result<(), ActionPlanPreflightErrorV1> {
        let channel = self.channel(entry, channel)?;
        let permissions = self.effective_channel_permissions(channel)?;
        if permissions.contains(Permissions::ADMINISTRATOR) || permissions.contains(required) {
            Ok(())
        } else {
            Err(ActionPlanPreflightErrorV1::BotPermissionMissing {
                entry,
                permission: required,
            })
        }
    }

    fn require_manageable_role(
        &self,
        entry: ActionEntryIdV1,
        reference: &PreflightRoleRefV1,
    ) -> Result<(), ActionPlanPreflightErrorV1> {
        match reference {
            PreflightRoleRefV1::Produced(reference) => {
                if !self.created_roles.contains(&reference.producer()) {
                    return Err(ActionPlanPreflightErrorV1::ExecutionInvariant { entry });
                }
                if self
                    .highest_bot_role()
                    .is_some_and(|role| role.position > 0)
                {
                    Ok(())
                } else {
                    Err(ActionPlanPreflightErrorV1::TargetRoleOutranksBot { entry })
                }
            }
            PreflightRoleRefV1::Existing(role_id) | PreflightRoleRefV1::Instance(role_id) => {
                if *role_id == RoleId(self.guild_id.0) {
                    return Err(ActionPlanPreflightErrorV1::TargetRoleUnassignable { entry });
                }
                let target = self
                    .roles
                    .get(role_id)
                    .ok_or(ActionPlanPreflightErrorV1::TargetRoleMissing { entry })?;
                if target.managed {
                    return Err(ActionPlanPreflightErrorV1::TargetRoleUnassignable { entry });
                }
                let highest = self
                    .highest_bot_role()
                    .ok_or(ActionPlanPreflightErrorV1::TargetRoleOutranksBot { entry })?;
                if compare_roles((highest.id, highest), (*role_id, target)) == Ordering::Greater {
                    Ok(())
                } else {
                    Err(ActionPlanPreflightErrorV1::TargetRoleOutranksBot { entry })
                }
            }
        }
    }

    fn guild_permissions(&self) -> Result<Permissions, ActionPlanPreflightErrorV1> {
        let bot = self
            .bot_member
            .as_ref()
            .ok_or(ActionPlanPreflightErrorV1::InvalidSnapshotMember)?;
        let everyone = RoleId(self.guild_id.0);
        let mut permissions = self
            .roles
            .get(&everyone)
            .map(|role| role.permissions)
            .ok_or(ActionPlanPreflightErrorV1::InvalidSnapshotRoleSet)?;
        for role_id in &bot.roles {
            permissions |= self
                .roles
                .get(role_id)
                .map(|role| role.permissions)
                .ok_or(ActionPlanPreflightErrorV1::InvalidSnapshotMember)?;
        }
        Ok(permissions)
    }

    fn highest_bot_role(&self) -> Option<&Role> {
        let bot = self.bot_member.as_ref()?;
        bot.roles
            .iter()
            .filter_map(|role_id| self.roles.get(role_id))
            .max_by(|left, right| compare_roles((left.id, left), (right.id, right)))
    }

    fn channel(
        &self,
        entry: ActionEntryIdV1,
        reference: &PreflightChannelRefV1,
    ) -> Result<&SimulatedChannelV1, ActionPlanPreflightErrorV1> {
        match reference {
            PreflightChannelRefV1::Existing(channel_id) => self.channels.get(channel_id),
            PreflightChannelRefV1::Produced(reference) => {
                self.created_channels.get(&reference.producer())
            }
        }
        .ok_or(ActionPlanPreflightErrorV1::ChannelUnavailable { entry })
    }

    fn channel_mut(
        &mut self,
        entry: ActionEntryIdV1,
        reference: &PreflightChannelRefV1,
    ) -> Result<&mut SimulatedChannelV1, ActionPlanPreflightErrorV1> {
        match reference {
            PreflightChannelRefV1::Existing(channel_id) => self.channels.get_mut(channel_id),
            PreflightChannelRefV1::Produced(reference) => {
                self.created_channels.get_mut(&reference.producer())
            }
        }
        .ok_or(ActionPlanPreflightErrorV1::ChannelUnavailable { entry })
    }

    fn simulated_target(&self, target: &PreflightOverwriteTargetV1) -> SimulatedOverwriteTargetV1 {
        match target {
            PreflightOverwriteTargetV1::Everyone => SimulatedOverwriteTargetV1::Everyone,
            PreflightOverwriteTargetV1::Role(PreflightRoleRefV1::Existing(role_id))
            | PreflightOverwriteTargetV1::Role(PreflightRoleRefV1::Instance(role_id)) => {
                if *role_id == RoleId(self.guild_id.0) {
                    SimulatedOverwriteTargetV1::Everyone
                } else {
                    SimulatedOverwriteTargetV1::Role(*role_id)
                }
            }
            PreflightOverwriteTargetV1::Role(PreflightRoleRefV1::Produced(reference)) => {
                SimulatedOverwriteTargetV1::ProducedRole(reference.producer())
            }
        }
    }

    fn effective_channel_permissions(
        &self,
        channel: &SimulatedChannelV1,
    ) -> Result<Permissions, ActionPlanPreflightErrorV1> {
        let bot = self
            .bot_member
            .as_ref()
            .ok_or(ActionPlanPreflightErrorV1::InvalidSnapshotMember)?;
        let base = self.guild_permissions()?;
        if base.contains(Permissions::ADMINISTRATOR) {
            return Ok(Permissions::all());
        }
        let mut permissions = base;
        if let Some(overwrite) = channel
            .overwrites
            .iter()
            .find(|overwrite| overwrite.target == SimulatedOverwriteTargetV1::Everyone)
        {
            permissions = apply_overwrite(permissions, overwrite.allow, overwrite.deny);
        }
        let bot_roles = bot.roles.iter().copied().collect::<BTreeSet<_>>();
        let mut allow = Permissions::empty();
        let mut deny = Permissions::empty();
        for overwrite in &channel.overwrites {
            if matches!(overwrite.target, SimulatedOverwriteTargetV1::Role(role_id) if bot_roles.contains(&role_id))
            {
                allow |= overwrite.allow;
                deny |= overwrite.deny;
            }
        }
        permissions = apply_overwrite(permissions, allow, deny);
        if let Some(overwrite) = channel
            .overwrites
            .iter()
            .find(|overwrite| overwrite.target == SimulatedOverwriteTargetV1::Member(bot.user_id))
        {
            permissions = apply_overwrite(permissions, overwrite.allow, overwrite.deny);
        }
        Ok(permissions)
    }
}

fn require_evidence(
    required: bool,
    present: bool,
    observation: FreshObservationV1,
) -> Result<(), ActionPlanPreflightErrorV1> {
    if required && !present {
        Err(ActionPlanPreflightErrorV1::MissingSnapshotEvidence(
            observation,
        ))
    } else {
        Ok(())
    }
}

fn canonical_roles(
    guild_id: discord_model::GuildId,
    roles: Option<Vec<Role>>,
) -> Result<BTreeMap<RoleId, Role>, ActionPlanPreflightErrorV1> {
    let Some(roles) = roles else {
        return Ok(BTreeMap::new());
    };
    if roles.is_empty() || roles.len() > MAX_GUILD_ROLES {
        return Err(ActionPlanPreflightErrorV1::InvalidSnapshotRoleSet);
    }
    let mut canonical = BTreeMap::new();
    for role in roles {
        if role.id.0 == 0 || canonical.insert(role.id, role).is_some() {
            return Err(ActionPlanPreflightErrorV1::InvalidSnapshotRoleSet);
        }
    }
    let everyone = canonical
        .get(&RoleId(guild_id.0))
        .ok_or(ActionPlanPreflightErrorV1::InvalidSnapshotRoleSet)?;
    if everyone.position != 0 || everyone.managed {
        return Err(ActionPlanPreflightErrorV1::InvalidSnapshotRoleSet);
    }
    Ok(canonical)
}

fn canonical_channels(
    guild_id: discord_model::GuildId,
    roles: &BTreeMap<RoleId, Role>,
    channels: Option<Vec<Channel>>,
) -> Result<BTreeMap<ChannelId, SimulatedChannelV1>, ActionPlanPreflightErrorV1> {
    let Some(channels) = channels else {
        return Ok(BTreeMap::new());
    };
    if channels.len() > MAX_GUILD_CHANNELS {
        return Err(ActionPlanPreflightErrorV1::InvalidSnapshotChannelSet);
    }
    let ids = channels
        .iter()
        .map(|channel| channel.id)
        .collect::<BTreeSet<_>>();
    if ids.len() != channels.len() || ids.iter().any(|id| id.0 == 0) {
        return Err(ActionPlanPreflightErrorV1::InvalidSnapshotChannelSet);
    }
    let mut canonical = BTreeMap::new();
    for channel in channels {
        if channel
            .parent_id
            .is_some_and(|parent| parent.0 == 0 || !ids.contains(&parent))
            || channel.overwrites.len() > MAX_CHANNEL_OVERWRITES
        {
            return Err(ActionPlanPreflightErrorV1::InvalidSnapshotChannelSet);
        }
        let mut targets = BTreeSet::new();
        let mut overwrites = Vec::with_capacity(channel.overwrites.len());
        for overwrite in channel.overwrites {
            let target = snapshot_target(guild_id, roles, overwrite.target)?;
            if !targets.insert(target) || overwrite.allow.intersects(overwrite.deny) {
                return Err(ActionPlanPreflightErrorV1::InvalidSnapshotChannelSet);
            }
            overwrites.push(SimulatedOverwriteV1 {
                target,
                allow: overwrite.allow,
                deny: overwrite.deny,
            });
        }
        canonical.insert(
            channel.id,
            SimulatedChannelV1 {
                channel_type: channel.channel_type,
                overwrites,
            },
        );
    }
    Ok(canonical)
}

fn snapshot_target(
    guild_id: discord_model::GuildId,
    roles: &BTreeMap<RoleId, Role>,
    target: OverwriteTarget,
) -> Result<SimulatedOverwriteTargetV1, ActionPlanPreflightErrorV1> {
    match target {
        OverwriteTarget::Role(role_id) => {
            if !roles.contains_key(&role_id) {
                return Err(ActionPlanPreflightErrorV1::InvalidSnapshotChannelSet);
            }
            if role_id == RoleId(guild_id.0) {
                Ok(SimulatedOverwriteTargetV1::Everyone)
            } else {
                Ok(SimulatedOverwriteTargetV1::Role(role_id))
            }
        }
        OverwriteTarget::Member(user_id) if user_id.0 != 0 => {
            Ok(SimulatedOverwriteTargetV1::Member(user_id))
        }
        OverwriteTarget::Member(_) => Err(ActionPlanPreflightErrorV1::InvalidSnapshotChannelSet),
    }
}

fn canonical_member(
    member: Option<Member>,
    roles: &BTreeMap<RoleId, Role>,
) -> Result<Option<Member>, ActionPlanPreflightErrorV1> {
    let Some(mut member) = member else {
        return Ok(None);
    };
    if member.user_id.0 == 0 {
        return Err(ActionPlanPreflightErrorV1::InvalidSnapshotMember);
    }
    member.roles.sort_unstable();
    let original_len = member.roles.len();
    member.roles.dedup();
    if original_len != member.roles.len()
        || member.roles.len() > MAX_GUILD_ROLES
        || member
            .roles
            .iter()
            .any(|role_id| !roles.contains_key(role_id))
    {
        return Err(ActionPlanPreflightErrorV1::InvalidSnapshotMember);
    }
    Ok(Some(member))
}

fn compare_roles((left_id, left): (RoleId, &Role), (right_id, right): (RoleId, &Role)) -> Ordering {
    left.position
        .cmp(&right.position)
        .then(right_id.0.cmp(&left_id.0))
}

fn apply_overwrite(permissions: Permissions, allow: Permissions, deny: Permissions) -> Permissions {
    Permissions::from_bits_retain((permissions.bits() & !deny.bits()) | allow.bits())
}
