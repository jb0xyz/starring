use std::num::NonZeroU32;

use automation_ruleset::{
    content_hash, RuleSetKey, RuleSetVersion, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, DeploymentId, FencingToken, InstallationId,
    ProcessInstanceId, PromotionId, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_registry::{
    ExactServingRouteV1, ServingSlotKeyV1, ServingSlotRegistryConfigV1, ServingSlotRegistryError,
    ServingSlotRegistryV1, SlotActivationOutcomeV1, SlotAdmissionStateV2, SlotDrainOutcomeV1,
    SlotInstallOutcomeV1, SlotLifecycleV1, SlotRemovalOutcomeV1, SlotSealKeyErrorV2, SlotSealKeyV2,
};
use automation_state::InteractionRuleSet;
use desired_state::ResourceKey;
use discord_model::{GuildId, RoleId, UserId};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

fn registry(max_active: u32) -> ServingSlotRegistryV1 {
    ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
        max_slots: NonZeroU32::new(128).unwrap(),
        max_active_interactions_per_slot: NonZeroU32::new(max_active).unwrap(),
        max_retired_routes_per_slot: NonZeroU32::new(4).unwrap(),
    })
}

fn deployment_identity(guild_id: u64, version: u32, process: &str) -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse(format!("deployment:{guild_id}:{version}:{process}"))
            .unwrap(),
        tenant_id: TenantId::parse(format!("tenant:{guild_id}")).unwrap(),
        installation_id: InstallationId::parse(format!("installation:{guild_id}")).unwrap(),
        promotion_id: PromotionId::parse("d".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse(format!(
            "activation:{guild_id}:{version}:{process}"
        ))
        .unwrap(),
    }
}

fn route(
    guild_id: u64,
    ruleset_key: &str,
    version: u32,
    generation: u64,
    process: &str,
    bound_role: Option<u64>,
) -> ExactServingRouteV1 {
    let definition = InteractionRuleSet {
        version,
        panels: Vec::new(),
        modals: Vec::new(),
        rules: Vec::new(),
    };
    let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
    let ruleset_key = RuleSetKey::parse(ruleset_key).unwrap();
    let ruleset = RuleSetVersion {
        guild_id: GuildId(guild_id),
        ruleset_key: ruleset_key.clone(),
        version: RuleSetVersionId::new(version).unwrap(),
        schema_version: CURRENT_RULESET_SCHEMA_VERSION,
        definition,
        content_hash,
        created_by: UserId(90),
    };
    let mut bindings = ResourceBindingMap::default();
    if let Some(role_id) = bound_role {
        bindings
            .role_bindings
            .insert(ResourceKey("member".to_string()), RoleId(role_id));
    }
    let binding_fingerprint = resource_binding_fingerprint_v2(&bindings);
    ExactServingRouteV1::new(
        deployment_identity(guild_id, version, process),
        RuntimeProcessIdentityV1 {
            target: RuntimeDeploymentTargetV1 {
                guild_id: GuildId(guild_id),
                ruleset_key,
                version: RuleSetVersionId::new(version).unwrap(),
                content_hash,
                binding_revision: BindingRevision::new(version as u64).unwrap(),
                binding_fingerprint,
            },
            runtime_generation: RuntimeGeneration::new(generation).unwrap(),
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

fn seal_key(value: u8) -> SlotSealKeyV2 {
    SlotSealKeyV2::try_from([value; 16].as_slice()).unwrap()
}

#[test]
fn two_guild_and_ruleset_slots_are_isolated() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let second = route(20, "study", 1, 1, "p2", None);
    let third = route(10, "welcome", 1, 1, "p3", None);
    let first_key = first.slot_key();
    let second_key = second.slot_key();
    let third_key = third.slot_key();
    let first_token = registry
        .install(first_key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    let second_token = registry
        .install(second_key.clone(), second.clone(), fence(2))
        .unwrap()
        .token;
    let third_token = registry
        .install(third_key.clone(), third.clone(), fence(3))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    registry.activate(&second_token, second.identity()).unwrap();
    registry.activate(&third_token, third.identity()).unwrap();
    registry.begin_drain(&first_token).unwrap();
    assert!(registry.serving_snapshot(&first_key).unwrap().is_none());
    assert_eq!(
        registry
            .serving_snapshot(&second_key)
            .unwrap()
            .unwrap()
            .identity(),
        second.identity()
    );
    assert_eq!(
        registry
            .serving_snapshot(&third_key)
            .unwrap()
            .unwrap()
            .identity(),
        third.identity()
    );
}

#[test]
fn staged_route_is_not_visible_until_exact_activation() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let receipt = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap();
    assert_eq!(receipt.outcome, SlotInstallOutcomeV1::Staged);
    assert!(registry.serving_snapshot(&key).unwrap().is_none());
    assert_eq!(
        registry.route_status(&receipt.token).unwrap().lifecycle,
        SlotLifecycleV1::Staged
    );
    assert_eq!(
        registry
            .activate(&receipt.token, candidate.identity())
            .unwrap(),
        SlotActivationOutcomeV1::Activated
    );
    assert_eq!(
        registry.serving_snapshot(&key).unwrap().unwrap().identity(),
        candidate.identity()
    );
}

#[test]
fn activation_requires_the_exact_staged_identity() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let wrong = route(10, "study", 2, 2, "p2", Some(50));
    let key = candidate.slot_key();
    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    assert_eq!(
        registry.activate(&token, wrong.identity()),
        Err(ServingSlotRegistryError::ActivationTargetMismatch)
    );
    assert!(registry.serving_snapshot(&key).unwrap().is_none());
    assert_eq!(
        registry.route_status(&token).unwrap().lifecycle,
        SlotLifecycleV1::Staged
    );
}

#[test]
fn stale_token_cannot_remove_an_aba_replacement() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    registry.begin_drain(&first_token).unwrap();
    assert_eq!(
        registry.remove(&first_token).unwrap(),
        SlotRemovalOutcomeV1::RemovedDraining
    );
    let replacement = route(10, "study", 2, 2, "p2", Some(51));
    let replacement_token = registry
        .install(key.clone(), replacement.clone(), fence(2))
        .unwrap()
        .token;
    registry
        .activate(&replacement_token, replacement.identity())
        .unwrap();
    assert_eq!(
        registry.remove(&first_token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(
        registry.serving_snapshot(&key).unwrap().unwrap().identity(),
        replacement.identity()
    );
}

#[test]
fn drain_prevents_new_admission_and_waits_for_in_flight_work() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    let admitted = registry.admit(&key).unwrap();
    assert_eq!(
        registry.begin_drain(&token).unwrap(),
        SlotDrainOutcomeV1::DrainStarted {
            active_interactions: 1
        }
    );
    assert_eq!(
        registry.admit(&key).err().unwrap(),
        ServingSlotRegistryError::NotServing
    );
    assert_eq!(
        registry.observe_drain(&token).unwrap().active_interactions,
        1
    );
    assert_eq!(
        registry.remove(&token),
        Err(ServingSlotRegistryError::ActiveInteractionsRemain { active: 1 })
    );
    drop(admitted);
    assert_eq!(
        registry.observe_drain(&token).unwrap(),
        automation_runtime_registry::SlotDrainObservationV1 {
            active_interactions: 0,
            drained: true
        }
    );
    assert_eq!(
        registry.remove(&token).unwrap(),
        SlotRemovalOutcomeV1::RemovedDraining
    );
}

#[test]
fn replacement_never_exposes_mixed_ruleset_and_bindings() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    let first_interaction = registry.admit(&key).unwrap();
    let replacement = route(10, "study", 2, 2, "p2", Some(700));
    let replacement_token = registry
        .install(key.clone(), replacement.clone(), fence(2))
        .unwrap()
        .token;
    assert_eq!(
        first_interaction.route().ruleset().version,
        RuleSetVersionId::new(1).unwrap()
    );
    assert!(first_interaction
        .route()
        .bindings()
        .role_bindings
        .is_empty());
    registry
        .activate(&replacement_token, replacement.identity())
        .unwrap();
    assert_eq!(
        registry.begin_drain(&first_token).unwrap(),
        SlotDrainOutcomeV1::AlreadyDraining {
            active_interactions: 1
        }
    );
    let after = registry.admit(&key).unwrap();
    assert_eq!(
        after.route().ruleset().version,
        RuleSetVersionId::new(2).unwrap()
    );
    assert_eq!(
        after
            .route()
            .bindings()
            .role_bindings
            .get(&ResourceKey("member".to_string())),
        Some(&RoleId(700))
    );
    assert_eq!(
        first_interaction.route().ruleset().version,
        RuleSetVersionId::new(1).unwrap()
    );
    assert!(first_interaction
        .route()
        .bindings()
        .role_bindings
        .is_empty());
    assert_eq!(
        registry
            .observe_drain(&first_token)
            .unwrap()
            .active_interactions,
        1
    );
    drop(first_interaction);
    assert!(registry.observe_drain(&first_token).unwrap().drained);
}

#[test]
fn active_interaction_accounting_is_bounded() {
    let registry = registry(1);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    let admitted = registry.admit(&key).unwrap();
    assert_eq!(
        registry.admit(&key).err().unwrap(),
        ServingSlotRegistryError::ActiveInteractionCapacityExceeded
    );
    drop(admitted);
    assert!(registry.admit(&key).is_ok());
}

#[test]
fn generation_and_fencing_high_water_reject_stale_installs() {
    let registry = registry(8);
    let first = route(10, "study", 1, 2, "p1", None);
    let key = first.slot_key();
    registry
        .install(key.clone(), first.clone(), fence(20))
        .unwrap();
    let older_generation = route(10, "study", 2, 1, "older", Some(1));
    assert_eq!(
        registry
            .install(key.clone(), older_generation, fence(21))
            .unwrap_err(),
        ServingSlotRegistryError::StaleRuntimeGeneration {
            minimum: RuntimeGeneration::new(2).unwrap(),
            actual: RuntimeGeneration::new(1).unwrap()
        }
    );
    let stale_same_generation = first.clone();
    assert_eq!(
        registry
            .install(key.clone(), stale_same_generation, fence(19))
            .unwrap_err(),
        ServingSlotRegistryError::StaleFencingToken {
            minimum: fence(20),
            actual: fence(19)
        }
    );
    let newer_generation = route(10, "study", 2, 3, "p2", Some(2));
    assert_eq!(
        registry
            .install(key, newer_generation, fence(1))
            .unwrap()
            .outcome,
        SlotInstallOutcomeV1::Staged
    );
}

#[test]
fn exact_route_constructor_rejects_mixed_bindings() {
    let valid = route(10, "study", 1, 1, "p1", None);
    let mut different_bindings = ResourceBindingMap::default();
    different_bindings
        .role_bindings
        .insert(ResourceKey("member".to_string()), RoleId(5));
    assert_eq!(
        ExactServingRouteV1::new(
            valid.deployment_identity().clone(),
            valid.identity().clone(),
            valid.ruleset().as_ref().clone(),
            different_bindings
        )
        .unwrap_err(),
        automation_runtime_registry::ExactServingRouteError::BindingFingerprintMismatch
    );
}

#[test]
fn exact_route_preserves_the_explicit_deployment_identity() {
    let route = route(10, "study", 3, 4, "identity-route", None);

    assert_eq!(
        route.deployment_identity(),
        &deployment_identity(10, 3, "identity-route")
    );
    assert_eq!(route.process_identity(), route.identity());
}

#[test]
fn serving_slot_key_is_typed_by_guild_and_ruleset() {
    let key = ServingSlotKeyV1::new(GuildId(9), RuleSetKey::parse("welcome").unwrap());
    assert_eq!(key.guild_id(), GuildId(9));
    assert_eq!(key.ruleset_key().as_str(), "welcome");
}

#[test]
fn idempotent_install_reports_the_current_lifecycle() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let first = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap();
    assert_eq!(
        registry
            .install(key.clone(), candidate.clone(), fence(1))
            .unwrap()
            .outcome,
        SlotInstallOutcomeV1::AlreadyStaged
    );
    registry
        .activate(&first.token, candidate.identity())
        .unwrap();
    assert_eq!(
        registry
            .install(key.clone(), candidate.clone(), fence(1))
            .unwrap()
            .outcome,
        SlotInstallOutcomeV1::AlreadyServing
    );
    registry.begin_drain(&first.token).unwrap();
    assert_eq!(
        registry.install(key, candidate, fence(1)).unwrap().outcome,
        SlotInstallOutcomeV1::AlreadyDraining
    );
}

#[test]
fn one_generation_cannot_be_reused_for_a_different_process_identity() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let conflicting = route(10, "study", 2, 1, "p2", Some(9));
    let key = first.slot_key();
    registry.install(key.clone(), first, fence(1)).unwrap();
    assert_eq!(
        registry.install(key, conflicting, fence(2)).unwrap_err(),
        ServingSlotRegistryError::RuntimeGenerationIdentityConflict
    );
}

#[test]
fn newer_high_water_rejects_old_current_mutations_and_reinstall() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    registry
        .install(key.clone(), replacement, fence(2))
        .unwrap();
    assert_eq!(
        registry.begin_drain(&first_token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(
        registry.install(key, first, fence(1)).unwrap_err(),
        ServingSlotRegistryError::StaleMutationToken
    );
}

#[test]
fn newer_high_water_rejects_removing_an_old_draining_current() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    registry.begin_drain(&first_token).unwrap();
    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    registry.install(key, replacement, fence(2)).unwrap();
    assert_eq!(
        registry.remove(&first_token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
}

#[test]
fn token_from_another_registry_is_never_authoritative() {
    let first_registry = registry(8);
    let second_registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let foreign = first_registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    second_registry
        .install(key, candidate.clone(), fence(1))
        .unwrap();
    assert_eq!(
        second_registry.activate(&foreign, candidate.identity()),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(
        second_registry.route_witness(&foreign),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(
        second_registry.advance_authority(&foreign, candidate.identity(), fence(2)),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
}

#[test]
fn active_limit_counts_current_and_retired_routes_together() {
    let registry = registry(1);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    let first_interaction = registry.admit(&key).unwrap();
    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    let replacement_token = registry
        .install(key.clone(), replacement.clone(), fence(2))
        .unwrap()
        .token;
    registry
        .activate(&replacement_token, replacement.identity())
        .unwrap();
    assert_eq!(
        registry.admit(&key).err().unwrap(),
        ServingSlotRegistryError::ActiveInteractionCapacityExceeded
    );
    drop(first_interaction);
    assert!(registry.admit(&key).is_ok());
}

#[test]
fn distinct_slot_tombstones_are_globally_bounded() {
    let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
        max_slots: NonZeroU32::new(1).unwrap(),
        max_active_interactions_per_slot: NonZeroU32::new(8).unwrap(),
        max_retired_routes_per_slot: NonZeroU32::new(4).unwrap(),
    });
    let first = route(10, "study", 1, 1, "p1", None);
    let first_key = first.slot_key();
    let first_token = registry
        .install(first_key, first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    registry.begin_drain(&first_token).unwrap();
    registry.remove(&first_token).unwrap();
    let second = route(20, "study", 1, 1, "p2", None);
    assert_eq!(
        registry
            .install(second.slot_key(), second, fence(1))
            .unwrap_err(),
        ServingSlotRegistryError::SlotCapacityExceeded
    );
}

#[test]
fn latest_authority_can_control_current_after_staged_cancellation() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    let replacement_token = registry.install(key, replacement, fence(2)).unwrap().token;
    registry.remove(&replacement_token).unwrap();
    assert_eq!(
        registry.begin_drain(&first_token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(
        registry
            .begin_drain_with_authority(&replacement_token, &first_token)
            .unwrap(),
        SlotDrainOutcomeV1::DrainStarted {
            active_interactions: 0
        }
    );
    assert_eq!(
        registry
            .remove_with_authority(&replacement_token, &first_token)
            .unwrap(),
        SlotRemovalOutcomeV1::RemovedDraining
    );
}

#[test]
fn staged_authority_advances_by_one_and_invalidates_the_old_token() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let old = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    let before = registry.route_witness(&old).unwrap();
    assert_eq!(before.identity, candidate.identity().clone());
    assert_eq!(before.fencing_token, fence(1));
    assert_eq!(before.lifecycle, SlotLifecycleV1::Staged);

    let other = route(10, "study", 2, 2, "p2", Some(50));
    assert_eq!(
        registry.advance_authority(&old, other.identity(), fence(2)),
        Err(ServingSlotRegistryError::AuthorityTargetMismatch)
    );
    for actual in [fence(1), fence(3)] {
        assert_eq!(
            registry.advance_authority(&old, candidate.identity(), actual),
            Err(ServingSlotRegistryError::NonSuccessorFencingToken {
                expected: fence(2),
                actual,
            })
        );
        assert_eq!(registry.route_witness(&old).unwrap(), before);
    }

    let current = registry
        .advance_authority(&old, candidate.identity(), fence(2))
        .unwrap();
    let after = registry.route_witness(&current).unwrap();
    assert_eq!(after.identity, before.identity);
    assert_eq!(after.incarnation, before.incarnation);
    assert_eq!(after.fencing_token, fence(2));
    assert_eq!(after.lifecycle, SlotLifecycleV1::Staged);
    assert_eq!(
        registry.route_witness(&old),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(
        registry.advance_authority(&old, candidate.identity(), fence(u64::MAX)),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(
        registry.install(key.clone(), candidate.clone(), fence(1)),
        Err(ServingSlotRegistryError::StaleFencingToken {
            minimum: fence(2),
            actual: fence(1),
        })
    );
    let replay = registry.install(key, candidate.clone(), fence(2)).unwrap();
    assert_eq!(replay.outcome, SlotInstallOutcomeV1::AlreadyStaged);
    assert_eq!(replay.token, current);
    registry.activate(&current, candidate.identity()).unwrap();
}

#[test]
fn serving_and_draining_authority_advancement_preserves_active_work() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let serving = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&serving, candidate.identity()).unwrap();
    let interaction = registry.admit(&key).unwrap();
    let renewed = registry
        .advance_authority(&serving, candidate.identity(), fence(2))
        .unwrap();
    let status = registry.route_status(&renewed).unwrap();
    assert_eq!(status.lifecycle, SlotLifecycleV1::Serving);
    assert_eq!(status.active_interactions, 1);
    assert_eq!(
        registry.begin_drain(&renewed).unwrap(),
        SlotDrainOutcomeV1::DrainStarted {
            active_interactions: 1
        }
    );
    let draining = registry
        .advance_authority(&renewed, candidate.identity(), fence(3))
        .unwrap();
    let witness = registry.route_witness(&draining).unwrap();
    assert_eq!(witness.lifecycle, SlotLifecycleV1::Draining);
    assert_eq!(witness.fencing_token, fence(3));
    assert_eq!(
        registry.remove(&draining),
        Err(ServingSlotRegistryError::ActiveInteractionsRemain { active: 1 })
    );
    drop(interaction);
    assert!(registry.observe_drain(&draining).unwrap().drained);
    assert_eq!(
        registry.remove(&draining).unwrap(),
        SlotRemovalOutcomeV1::RemovedDraining
    );
}

#[test]
fn retired_or_exhausted_authority_cannot_be_advanced() {
    let active_registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = active_registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    active_registry
        .activate(&first_token, first.identity())
        .unwrap();
    let interaction = active_registry.admit(&key).unwrap();
    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    let replacement_token = active_registry
        .install(key, replacement.clone(), fence(2))
        .unwrap()
        .token;
    active_registry
        .activate(&replacement_token, replacement.identity())
        .unwrap();
    assert_eq!(
        active_registry.advance_authority(&first_token, first.identity(), fence(2)),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    drop(interaction);

    let exhausted_registry = registry(8);
    let exhausted = route(20, "study", 1, 1, "p3", None);
    let exhausted_token = exhausted_registry
        .install(exhausted.slot_key(), exhausted.clone(), fence(u64::MAX))
        .unwrap()
        .token;
    assert_eq!(
        exhausted_registry.advance_authority(
            &exhausted_token,
            exhausted.identity(),
            fence(u64::MAX),
        ),
        Err(ServingSlotRegistryError::FencingTokenExhausted)
    );
    assert_eq!(
        exhausted_registry
            .route_witness(&exhausted_token)
            .unwrap()
            .fencing_token,
        fence(u64::MAX)
    );
}

#[test]
fn authority_advancement_does_not_consume_an_incarnation() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let first_token = registry
        .install(first.slot_key(), first.clone(), fence(1))
        .unwrap()
        .token;
    let first_witness = registry.route_witness(&first_token).unwrap();
    let renewed = registry
        .advance_authority(&first_token, first.identity(), fence(2))
        .unwrap();
    registry.remove(&renewed).unwrap();

    let second = route(10, "study", 2, 2, "p2", Some(50));
    let second_token = registry
        .install(second.slot_key(), second, fence(1))
        .unwrap()
        .token;
    let second_witness = registry.route_witness(&second_token).unwrap();
    assert_eq!(
        second_witness.incarnation.get(),
        first_witness.incarnation.get() + 1
    );
}

#[test]
fn replacement_authority_can_remove_a_refenced_retired_route() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    let interaction = registry.admit(&key).unwrap();
    let old_snapshot = interaction.token().clone();
    let first_renewed = registry
        .advance_authority(&first_token, first.identity(), fence(2))
        .unwrap();
    assert_eq!(
        registry.route_witness(&old_snapshot),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );

    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    let replacement_token = registry
        .install(key, replacement.clone(), fence(3))
        .unwrap()
        .token;
    registry
        .activate(&replacement_token, replacement.identity())
        .unwrap();
    let retired = registry.route_witness(&first_renewed).unwrap();
    assert_eq!(retired.lifecycle, SlotLifecycleV1::Draining);
    assert_eq!(retired.fencing_token, fence(2));
    assert_eq!(
        registry
            .route_status(&first_renewed)
            .unwrap()
            .active_interactions,
        1
    );
    drop(interaction);
    assert!(registry.observe_drain(&first_renewed).unwrap().drained);
    let before_removal = registry.recovery_observation_v2().unwrap();
    assert_eq!(
        registry
            .remove_with_authority(&replacement_token, &first_renewed)
            .unwrap(),
        SlotRemovalOutcomeV1::RemovedDraining
    );
    let after_removal = registry.recovery_observation_v2().unwrap();
    assert_eq!(
        after_removal.observation_sequence().get(),
        before_removal.observation_sequence().get() + 1
    );
    assert_eq!(
        after_removal.draining_route_count() + 1,
        before_removal.draining_route_count()
    );
}

#[test]
fn atomic_v2_observation_tracks_only_effective_slot_mutations() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    assert_eq!(registry.atomic_observation_v2(&key).unwrap(), None);

    let installed = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap();
    let staged = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(staged.admission_state, SlotAdmissionStateV2::Staged);
    assert_eq!(staged.admission_generation.get(), 1);
    assert_eq!(staged.observation_sequence.get(), 1);
    assert_eq!(staged.active_interactions, 0);
    let staged_route = staged.route.as_ref().unwrap();
    assert_eq!(staged_route.identity, candidate.identity().clone());
    assert_eq!(staged_route.fencing_token, fence(1));
    assert_eq!(staged_route.lifecycle, SlotLifecycleV1::Staged);

    assert_eq!(
        registry
            .install(key.clone(), candidate.clone(), fence(1))
            .unwrap()
            .outcome,
        SlotInstallOutcomeV1::AlreadyStaged
    );
    assert_eq!(
        registry.atomic_observation_v2(&key).unwrap().unwrap(),
        staged
    );

    let activation = registry
        .activate_with_sequence_v2(&installed.token, candidate.identity())
        .unwrap();
    assert_eq!(activation.outcome(), SlotActivationOutcomeV1::Activated);
    assert_eq!(activation.activation_sequence().get(), 1);
    assert_eq!(
        activation.observation().admission_state,
        SlotAdmissionStateV2::Serving
    );
    assert_eq!(activation.observation().admission_generation.get(), 2);
    assert_eq!(activation.observation().observation_sequence.get(), 2);

    let replay = registry
        .activate_with_sequence_v2(&installed.token, candidate.identity())
        .unwrap();
    assert_eq!(replay.outcome(), SlotActivationOutcomeV1::AlreadyServing);
    assert_eq!(
        replay.activation_sequence(),
        activation.activation_sequence()
    );
    assert_eq!(replay.observation(), activation.observation());

    let renewed = registry
        .advance_authority(&installed.token, candidate.identity(), fence(2))
        .unwrap();
    let refenced = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(refenced.admission_generation.get(), 3);
    assert_eq!(refenced.observation_sequence.get(), 3);
    assert_eq!(refenced.route.as_ref().unwrap().fencing_token, fence(2));

    registry.begin_drain(&renewed).unwrap();
    let draining = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(draining.admission_state, SlotAdmissionStateV2::Draining);
    assert_eq!(draining.admission_generation.get(), 4);
    assert_eq!(draining.observation_sequence.get(), 4);
    registry.begin_drain(&renewed).unwrap();
    assert_eq!(
        registry.atomic_observation_v2(&key).unwrap().unwrap(),
        draining
    );

    registry.remove(&renewed).unwrap();
    let empty = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(empty.admission_state, SlotAdmissionStateV2::Empty);
    assert_eq!(empty.admission_generation.get(), 5);
    assert_eq!(empty.observation_sequence.get(), 5);
    assert_eq!(empty.route, None);
}

#[test]
fn v2_guard_acquire_and_drop_advance_observation_only() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    let before = registry.atomic_observation_v2(&key).unwrap().unwrap();

    let admitted = registry
        .admit_at_generation_v2(&key, before.admission_generation)
        .unwrap();
    assert_eq!(
        admitted.observation().admission_generation,
        before.admission_generation
    );
    assert_eq!(
        admitted.observation().observation_sequence.get(),
        before.observation_sequence.get() + 1
    );
    assert_eq!(admitted.observation().active_interactions, 1);
    assert_eq!(admitted.route().identity(), candidate.identity());

    let while_active = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(while_active, admitted.observation().clone());
    drop(admitted);
    let after = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(after.admission_generation, before.admission_generation);
    assert_eq!(
        after.observation_sequence.get(),
        before.observation_sequence.get() + 2
    );
    assert_eq!(after.active_interactions, 0);
}

#[test]
fn v2_admission_rejects_a_generation_invalidated_by_replacement() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    let before = registry.atomic_observation_v2(&key).unwrap().unwrap();

    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    registry
        .install(key.clone(), replacement, fence(2))
        .unwrap();
    let after = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(after.admission_state, SlotAdmissionStateV2::Serving);
    assert_eq!(
        after.route.as_ref().unwrap().identity,
        first.identity().clone()
    );
    assert_eq!(
        registry
            .admit_at_generation_v2(&key, before.admission_generation)
            .err()
            .unwrap(),
        ServingSlotRegistryError::AdmissionGenerationMismatch {
            expected: before.admission_generation,
            actual: after.admission_generation,
        }
    );
}

#[test]
fn guard_count_changes_do_not_invalidate_the_admission_generation() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    let generation = registry
        .atomic_observation_v2(&key)
        .unwrap()
        .unwrap()
        .admission_generation;
    let first = registry.admit_at_generation_v2(&key, generation).unwrap();
    let second = registry.admit_at_generation_v2(&key, generation).unwrap();
    assert_eq!(first.observation().admission_generation, generation);
    assert_eq!(second.observation().admission_generation, generation);
    assert!(second.observation().observation_sequence > first.observation().observation_sequence);
    drop(first);
    drop(second);
    assert_eq!(
        registry
            .atomic_observation_v2(&key)
            .unwrap()
            .unwrap()
            .admission_generation,
        generation
    );
}

#[test]
fn activation_sequence_is_monotonic_and_replay_stable() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    let first_activation = registry
        .activate_with_sequence_v2(&first_token, first.identity())
        .unwrap();
    assert_eq!(first_activation.activation_sequence().get(), 1);
    assert_eq!(
        registry
            .activate_with_sequence_v2(&first_token, first.identity())
            .unwrap()
            .activation_sequence(),
        first_activation.activation_sequence()
    );

    let replacement = route(10, "study", 2, 2, "p2", Some(50));
    let replacement_token = registry
        .install(key, replacement.clone(), fence(2))
        .unwrap()
        .token;
    let replacement_activation = registry
        .activate_with_sequence_v2(&replacement_token, replacement.identity())
        .unwrap();
    assert_eq!(replacement_activation.activation_sequence().get(), 2);
    assert_eq!(
        replacement_activation.route().identity,
        replacement.identity().clone()
    );
}

#[test]
fn v2_tombstone_counters_are_retained_and_slots_are_isolated() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let first_key = first.slot_key();
    let first_token = registry
        .install(first_key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();

    let second = route(20, "study", 1, 1, "p2", None);
    let second_key = second.slot_key();
    let second_token = registry
        .install(second_key.clone(), second.clone(), fence(1))
        .unwrap()
        .token;
    let second_before = registry
        .atomic_observation_v2(&second_key)
        .unwrap()
        .unwrap();

    registry.begin_drain(&first_token).unwrap();
    registry.remove(&first_token).unwrap();
    let first_empty = registry.atomic_observation_v2(&first_key).unwrap().unwrap();
    assert_eq!(first_empty.admission_state, SlotAdmissionStateV2::Empty);
    assert_eq!(
        registry
            .atomic_observation_v2(&second_key)
            .unwrap()
            .unwrap(),
        second_before
    );

    let first_replacement = route(10, "study", 2, 2, "p3", Some(51));
    registry
        .install(first_key.clone(), first_replacement, fence(2))
        .unwrap();
    let first_reinstalled = registry.atomic_observation_v2(&first_key).unwrap().unwrap();
    assert_eq!(
        first_reinstalled.admission_generation.get(),
        first_empty.admission_generation.get() + 1
    );
    assert_eq!(
        first_reinstalled.observation_sequence.get(),
        first_empty.observation_sequence.get() + 1
    );
    assert_eq!(
        registry.route_witness(&second_token).unwrap().identity,
        second.identity().clone()
    );
}

#[test]
fn slot_seal_key_accepts_only_exact_binary_identity() {
    let bytes = [7_u8; 16];
    let key = SlotSealKeyV2::try_from(bytes.as_slice()).unwrap();
    assert_eq!(key.as_bytes(), &bytes);
    for invalid in [vec![7_u8; 15], vec![7_u8; 17]] {
        assert_eq!(
            SlotSealKeyV2::try_from(invalid.as_slice()),
            Err(SlotSealKeyErrorV2::InvalidLength)
        );
    }
}

#[test]
fn drain_claim_seal_blocks_admission_and_ordinary_mutation() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    let active = registry.admit(&key).unwrap();
    let before = registry.atomic_observation_v2(&key).unwrap().unwrap();
    let (seal, sealed) = registry
        .seal_drain_claim_v2(&key, seal_key(7), Some(&before))
        .unwrap();
    assert_eq!(seal.key(), &key);
    assert_eq!(seal.seal_key(), seal_key(7));
    assert_eq!(seal.seal_generation().get(), 1);
    assert_eq!(seal.route(), before.route.as_ref());
    assert_eq!(
        sealed.admission_state,
        SlotAdmissionStateV2::DrainClaimSealed {
            seal_key: seal_key(7),
            seal_generation: seal.seal_generation(),
        }
    );
    assert_eq!(
        sealed.admission_generation.get(),
        before.admission_generation.get() + 1
    );
    assert_eq!(
        sealed.observation_sequence.get(),
        before.observation_sequence.get() + 1
    );
    assert!(registry.serving_snapshot(&key).unwrap().is_none());
    assert_eq!(
        registry.admit(&key).err().unwrap(),
        ServingSlotRegistryError::SlotSealed
    );
    assert_eq!(
        registry
            .install(key.clone(), candidate.clone(), fence(1))
            .unwrap_err(),
        ServingSlotRegistryError::SlotSealed
    );
    assert_eq!(
        registry.activate(&token, candidate.identity()),
        Err(ServingSlotRegistryError::SlotSealed)
    );
    assert_eq!(
        registry.begin_drain(&token),
        Err(ServingSlotRegistryError::SlotSealed)
    );
    assert_eq!(
        registry.advance_authority(&token, candidate.identity(), fence(2)),
        Err(ServingSlotRegistryError::SlotSealed)
    );
    assert_eq!(
        registry.remove(&token),
        Err(ServingSlotRegistryError::SlotSealed)
    );

    drop(active);
    let after_drop = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(after_drop.active_interactions, 0);
    assert_eq!(after_drop.admission_generation, sealed.admission_generation);
    assert_eq!(
        after_drop.observation_sequence.get(),
        sealed.observation_sequence.get() + 1
    );
    let reopened = registry.unseal_drain_claim_v2(seal).unwrap();
    assert_eq!(reopened.admission_state, SlotAdmissionStateV2::Serving);
    assert_eq!(
        reopened.admission_generation.get(),
        sealed.admission_generation.get() + 1
    );
    assert_eq!(
        reopened.observation_sequence.get(),
        after_drop.observation_sequence.get() + 1
    );
    assert!(registry.admit(&key).is_ok());
}

#[test]
fn empty_slot_seal_materializes_a_fenced_tombstone() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    assert_eq!(registry.atomic_observation_v2(&key).unwrap(), None);
    let (seal, sealed) = registry
        .seal_drain_claim_v2(&key, seal_key(8), None)
        .unwrap();
    assert_eq!(sealed.route, None);
    assert_eq!(sealed.active_interactions, 0);
    assert_eq!(sealed.admission_generation.get(), 1);
    assert_eq!(sealed.observation_sequence.get(), 1);
    assert!(matches!(
        sealed.admission_state,
        SlotAdmissionStateV2::DrainClaimSealed { .. }
    ));
    assert_eq!(
        registry
            .install(key.clone(), candidate.clone(), fence(1))
            .unwrap_err(),
        ServingSlotRegistryError::SlotSealed
    );
    let empty = registry.unseal_drain_claim_v2(seal).unwrap();
    assert_eq!(empty.admission_state, SlotAdmissionStateV2::Empty);
    assert_eq!(empty.admission_generation.get(), 2);
    assert_eq!(empty.observation_sequence.get(), 2);
    registry.install(key.clone(), candidate, fence(1)).unwrap();
    let staged = registry.atomic_observation_v2(&key).unwrap().unwrap();
    assert_eq!(staged.admission_state, SlotAdmissionStateV2::Staged);
    assert_eq!(staged.admission_generation.get(), 3);
    assert_eq!(staged.observation_sequence.get(), 3);
}

#[test]
fn empty_recovery_cursor_seals_and_restores_one_exact_empty_slot() {
    let registry = registry(8);
    let key = route(10, "study", 1, 1, "p1", None).slot_key();
    let source_guard = registry.recovery_observation_guard_v2().unwrap();
    let source_global = source_guard.observation();
    let source_cursor = source_guard.into_empty_cursor().unwrap();
    let sealed = registry
        .seal_empty_recovery_drain_claim_v2(source_cursor, &key, seal_key(10))
        .unwrap();

    assert!(sealed.source_slot_observation().is_none());
    assert_eq!(sealed.seal().key(), &key);
    assert_eq!(sealed.seal().seal_generation().get(), 1);
    assert_eq!(sealed.seal().route(), None);
    assert_eq!(sealed.slot_observation().route, None);
    assert_eq!(sealed.slot_observation().active_interactions, 0);
    assert_eq!(
        sealed.slot_observation().admission_state,
        SlotAdmissionStateV2::DrainClaimSealed {
            seal_key: seal_key(10),
            seal_generation: sealed.seal().seal_generation(),
        }
    );
    assert_eq!(
        sealed.registry_observation().observation_sequence().get(),
        source_global.observation_sequence().get() + 1
    );
    assert_eq!(sealed.registry_observation().retained_slot_count(), 1);
    assert_eq!(
        sealed
            .registry_observation()
            .retained_empty_tombstone_count(),
        0
    );
    assert_eq!(sealed.registry_observation().sealed_slot_count(), 1);

    let unsealed = registry
        .unseal_empty_recovery_drain_claim_v2(sealed)
        .unwrap();
    assert_eq!(unsealed.slot_observation().route, None);
    assert_eq!(
        unsealed.slot_observation().admission_state,
        SlotAdmissionStateV2::Empty
    );
    assert_eq!(unsealed.registry_observation().retained_slot_count(), 1);
    assert_eq!(
        unsealed
            .registry_observation()
            .retained_empty_tombstone_count(),
        1
    );
    assert_eq!(unsealed.registry_observation().sealed_slot_count(), 0);
    let restored = registry
        .revalidate_empty_recovery_cursor_v2(&unsealed.into_cursor())
        .unwrap();
    assert!(restored.is_recovery_empty());
}

#[test]
fn empty_recovery_seal_preserves_tombstone_local_generation() {
    let registry = registry(8);
    let key = route(10, "study", 1, 1, "p1", None).slot_key();
    let (first, _) = registry
        .seal_drain_claim_v2(&key, seal_key(11), None)
        .unwrap();
    registry.unseal_drain_claim_v2(first).unwrap();
    let source_slot = registry.atomic_observation_v2(&key).unwrap().unwrap();
    let cursor = registry
        .recovery_observation_guard_v2()
        .unwrap()
        .into_empty_cursor()
        .unwrap();
    let sealed = registry
        .seal_empty_recovery_drain_claim_v2(cursor, &key, seal_key(12))
        .unwrap();

    assert_eq!(sealed.source_slot_observation(), Some(&source_slot));
    assert_eq!(sealed.seal().seal_generation().get(), 2);
    assert_eq!(
        sealed.slot_observation().admission_generation.get(),
        source_slot.admission_generation.get() + 1
    );
    assert_eq!(
        sealed.slot_observation().observation_sequence.get(),
        source_slot.observation_sequence.get() + 1
    );
}

#[test]
fn empty_recovery_unseal_rejects_cross_thread_s1_advance_without_mutating_target() {
    let registry = registry(8);
    let target_key = route(10, "study", 1, 1, "p1", None).slot_key();
    let unrelated_key = route(11, "other", 1, 1, "p1", None).slot_key();
    let cursor = registry
        .recovery_observation_guard_v2()
        .unwrap()
        .into_empty_cursor()
        .unwrap();
    let target = registry
        .seal_empty_recovery_drain_claim_v2(cursor, &target_key, seal_key(16))
        .unwrap();
    let unrelated_registry = registry.clone();
    std::thread::spawn(move || {
        let (unrelated, _) = unrelated_registry
            .seal_drain_claim_v2(&unrelated_key, seal_key(17), None)
            .unwrap();
        unrelated_registry.unseal_drain_claim_v2(unrelated).unwrap();
    })
    .join()
    .unwrap();
    let before = registry
        .atomic_observation_v2(&target_key)
        .unwrap()
        .unwrap();

    assert_eq!(
        registry
            .unseal_empty_recovery_drain_claim_v2(target)
            .err()
            .unwrap(),
        ServingSlotRegistryError::StaleSlotSeal
    );
    assert_eq!(
        registry.atomic_observation_v2(&target_key).unwrap(),
        Some(before)
    );
    assert_eq!(
        registry
            .install(
                target_key.clone(),
                route(10, "study", 1, 1, "p1", None),
                fence(1),
            )
            .unwrap_err(),
        ServingSlotRegistryError::SlotSealed
    );
}

#[test]
fn empty_recovery_seal_rejects_stale_foreign_and_nonempty_cursors() {
    let source = registry(8);
    let foreign = registry(8);
    let key = route(10, "study", 1, 1, "p1", None).slot_key();
    let stale = source
        .recovery_observation_guard_v2()
        .unwrap()
        .into_empty_cursor()
        .unwrap();
    let (advance, _) = source
        .seal_drain_claim_v2(&key, seal_key(13), None)
        .unwrap();
    source.unseal_drain_claim_v2(advance).unwrap();
    assert_eq!(
        source
            .seal_empty_recovery_drain_claim_v2(stale, &key, seal_key(14))
            .err()
            .unwrap(),
        ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor
    );

    let foreign_cursor = source
        .recovery_observation_guard_v2()
        .unwrap()
        .into_empty_cursor()
        .unwrap();
    assert_eq!(
        foreign
            .seal_empty_recovery_drain_claim_v2(foreign_cursor, &key, seal_key(15))
            .err()
            .unwrap(),
        ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor
    );

    let occupied = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    occupied
        .install(candidate.slot_key(), candidate, fence(1))
        .unwrap();
    assert_eq!(
        occupied
            .recovery_observation_guard_v2()
            .unwrap()
            .into_empty_cursor()
            .err()
            .unwrap(),
        ServingSlotRegistryError::RegistryRecoveryNotEmpty
    );
}

#[test]
fn seal_requires_the_exact_atomic_observation() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    let stale = registry.atomic_observation_v2(&key).unwrap().unwrap();
    let admitted = registry.admit(&key).unwrap();
    assert_eq!(
        registry
            .seal_drain_claim_v2(&key, seal_key(9), Some(&stale))
            .err()
            .unwrap(),
        ServingSlotRegistryError::StaleSlotObservation
    );
    drop(admitted);
    assert_eq!(
        registry
            .seal_drain_claim_v2(&key, seal_key(9), None)
            .err()
            .unwrap(),
        ServingSlotRegistryError::StaleSlotObservation
    );
}

#[test]
fn seal_and_admission_race_has_one_linearized_winner() {
    use std::sync::{Arc, Barrier};

    for iteration in 0..64 {
        let registry = registry(8);
        let candidate = route(10, "study", 1, 1, "p1", None);
        let key = candidate.slot_key();
        let token = registry
            .install(key.clone(), candidate.clone(), fence(1))
            .unwrap()
            .token;
        registry.activate(&token, candidate.identity()).unwrap();
        let before = registry.atomic_observation_v2(&key).unwrap().unwrap();
        let barrier = Arc::new(Barrier::new(3));
        std::thread::scope(|scope| {
            let seal_barrier = Arc::clone(&barrier);
            let seal_registry = &registry;
            let seal_key_ref = &key;
            let seal_before = &before;
            let seal = scope.spawn(move || {
                seal_barrier.wait();
                seal_registry.seal_drain_claim_v2(
                    seal_key_ref,
                    seal_key((iteration + 1) as u8),
                    Some(seal_before),
                )
            });
            let admit_barrier = Arc::clone(&barrier);
            let admit_registry = &registry;
            let admit_key = &key;
            let admit_before = &before;
            let admit = scope.spawn(move || {
                admit_barrier.wait();
                admit_registry.admit_at_generation_v2(admit_key, admit_before.admission_generation)
            });
            barrier.wait();
            let seal_result = seal.join().unwrap();
            let admit_result = admit.join().unwrap();
            match (seal_result, admit_result) {
                (Ok((capability, _)), Err(ServingSlotRegistryError::SlotSealed)) => {
                    registry.unseal_drain_claim_v2(capability).unwrap();
                }
                (Err(ServingSlotRegistryError::StaleSlotObservation), Ok(admitted)) => {
                    drop(admitted);
                }
                _ => panic!("seal and admission did not linearize"),
            }
        });
    }
}

#[test]
fn recovery_observation_tracks_every_effective_mutation_and_ignores_replays() {
    let registry = registry(8);
    let initial = registry.recovery_observation_v2().unwrap();
    assert_eq!(initial.observation_sequence().get(), 1);
    assert_eq!(initial.retained_slot_count(), 0);
    assert!(initial.is_recovery_empty());

    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let installed = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap();
    let staged = registry.recovery_observation_v2().unwrap();
    assert_eq!(staged.observation_sequence().get(), 2);
    assert_eq!(staged.staged_route_count(), 1);
    assert!(!staged.is_recovery_empty());

    registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap();
    assert_eq!(registry.recovery_observation_v2().unwrap(), staged);

    registry
        .activate_with_sequence_v2(&installed.token, candidate.identity())
        .unwrap();
    let serving = registry.recovery_observation_v2().unwrap();
    assert_eq!(serving.observation_sequence().get(), 3);
    assert_eq!(serving.staged_route_count(), 0);
    assert_eq!(serving.serving_route_count(), 1);
    registry
        .activate_with_sequence_v2(&installed.token, candidate.identity())
        .unwrap();
    assert_eq!(registry.recovery_observation_v2().unwrap(), serving);

    let renewed = registry
        .advance_authority(&installed.token, candidate.identity(), fence(2))
        .unwrap();
    let refenced = registry.recovery_observation_v2().unwrap();
    assert_eq!(refenced.observation_sequence().get(), 4);
    assert_eq!(refenced.serving_route_count(), 1);

    let active = registry.admit(&key).unwrap();
    let admitted = registry.recovery_observation_v2().unwrap();
    assert_eq!(admitted.observation_sequence().get(), 5);
    assert_eq!(admitted.active_interaction_count(), 1);
    drop(active);
    let released = registry.recovery_observation_v2().unwrap();
    assert_eq!(released.observation_sequence().get(), 6);
    assert_eq!(released.active_interaction_count(), 0);

    registry.begin_drain(&renewed).unwrap();
    let draining = registry.recovery_observation_v2().unwrap();
    assert_eq!(draining.observation_sequence().get(), 7);
    assert_eq!(draining.serving_route_count(), 0);
    assert_eq!(draining.draining_route_count(), 1);
    registry.begin_drain(&renewed).unwrap();
    assert_eq!(registry.recovery_observation_v2().unwrap(), draining);

    registry.remove(&renewed).unwrap();
    let empty = registry.recovery_observation_v2().unwrap();
    assert_eq!(empty.observation_sequence().get(), 8);
    assert_eq!(empty.retained_slot_count(), 1);
    assert_eq!(empty.retained_empty_tombstone_count(), 1);
    assert!(empty.is_recovery_empty());
}

#[test]
fn recovery_observation_counts_hidden_staged_and_retired_routes() {
    let registry = registry(8);
    let first = route(10, "study", 1, 1, "p1", None);
    let key = first.slot_key();
    let first_token = registry
        .install(key.clone(), first.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&first_token, first.identity()).unwrap();
    let active = registry.admit(&key).unwrap();

    let second = route(10, "study", 2, 2, "p2", Some(50));
    let second_token = registry
        .install(key.clone(), second.clone(), fence(2))
        .unwrap()
        .token;
    let replacing = registry.recovery_observation_v2().unwrap();
    assert_eq!(replacing.observation_sequence().get(), 5);
    assert_eq!(replacing.staged_route_count(), 1);
    assert_eq!(replacing.serving_route_count(), 1);
    assert_eq!(replacing.active_interaction_count(), 1);

    registry.activate(&second_token, second.identity()).unwrap();
    let third = route(10, "study", 3, 3, "p3", Some(51));
    registry.install(key, third, fence(3)).unwrap();
    let complete = registry.recovery_observation_v2().unwrap();
    assert_eq!(complete.observation_sequence().get(), 7);
    assert_eq!(complete.staged_route_count(), 1);
    assert_eq!(complete.serving_route_count(), 1);
    assert_eq!(complete.draining_route_count(), 1);
    assert_eq!(complete.active_interaction_count(), 1);

    drop(active);
    let zero_active_retired = registry.recovery_observation_v2().unwrap();
    assert_eq!(zero_active_retired.observation_sequence().get(), 8);
    assert_eq!(zero_active_retired.draining_route_count(), 1);
    assert_eq!(zero_active_retired.active_interaction_count(), 0);
    assert!(!zero_active_retired.is_recovery_empty());
}

#[test]
fn recovery_observation_treats_empty_seals_as_obligations_and_tombstones_as_empty() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let (seal, _) = registry
        .seal_drain_claim_v2(&key, seal_key(10), None)
        .unwrap();
    let sealed_empty = registry.recovery_observation_v2().unwrap();
    assert_eq!(sealed_empty.observation_sequence().get(), 2);
    assert_eq!(sealed_empty.retained_slot_count(), 1);
    assert_eq!(sealed_empty.sealed_slot_count(), 1);
    assert!(!sealed_empty.is_recovery_empty());

    registry.unseal_drain_claim_v2(seal).unwrap();
    let tombstone = registry.recovery_observation_v2().unwrap();
    assert_eq!(tombstone.observation_sequence().get(), 3);
    assert_eq!(tombstone.retained_empty_tombstone_count(), 1);
    assert!(tombstone.is_recovery_empty());

    let token = registry
        .install(key.clone(), candidate.clone(), fence(1))
        .unwrap()
        .token;
    registry.activate(&token, candidate.identity()).unwrap();
    let before = registry.atomic_observation_v2(&key).unwrap().unwrap();
    let (populated_seal, _) = registry
        .seal_drain_claim_v2(&key, seal_key(11), Some(&before))
        .unwrap();
    let sealed_serving = registry.recovery_observation_v2().unwrap();
    assert_eq!(sealed_serving.observation_sequence().get(), 6);
    assert_eq!(sealed_serving.sealed_slot_count(), 1);
    assert_eq!(sealed_serving.serving_route_count(), 1);
    assert!(!sealed_serving.is_recovery_empty());
    registry.unseal_drain_claim_v2(populated_seal).unwrap();
    let reopened_serving = registry.recovery_observation_v2().unwrap();
    assert_eq!(reopened_serving.observation_sequence().get(), 7);
    assert_eq!(reopened_serving.sealed_slot_count(), 0);
    assert_eq!(reopened_serving.serving_route_count(), 1);
}

#[test]
fn recovery_empty_aba_and_cross_slot_mutations_have_successor_sequences() {
    let registry = registry(8);
    let before = registry.recovery_observation_v2().unwrap();
    let candidate = route(10, "study", 1, 1, "p1", None);
    let key = candidate.slot_key();
    let token = registry.install(key, candidate, fence(1)).unwrap().token;
    registry.remove(&token).unwrap();
    let after = registry.recovery_observation_v2().unwrap();
    assert!(before.is_recovery_empty());
    assert!(after.is_recovery_empty());
    assert_eq!(before.observation_sequence().get(), 1);
    assert_eq!(after.observation_sequence().get(), 3);

    let first = route(20, "study", 1, 1, "p2", None);
    let second = route(30, "study", 1, 1, "p3", None);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    std::thread::scope(|scope| {
        let first_barrier = barrier.clone();
        let first_registry = &registry;
        scope.spawn(move || {
            first_barrier.wait();
            first_registry
                .install(first.slot_key(), first, fence(1))
                .unwrap();
        });
        let second_barrier = barrier.clone();
        let second_registry = &registry;
        scope.spawn(move || {
            second_barrier.wait();
            second_registry
                .install(second.slot_key(), second, fence(1))
                .unwrap();
        });
        barrier.wait();
    });
    let concurrent = registry.recovery_observation_v2().unwrap();
    assert_eq!(concurrent.observation_sequence().get(), 5);
    assert_eq!(concurrent.staged_route_count(), 2);
}

#[test]
fn recovery_observation_races_are_whole_before_or_after_snapshots() {
    for iteration in 0..32 {
        let registry = registry(8);
        let candidate = route(10, "study", 1, 1, "p1", None);
        let key = candidate.slot_key();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        std::thread::scope(|scope| {
            let observer_barrier = barrier.clone();
            let observer_registry = &registry;
            let observer = scope.spawn(move || {
                observer_barrier.wait();
                observer_registry.recovery_observation_v2().unwrap()
            });
            let install_barrier = barrier.clone();
            let install_registry = &registry;
            let install = scope.spawn(move || {
                install_barrier.wait();
                install_registry.install(key, candidate, fence(1)).unwrap();
            });
            barrier.wait();
            let observed = observer.join().unwrap();
            install.join().unwrap();
            match observed.observation_sequence().get() {
                1 => assert!(observed.is_recovery_empty()),
                2 => assert_eq!(observed.staged_route_count(), 1),
                _ => panic!("registry observation did not linearize in iteration {iteration}"),
            }
        });
        let final_observation = registry.recovery_observation_v2().unwrap();
        assert_eq!(final_observation.observation_sequence().get(), 2);
        assert_eq!(final_observation.staged_route_count(), 1);
    }
}

#[test]
fn recovery_observation_and_guard_drop_linearize_as_whole_snapshots() {
    for iteration in 0..32 {
        let registry = registry(8);
        let candidate = route(10, "study", 1, 1, "p1", None);
        let key = candidate.slot_key();
        let token = registry
            .install(key, candidate.clone(), fence(1))
            .unwrap()
            .token;
        registry.activate(&token, candidate.identity()).unwrap();
        let active = registry.admit(token.key()).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        std::thread::scope(|scope| {
            let observer_barrier = barrier.clone();
            let observer_registry = &registry;
            let observer = scope.spawn(move || {
                observer_barrier.wait();
                observer_registry.recovery_observation_v2().unwrap()
            });
            let drop_barrier = barrier.clone();
            let release = scope.spawn(move || {
                drop_barrier.wait();
                drop(active);
            });
            barrier.wait();
            let observed = observer.join().unwrap();
            release.join().unwrap();
            match observed.observation_sequence().get() {
                4 => assert_eq!(observed.active_interaction_count(), 1),
                5 => assert_eq!(observed.active_interaction_count(), 0),
                _ => panic!("guard observation did not linearize in iteration {iteration}"),
            }
        });
        let final_observation = registry.recovery_observation_v2().unwrap();
        assert_eq!(final_observation.observation_sequence().get(), 5);
        assert_eq!(final_observation.active_interaction_count(), 0);
    }
}

#[test]
fn recovery_guard_mints_and_revalidates_an_initial_empty_recovery_cursor() {
    let registry = registry(8);
    let guard = registry.recovery_observation_guard_v2().unwrap();
    let observed = guard.observation();
    assert!(observed.is_recovery_empty());
    assert_eq!(observed.observation_sequence().get(), 1);

    let cursor = guard.into_empty_cursor().unwrap();
    assert_eq!(
        format!("{cursor:?}"),
        "RegistryEmptyRecoveryCursorV2(<redacted>)"
    );
    assert_eq!(
        registry
            .revalidate_empty_recovery_cursor_v2(&cursor)
            .unwrap(),
        observed
    );
}

#[test]
fn empty_recovery_cursor_rejects_a_foreign_registry_at_the_same_sequence() {
    let source = registry(8);
    let foreign = registry(8);
    let cursor = source
        .recovery_observation_guard_v2()
        .unwrap()
        .into_empty_cursor()
        .unwrap();
    assert_eq!(
        foreign
            .recovery_observation_v2()
            .unwrap()
            .observation_sequence(),
        source
            .recovery_observation_v2()
            .unwrap()
            .observation_sequence()
    );
    assert_eq!(
        foreign.revalidate_empty_recovery_cursor_v2(&cursor),
        Err(ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor)
    );
}

#[test]
fn every_effective_registry_change_stales_an_empty_cursor_across_empty_aba() {
    let registry = registry(8);
    let cursor = registry
        .recovery_observation_guard_v2()
        .unwrap()
        .into_empty_cursor()
        .unwrap();
    let candidate = route(10, "study", 1, 1, "p1", None);
    let token = registry
        .install(candidate.slot_key(), candidate, fence(1))
        .unwrap()
        .token;
    registry.remove(&token).unwrap();
    assert!(registry
        .recovery_observation_v2()
        .unwrap()
        .is_recovery_empty());
    assert_eq!(
        registry.revalidate_empty_recovery_cursor_v2(&cursor),
        Err(ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor)
    );
}

#[test]
fn recovery_guard_cannot_mint_a_cursor_from_a_nonempty_registry() {
    let registry = registry(8);
    let candidate = route(10, "study", 1, 1, "p1", None);
    registry
        .install(candidate.slot_key(), candidate, fence(1))
        .unwrap();
    let guard = registry.recovery_observation_guard_v2().unwrap();
    assert!(!guard.observation().is_recovery_empty());
    assert!(matches!(
        guard.into_empty_cursor(),
        Err(ServingSlotRegistryError::RegistryRecoveryNotEmpty)
    ));
}

#[test]
fn recovery_guard_holds_the_registry_lock_until_cursor_mint_finishes() {
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Duration;

    let registry = registry(8);
    let guard = registry.recovery_observation_guard_v2().unwrap();
    let candidate = route(10, "study", 1, 1, "p1", None);
    let (started_sender, started_receiver) = channel();
    let (completed_sender, completed_receiver) = channel();
    std::thread::scope(|scope| {
        let registry_ref = &registry;
        scope.spawn(move || {
            started_sender.send(()).unwrap();
            let result = registry_ref.install(candidate.slot_key(), candidate, fence(1));
            completed_sender.send(result).unwrap();
        });
        started_receiver.recv().unwrap();
        assert!(matches!(
            completed_receiver.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));
        let cursor = guard.into_empty_cursor().unwrap();
        completed_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(
            registry.revalidate_empty_recovery_cursor_v2(&cursor),
            Err(ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor)
        );
    });
}

#[test]
fn dropping_a_recovery_guard_releases_the_registry_lock() {
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Duration;

    let registry = registry(8);
    let guard = registry.recovery_observation_guard_v2().unwrap();
    let candidate = route(10, "study", 1, 1, "p1", None);
    let (started_sender, started_receiver) = channel();
    let (completed_sender, completed_receiver) = channel();
    std::thread::scope(|scope| {
        let registry_ref = &registry;
        scope.spawn(move || {
            started_sender.send(()).unwrap();
            let result = registry_ref.install(candidate.slot_key(), candidate, fence(1));
            completed_sender.send(result).unwrap();
        });
        started_receiver.recv().unwrap();
        assert!(matches!(
            completed_receiver.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));
        drop(guard);
        completed_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
    });
}
