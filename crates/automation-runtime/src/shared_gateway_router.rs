use std::time::Duration;

use automation_instance::{InstanceId, InstanceRouteReaderV1, InstanceStatus, InstanceStoreError};
use automation_ruleset::{RuleSetKey, RuleSetKeyError};
use automation_runtime_registry::{
    AdmittedInteractionV1, ServingSlotKeyV1, ServingSlotRegistryError, ServingSlotRegistryV1,
};
use discord_model::GuildId;

use crate::custom_id::{decode, CustomIdError, ParsedCustomId};

const MAX_INSTANCE_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedGatewayRouteConfigV1 {
    instance_lookup_timeout: Duration,
}

impl SharedGatewayRouteConfigV1 {
    pub fn new(
        instance_lookup_timeout: Duration,
    ) -> Result<Self, SharedGatewayRouteConfigurationErrorV1> {
        if instance_lookup_timeout.is_zero()
            || instance_lookup_timeout > MAX_INSTANCE_LOOKUP_TIMEOUT
        {
            return Err(SharedGatewayRouteConfigurationErrorV1::InvalidInstanceLookupTimeout);
        }
        Ok(Self {
            instance_lookup_timeout,
        })
    }

    pub fn instance_lookup_timeout(self) -> Duration {
        self.instance_lookup_timeout
    }
}

impl Default for SharedGatewayRouteConfigV1 {
    fn default() -> Self {
        Self {
            instance_lookup_timeout: Duration::from_millis(500),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SharedGatewayRouteConfigurationErrorV1 {
    #[error("shared gateway instance lookup timeout is invalid")]
    InvalidInstanceLookupTimeout,
}

impl SharedGatewayRouteConfigurationErrorV1 {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidInstanceLookupTimeout => "shared_gateway_invalid_instance_lookup_timeout",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedGatewayRouteErrorV1 {
    InvalidStarringCustomId(CustomIdError),
    GuildMismatch,
    InvalidRuleSetKey(RuleSetKeyError),
    InvalidInstanceId,
    InstanceLookupFailed,
    InstanceLookupTimedOut,
    InstanceNotFound,
    InstanceGuildMismatch,
    InstanceInactive(InstanceStatus),
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
            Self::InstanceLookupTimedOut => "shared_gateway_instance_lookup_timed_out",
            Self::InstanceNotFound => "shared_gateway_instance_not_found",
            Self::InstanceGuildMismatch => "shared_gateway_instance_guild_mismatch",
            Self::InstanceInactive(_) => "shared_gateway_instance_inactive",
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
    instances: &impl InstanceRouteReaderV1,
    guild_id: GuildId,
    custom_id: &str,
) -> Result<Option<AdmittedInteractionV1>, SharedGatewayRouteErrorV1> {
    admit_shared_gateway_route_with_config_v1(
        registry,
        instances,
        guild_id,
        custom_id,
        SharedGatewayRouteConfigV1::default(),
    )
    .await
}

pub async fn admit_shared_gateway_route_with_config_v1(
    registry: &ServingSlotRegistryV1,
    instances: &impl InstanceRouteReaderV1,
    guild_id: GuildId,
    custom_id: &str,
    config: SharedGatewayRouteConfigV1,
) -> Result<Option<AdmittedInteractionV1>, SharedGatewayRouteErrorV1> {
    let Some(hint) = parse_shared_gateway_route_v1(guild_id, custom_id)? else {
        return Ok(None);
    };
    let key = match hint {
        SharedGatewayRouteHintV1::Static(key) => key,
        SharedGatewayRouteHintV1::Instance(instance_id) => {
            let instance = tokio::time::timeout(
                config.instance_lookup_timeout(),
                instances.read_instance_route_v1(guild_id, &instance_id),
            )
            .await
            .map_err(|_| SharedGatewayRouteErrorV1::InstanceLookupTimedOut)?
            .map_err(map_instance_lookup_error)?
            .ok_or(SharedGatewayRouteErrorV1::InstanceNotFound)?;
            if instance.guild_id != guild_id {
                return Err(SharedGatewayRouteErrorV1::InstanceGuildMismatch);
            }
            if instance.status != InstanceStatus::Active {
                return Err(SharedGatewayRouteErrorV1::InstanceInactive(instance.status));
            }
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
        InstanceStoreError::TimedOut => SharedGatewayRouteErrorV1::InstanceLookupTimedOut,
        InstanceStoreError::DuplicateInstance | InstanceStoreError::Backend(_) => {
            SharedGatewayRouteErrorV1::InstanceLookupFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::num::NonZeroU32;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use automation_instance::{
        AutomationInstance, InMemoryInstanceStore, InstanceKind, InstanceResources,
        InstanceRuleSetVersion, InstanceStatus, InstanceStore,
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

    enum FakeLookupV1 {
        Pending,
        Found(AutomationInstance),
        Failed(InstanceStoreError),
    }

    struct FakeInstanceStoreV1 {
        lookup: FakeLookupV1,
        get_calls: AtomicUsize,
    }

    impl FakeInstanceStoreV1 {
        fn pending() -> Self {
            Self {
                lookup: FakeLookupV1::Pending,
                get_calls: AtomicUsize::new(0),
            }
        }

        fn found(instance: AutomationInstance) -> Self {
            Self {
                lookup: FakeLookupV1::Found(instance),
                get_calls: AtomicUsize::new(0),
            }
        }

        fn failed(error: InstanceStoreError) -> Self {
            Self {
                lookup: FakeLookupV1::Failed(error),
                get_calls: AtomicUsize::new(0),
            }
        }

        fn get_calls(&self) -> usize {
            self.get_calls.load(Ordering::SeqCst)
        }

        fn unsupported<T>() -> Result<T, InstanceStoreError> {
            Err(InstanceStoreError::Backend(
                "fake instance store operation is unsupported".to_string(),
            ))
        }
    }

    impl InstanceStore for FakeInstanceStoreV1 {
        async fn register(&self, _instance: AutomationInstance) -> Result<(), InstanceStoreError> {
            Self::unsupported()
        }

        async fn get(
            &self,
            _guild_id: GuildId,
            _instance_id: &InstanceId,
        ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
            self.get_calls.fetch_add(1, Ordering::SeqCst);
            match &self.lookup {
                FakeLookupV1::Pending => pending().await,
                FakeLookupV1::Found(instance) => Ok(Some(instance.clone())),
                FakeLookupV1::Failed(error) => Err(error.clone()),
            }
        }

        async fn list_by_guild(
            &self,
            _guild_id: GuildId,
        ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
            Self::unsupported()
        }

        async fn update_status(
            &self,
            _guild_id: GuildId,
            _instance_id: &InstanceId,
            _status: InstanceStatus,
        ) -> Result<(), InstanceStoreError> {
            Self::unsupported()
        }

        async fn transition_to_deleting(
            &self,
            _guild_id: GuildId,
            _instance_id: &InstanceId,
        ) -> Result<(), InstanceStoreError> {
            Self::unsupported()
        }

        async fn mark_deleted(
            &self,
            _guild_id: GuildId,
            _instance_id: &InstanceId,
        ) -> Result<(), InstanceStoreError> {
            Self::unsupported()
        }

        async fn list_deleting(
            &self,
            _guild_id: GuildId,
        ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
            Self::unsupported()
        }
    }

    impl InstanceRouteReaderV1 for FakeInstanceStoreV1 {
        async fn read_instance_route_v1(
            &self,
            guild_id: GuildId,
            instance_id: &InstanceId,
        ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
            InstanceStore::get(self, guild_id, instance_id).await
        }
    }

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

    fn instance(guild_id: GuildId, key: &str, status: InstanceStatus) -> AutomationInstance {
        AutomationInstance {
            id: InstanceId::parse("room_001").unwrap(),
            guild_id,
            ruleset_key: key.to_string(),
            ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
            kind: InstanceKind("study_room".to_string()),
            created_by: UserId(17),
            resources: InstanceResources::default(),
            status,
        }
    }

    async fn register_instance(store: &InMemoryInstanceStore, guild_id: GuildId, key: &str) {
        store
            .register(instance(guild_id, key, InstanceStatus::Active))
            .await
            .unwrap();
    }

    #[test]
    fn instance_lookup_timeout_configuration_is_nonzero_and_bounded() {
        assert_eq!(
            SharedGatewayRouteConfigV1::default().instance_lookup_timeout(),
            Duration::from_millis(500)
        );
        assert_eq!(
            SharedGatewayRouteConfigV1::new(Duration::ZERO),
            Err(SharedGatewayRouteConfigurationErrorV1::InvalidInstanceLookupTimeout)
        );
        assert!(SharedGatewayRouteConfigV1::new(Duration::from_secs(2)).is_ok());
        let error =
            SharedGatewayRouteConfigV1::new(Duration::from_secs(2) + Duration::from_nanos(1))
                .unwrap_err();
        assert_eq!(
            error,
            SharedGatewayRouteConfigurationErrorV1::InvalidInstanceLookupTimeout
        );
        assert_eq!(
            error.code(),
            "shared_gateway_invalid_instance_lookup_timeout"
        );
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
    async fn pending_instance_lookup_times_out_and_static_route_skips_lookup() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        let instances = FakeInstanceStoreV1::pending();
        let config = SharedGatewayRouteConfigV1::new(Duration::from_millis(1)).unwrap();
        let error = admit_shared_gateway_route_with_config_v1(
            &registry,
            &instances,
            guild_id,
            "starring:i:room_001:join",
            config,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error, SharedGatewayRouteErrorV1::InstanceLookupTimedOut);
        assert_eq!(error.code(), "shared_gateway_instance_lookup_timed_out");
        assert_eq!(instances.get_calls(), 1);
        let admitted = admit_shared_gateway_route_with_config_v1(
            &registry,
            &instances,
            guild_id,
            "starring:7:study:button:create",
            config,
        )
        .await
        .unwrap();
        assert!(admitted.is_some());
        assert_eq!(instances.get_calls(), 1);
    }

    #[tokio::test]
    async fn typed_store_timeout_preserves_gateway_timeout_classification() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        let instances = FakeInstanceStoreV1::failed(InstanceStoreError::TimedOut);
        let error = admit_shared_gateway_route_v1(
            &registry,
            &instances,
            guild_id,
            "starring:i:room_001:join",
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error, SharedGatewayRouteErrorV1::InstanceLookupTimedOut);
        assert_eq!(error.code(), "shared_gateway_instance_lookup_timed_out");
        assert_eq!(instances.get_calls(), 1);
    }

    #[tokio::test]
    async fn corrupt_instance_guild_is_rejected_before_admission() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        let instances =
            FakeInstanceStoreV1::found(instance(GuildId(8), "study", InstanceStatus::Active));
        let error = admit_shared_gateway_route_v1(
            &registry,
            &instances,
            guild_id,
            "starring:i:room_001:join",
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error, SharedGatewayRouteErrorV1::InstanceGuildMismatch);
        assert_eq!(error.code(), "shared_gateway_instance_guild_mismatch");
    }

    #[tokio::test]
    async fn inactive_instances_are_rejected_before_admission() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        for status in [
            InstanceStatus::Deleting,
            InstanceStatus::Disabled,
            InstanceStatus::Deleted,
        ] {
            let instances = FakeInstanceStoreV1::found(instance(guild_id, "study", status));
            let error = admit_shared_gateway_route_v1(
                &registry,
                &instances,
                guild_id,
                "starring:i:room_001:join",
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error, SharedGatewayRouteErrorV1::InstanceInactive(status));
            assert_eq!(error.code(), "shared_gateway_instance_inactive");
        }
    }

    #[tokio::test]
    async fn lookup_backend_detail_is_reduced_to_a_stable_code() {
        let guild_id = GuildId(7);
        let registry = serving_registry(guild_id, "study");
        let instances = FakeInstanceStoreV1::failed(InstanceStoreError::Backend(
            "postgres://private-user:private-password@private-host".to_string(),
        ));
        let error = admit_shared_gateway_route_v1(
            &registry,
            &instances,
            guild_id,
            "starring:i:room_001:join",
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error, SharedGatewayRouteErrorV1::InstanceLookupFailed);
        assert_eq!(error.code(), "shared_gateway_instance_lookup_failed");
        assert!(!error.code().contains("private"));
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
