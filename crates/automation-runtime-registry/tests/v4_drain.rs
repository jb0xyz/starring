use std::convert::Infallible;
use std::future::Future;
use std::num::{NonZeroU32, NonZeroU64};
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use automation_ruleset::{
    content_hash, RuleSetKey, RuleSetVersion, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCanonicalDrainIntentStateV2,
    RuntimeCanonicalProductDrainV2, RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentV2, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2,
    RuntimePersistedProductDrainRootV2, RuntimePersistedUnclaimedPendingDrainIntentV2,
    RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeRecoveryIdV2, RuntimeServingSlotV2,
    RuntimeStartupRecoveryStateV2, RuntimeStartupServingStateV2,
};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, DeploymentId, DeploymentRevision, FencingToken,
    InstallationId, ProcessInstanceId, PromotionId, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_registry::{
    ExactServingRouteV1, PreviousRouteEnvelopeV4, ServingSlotRegistryConfigV1,
    ServingSlotRegistryError, ServingSlotRegistryV1, SlotLifecycleV1, SlotMutationTokenV1,
};
use automation_runtime_worker::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeAcceptedPendingDrainSelectionV4,
    RuntimeAcceptedStartupRecoveryOutcomeV2, RuntimeAuthorizedStartupRecoveryExecutionV2,
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeGatewayClosedLifecycleV2, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimePendingDrainCandidateEvidenceInputV4,
    RuntimePendingDrainCertificationEvidenceV4, RuntimePendingDrainEvidenceDigestV4,
    RuntimePendingDrainFinalizerPortV4, RuntimePendingDrainFinalizerRegistrationV4,
    RuntimePendingDrainRegistryTransitionPortV4, RuntimePendingDrainSelectionOutcomeV4,
    RuntimePendingDrainSelectionReceiptV4, RuntimePendingDrainServingEvidenceV4,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
    RuntimeRoutedDrainClaimExecutionPortV4, RuntimeRoutedDrainClaimExecutionResolutionV4,
    RuntimeRoutedDrainClaimPortOutcomeV4, RuntimeRoutedDrainDeterminateNonCommitPortObservationV4,
    RuntimeRoutedDrainRollbackPortV4, RuntimeRoutedSealedClaimV4,
    RuntimeSelectedUnclaimedPendingDrainV4, RuntimeStartupRecoveryContinuationV2,
};
use automation_state::InteractionRuleSet;
use chrono::{DateTime, Utc};
use discord_model::{GuildId, UserId};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn evidence_digest(value: u8) -> RuntimePendingDrainEvidenceDigestV4 {
    RuntimePendingDrainEvidenceDigestV4::new([value; 32]).unwrap()
}

fn registry(max_active: u32) -> ServingSlotRegistryV1 {
    ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
        max_slots: NonZeroU32::new(128).unwrap(),
        max_active_interactions_per_slot: NonZeroU32::new(max_active).unwrap(),
        max_retired_routes_per_slot: NonZeroU32::new(4).unwrap(),
    })
}

fn route(process: &str) -> ExactServingRouteV1 {
    let definition = InteractionRuleSet {
        version: 1,
        panels: Vec::new(),
        modals: Vec::new(),
        rules: Vec::new(),
    };
    let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
    let ruleset_key = RuleSetKey::parse("studyroom").unwrap();
    let ruleset = RuleSetVersion {
        guild_id: GuildId(9_223_372_036_854_775_808),
        ruleset_key: ruleset_key.clone(),
        version: RuleSetVersionId::FIRST,
        schema_version: CURRENT_RULESET_SCHEMA_VERSION,
        definition,
        content_hash,
        created_by: UserId(90),
    };
    let bindings = ResourceBindingMap::default();
    let binding_fingerprint = resource_binding_fingerprint_v2(&bindings);
    ExactServingRouteV1::new(
        RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse(format!("deployment:{process}")).unwrap(),
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            promotion_id: PromotionId::parse("e".repeat(64)).unwrap(),
            activation_request_id: ActivationRequestId::parse(format!("activation:{process}"))
                .unwrap(),
        },
        RuntimeProcessIdentityV1 {
            target: RuntimeDeploymentTargetV1 {
                guild_id: ruleset.guild_id,
                ruleset_key,
                version: RuleSetVersionId::FIRST,
                content_hash,
                binding_revision: BindingRevision::new(1).unwrap(),
                binding_fingerprint,
            },
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            process_instance_id: ProcessInstanceId::parse(process).unwrap(),
        },
        ruleset,
        bindings,
    )
    .unwrap()
}

fn fence(value: u64) -> FencingToken {
    FencingToken::new(value).unwrap()
}

fn install_active(
    registry: &ServingSlotRegistryV1,
    candidate: &ExactServingRouteV1,
    fencing_token: FencingToken,
) -> SlotMutationTokenV1 {
    let token = registry
        .install(candidate.slot_key(), candidate.clone(), fencing_token)
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    token
}

fn readiness(checked_at: i64) -> RuntimeCapabilityReadinessSetV2 {
    let receipt = |kind, role, offset| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring",
            role,
            at(checked_at + offset),
        )
        .unwrap()
    };
    RuntimeCapabilityReadinessSetV2::new(
        receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 0),
        receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 1),
        receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 2),
        receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 3),
        receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e", 4),
    )
    .unwrap()
}

fn begin_execution() -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let process = ProcessInstanceId::parse("process:current").unwrap();
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let generation = lifecycle.snapshot().generation();
    let paused = RuntimePausedGatewayObservationV2::new(
        generation,
        process.clone(),
        non_zero(2),
        RuntimeGatewayReadyKindV2::Ready,
        non_zero(3),
        RuntimePausedGatewaySequenceV2::new(
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(5)),
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(4)),
            None,
        )
        .unwrap(),
    );
    let registry = accept_runtime_registry_recovery_empty_observation_v2(
        process.clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(6)),
            retained_slot_count: 0,
            retained_empty_tombstone_count: 0,
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 0,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        },
    )
    .unwrap();
    let (_, mut permit) = lifecycle
        .begin_recovery(
            generation,
            RuntimeClosedRecoveryInputV2::new(
                RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
                RuntimeGatewayOwnerLeaseReceiptV1 {
                    lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                        process_instance_id: process,
                        lease_epoch: non_zero(7),
                        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
                    },
                    owner_revision: non_zero(8),
                    database_now: at(100),
                    expires_at: at(300),
                },
                readiness(100),
                paused,
                RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
            ),
        )
        .unwrap();
    let iteration = lifecycle
        .refresh_recovery_readiness(&mut permit, readiness(200))
        .unwrap();
    let observation = lifecycle
        .begin_startup_recovery_observation(&mut permit, iteration)
        .unwrap();
    let request = observation.request();
    let receipt = automation_runtime_controller::RuntimeStartupRecoveryObservationReceiptV2 {
        correlation: request.correlation.clone(),
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: request.gateway_owner_lease_id.clone(),
            owner_revision: request.expected_owner_revision,
            database_now: at(201),
            expires_at: request.expected_owner_expires_at,
        },
        state: RuntimeStartupRecoveryStateV2 {
            serving: RuntimeStartupServingStateV2::Empty,
            recoverable_awaiting_certification_count: 0,
            suspended_local_effect_count: 0,
            pending_runtime_drain_intent_count: 1,
            acknowledged_product_handoff_count: 0,
        },
    };
    let completed = observation.complete(receipt);
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("expected continuation")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(
            automation_runtime_worker::RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent
        )
    );
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    (lifecycle, permit, authorization)
}

fn request_owner(
    request: &automation_runtime_worker::RuntimeStartupRecoveryExecutionRequestV2,
    database_now: i64,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.gateway_owner_lease_id().clone(),
        owner_revision: request.expected_owner_revision(),
        database_now: at(database_now),
        expires_at: request.expected_owner_expires_at(),
    }
}

fn persisted_root(target: &RuntimeDeploymentTargetV1) -> RuntimePersistedProductDrainRootV2 {
    let product = RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse("00112233445566778899aabbccddeeff")
            .unwrap(),
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            deployment_id: DeploymentId::parse("deployment:1").unwrap(),
        },
        expected_revision: DeploymentRevision::new(11).unwrap(),
        slot: RuntimeServingSlotV2::from_target(target),
        expected_target: target.clone(),
        mutation_kind: RuntimeProductMutationKindV2::AuthorityChange,
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap(),
    };
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product,
        RuntimeDrainIntentIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap(),
    )
    .unwrap();
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    RuntimePersistedProductDrainRootV2::from_persisted(
        product.scope.clone(),
        product.expected_revision,
        &product.operation_id,
        drain.key.scope.clone(),
        drain.key.slot.clone(),
        drain.key.expected_revision,
        &drain.key.intent_id,
        &drain.key.expected_target,
        canonical.product_mutation_request_bytes(),
        canonical.product_mutation_digest(),
        canonical.drain_intent_request_bytes(),
        canonical.drain_intent_digest(),
    )
    .unwrap()
}

fn unclaimed_source(
    target: &RuntimeDeploymentTargetV1,
) -> RuntimePersistedUnclaimedPendingDrainIntentV2 {
    let root = persisted_root(target);
    let intent = RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(5), None).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedUnclaimedPendingDrainIntentV2::from_persisted(
        &root,
        non_zero(5),
        canonical.persisted_state().unwrap(),
        canonical.state_bytes(),
    )
    .unwrap()
}

fn selected_unclaimed(
    candidate_route: &ExactServingRouteV1,
) -> (
    RuntimeSelectedUnclaimedPendingDrainV4,
    RuntimePersistedUnclaimedPendingDrainIntentV2,
    RuntimeGatewayOwnerLeaseReceiptV1,
) {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let owner = request_owner(selection.request(), 210);
    let source = unclaimed_source(&candidate_route.identity().target);
    let observed_at = owner.database_now;
    let candidate = automation_runtime_worker::RuntimeUnclaimedPendingDrainCandidateV4::new(
        source.clone(),
        RuntimePendingDrainCandidateEvidenceInputV4 {
            source_state_digest:
                automation_runtime_controller::RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    source.canonical().state_bytes(),
                ),
            source_deployment_fence: fence(19),
            selection_database_now: owner.database_now,
            current_owner: owner.clone(),
            claim_journal: None,
            refence_journal: None,
            serving: RuntimePendingDrainServingEvidenceV4::absent(observed_at, evidence_digest(20)),
            certification: RuntimePendingDrainCertificationEvidenceV4::no_operation_reserved(
                evidence_digest(21),
            ),
        },
    )
    .unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::Unclaimed(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV4::Unclaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected unclaimed selection")
    };
    (selected, source, owner)
}

fn exact_route(
    route: &automation_runtime_registry::SlotRouteWitnessV1,
) -> automation_runtime_controller::RuntimeExactLocalRouteIdentityV2 {
    automation_runtime_controller::RuntimeExactLocalRouteIdentityV2 {
        identity: route.identity.clone(),
        controller_fencing_token: route.fencing_token,
        route_incarnation: route.incarnation,
    }
}

struct ImmediateFinalizer;

impl RuntimePendingDrainFinalizerPortV4 for ImmediateFinalizer {
    type Error = Infallible;

    async fn register<A: Send>(
        &self,
        registration: RuntimePendingDrainFinalizerRegistrationV4<A>,
    ) -> Result<RuntimePendingDrainFinalizerRegistrationV4<A>, Self::Error> {
        Ok(registration)
    }

    async fn join<A: Send>(
        &self,
        join: automation_runtime_worker::RuntimePendingDrainFinalizerJoinV4<A>,
        _operation_cutoff: Instant,
    ) -> Result<automation_runtime_worker::RuntimePendingDrainFinalizerJoinV4<A>, Self::Error> {
        Ok(join)
    }

    async fn transfer<A: Send>(
        &self,
        transfer: automation_runtime_worker::RuntimePendingDrainFinalizerTransferV4<A>,
    ) -> Result<automation_runtime_worker::RuntimePendingDrainFinalizerTransferV4<A>, Self::Error>
    {
        Ok(transfer)
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

struct DeterminateNonCommitPort {
    source: RuntimePersistedUnclaimedPendingDrainIntentV2,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
    route: automation_runtime_controller::RuntimeExactLocalRouteIdentityV2,
    slot_observation_sequence: NonZeroU64,
    registry_observation_sequence: NonZeroU64,
}

impl RuntimeRoutedDrainClaimExecutionPortV4 for DeterminateNonCommitPort {
    type Error = Infallible;

    fn execute_routed_drain_claim_v4<R: Send>(
        &self,
        authorization: &automation_runtime_worker::RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimeRoutedSealedClaimV4<R>,
        >,
        _operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeRoutedDrainClaimPortOutcomeV4, Self::Error>> + Send
    {
        let observation = RuntimeRoutedDrainDeterminateNonCommitPortObservationV4 {
            action_identity: authorization.authorization().action_identity().clone(),
            source: self.source.clone(),
            source_state_digest: authorization.identity().source_state_digest().clone(),
            owner: self.owner.clone(),
            registry_lifetime_digest: authorization.identity().registry_lifetime_digest(),
            seal_generation: authorization.identity().seal_generation(),
            route: self.route.clone(),
            slot_observation_sequence: self.slot_observation_sequence,
            registry_observation_sequence: self.registry_observation_sequence,
            observation_digest: evidence_digest(61),
            observed_at: at(220),
        };
        async move { Ok(RuntimeRoutedDrainClaimPortOutcomeV4::DeterminateNotCommitted(observation)) }
    }
}

#[test]
fn checked_routed_seal_blocks_ordinary_mutation_and_redacts_capabilities() {
    let registry = registry(8);
    let candidate = route("process:current");
    let key = candidate.slot_key();
    let token = install_active(&registry, &candidate, fence(19));
    let observed = registry.observe_routed_v4(&token).unwrap();
    let (selected, _, _) = selected_unclaimed(&candidate);
    let sealed = selected.seal_routed(&registry, observed).unwrap();

    assert!(format!("{sealed:?}").contains("<redacted>"));
    assert_eq!(
        registry.admit(&key).err(),
        Some(ServingSlotRegistryError::SlotSealed)
    );
    assert_eq!(
        registry.activate(&token, candidate.identity()),
        Err(ServingSlotRegistryError::SlotSealed)
    );
    assert_eq!(
        registry.advance_authority(&token, candidate.identity(), fence(20)),
        Err(ServingSlotRegistryError::SlotSealed)
    );
    assert_eq!(
        registry.begin_drain(&token),
        Err(ServingSlotRegistryError::SlotSealed)
    );
    assert_eq!(
        registry.remove(&token),
        Err(ServingSlotRegistryError::SlotSealed)
    );
}

#[test]
fn checked_determinate_non_commit_consumes_the_sealed_source_and_reopens_exact_route() {
    let registry = registry(8);
    let candidate = route("process:current");
    let key = candidate.slot_key();
    let token = install_active(&registry, &candidate, fence(19));
    let observed = registry.observe_routed_v4(&token).unwrap();
    let route = exact_route(observed.route());
    let (selected, source, owner) = selected_unclaimed(&candidate);
    let sealed = selected.seal_routed(&registry, observed).unwrap();
    let slot_observation = registry.atomic_observation_v2(&key).unwrap().unwrap();
    let registry_observation = registry.recovery_observation_v2().unwrap();
    let registered = block_on(
        sealed
            .into_finalizer_registration()
            .register(&ImmediateFinalizer),
    )
    .unwrap();
    let resolution = block_on(registered.execute_routed_claim(
        &DeterminateNonCommitPort {
            source,
            owner,
            route,
            slot_observation_sequence: slot_observation.observation_sequence,
            registry_observation_sequence:
                registry_observation.observation_sequence().as_non_zero(),
        },
        Instant::now(),
    ))
    .unwrap();
    let RuntimeRoutedDrainClaimExecutionResolutionV4::DeterminateNotCommitted(rollback) =
        resolution
    else {
        panic!("expected determinate non-commit")
    };

    let reopened = rollback.rollback(&registry).unwrap();
    assert_eq!(reopened.route().fencing_token, fence(19));
    assert_eq!(reopened.route().lifecycle, SlotLifecycleV1::Serving);
    assert!(registry.admit(&key).is_ok());
}

#[test]
fn checked_rollback_rejects_foreign_registry_lifetime() {
    let source_registry = registry(8);
    let foreign_registry = registry(8);
    let candidate = route("process:current");
    let key = candidate.slot_key();
    let token = install_active(&source_registry, &candidate, fence(19));
    let observed = source_registry.observe_routed_v4(&token).unwrap();
    let route = exact_route(observed.route());
    let (selected, source, owner) = selected_unclaimed(&candidate);
    let sealed = selected.seal_routed(&source_registry, observed).unwrap();
    let slot_observation = source_registry
        .atomic_observation_v2(&key)
        .unwrap()
        .unwrap();
    let registry_observation = source_registry.recovery_observation_v2().unwrap();
    let registered = block_on(
        sealed
            .into_finalizer_registration()
            .register(&ImmediateFinalizer),
    )
    .unwrap();
    let resolution = block_on(registered.execute_routed_claim(
        &DeterminateNonCommitPort {
            source,
            owner,
            route,
            slot_observation_sequence: slot_observation.observation_sequence,
            registry_observation_sequence:
                registry_observation.observation_sequence().as_non_zero(),
        },
        Instant::now(),
    ))
    .unwrap();
    let RuntimeRoutedDrainClaimExecutionResolutionV4::DeterminateNotCommitted(rollback) =
        resolution
    else {
        panic!("expected determinate non-commit")
    };

    assert!(matches!(
        rollback.rollback(&foreign_registry),
        Err(
            automation_runtime_worker::RuntimePendingDrainBoundaryErrorV4::Port(
                ServingSlotRegistryError::V4RegistryMismatch
            )
        )
    ));
    assert_eq!(
        source_registry.admit(&key).err(),
        Some(ServingSlotRegistryError::SlotSealed)
    );
}

#[test]
fn checked_routed_seal_rejects_guard_race_and_foreign_registry() {
    let race_registry = registry(8);
    let candidate = route("process:current");
    let key = candidate.slot_key();
    let token = install_active(&race_registry, &candidate, fence(19));
    let observed = race_registry.observe_routed_v4(&token).unwrap();
    let active = race_registry.admit(&key).unwrap();
    let (selected, _, _) = selected_unclaimed(&candidate);
    assert!(matches!(
        selected.seal_routed(&race_registry, observed),
        Err(
            automation_runtime_worker::RuntimePendingDrainBoundaryErrorV4::Port(
                ServingSlotRegistryError::V4ObservationMismatch
            )
        )
    ));
    drop(active);

    let source_registry = registry(8);
    let foreign_registry = registry(8);
    let token = install_active(&source_registry, &candidate, fence(19));
    let observed = source_registry.observe_routed_v4(&token).unwrap();
    let (selected, _, _) = selected_unclaimed(&candidate);
    assert!(matches!(
        selected.seal_routed(&foreign_registry, observed),
        Err(
            automation_runtime_worker::RuntimePendingDrainBoundaryErrorV4::Port(
                ServingSlotRegistryError::V4RegistryMismatch
            )
        )
    ));
}

#[test]
fn previous_route_envelope_is_redacted_and_requires_adjacent_fences() {
    let candidate = route("process:previous");
    let key = candidate.slot_key();
    assert_eq!(
        PreviousRouteEnvelopeV4::new(
            key.clone(),
            candidate.identity().clone(),
            non_zero(7),
            fence(19),
            fence(21),
        ),
        Err(ServingSlotRegistryError::V4FenceMismatch)
    );
    let envelope = PreviousRouteEnvelopeV4::new(
        key,
        candidate.identity().clone(),
        non_zero(7),
        fence(19),
        fence(20),
    )
    .unwrap();
    assert_eq!(
        format!("{envelope:?}"),
        "PreviousRouteEnvelopeV4(<redacted>)"
    );
}

#[test]
fn registry_implements_checked_worker_transition_ports() {
    fn assert_ports<T>()
    where
        T: RuntimePendingDrainRegistryTransitionPortV4
            + RuntimeRoutedDrainRollbackPortV4<Error = ServingSlotRegistryError>,
    {
    }

    assert_ports::<ServingSlotRegistryV1>();
}
