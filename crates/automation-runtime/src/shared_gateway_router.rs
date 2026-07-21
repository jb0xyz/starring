use automation_instance::{InstanceId, InstanceStore, InstanceStoreError};
use automation_ruleset::{RuleSetKey, RuleSetKeyError};
use automation_runtime_registry::{
    AdmittedInteractionV1, ServingSlotKeyV1, ServingSlotRegistryError, ServingSlotRegistryV1,
};
use discord_model::GuildId;

use crate::custom_id::{decode, CustomIdError, ParsedCustomId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedGatewayRouteErrorV1 {
    InvalidStarringCustomId(CustomIdError),
    GuildMismatch,
    InvalidRuleSetKey(RuleSetKeyError),
    InvalidInstanceId,
    InstanceLookupFailed,
    InstanceNotFound,
    Registry(ServingSlotRegistryError),
}

impl SharedGatewayRouteErrorV1 {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidStarringCustomId(_) => "shared_gateway_invalid_custom_id",
            Self::GuildMismatch => "shared_gateway_guild_mismatch",
            Self::InvalidRuleSetKey(_) => "shared_gateway_invalid_ruleset_key",
            Self::InvalidInstanceId => "shared_gateway_invalid_instance_id",
            Self::InstanceLookupFailed => "shared_gateway_instance_lookup_failed",
            Self::InstanceNotFound => "shared_gateway_instance_not_found",
            Self::Registry(ServingSlotRegistryError::NotServing) => {
                "shared_gateway_route_not_serving"
            }
            Self::Registry(ServingSlotRegistryError::ActiveInteractionCapacityExceeded) => {
                "shared_gateway_slot_capacity_exceeded"
            }
            Self::Registry(_) => "shared_gateway_registry_failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedGatewayRouteHintV1 {
    Static(ServingSlotKeyV1),
    Instance(InstanceId),
}

pub fn parse_shared_gateway_route_v1(
    guild_id: GuildId,
    custom_id: &str,
) -> Result<Option<SharedGatewayRouteHintV1>, SharedGatewayRouteErrorV1> {
    if !custom_id.starts_with("starring:") {
        return Ok(None);
    }
    match decode(custom_id).map_err(SharedGatewayRouteErrorV1::InvalidStarringCustomId)? {
        ParsedCustomId::Component {
            guild_id: encoded_guild_id,
            ruleset_key,
            ..
        } => {
            if encoded_guild_id != guild_id {
                return Err(SharedGatewayRouteErrorV1::GuildMismatch);
            }
            let ruleset_key = RuleSetKey::parse(&ruleset_key)
                .map_err(SharedGatewayRouteErrorV1::InvalidRuleSetKey)?;
            Ok(Some(SharedGatewayRouteHintV1::Static(
                ServingSlotKeyV1::new(guild_id, ruleset_key),
            )))
        }
        ParsedCustomId::InstanceAction { instance_id, .. } => {
            let instance_id = InstanceId::parse(&instance_id)
                .map_err(|_| SharedGatewayRouteErrorV1::InvalidInstanceId)?;
            Ok(Some(SharedGatewayRouteHintV1::Instance(instance_id)))
        }
    }
}

pub async fn admit_shared_gateway_route_v1(
    registry: &ServingSlotRegistryV1,
    instances: &impl InstanceStore,
    guild_id: GuildId,
    custom_id: &str,
) -> Result<Option<AdmittedInteractionV1>, SharedGatewayRouteErrorV1> {
    let Some(hint) = parse_shared_gateway_route_v1(guild_id, custom_id)? else {
        return Ok(None);
    };
    let key = match hint {
        SharedGatewayRouteHintV1::Static(key) => key,
        SharedGatewayRouteHintV1::Instance(instance_id) => {
            let instance = instances
                .get(guild_id, &instance_id)
                .await
                .map_err(map_instance_lookup_error)?
                .ok_or(SharedGatewayRouteErrorV1::InstanceNotFound)?;
            let ruleset_key = RuleSetKey::parse(&instance.ruleset_key)
                .map_err(SharedGatewayRouteErrorV1::InvalidRuleSetKey)?;
            ServingSlotKeyV1::new(guild_id, ruleset_key)
        }
    };
    registry
        .admit(&key)
        .map(Some)
        .map_err(SharedGatewayRouteErrorV1::Registry)
}

fn map_instance_lookup_error(error: InstanceStoreError) -> SharedGatewayRouteErrorV1 {
    match error {
        InstanceStoreError::NotFound => SharedGatewayRouteErrorV1::InstanceNotFound,
        InstanceStoreError::DuplicateInstance | InstanceStoreError::Backend(_) => {
            SharedGatewayRouteErrorV1::InstanceLookupFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_instance::{
        AutomationInstance, InMemoryInstanceStore, InstanceKind, InstanceResources,
        InstanceRuleSetVersion, InstanceStatus,
    };
    use automation_ruleset::{
        content_hash, RuleSetVersion, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
    };
    use automation_runtime_convergence::{
        BindingRevision, FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1,
    };
    use automation_runtime_registry::{
        ExactServingRouteV1, ServingSlotRegistryConfigV1, ServingSlotRegistryV1,
    };
    use automation_state::InteractionRuleSet;
    use discord_model::UserId;
    use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

    use super::*;

    fn serving_registry(guild_id: GuildId, key: &str) -> ServingSlotRegistryV1 {
        let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
            max_slots: NonZeroU32::new(4).unwrap(),
            max_active_interactions_per_slot: NonZeroU32::new(2).unwrap(),
            max_retired_routes_per_slot: NonZeroU32::new(2).unwrap(),
        });
        let ruleset_key = RuleSetKey::parse(key).unwrap();
        let definition = InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: Vec::new(),
        };
        let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
        let bindings = ResourceBindingMap::default();
        let target = RuntimeDeploymentTargetV1 {
            guild_id,
            ruleset_key: ruleset_key.clone(),
            version: RuleSetVersionId::FIRST,
            content_hash,
            binding_revision: BindingRevision::new(1).unwrap(),
            binding_fingerprint: resource_binding_fingerprint_v2(&bindings),
        };
        let identity = RuntimeProcessIdentityV1 {
            target: target.clone(),
            runtime_generation: RuntimeGeneration::new(1).unwrap(),
            process_instance_id: ProcessInstanceId::parse("shared-gateway-process").unwrap(),
        };
        let route = ExactServingRouteV1::new(
            identity.clone(),
            RuleSetVersion {
                guild_id,
                ruleset_key,
                version: target.version,
                schema_version: CURRENT_RULESET_SCHEMA_VERSION,
                definition,
                content_hash,
                created_by: UserId(9),
            },
            bindings,
        )
        .unwrap();
        let token = registry
            .install(
                route.slot_key(),
                route.clone(),
                FencingToken::new(1).unwrap(),
            )
            .unwrap()
            .token;
        registry.activate(&token, &identity).unwrap();
        registry
    }

    async fn register_instance(store: &InMemoryInstanceStore, guild_id: GuildId, key: &str) {
        store
            .register(AutomationInstance {
                id: InstanceId::parse("room_001").unwrap(),
                guild_id,
                ruleset_key: key.to_string(),
                ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
                kind: InstanceKind("study_room".to_string()),
                created_by: UserId(17),
                resources: InstanceResources::default(),
                status: InstanceStatus::Active,
            })
            .await
            .unwrap();
    }

    #[test]
    fn foreign_custom_ids_are_ignored() {
        assert_eq!(
            parse_shared_gateway_route_v1(GuildId(7), "another:button"),
            Ok(None)
        );
    }

    #[test]
    fn static_route_requires_matching_guild_and_valid_key() {
        assert_eq!(
            parse_shared_gateway_route_v1(GuildId(7), "starring:9:study:button:create"),
            Err(SharedGatewayRouteErrorV1::GuildMismatch)
        );
        assert!(matches!(
            parse_shared_gateway_route_v1(GuildId(7), "starring:7:bad key:button:create"),
            Err(SharedGatewayRouteErrorV1::InvalidRuleSetKey(_))
        ));
    }

    #[tokio::test]
    async fn static_route_admission_holds_the_exact_serving_route() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let admitted = admit_shared_gateway_route_v1(
            &registry,
            &instances,
            guild_id,
            "starring:7:study:button:create",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            admitted.route().identity().target.ruleset_key.as_str(),
            "study"
        );
        assert_eq!(admitted.route().identity().target.guild_id, guild_id);
    }

    #[tokio::test]
    async fn instance_route_is_resolved_before_registry_admission() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        register_instance(&instances, guild_id, "study").await;
        let admitted = admit_shared_gateway_route_v1(
            &registry,
            &instances,
            guild_id,
            "starring:i:room_001:join",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            admitted.route().identity().target.ruleset_key.as_str(),
            "study"
        );
    }

    #[tokio::test]
    async fn missing_instance_and_non_serving_routes_fail_closed() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let missing = admit_shared_gateway_route_v1(
            &registry,
            &instances,
            guild_id,
            "starring:i:missing:join",
        )
        .await;
        assert!(matches!(
            missing,
            Err(SharedGatewayRouteErrorV1::InstanceNotFound)
        ));
        assert!(matches!(
            admit_shared_gateway_route_v1(
                &registry,
                &instances,
                guild_id,
                "starring:7:other:button:create",
            )
            .await,
            Err(SharedGatewayRouteErrorV1::Registry(
                ServingSlotRegistryError::NotServing
            ))
        ));
    }
}
