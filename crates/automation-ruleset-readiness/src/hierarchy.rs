use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display, Formatter};

use automation_ruleset::RuleSetVersion;
use automation_state::{ActionSpec, InstanceRef, RoleRef};
use discord_model::{GuildId, RoleId};
use resource_resolution::ResourceBindingMap;

const MAX_GUILD_ROLES: usize = 250;
const MAX_BOT_ROLES: usize = 250;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuildRoleStateV1 {
    pub position: i64,
    pub managed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuildRoleHierarchyErrorV1 {
    InvalidGuild,
    RoleLimitExceeded,
    InvalidRole,
    EveryoneMissing,
    EveryoneInvalid,
    BotRoleLimitExceeded,
    DuplicateBotRole,
    InvalidBotRole,
    BotRoleMissing,
}

impl Display for GuildRoleHierarchyErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidGuild => "guild role hierarchy has an invalid guild identity",
            Self::RoleLimitExceeded => "guild role hierarchy exceeds its role bound",
            Self::InvalidRole => "guild role hierarchy has an invalid role identity",
            Self::EveryoneMissing => "guild role hierarchy is missing the everyone role",
            Self::EveryoneInvalid => "guild role hierarchy has an invalid everyone role",
            Self::BotRoleLimitExceeded => "guild role hierarchy exceeds its bot role bound",
            Self::DuplicateBotRole => "guild role hierarchy has a duplicate bot role",
            Self::InvalidBotRole => "guild role hierarchy has an invalid bot role",
            Self::BotRoleMissing => "guild role hierarchy is missing a bot role",
        })
    }
}

impl std::error::Error for GuildRoleHierarchyErrorV1 {}

#[derive(Clone, PartialEq, Eq)]
pub struct GuildRoleHierarchyV1 {
    guild_id: GuildId,
    roles: BTreeMap<RoleId, GuildRoleStateV1>,
    bot_role_ids: Vec<RoleId>,
}

impl GuildRoleHierarchyV1 {
    pub fn new(
        guild_id: GuildId,
        roles: BTreeMap<RoleId, GuildRoleStateV1>,
        bot_role_ids: Vec<RoleId>,
    ) -> Result<Self, GuildRoleHierarchyErrorV1> {
        if guild_id.0 == 0 {
            return Err(GuildRoleHierarchyErrorV1::InvalidGuild);
        }
        if roles.is_empty() || roles.len() > MAX_GUILD_ROLES {
            return Err(GuildRoleHierarchyErrorV1::RoleLimitExceeded);
        }
        if roles.keys().any(|role_id| role_id.0 == 0) {
            return Err(GuildRoleHierarchyErrorV1::InvalidRole);
        }
        let everyone_id = RoleId(guild_id.0);
        let everyone = roles
            .get(&everyone_id)
            .ok_or(GuildRoleHierarchyErrorV1::EveryoneMissing)?;
        if everyone.position != 0 || everyone.managed {
            return Err(GuildRoleHierarchyErrorV1::EveryoneInvalid);
        }
        if bot_role_ids.len() > MAX_BOT_ROLES {
            return Err(GuildRoleHierarchyErrorV1::BotRoleLimitExceeded);
        }
        let bot_role_count = bot_role_ids.len();
        let mut canonical_bot_roles = bot_role_ids.into_iter().collect::<BTreeSet<_>>();
        if canonical_bot_roles.is_empty() {
            return Ok(Self {
                guild_id,
                roles,
                bot_role_ids: Vec::new(),
            });
        }
        if canonical_bot_roles.len() != bot_role_count {
            return Err(GuildRoleHierarchyErrorV1::DuplicateBotRole);
        }
        if canonical_bot_roles.remove(&everyone_id)
            || canonical_bot_roles.iter().any(|role_id| role_id.0 == 0)
        {
            return Err(GuildRoleHierarchyErrorV1::InvalidBotRole);
        }
        if canonical_bot_roles
            .iter()
            .any(|role_id| !roles.contains_key(role_id))
        {
            return Err(GuildRoleHierarchyErrorV1::BotRoleMissing);
        }
        Ok(Self {
            guild_id,
            roles,
            bot_role_ids: canonical_bot_roles.into_iter().collect(),
        })
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn roles(&self) -> &BTreeMap<RoleId, GuildRoleStateV1> {
        &self.roles
    }

    pub fn bot_role_ids(&self) -> &[RoleId] {
        &self.bot_role_ids
    }

    fn highest_bot_role(&self) -> Option<(RoleId, GuildRoleStateV1)> {
        self.bot_role_ids
            .iter()
            .filter_map(|role_id| self.roles.get(role_id).map(|state| (*role_id, *state)))
            .max_by(compare_roles)
    }

    fn can_create_role(&self) -> bool {
        let everyone_id = RoleId(self.guild_id.0);
        let Some(everyone) = self.roles.get(&everyone_id).copied() else {
            return false;
        };
        self.highest_bot_role().is_some_and(|highest| {
            compare_roles(&highest, &(everyone_id, everyone)) == Ordering::Greater
        })
    }

    fn can_manage_role(&self, role_id: RoleId) -> Result<(), RoleHierarchyReadinessErrorV1> {
        if role_id == RoleId(self.guild_id.0) {
            return Err(RoleHierarchyReadinessErrorV1::TargetRoleUnassignable);
        }
        let target = self
            .roles
            .get(&role_id)
            .copied()
            .ok_or(RoleHierarchyReadinessErrorV1::TargetRoleMissing)?;
        if target.managed {
            return Err(RoleHierarchyReadinessErrorV1::TargetRoleUnassignable);
        }
        if self
            .highest_bot_role()
            .is_some_and(|highest| compare_roles(&highest, &(role_id, target)) == Ordering::Greater)
        {
            Ok(())
        } else {
            Err(RoleHierarchyReadinessErrorV1::TargetRoleOutranksBot)
        }
    }
}

impl Debug for GuildRoleHierarchyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GuildRoleHierarchyV1")
            .field("guild_id", &"<redacted>")
            .field("role_count", &self.roles.len())
            .field("bot_role_count", &self.bot_role_ids.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleHierarchyReadinessErrorV1 {
    EvidenceUnavailable,
    ContextGuildMismatch,
    TargetBindingMissing,
    TargetRoleMissing,
    TargetRoleUnassignable,
    TargetRoleOutranksBot,
    BotHierarchyInsufficient,
    CreatedRoleReferenceInvalid,
    UnsupportedDynamicRoleReference,
}

impl Display for RoleHierarchyReadinessErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EvidenceUnavailable => "role hierarchy evidence is unavailable",
            Self::ContextGuildMismatch => "role hierarchy evidence belongs to a different guild",
            Self::TargetBindingMissing => {
                "a role binding required for hierarchy readiness is missing"
            }
            Self::TargetRoleMissing => "a role required for hierarchy readiness is missing",
            Self::TargetRoleUnassignable => "a role required by the RuleSet cannot be assigned",
            Self::TargetRoleOutranksBot => {
                "a role required by the RuleSet is not below the bot hierarchy"
            }
            Self::BotHierarchyInsufficient => "the bot has no role above the everyone role",
            Self::CreatedRoleReferenceInvalid => {
                "a created role reference is not available at its use site"
            }
            Self::UnsupportedDynamicRoleReference => {
                "a dynamic role reference cannot be verified at Apply time"
            }
        })
    }
}

impl std::error::Error for RoleHierarchyReadinessErrorV1 {}

#[must_use]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RoleHierarchyReadyV1 {
    checked_existing_grants: usize,
    created_role_postcheck_required: bool,
    instance_role_runtime_recheck_required: bool,
}

impl RoleHierarchyReadyV1 {
    pub fn checked_existing_grants(self) -> usize {
        self.checked_existing_grants
    }

    pub fn created_role_postcheck_required(self) -> bool {
        self.created_role_postcheck_required
    }

    pub fn instance_role_runtime_recheck_required(self) -> bool {
        self.instance_role_runtime_recheck_required
    }

    pub fn runtime_guard_required(self) -> bool {
        self.checked_existing_grants > 0
            || self.created_role_postcheck_required
            || self.instance_role_runtime_recheck_required
    }
}

impl Debug for RoleHierarchyReadyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RoleHierarchyReadyV1")
            .field("checked_existing_grants", &self.checked_existing_grants)
            .field(
                "created_role_postcheck_required",
                &self.created_role_postcheck_required,
            )
            .field(
                "instance_role_runtime_recheck_required",
                &self.instance_role_runtime_recheck_required,
            )
            .finish()
    }
}

pub fn check_role_hierarchy_v1(
    artifact: &RuleSetVersion,
    bindings: &ResourceBindingMap,
    hierarchy: Option<&GuildRoleHierarchyV1>,
) -> Result<RoleHierarchyReadyV1, RoleHierarchyReadinessErrorV1> {
    if !requires_role_management(&artifact.definition) {
        return Ok(RoleHierarchyReadyV1 {
            checked_existing_grants: 0,
            created_role_postcheck_required: false,
            instance_role_runtime_recheck_required: false,
        });
    }
    let hierarchy = hierarchy.ok_or(RoleHierarchyReadinessErrorV1::EvidenceUnavailable)?;
    if hierarchy.guild_id != artifact.guild_id {
        return Err(RoleHierarchyReadinessErrorV1::ContextGuildMismatch);
    }
    let mut checked_existing_grants = 0_usize;
    let mut created_role_postcheck_required = false;
    let mut instance_role_runtime_recheck_required = false;
    for rule in &artifact.definition.rules {
        let mut created_roles = BTreeSet::new();
        for action in &rule.actions {
            match action {
                ActionSpec::CreateRole { key, .. } => {
                    require_role_creation(hierarchy)?;
                    created_roles.insert(key.as_str());
                    created_role_postcheck_required = true;
                }
                ActionSpec::GrantRole { role, .. } => match role {
                    RoleRef::Existing(key) => {
                        let role_id = bindings
                            .role_bindings
                            .get(key)
                            .copied()
                            .ok_or(RoleHierarchyReadinessErrorV1::TargetBindingMissing)?;
                        hierarchy.can_manage_role(role_id)?;
                        checked_existing_grants += 1;
                    }
                    RoleRef::Created(created) => {
                        if !created_roles.contains(created.created.as_str()) {
                            return Err(RoleHierarchyReadinessErrorV1::CreatedRoleReferenceInvalid);
                        }
                        require_role_creation(hierarchy)?;
                        created_role_postcheck_required = true;
                    }
                    RoleRef::Instance {
                        instance: InstanceRef::Event,
                        ..
                    } => {
                        require_role_creation(hierarchy)?;
                        instance_role_runtime_recheck_required = true;
                    }
                    RoleRef::Instance {
                        instance: InstanceRef::Created(_),
                        ..
                    } => {
                        return Err(RoleHierarchyReadinessErrorV1::UnsupportedDynamicRoleReference);
                    }
                },
                ActionSpec::RespondEphemeral { .. }
                | ActionSpec::OpenModal { .. }
                | ActionSpec::CreateChannel { .. }
                | ActionSpec::UpsertOverwrite { .. }
                | ActionSpec::PostPanel { .. }
                | ActionSpec::DeferEphemeral
                | ActionSpec::EditResponse { .. }
                | ActionSpec::RegisterInstance { .. } => {}
                ActionSpec::TeardownInstance { .. } => {
                    require_role_creation(hierarchy)?;
                    instance_role_runtime_recheck_required = true;
                }
            }
        }
    }
    Ok(RoleHierarchyReadyV1 {
        checked_existing_grants,
        created_role_postcheck_required,
        instance_role_runtime_recheck_required,
    })
}

fn requires_role_management(ruleset: &automation_state::InteractionRuleSet) -> bool {
    ruleset.rules.iter().any(|rule| {
        rule.actions.iter().any(|action| {
            matches!(
                action,
                ActionSpec::CreateRole { .. }
                    | ActionSpec::GrantRole { .. }
                    | ActionSpec::TeardownInstance { .. }
            )
        })
    })
}

fn require_role_creation(
    hierarchy: &GuildRoleHierarchyV1,
) -> Result<(), RoleHierarchyReadinessErrorV1> {
    if hierarchy.can_create_role() {
        Ok(())
    } else {
        Err(RoleHierarchyReadinessErrorV1::BotHierarchyInsufficient)
    }
}

fn compare_roles(
    (left_id, left): &(RoleId, GuildRoleStateV1),
    (right_id, right): &(RoleId, GuildRoleStateV1),
) -> Ordering {
    left.position
        .cmp(&right.position)
        .then(right_id.0.cmp(&left_id.0))
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{
        content_hash, RuleSetKey, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
    };
    use automation_state::{ActionTarget, CreatedRef, InteractionRule, RoleRef, TriggerSpec};
    use desired_state::ResourceKey;

    use super::*;

    const GUILD: GuildId = GuildId(7);

    fn artifact(actions: Vec<ActionSpec>) -> RuleSetVersion {
        let definition = automation_state::InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: vec![InteractionRule {
                key: "rule".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "test".to_string(),
                },
                actions,
            }],
        };
        RuleSetVersion {
            guild_id: GUILD,
            ruleset_key: RuleSetKey::parse("hierarchy").unwrap(),
            version: RuleSetVersionId::FIRST,
            schema_version: CURRENT_RULESET_SCHEMA_VERSION,
            content_hash: content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap(),
            definition,
            created_by: discord_model::UserId(1),
        }
    }

    fn hierarchy(
        bot_role_id: u64,
        bot_position: i64,
        target_role_id: u64,
        target_position: i64,
        target_managed: bool,
    ) -> GuildRoleHierarchyV1 {
        GuildRoleHierarchyV1::new(
            GUILD,
            BTreeMap::from([
                (
                    RoleId(GUILD.0),
                    GuildRoleStateV1 {
                        position: 0,
                        managed: false,
                    },
                ),
                (
                    RoleId(bot_role_id),
                    GuildRoleStateV1 {
                        position: bot_position,
                        managed: true,
                    },
                ),
                (
                    RoleId(target_role_id),
                    GuildRoleStateV1 {
                        position: target_position,
                        managed: target_managed,
                    },
                ),
            ]),
            vec![RoleId(bot_role_id)],
        )
        .unwrap()
    }

    fn grant_existing() -> RuleSetVersion {
        artifact(vec![ActionSpec::GrantRole {
            role: RoleRef::Existing(ResourceKey("member".to_string())),
            target: ActionTarget::Actor,
        }])
    }

    fn bindings(role_id: u64) -> ResourceBindingMap {
        let mut bindings = ResourceBindingMap::default();
        bindings
            .role_bindings
            .insert(ResourceKey("member".to_string()), RoleId(role_id));
        bindings
    }

    #[test]
    fn non_role_rules_do_not_require_hierarchy_evidence() {
        assert_eq!(
            check_role_hierarchy_v1(
                &artifact(vec![ActionSpec::RespondEphemeral {
                    content: "ok".to_string(),
                }]),
                &ResourceBindingMap::default(),
                None,
            ),
            Ok(RoleHierarchyReadyV1 {
                checked_existing_grants: 0,
                created_role_postcheck_required: false,
                instance_role_runtime_recheck_required: false,
            })
        );
    }

    #[test]
    fn role_creation_requires_a_bot_role_above_everyone() {
        let ruleset = artifact(vec![ActionSpec::CreateRole {
            key: "created".to_string(),
            name: "Created".to_string(),
        }]);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &ResourceBindingMap::default(), None),
            Err(RoleHierarchyReadinessErrorV1::EvidenceUnavailable)
        );
        let insufficient = hierarchy(100, 0, 200, 1, false);
        assert_eq!(
            check_role_hierarchy_v1(
                &ruleset,
                &ResourceBindingMap::default(),
                Some(&insufficient),
            ),
            Err(RoleHierarchyReadinessErrorV1::BotHierarchyInsufficient)
        );
        let sufficient = hierarchy(100, 10, 200, 1, false);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &ResourceBindingMap::default(), Some(&sufficient),),
            Ok(RoleHierarchyReadyV1 {
                checked_existing_grants: 0,
                created_role_postcheck_required: true,
                instance_role_runtime_recheck_required: false,
            })
        );
    }

    #[test]
    fn existing_role_must_be_assignable_and_below_the_bot() {
        let ruleset = grant_existing();
        let lower = hierarchy(100, 10, 200, 9, false);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(200), Some(&lower)),
            Ok(RoleHierarchyReadyV1 {
                checked_existing_grants: 1,
                created_role_postcheck_required: false,
                instance_role_runtime_recheck_required: false,
            })
        );
        assert!(
            check_role_hierarchy_v1(&ruleset, &bindings(200), Some(&lower))
                .unwrap()
                .runtime_guard_required()
        );
        let higher = hierarchy(100, 10, 200, 11, false);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(200), Some(&higher)),
            Err(RoleHierarchyReadinessErrorV1::TargetRoleOutranksBot)
        );
        let managed = hierarchy(100, 10, 200, 9, true);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(200), Some(&managed)),
            Err(RoleHierarchyReadinessErrorV1::TargetRoleUnassignable)
        );
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(GUILD.0), Some(&lower)),
            Err(RoleHierarchyReadinessErrorV1::TargetRoleUnassignable)
        );
    }

    #[test]
    fn equal_positions_use_discord_role_id_ordering() {
        let ruleset = grant_existing();
        let bot_with_lower_id = hierarchy(100, 10, 200, 10, false);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(200), Some(&bot_with_lower_id),),
            Ok(RoleHierarchyReadyV1 {
                checked_existing_grants: 1,
                created_role_postcheck_required: false,
                instance_role_runtime_recheck_required: false,
            })
        );
        let bot_with_higher_id = hierarchy(200, 10, 100, 10, false);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(100), Some(&bot_with_higher_id),),
            Err(RoleHierarchyReadinessErrorV1::TargetRoleOutranksBot)
        );
    }

    #[test]
    fn dynamic_created_and_instance_roles_require_creation_hierarchy() {
        let sufficient = hierarchy(100, 10, 200, 1, false);
        let created = artifact(vec![
            ActionSpec::CreateRole {
                key: "created".to_string(),
                name: "Created".to_string(),
            },
            ActionSpec::GrantRole {
                role: RoleRef::Created(CreatedRef {
                    created: "created".to_string(),
                }),
                target: ActionTarget::Actor,
            },
        ]);
        let created_ready =
            check_role_hierarchy_v1(&created, &ResourceBindingMap::default(), Some(&sufficient))
                .unwrap();
        assert!(created_ready.created_role_postcheck_required());
        assert!(!created_ready.instance_role_runtime_recheck_required());

        let instance = artifact(vec![ActionSpec::GrantRole {
            role: RoleRef::Instance {
                instance: automation_state::InstanceRef::Event,
                alias: "member".to_string(),
            },
            target: ActionTarget::Actor,
        }]);
        let instance_ready =
            check_role_hierarchy_v1(&instance, &ResourceBindingMap::default(), Some(&sufficient))
                .unwrap();
        assert!(!instance_ready.created_role_postcheck_required());
        assert!(instance_ready.instance_role_runtime_recheck_required());

        let forward = artifact(vec![ActionSpec::GrantRole {
            role: RoleRef::Created(CreatedRef {
                created: "missing".to_string(),
            }),
            target: ActionTarget::Actor,
        }]);
        assert_eq!(
            check_role_hierarchy_v1(&forward, &ResourceBindingMap::default(), Some(&sufficient),),
            Err(RoleHierarchyReadinessErrorV1::CreatedRoleReferenceInvalid)
        );
    }

    #[test]
    fn hierarchy_constructor_rejects_duplicate_or_unknown_bot_roles() {
        let roles = BTreeMap::from([(
            RoleId(GUILD.0),
            GuildRoleStateV1 {
                position: 0,
                managed: false,
            },
        )]);
        assert_eq!(
            GuildRoleHierarchyV1::new(GUILD, roles.clone(), vec![RoleId(9), RoleId(9)]),
            Err(GuildRoleHierarchyErrorV1::DuplicateBotRole)
        );
        assert_eq!(
            GuildRoleHierarchyV1::new(GUILD, roles, vec![RoleId(9)]),
            Err(GuildRoleHierarchyErrorV1::BotRoleMissing)
        );
    }

    #[test]
    fn hierarchy_context_and_role_observations_fail_closed() {
        let ruleset = grant_existing();
        let other_guild = GuildRoleHierarchyV1::new(
            GuildId(8),
            BTreeMap::from([(
                RoleId(8),
                GuildRoleStateV1 {
                    position: 0,
                    managed: false,
                },
            )]),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(200), Some(&other_guild)),
            Err(RoleHierarchyReadinessErrorV1::ContextGuildMismatch)
        );

        let observed = hierarchy(100, 10, 200, 1, false);
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &ResourceBindingMap::default(), Some(&observed)),
            Err(RoleHierarchyReadinessErrorV1::TargetBindingMissing)
        );
        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &bindings(300), Some(&observed)),
            Err(RoleHierarchyReadinessErrorV1::TargetRoleMissing)
        );

        let non_contiguous_positions = BTreeMap::from([
            (
                RoleId(GUILD.0),
                GuildRoleStateV1 {
                    position: 0,
                    managed: false,
                },
            ),
            (
                RoleId(100),
                GuildRoleStateV1 {
                    position: -10,
                    managed: true,
                },
            ),
            (
                RoleId(200),
                GuildRoleStateV1 {
                    position: 500,
                    managed: false,
                },
            ),
        ]);
        assert!(
            GuildRoleHierarchyV1::new(GUILD, non_contiguous_positions, vec![RoleId(100)],).is_ok()
        );
    }

    #[test]
    fn overwrite_role_targets_do_not_invoke_grant_hierarchy() {
        let ruleset = artifact(vec![ActionSpec::UpsertOverwrite {
            channel: automation_state::ChannelRef::Existing(ResourceKey("channel".to_string())),
            target: automation_state::OverwriteTargetSpec::Role(RoleRef::Existing(ResourceKey(
                "member".to_string(),
            ))),
            allow: discord_model::Permissions::VIEW_CHANNEL,
            deny: discord_model::Permissions::empty(),
        }]);

        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &ResourceBindingMap::default(), None),
            Ok(RoleHierarchyReadyV1 {
                checked_existing_grants: 0,
                created_role_postcheck_required: false,
                instance_role_runtime_recheck_required: false,
            })
        );
    }

    #[test]
    fn unsupported_created_instance_role_is_rejected() {
        let ruleset = artifact(vec![ActionSpec::GrantRole {
            role: RoleRef::Instance {
                instance: InstanceRef::Created(CreatedRef {
                    created: "instance".to_string(),
                }),
                alias: "member".to_string(),
            },
            target: ActionTarget::Actor,
        }]);
        let observed = hierarchy(100, 10, 200, 1, false);

        assert_eq!(
            check_role_hierarchy_v1(&ruleset, &ResourceBindingMap::default(), Some(&observed)),
            Err(RoleHierarchyReadinessErrorV1::UnsupportedDynamicRoleReference)
        );
    }

    #[test]
    fn teardown_requires_dynamic_role_recheck() {
        let ruleset = artifact(vec![ActionSpec::TeardownInstance {
            instance: InstanceRef::Event,
        }]);
        let observed = hierarchy(100, 10, 200, 1, false);

        let ready =
            check_role_hierarchy_v1(&ruleset, &ResourceBindingMap::default(), Some(&observed))
                .unwrap();

        assert!(ready.instance_role_runtime_recheck_required());
        assert!(!ready.created_role_postcheck_required());
    }

    #[test]
    fn hierarchy_debug_output_is_redacted() {
        let observed = hierarchy(100, 10, 200, 1, false);
        let rendered = format!("{observed:?}");

        assert!(!rendered.contains("100"));
        assert!(!rendered.contains("200"));
        assert!(rendered.contains("role_count"));
    }
}
