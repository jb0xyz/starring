use std::num::NonZeroUsize;
use std::sync::Arc;

use automation_instance::InstanceStore;
use automation_runtime_registry::{
    AdmittedInteractionV1, ExactServingRouteV1, ServingSlotRegistryV1, SlotMutationTokenV1,
};
use discord_model::GuildId;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::shared_gateway_control::{
    GatewayConnectionEpochV3, GatewayConnectionObserverV3, GatewayReadyLeaseV3,
    SharedGatewayControlV3,
};
use crate::shared_gateway_router::{admit_shared_gateway_route_v1, SharedGatewayRouteErrorV1};

pub const MAX_SHARED_GATEWAY_GLOBAL_ADMISSIONS_V3: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedGatewayAdmissionConfigV3 {
    global_capacity: NonZeroUsize,
}

impl SharedGatewayAdmissionConfigV3 {
    pub fn new(
        global_capacity: NonZeroUsize,
    ) -> Result<Self, SharedGatewayAdmissionConfigurationErrorV3> {
        if global_capacity.get() > MAX_SHARED_GATEWAY_GLOBAL_ADMISSIONS_V3 {
            return Err(SharedGatewayAdmissionConfigurationErrorV3::GlobalCapacity);
        }
        Ok(Self { global_capacity })
    }

    pub fn global_capacity(self) -> NonZeroUsize {
        self.global_capacity
    }
}

impl Default for SharedGatewayAdmissionConfigV3 {
    fn default() -> Self {
        Self {
            global_capacity: NonZeroUsize::new(256)
                .expect("default global admission capacity is non-zero"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SharedGatewayAdmissionConfigurationErrorV3 {
    #[error("shared gateway global admission capacity is invalid")]
    GlobalCapacity,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SharedGatewayAdmissionErrorV3 {
    #[error("shared gateway is not ready for admission")]
    NotReady,
    #[error("shared gateway global admission capacity is exhausted")]
    Overloaded,
    #[error("shared gateway route admission failed")]
    Router(SharedGatewayRouteErrorV1),
}

impl SharedGatewayAdmissionErrorV3 {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotReady => "shared_gateway_admission_not_ready",
            Self::Overloaded => "shared_gateway_admission_overloaded",
            Self::Router(_) => "shared_gateway_admission_route_failed",
        }
    }
}

#[derive(Clone)]
pub struct SharedGatewayAdmissionBudgetV3 {
    capacity: NonZeroUsize,
    permits: Arc<Semaphore>,
}

impl SharedGatewayAdmissionBudgetV3 {
    pub fn new(config: SharedGatewayAdmissionConfigV3) -> Self {
        Self {
            capacity: config.global_capacity,
            permits: Arc::new(Semaphore::new(config.global_capacity.get())),
        }
    }

    pub fn capacity(&self) -> NonZeroUsize {
        self.capacity
    }

    pub async fn admit(
        &self,
        control: &SharedGatewayControlV3,
        ready_lease: &GatewayReadyLeaseV3,
        registry: &ServingSlotRegistryV1,
        instances: &impl InstanceStore,
        guild_id: GuildId,
        custom_id: &str,
    ) -> Result<Option<SharedGatewayAdmittedInteractionV3>, SharedGatewayAdmissionErrorV3> {
        self.admit_with_observer(
            &control.connection_observer(),
            ready_lease,
            registry,
            instances,
            guild_id,
            custom_id,
        )
        .await
    }

    pub async fn admit_with_observer(
        &self,
        observer: &GatewayConnectionObserverV3,
        ready_lease: &GatewayReadyLeaseV3,
        registry: &ServingSlotRegistryV1,
        instances: &impl InstanceStore,
        guild_id: GuildId,
        custom_id: &str,
    ) -> Result<Option<SharedGatewayAdmittedInteractionV3>, SharedGatewayAdmissionErrorV3> {
        self.try_reserve(observer, ready_lease)?
            .admit(registry, instances, guild_id, custom_id)
            .await
    }

    pub fn try_reserve(
        &self,
        observer: &GatewayConnectionObserverV3,
        ready_lease: &GatewayReadyLeaseV3,
    ) -> Result<SharedGatewayAdmissionReservationV3, SharedGatewayAdmissionErrorV3> {
        if !observer.ready_lease_is_current(ready_lease) {
            return Err(SharedGatewayAdmissionErrorV3::NotReady);
        }
        let global_permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| SharedGatewayAdmissionErrorV3::Overloaded)?;
        if !observer.ready_lease_is_current(ready_lease) {
            return Err(SharedGatewayAdmissionErrorV3::NotReady);
        }
        Ok(SharedGatewayAdmissionReservationV3 {
            observer: observer.clone(),
            ready_lease: *ready_lease,
            global_permit,
        })
    }
}

pub struct SharedGatewayAdmissionReservationV3 {
    observer: GatewayConnectionObserverV3,
    ready_lease: GatewayReadyLeaseV3,
    global_permit: OwnedSemaphorePermit,
}

impl SharedGatewayAdmissionReservationV3 {
    pub fn epoch(&self) -> GatewayConnectionEpochV3 {
        self.ready_lease.epoch()
    }

    pub async fn admit(
        self,
        registry: &ServingSlotRegistryV1,
        instances: &impl InstanceStore,
        guild_id: GuildId,
        custom_id: &str,
    ) -> Result<Option<SharedGatewayAdmittedInteractionV3>, SharedGatewayAdmissionErrorV3> {
        if !self.observer.ready_lease_is_current(&self.ready_lease) {
            return Err(SharedGatewayAdmissionErrorV3::NotReady);
        }
        let Some(admitted) =
            admit_shared_gateway_route_v1(registry, instances, guild_id, custom_id)
                .await
                .map_err(SharedGatewayAdmissionErrorV3::Router)?
        else {
            return Ok(None);
        };
        if !self.observer.ready_lease_is_current(&self.ready_lease) {
            return Err(SharedGatewayAdmissionErrorV3::NotReady);
        }
        Ok(Some(SharedGatewayAdmittedInteractionV3 {
            admitted,
            ready_lease: self.ready_lease,
            _global_permit: self.global_permit,
        }))
    }
}

pub struct SharedGatewayAdmittedInteractionV3 {
    admitted: AdmittedInteractionV1,
    ready_lease: GatewayReadyLeaseV3,
    _global_permit: OwnedSemaphorePermit,
}

impl SharedGatewayAdmittedInteractionV3 {
    pub fn route(&self) -> &ExactServingRouteV1 {
        self.admitted.route()
    }

    pub fn token(&self) -> &SlotMutationTokenV1 {
        self.admitted.token()
    }

    pub fn epoch(&self) -> GatewayConnectionEpochV3 {
        self.ready_lease.epoch()
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroUsize};
    use std::time::Duration;

    use automation_instance::{
        AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
        InstanceRuleSetVersion, InstanceStatus, InstanceStore, InstanceStoreError,
    };
    use automation_ruleset::{
        content_hash, RuleSetKey, RuleSetVersion, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
    };
    use automation_runtime_convergence::{
        BindingRevision, FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1,
    };
    use automation_runtime_registry::{
        ExactServingRouteV1, ServingSlotKeyV1, ServingSlotRegistryConfigV1,
        ServingSlotRegistryError, ServingSlotRegistryV1,
    };
    use automation_state::InteractionRuleSet;
    use discord_model::UserId;
    use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

    use crate::shared_gateway_control::{
        shared_gateway_control_channel_v3, GatewayControlConfigV3, GatewayDisconnectKindV3,
        GatewayReadyKindV3, SharedGatewayRuntimeControlV3,
    };

    use super::*;

    struct ControlledInstanceStore {
        inner: InMemoryInstanceStore,
        lookup_started: tokio::sync::Notify,
        lookup_release: tokio::sync::Notify,
    }

    impl ControlledInstanceStore {
        fn new() -> Self {
            Self {
                inner: InMemoryInstanceStore::new(),
                lookup_started: tokio::sync::Notify::new(),
                lookup_release: tokio::sync::Notify::new(),
            }
        }
    }

    impl InstanceStore for ControlledInstanceStore {
        async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
            self.inner.register(instance).await
        }

        async fn get(
            &self,
            guild_id: GuildId,
            instance_id: &InstanceId,
        ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
            self.lookup_started.notify_one();
            self.lookup_release.notified().await;
            self.inner.get(guild_id, instance_id).await
        }

        async fn list_by_guild(
            &self,
            guild_id: GuildId,
        ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
            self.inner.list_by_guild(guild_id).await
        }

        async fn update_status(
            &self,
            guild_id: GuildId,
            instance_id: &InstanceId,
            status: InstanceStatus,
        ) -> Result<(), InstanceStoreError> {
            self.inner
                .update_status(guild_id, instance_id, status)
                .await
        }

        async fn transition_to_deleting(
            &self,
            guild_id: GuildId,
            instance_id: &InstanceId,
        ) -> Result<(), InstanceStoreError> {
            self.inner
                .transition_to_deleting(guild_id, instance_id)
                .await
        }

        async fn mark_deleted(
            &self,
            guild_id: GuildId,
            instance_id: &InstanceId,
        ) -> Result<(), InstanceStoreError> {
            self.inner.mark_deleted(guild_id, instance_id).await
        }

        async fn list_deleting(
            &self,
            guild_id: GuildId,
        ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
            self.inner.list_deleting(guild_id).await
        }
    }

    fn registry(per_slot_capacity: u32) -> ServingSlotRegistryV1 {
        ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
            max_slots: NonZeroU32::new(4).unwrap(),
            max_active_interactions_per_slot: NonZeroU32::new(per_slot_capacity).unwrap(),
            max_retired_routes_per_slot: NonZeroU32::new(2).unwrap(),
        })
    }

    fn install_route(registry: &ServingSlotRegistryV1, guild_id: GuildId, key: &str) {
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
            binding_revision: BindingRevision::FIRST,
            binding_fingerprint: resource_binding_fingerprint_v2(&bindings),
        };
        let identity = RuntimeProcessIdentityV1 {
            target: target.clone(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: ProcessInstanceId::parse(format!("process-{key}")).unwrap(),
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
            .install(route.slot_key(), route, FencingToken::new(1).unwrap())
            .unwrap()
            .token;
        registry.activate(&token, &identity).unwrap();
    }

    async fn register_instance(store: &impl InstanceStore, guild_id: GuildId, key: &str) {
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

    fn connected_control() -> (
        SharedGatewayControlV3,
        SharedGatewayRuntimeControlV3,
        GatewayReadyLeaseV3,
    ) {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let lease = control.issue_ready_lease(epoch).unwrap();
        (control, runtime, lease)
    }

    fn budget(capacity: usize) -> SharedGatewayAdmissionBudgetV3 {
        SharedGatewayAdmissionBudgetV3::new(
            SharedGatewayAdmissionConfigV3::new(NonZeroUsize::new(capacity).unwrap()).unwrap(),
        )
    }

    fn admission_error(
        result: Result<Option<SharedGatewayAdmittedInteractionV3>, SharedGatewayAdmissionErrorV3>,
    ) -> SharedGatewayAdmissionErrorV3 {
        match result {
            Ok(_) => panic!("shared gateway admission must fail"),
            Err(error) => error,
        }
    }

    #[test]
    fn configuration_bounds_global_capacity() {
        assert_eq!(
            SharedGatewayAdmissionConfigV3::default()
                .global_capacity()
                .get(),
            256
        );
        assert_eq!(
            SharedGatewayAdmissionConfigV3::new(
                NonZeroUsize::new(MAX_SHARED_GATEWAY_GLOBAL_ADMISSIONS_V3).unwrap()
            )
            .unwrap()
            .global_capacity()
            .get(),
            MAX_SHARED_GATEWAY_GLOBAL_ADMISSIONS_V3
        );
        assert_eq!(
            SharedGatewayAdmissionConfigV3::new(
                NonZeroUsize::new(MAX_SHARED_GATEWAY_GLOBAL_ADMISSIONS_V3 + 1).unwrap()
            ),
            Err(SharedGatewayAdmissionConfigurationErrorV3::GlobalCapacity)
        );
    }

    #[test]
    fn reservation_holds_capacity_before_async_route_work_starts() {
        let (control, _runtime, lease) = connected_control();
        let observer = control.connection_observer();
        let budget = budget(1);
        let reservation = budget.try_reserve(&observer, &lease).unwrap();
        assert_eq!(reservation.epoch(), lease.epoch());
        assert!(matches!(
            budget.try_reserve(&observer, &lease),
            Err(SharedGatewayAdmissionErrorV3::Overloaded)
        ));
        drop(reservation);
        assert!(budget.try_reserve(&observer, &lease).is_ok());
    }

    #[tokio::test]
    async fn stale_reservation_fails_closed_and_returns_capacity() {
        let guild_id = GuildId(7);
        let registry = registry(1);
        install_route(&registry, guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let (control, mut runtime, lease) = connected_control();
        let observer = control.connection_observer();
        let budget = budget(1);
        let reservation = budget.try_reserve(&observer, &lease).unwrap();
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
        let error = admission_error(
            reservation
                .admit(
                    &registry,
                    &instances,
                    guild_id,
                    "starring:7:study:button:create",
                )
                .await,
        );
        assert_eq!(error, SharedGatewayAdmissionErrorV3::NotReady);
        let next_epoch = runtime.mark_connected(GatewayReadyKindV3::Resumed).unwrap();
        let next_lease = observer.issue_ready_lease(next_epoch).unwrap();
        assert!(budget.try_reserve(&observer, &next_lease).is_ok());
    }

    #[tokio::test]
    async fn pre_pause_reservation_cannot_cross_a_pause_resume_barrier() {
        let guild_id = GuildId(7);
        let registry = registry(1);
        install_route(&registry, guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let (control, mut runtime, lease) = connected_control();
        let observer = control.connection_observer();
        let budget = budget(1);
        let reservation = budget.try_reserve(&observer, &lease).unwrap();
        let (paused, _) = tokio::join!(control.pause_admission(), runtime.process_next_command());
        assert!(paused.is_ok());
        let (resumed, _) = tokio::join!(
            control.resume_admission(lease.epoch()),
            runtime.process_next_command()
        );
        assert!(resumed.is_ok());
        assert_eq!(
            admission_error(
                reservation
                    .admit(
                        &registry,
                        &instances,
                        guild_id,
                        "starring:7:study:button:create",
                    )
                    .await,
            ),
            SharedGatewayAdmissionErrorV3::NotReady
        );
        let current = observer.issue_ready_lease(lease.epoch()).unwrap();
        assert!(budget.try_reserve(&observer, &current).is_ok());
    }

    #[tokio::test]
    async fn global_capacity_never_queues_and_spans_slots() {
        let guild_id = GuildId(7);
        let registry = registry(2);
        install_route(&registry, guild_id, "study");
        install_route(&registry, guild_id, "other");
        let instances = InMemoryInstanceStore::new();
        register_instance(&instances, guild_id, "other").await;
        let (control, _runtime, lease) = connected_control();
        let budget = budget(1);
        let first = budget
            .admit(
                &control,
                &lease,
                &registry,
                &instances,
                guild_id,
                "starring:7:study:button:create",
            )
            .await
            .unwrap()
            .unwrap();
        let overloaded = admission_error(
            tokio::time::timeout(
                Duration::from_millis(50),
                budget.admit(
                    &control,
                    &lease,
                    &registry,
                    &instances,
                    guild_id,
                    "starring:i:room_001:join",
                ),
            )
            .await
            .expect("global overload must never wait for capacity"),
        );
        assert_eq!(overloaded, SharedGatewayAdmissionErrorV3::Overloaded);
        assert_eq!(overloaded.code(), "shared_gateway_admission_overloaded");
        drop(first);
        let second = budget
            .admit(
                &control,
                &lease,
                &registry,
                &instances,
                guild_id,
                "starring:i:room_001:join",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            second.route().identity().target.ruleset_key.as_str(),
            "other"
        );
    }

    #[tokio::test]
    async fn slot_capacity_remains_a_redacted_router_failure() {
        let guild_id = GuildId(7);
        let registry = registry(1);
        install_route(&registry, guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let (control, _runtime, lease) = connected_control();
        let budget = budget(2);
        let first = budget
            .admit(
                &control,
                &lease,
                &registry,
                &instances,
                guild_id,
                "starring:7:study:button:create",
            )
            .await
            .unwrap()
            .unwrap();
        let error = admission_error(
            budget
                .admit(
                    &control,
                    &lease,
                    &registry,
                    &instances,
                    guild_id,
                    "starring:7:study:button:create",
                )
                .await,
        );
        assert!(matches!(
            error,
            SharedGatewayAdmissionErrorV3::Router(SharedGatewayRouteErrorV1::Registry(
                ServingSlotRegistryError::ActiveInteractionCapacityExceeded
            ))
        ));
        assert_eq!(error.code(), "shared_gateway_admission_route_failed");
        drop(first);
    }

    #[tokio::test]
    async fn foreign_and_failed_routes_return_the_global_permit() {
        let guild_id = GuildId(7);
        let registry = registry(1);
        install_route(&registry, guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let (control, _runtime, lease) = connected_control();
        let observer = control.connection_observer();
        let budget = budget(1);
        assert!(budget
            .admit_with_observer(
                &observer,
                &lease,
                &registry,
                &instances,
                guild_id,
                "another:button",
            )
            .await
            .unwrap()
            .is_none());
        let error = admission_error(
            budget
                .admit(
                    &control,
                    &lease,
                    &registry,
                    &instances,
                    guild_id,
                    "starring:7:missing:button:create",
                )
                .await,
        );
        assert_eq!(error.code(), "shared_gateway_admission_route_failed");
        let admitted = budget
            .admit(
                &control,
                &lease,
                &registry,
                &instances,
                guild_id,
                "starring:7:study:button:create",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(admitted.epoch(), lease.epoch());
        assert_eq!(admitted.token().key().guild_id(), guild_id);
    }

    #[tokio::test]
    async fn invalidated_ready_lease_fails_before_global_admission() {
        let guild_id = GuildId(7);
        let registry = registry(1);
        install_route(&registry, guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let (control, mut runtime, lease) = connected_control();
        let budget = budget(1);
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
        let error = admission_error(
            budget
                .admit(
                    &control,
                    &lease,
                    &registry,
                    &instances,
                    guild_id,
                    "starring:7:study:button:create",
                )
                .await,
        );
        assert_eq!(error, SharedGatewayAdmissionErrorV3::NotReady);
        assert_eq!(error.code(), "shared_gateway_admission_not_ready");
        let next_epoch = runtime.mark_connected(GatewayReadyKindV3::Resumed).unwrap();
        let next_lease = control.issue_ready_lease(next_epoch).unwrap();
        assert!(budget
            .admit(
                &control,
                &next_lease,
                &registry,
                &instances,
                guild_id,
                "starring:7:study:button:create",
            )
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn epoch_change_during_instance_lookup_cannot_return_stale_admission() {
        let guild_id = GuildId(7);
        let registry = registry(1);
        install_route(&registry, guild_id, "study");
        let instances = ControlledInstanceStore::new();
        register_instance(&instances, guild_id, "study").await;
        let (control, mut runtime, lease) = connected_control();
        let budget = budget(1);
        let slot = ServingSlotKeyV1::new(guild_id, RuleSetKey::parse("study").unwrap());
        let token = registry
            .serving_snapshot(&slot)
            .unwrap()
            .unwrap()
            .token()
            .clone();
        let admission = budget.admit(
            &control,
            &lease,
            &registry,
            &instances,
            guild_id,
            "starring:i:room_001:join",
        );
        let invalidate = async {
            instances.lookup_started.notified().await;
            runtime
                .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
                .unwrap();
            let next_epoch = runtime.mark_connected(GatewayReadyKindV3::Resumed).unwrap();
            instances.lookup_release.notify_one();
            next_epoch
        };
        let (result, next_epoch) = tokio::join!(admission, invalidate);
        assert_eq!(
            admission_error(result),
            SharedGatewayAdmissionErrorV3::NotReady
        );
        assert_eq!(
            registry.route_status(&token).unwrap().active_interactions,
            0
        );
        let next_lease = control.issue_ready_lease(next_epoch).unwrap();
        assert!(budget
            .admit(
                &control,
                &next_lease,
                &registry,
                &instances,
                guild_id,
                "starring:7:study:button:create",
            )
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn wrapper_drop_releases_global_and_slot_guards_together() {
        let guild_id = GuildId(7);
        let registry = registry(1);
        install_route(&registry, guild_id, "study");
        let instances = InMemoryInstanceStore::new();
        let (control, _runtime, lease) = connected_control();
        let budget = budget(1);
        let first = budget
            .admit(
                &control,
                &lease,
                &registry,
                &instances,
                guild_id,
                "starring:7:study:button:create",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            registry
                .route_status(first.token())
                .unwrap()
                .active_interactions,
            1
        );
        assert_eq!(
            admission_error(
                budget
                    .admit(
                        &control,
                        &lease,
                        &registry,
                        &instances,
                        guild_id,
                        "starring:7:study:button:create",
                    )
                    .await,
            ),
            SharedGatewayAdmissionErrorV3::Overloaded
        );
        let token = first.token().clone();
        drop(first);
        assert_eq!(
            registry.route_status(&token).unwrap().active_interactions,
            0
        );
        assert!(budget
            .admit(
                &control,
                &lease,
                &registry,
                &instances,
                guild_id,
                "starring:7:study:button:create",
            )
            .await
            .unwrap()
            .is_some());
    }
}
