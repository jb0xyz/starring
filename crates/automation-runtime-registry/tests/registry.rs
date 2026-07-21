use std::num::NonZeroU32;

use automation_ruleset::{
    content_hash, RuleSetKey, RuleSetVersion, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_convergence::{
    BindingRevision, FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1,
};
use automation_runtime_registry::{
    ExactServingRouteV1, ServingSlotKeyV1, ServingSlotRegistryConfigV1, ServingSlotRegistryError,
    ServingSlotRegistryV1, SlotActivationOutcomeV1, SlotDrainOutcomeV1, SlotInstallOutcomeV1,
    SlotLifecycleV1, SlotRemovalOutcomeV1,
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
            valid.identity().clone(),
            valid.ruleset().as_ref().clone(),
            different_bindings
        )
        .unwrap_err(),
        automation_runtime_registry::ExactServingRouteError::BindingFingerprintMismatch
    );
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
