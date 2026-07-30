use std::collections::BTreeMap;
use std::sync::Arc;

use automation_ruleset::RuleSetVersion;
use automation_ruleset_readiness::{
    build_readiness_context, check_readiness, check_role_hierarchy_v1, GuildCapabilities,
    GuildRoleHierarchyErrorV1, GuildRoleHierarchyV1, GuildRoleStateV1, ReadinessContextError,
    ReadinessError, RoleHierarchyReadinessErrorV1, RoleHierarchyReadyV1, RuleSetReadinessInput,
    RuntimeRuleSet,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, RoleId};
use resource_resolution::ResourceBindingMap;
use tokio::sync::OnceCell;
use twilight_http::Client;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

use crate::strict_panel_installer::TwilightStrictPanelInstaller;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeObservedRoleV1 {
    pub permissions: Permissions,
    pub position: i64,
    pub managed: bool,
}

pub struct RuntimeReadinessContextV1 {
    guild_capabilities: GuildCapabilities,
    role_permissions: BTreeMap<ResourceKey, Permissions>,
    role_hierarchy: GuildRoleHierarchyV1,
}

impl RuntimeReadinessContextV1 {
    pub fn guild_capabilities(&self) -> &GuildCapabilities {
        &self.guild_capabilities
    }

    pub fn role_permissions(&self) -> &BTreeMap<ResourceKey, Permissions> {
        &self.role_permissions
    }

    pub fn role_hierarchy(&self) -> &GuildRoleHierarchyV1 {
        &self.role_hierarchy
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeReadinessSnapshotErrorV1 {
    #[error("Discord bot identity is unavailable")]
    BotIdentityUnavailable,
    #[error("Discord bot identity response is invalid")]
    BotIdentityInvalid,
    #[error("Discord guild roles are unavailable")]
    GuildRolesUnavailable,
    #[error("Discord guild roles response is invalid")]
    GuildRolesInvalid,
    #[error("Discord bot member is unavailable")]
    BotMemberUnavailable,
    #[error("Discord bot member response is invalid")]
    BotMemberInvalid,
    #[error("Discord everyone role is missing")]
    EveryoneRoleMissing,
    #[error("a bound Discord role is missing")]
    BoundRoleMissing,
    #[error("Discord role hierarchy is invalid: {0}")]
    RoleHierarchyInvalid(GuildRoleHierarchyErrorV1),
}

impl RuntimeReadinessSnapshotErrorV1 {
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::BotIdentityUnavailable | Self::GuildRolesUnavailable | Self::BotMemberUnavailable
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeTargetReadinessErrorV1 {
    Readiness(ReadinessError),
    RoleHierarchy(RoleHierarchyReadinessErrorV1),
}

pub struct RuntimeTargetReadyV1 {
    runtime_ruleset: RuntimeRuleSet,
    role_hierarchy: RoleHierarchyReadyV1,
}

impl RuntimeTargetReadyV1 {
    pub fn runtime_ruleset(&self) -> &RuntimeRuleSet {
        &self.runtime_ruleset
    }

    pub fn role_hierarchy(&self) -> RoleHierarchyReadyV1 {
        self.role_hierarchy
    }

    pub fn into_runtime_ruleset(self) -> RuntimeRuleSet {
        self.runtime_ruleset
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDiscordPreflightErrorV1 {
    #[error("Discord readiness snapshot failed: {0}")]
    Snapshot(RuntimeReadinessSnapshotErrorV1),
    #[error("runtime target readiness check failed")]
    Target(RuntimeTargetReadinessErrorV1),
    #[error("runtime target guild does not match the requested Discord guild")]
    TargetGuildMismatch,
}

impl RuntimeDiscordPreflightErrorV1 {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Snapshot(error) => error.is_retryable(),
            Self::Target(_) | Self::TargetGuildMismatch => false,
        }
    }
}

#[derive(Clone)]
pub struct OwnedDiscordRuntimePreflightV1 {
    http: Arc<Client>,
    bot_user_id: Arc<OnceCell<Id<UserMarker>>>,
}

impl OwnedDiscordRuntimePreflightV1 {
    pub fn new(discord_token: String) -> Self {
        Self {
            http: Arc::new(Client::new(discord_token)),
            bot_user_id: Arc::new(OnceCell::new()),
        }
    }

    pub async fn preflight(
        &self,
        guild_id: GuildId,
        artifact: &RuleSetVersion,
        bindings: &ResourceBindingMap,
    ) -> Result<RuntimeTargetReadyV1, RuntimeDiscordPreflightErrorV1> {
        if artifact.guild_id != guild_id {
            return Err(RuntimeDiscordPreflightErrorV1::TargetGuildMismatch);
        }
        let bot_user_id = *self
            .bot_user_id
            .get_or_try_init(|| async {
                TwilightRuntimeReadinessProvider::new(self.http.as_ref())
                    .await
                    .map(|provider| provider.bot_user_id)
            })
            .await
            .map_err(RuntimeDiscordPreflightErrorV1::Snapshot)?;
        let provider = TwilightRuntimeReadinessProvider {
            http: self.http.as_ref(),
            bot_user_id,
        };
        let context = provider
            .snapshot(guild_id, bindings)
            .await
            .map_err(RuntimeDiscordPreflightErrorV1::Snapshot)?;
        check_runtime_target_readiness_v1(artifact, bindings, &context)
            .map_err(RuntimeDiscordPreflightErrorV1::Target)
    }
}

#[derive(Clone)]
pub struct OwnedDiscordRuntimeOperationsV2 {
    preflight: OwnedDiscordRuntimePreflightV1,
}

impl OwnedDiscordRuntimeOperationsV2 {
    pub fn new(discord_token: String) -> Self {
        Self {
            preflight: OwnedDiscordRuntimePreflightV1::new(discord_token),
        }
    }

    pub async fn preflight(
        &self,
        guild_id: GuildId,
        artifact: &RuleSetVersion,
        bindings: &ResourceBindingMap,
    ) -> Result<RuntimeTargetReadyV1, RuntimeDiscordPreflightErrorV1> {
        self.preflight.preflight(guild_id, artifact, bindings).await
    }

    pub fn strict_panel_installer(&self) -> TwilightStrictPanelInstaller<'_> {
        TwilightStrictPanelInstaller::new(self.preflight.http.as_ref())
    }
}

pub fn build_runtime_readiness_context_v1(
    guild_id: GuildId,
    bindings: &ResourceBindingMap,
    observed_roles: BTreeMap<RoleId, RuntimeObservedRoleV1>,
    bot_role_ids: Vec<RoleId>,
) -> Result<RuntimeReadinessContextV1, RuntimeReadinessSnapshotErrorV1> {
    let role_permissions = observed_roles
        .iter()
        .map(|(role_id, role)| (*role_id, role.permissions))
        .collect();
    let hierarchy_roles = observed_roles
        .into_iter()
        .map(|(role_id, role)| {
            (
                role_id,
                GuildRoleStateV1 {
                    position: role.position,
                    managed: role.managed,
                },
            )
        })
        .collect();
    let (guild_capabilities, role_permissions) =
        build_readiness_context(guild_id, bindings, &role_permissions, &bot_role_ids)
            .map_err(map_context_error)?;
    let role_hierarchy = GuildRoleHierarchyV1::new(guild_id, hierarchy_roles, bot_role_ids)
        .map_err(RuntimeReadinessSnapshotErrorV1::RoleHierarchyInvalid)?;
    Ok(RuntimeReadinessContextV1 {
        guild_capabilities,
        role_permissions,
        role_hierarchy,
    })
}

pub fn check_runtime_target_readiness_v1(
    artifact: &RuleSetVersion,
    bindings: &ResourceBindingMap,
    context: &RuntimeReadinessContextV1,
) -> Result<RuntimeTargetReadyV1, RuntimeTargetReadinessErrorV1> {
    let runtime_ruleset = check_readiness(RuleSetReadinessInput {
        artifact,
        bindings,
        guild_capabilities: context.guild_capabilities(),
        role_permissions: context.role_permissions(),
    })
    .map_err(RuntimeTargetReadinessErrorV1::Readiness)?;
    let role_hierarchy =
        check_role_hierarchy_v1(artifact, bindings, Some(context.role_hierarchy()))
            .map_err(RuntimeTargetReadinessErrorV1::RoleHierarchy)?;
    Ok(RuntimeTargetReadyV1 {
        runtime_ruleset,
        role_hierarchy,
    })
}

pub struct TwilightRuntimeReadinessProvider<'a> {
    http: &'a Client,
    bot_user_id: Id<UserMarker>,
}

impl<'a> TwilightRuntimeReadinessProvider<'a> {
    pub async fn new(http: &'a Client) -> Result<Self, RuntimeReadinessSnapshotErrorV1> {
        let bot = http
            .current_user()
            .await
            .map_err(|_| RuntimeReadinessSnapshotErrorV1::BotIdentityUnavailable)?
            .model()
            .await
            .map_err(|_| RuntimeReadinessSnapshotErrorV1::BotIdentityInvalid)?;
        Ok(Self {
            http,
            bot_user_id: bot.id,
        })
    }

    pub async fn snapshot(
        &self,
        guild_id: GuildId,
        bindings: &ResourceBindingMap,
    ) -> Result<RuntimeReadinessContextV1, RuntimeReadinessSnapshotErrorV1> {
        let guild = Id::<GuildMarker>::new(guild_id.0);
        let (roles_response, member_response) = tokio::join!(
            self.http.roles(guild),
            self.http.guild_member(guild, self.bot_user_id)
        );
        let roles_response =
            roles_response.map_err(|_| RuntimeReadinessSnapshotErrorV1::GuildRolesUnavailable)?;
        let member_response =
            member_response.map_err(|_| RuntimeReadinessSnapshotErrorV1::BotMemberUnavailable)?;
        let (roles, member) = tokio::join!(roles_response.model(), member_response.model());
        let roles = roles.map_err(|_| RuntimeReadinessSnapshotErrorV1::GuildRolesInvalid)?;
        let member = member.map_err(|_| RuntimeReadinessSnapshotErrorV1::BotMemberInvalid)?;
        let observed_roles = roles
            .into_iter()
            .map(|role| {
                (
                    RoleId(role.id.get()),
                    RuntimeObservedRoleV1 {
                        permissions: Permissions::from_bits_retain(role.permissions.bits()),
                        position: role.position,
                        managed: role.managed,
                    },
                )
            })
            .collect();
        let bot_role_ids = member
            .roles
            .into_iter()
            .map(|role| RoleId(role.get()))
            .collect();
        build_runtime_readiness_context_v1(guild_id, bindings, observed_roles, bot_role_ids)
    }
}

fn map_context_error(error: ReadinessContextError) -> RuntimeReadinessSnapshotErrorV1 {
    match error {
        ReadinessContextError::EveryoneRoleMissing => {
            RuntimeReadinessSnapshotErrorV1::EveryoneRoleMissing
        }
        ReadinessContextError::BoundRoleMissing { .. } => {
            RuntimeReadinessSnapshotErrorV1::BoundRoleMissing
        }
    }
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{content_hash, RuleSetKey, RuleSetSchemaVersion, RuleSetVersionId};
    use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
    use discord_model::UserId;

    use super::*;

    const GUILD: GuildId = GuildId(7);

    fn roles(bot_permissions: Permissions) -> BTreeMap<RoleId, RuntimeObservedRoleV1> {
        [
            (
                RoleId(GUILD.0),
                RuntimeObservedRoleV1 {
                    permissions: Permissions::VIEW_CHANNEL,
                    position: 0,
                    managed: false,
                },
            ),
            (
                RoleId(9),
                RuntimeObservedRoleV1 {
                    permissions: bot_permissions,
                    position: 1,
                    managed: false,
                },
            ),
        ]
        .into_iter()
        .collect()
    }

    fn artifact(actions: Vec<ActionSpec>) -> RuleSetVersion {
        let mut complete_actions = vec![ActionSpec::DeferEphemeral];
        complete_actions.extend(actions);
        complete_actions.push(ActionSpec::EditResponse {
            content: "done".to_string(),
        });
        let definition = InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: vec![InteractionRule {
                key: "rule".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "run".to_string(),
                },
                actions: complete_actions,
            }],
        };
        let schema_version = RuleSetSchemaVersion::new(1).unwrap();
        let content_hash = content_hash(schema_version, &definition).unwrap();
        RuleSetVersion {
            guild_id: GUILD,
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::new(1).unwrap(),
            schema_version,
            definition,
            content_hash,
            created_by: UserId(1),
        }
    }

    #[test]
    fn exact_runtime_readiness_accepts_current_manage_role_authority() {
        let context = build_runtime_readiness_context_v1(
            GUILD,
            &ResourceBindingMap::default(),
            roles(Permissions::MANAGE_ROLES),
            vec![RoleId(9)],
        )
        .unwrap();
        let artifact = artifact(vec![
            ActionSpec::CreateRole {
                key: "member".to_string(),
                name: "Member".to_string(),
            },
            ActionSpec::GrantRole {
                target: automation_state::ActionTarget::Actor,
                role: automation_state::RoleRef::Created(automation_state::CreatedRef {
                    created: "member".to_string(),
                }),
            },
        ]);

        let ready =
            check_runtime_target_readiness_v1(&artifact, &ResourceBindingMap::default(), &context)
                .unwrap();

        assert_eq!(ready.runtime_ruleset().version, artifact.version);
        assert!(ready.role_hierarchy().runtime_guard_required());
    }

    #[test]
    fn missing_everyone_role_fails_closed() {
        let result = build_runtime_readiness_context_v1(
            GUILD,
            &ResourceBindingMap::default(),
            BTreeMap::new(),
            Vec::new(),
        );

        assert!(matches!(
            result,
            Err(RuntimeReadinessSnapshotErrorV1::EveryoneRoleMissing)
        ));
    }

    #[test]
    fn missing_capability_rejects_target() {
        let context = build_runtime_readiness_context_v1(
            GUILD,
            &ResourceBindingMap::default(),
            roles(Permissions::VIEW_CHANNEL),
            vec![RoleId(9)],
        )
        .unwrap();
        let artifact = artifact(vec![ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "Member".to_string(),
        }]);

        assert!(matches!(
            check_runtime_target_readiness_v1(&artifact, &ResourceBindingMap::default(), &context),
            Err(RuntimeTargetReadinessErrorV1::Readiness(
                ReadinessError::MissingCapabilities { .. }
            ))
        ));
    }

    #[test]
    fn transient_classification_is_closed() {
        assert!(RuntimeReadinessSnapshotErrorV1::GuildRolesUnavailable.is_retryable());
        assert!(!RuntimeReadinessSnapshotErrorV1::GuildRolesInvalid.is_retryable());
        assert!(!RuntimeReadinessSnapshotErrorV1::BoundRoleMissing.is_retryable());
    }

    #[test]
    fn owned_preflight_is_clone_send_and_sync() {
        fn assert_clone_send_sync<T: Clone + Send + Sync>() {}

        assert_clone_send_sync::<OwnedDiscordRuntimePreflightV1>();
        assert_clone_send_sync::<OwnedDiscordRuntimeOperationsV2>();
    }

    #[tokio::test]
    async fn owned_runtime_operations_reuse_one_private_http_capability() {
        let operations = OwnedDiscordRuntimeOperationsV2::new("unused-token".to_owned());
        let _installer = operations.strict_panel_installer();
    }

    #[test]
    fn owned_preflight_preserves_retryability() {
        assert!(RuntimeDiscordPreflightErrorV1::Snapshot(
            RuntimeReadinessSnapshotErrorV1::BotIdentityUnavailable
        )
        .is_retryable());
        assert!(!RuntimeDiscordPreflightErrorV1::Snapshot(
            RuntimeReadinessSnapshotErrorV1::BotIdentityInvalid
        )
        .is_retryable());
        assert!(
            !RuntimeDiscordPreflightErrorV1::Target(RuntimeTargetReadinessErrorV1::Readiness(
                ReadinessError::HashMismatch
            ))
            .is_retryable()
        );
        assert!(!RuntimeDiscordPreflightErrorV1::TargetGuildMismatch.is_retryable());
    }

    #[tokio::test]
    async fn owned_preflight_rejects_guild_mismatch_before_discord_io() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let provider = OwnedDiscordRuntimePreflightV1::new("unused-token".to_string());
        let artifact = artifact(Vec::new());

        let result = provider
            .preflight(
                GuildId(GUILD.0 + 1),
                &artifact,
                &ResourceBindingMap::default(),
            )
            .await;

        assert!(matches!(
            result,
            Err(RuntimeDiscordPreflightErrorV1::TargetGuildMismatch)
        ));
    }
}
