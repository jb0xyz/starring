use automation_instance::{
    AutomationInstance, InstanceId, InstanceRouteReaderV1, InstanceStatus, InstanceStoreError,
};
use automation_ruleset::{
    RuleSetKey, RuleSetStore, RuleSetStoreError, RuleSetVersion, RuleSetVersionId,
};
use discord_model::GuildId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPinnedInstanceV1 {
    pub instance: AutomationInstance,
    pub artifact: RuleSetVersion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinnedInstanceResolverErrorV1 {
    InstanceLookup(InstanceStoreError),
    InstanceNotFound,
    InstanceInactive(InstanceStatus),
    PinnedKeyInvalid,
    VersionLookup(RuleSetStoreError),
    PinnedVersionMissing,
}

#[allow(async_fn_in_trait)]
pub trait PinnedInstanceResolverV1 {
    async fn resolve_pinned_instance_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<ResolvedPinnedInstanceV1, PinnedInstanceResolverErrorV1>;
}

pub struct LegacyStoreBackedPinnedInstanceResolverV1<'a, I, R> {
    instances: &'a I,
    rulesets: &'a R,
}

impl<'a, I, R> LegacyStoreBackedPinnedInstanceResolverV1<'a, I, R> {
    pub fn new(instances: &'a I, rulesets: &'a R) -> Self {
        Self {
            instances,
            rulesets,
        }
    }
}

impl<I, R> PinnedInstanceResolverV1 for LegacyStoreBackedPinnedInstanceResolverV1<'_, I, R>
where
    I: InstanceRouteReaderV1,
    R: RuleSetStore,
{
    async fn resolve_pinned_instance_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<ResolvedPinnedInstanceV1, PinnedInstanceResolverErrorV1> {
        let instance = self
            .instances
            .read_instance_route_v1(guild_id, instance_id)
            .await
            .map_err(PinnedInstanceResolverErrorV1::InstanceLookup)?
            .ok_or(PinnedInstanceResolverErrorV1::InstanceNotFound)?;
        if instance.status != InstanceStatus::Active {
            return Err(PinnedInstanceResolverErrorV1::InstanceInactive(
                instance.status,
            ));
        }
        let key = RuleSetKey::parse(&instance.ruleset_key)
            .map_err(|_| PinnedInstanceResolverErrorV1::PinnedKeyInvalid)?;
        let version = RuleSetVersionId::new(instance.ruleset_version.get())
            .map_err(|_| PinnedInstanceResolverErrorV1::PinnedKeyInvalid)?;
        let artifact = self
            .rulesets
            .get_version(guild_id, &key, version)
            .await
            .map_err(PinnedInstanceResolverErrorV1::VersionLookup)?
            .ok_or(PinnedInstanceResolverErrorV1::PinnedVersionMissing)?;
        if artifact.guild_id != guild_id
            || artifact.ruleset_key != key
            || artifact.version != version
        {
            return Err(PinnedInstanceResolverErrorV1::PinnedVersionMissing);
        }
        Ok(ResolvedPinnedInstanceV1 { instance, artifact })
    }
}
