use std::num::NonZeroU64;

use automation_ruleset::RuleSetKey;
use automation_runtime_convergence::{
    FencingToken, RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
};
use discord_model::GuildId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeServingSlotV2 {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
}

impl RuntimeServingSlotV2 {
    pub fn new(guild_id: GuildId, ruleset_key: RuleSetKey) -> Self {
        Self {
            guild_id,
            ruleset_key,
        }
    }

    pub fn from_target(target: &RuntimeDeploymentTargetV1) -> Self {
        Self::new(target.guild_id, target.ruleset_key.clone())
    }

    pub fn matches_target(&self, target: &RuntimeDeploymentTargetV1) -> bool {
        self.guild_id == target.guild_id && self.ruleset_key == target.ruleset_key
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeExactLocalRouteIdentityV2 {
    pub identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
    pub route_incarnation: NonZeroU64,
}

impl RuntimeExactLocalRouteIdentityV2 {
    pub fn slot(&self) -> RuntimeServingSlotV2 {
        RuntimeServingSlotV2::from_target(&self.identity.target)
    }
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{RuntimeExactLocalRouteIdentityV2, RuntimeServingSlotV2};

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(7),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        }
    }

    fn route() -> RuntimeExactLocalRouteIdentityV2 {
        RuntimeExactLocalRouteIdentityV2 {
            identity: RuntimeProcessIdentityV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::new(4).unwrap(),
                process_instance_id: ProcessInstanceId::parse("process").unwrap(),
            },
            controller_fencing_token: FencingToken::new(5).unwrap(),
            route_incarnation: std::num::NonZeroU64::new(6).unwrap(),
        }
    }

    #[test]
    fn serving_slot_matches_only_its_exact_target_slot() {
        let target = target();
        let slot = RuntimeServingSlotV2::from_target(&target);

        assert!(slot.matches_target(&target));

        let mut other = target;
        other.ruleset_key = RuleSetKey::parse("other").unwrap();
        assert!(!slot.matches_target(&other));
    }

    #[test]
    fn local_route_identity_derives_the_exact_slot() {
        let route = route();

        assert_eq!(
            route.slot(),
            RuntimeServingSlotV2::new(GuildId(7), RuleSetKey::parse("studyroom").unwrap())
        );
    }

    #[test]
    fn route_identity_wire_shape_is_canonical_and_strict() {
        let expected_route = route();
        let encoded = serde_json::to_string(&expected_route).unwrap();

        assert_eq!(
            encoded,
            concat!(
                "{\"identity\":{\"target\":{\"guild_id\":\"7\",",
                "\"ruleset_key\":\"studyroom\",\"version\":1,",
                "\"content_hash\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",\"binding_revision\":3,",
                "\"binding_fingerprint\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},\"runtime_generation\":4,",
                "\"process_instance_id\":\"process\"},",
                "\"controller_fencing_token\":5,\"route_incarnation\":6}"
            )
        );
        assert_eq!(
            serde_json::from_str::<RuntimeExactLocalRouteIdentityV2>(&encoded).unwrap(),
            expected_route
        );

        let mut unknown = serde_json::to_value(route()).unwrap();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), serde_json::json!(true));
        assert!(serde_json::from_value::<RuntimeExactLocalRouteIdentityV2>(unknown).is_err());

        let mut zero = serde_json::to_value(route()).unwrap();
        zero.as_object_mut()
            .unwrap()
            .insert("route_incarnation".to_string(), serde_json::json!(0));
        assert!(serde_json::from_value::<RuntimeExactLocalRouteIdentityV2>(zero).is_err());
    }
}
